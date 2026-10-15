# Synthetic collections (U11: R1804, R1805, R1963;
# specs/fixtures/synthetic-collections.md). Sourced by tools/mg.
#
# fixtures (dev container) builds meguri-cli with the `fixtures` cargo
# profile and runs `meguri-cli generate` into build/fixtures/ (F10.1):
#   <size> and worst cases  <user-shape sha256, 16 hex>-<seed>-<size or name>/
#   recipes                 recipes/<name>-<seed>/
# each holding collection.anki2, collection.media/ and manifest.json. A size
# or worst-case directory whose manifest matches the request is reused
# (`cached`); recipes are always made again (meguri-cli generate). Each run
# holds its directory's lock (.<name>.lock beside it) and publishes the
# directory whole, so concurrent runs never mix two outputs. Every run
# must stay within the R1804 budget for 1M cards: 10 minutes and 2 GB peak
# memory.
#
# fixtures-check (host) is F11's T1: it runs fixtures, then
# `tools/mg oracle check` on each collection, and checks what F11 asks.
# When recipes are among its targets it also launches them through
# -MeguriFixture in the app process on this Mac (MeguriSnapshotTests'
# RecipeFixtureTests, as tools/mg snapshots runs its suites): U11's "UI
# recipes load through the launch option".

mg_job fixtures docker:dev mg_fixtures \
  "Synthetic collections into build/fixtures: <size> (10k 100k 150k 500k 1m, or a card count) | <worst case> | <recipe> | worst-cases | recipes ... [--seed N] [--media files] [--cards N (worst cases)]"
mg_job fixtures-check host mg_fixtures_check \
  "Oracle checks of generated collections (F11 T1): [target...] (default 10k 100k worst-cases recipes) [--seed N]; runs tools/mg fixtures, then tools/mg oracle check on each: no Check Database problem, nothing changed but col and config, counts equal the manifest's, the pinned day; the due worst case's deck count and the large-field worst case's render; recipes also launched through -MeguriFixture in the app on this Mac (RecipeFixtureTests)"

MG_FIXTURES_BUDGET_MS=600000
MG_FIXTURES_BUDGET_KB=2097152
MG_FIXTURES_SHAPE=core/crates/meguri-testkit/profiles/user-shape.json
MG_FIXTURES_WORST=core/crates/meguri-testkit/profiles/worst-cases
MG_FIXTURES_RECIPES=core/crates/meguri-testkit/fixtures/recipes

# mg__fixtures_names <dir> <suffix>: the file stems in a directory, one per line.
mg__fixtures_names() {
  local f
  for f in "$1"/*"$2"; do
    [ -f "$f" ] || continue
    f="${f##*/}"
    printf '%s\n' "${f%"$2"}"
  done
}

mg__fixtures_usage() {
  mg_failure usage "$1; usage: tools/mg fixtures <size|worst case|recipe|worst-cases|recipes>... [--seed N] [--media files] [--cards N]" "tools/mg fixtures 10k"
  mg_status error
}

# tools/mg fixtures <target>... [--seed N] [--media files] [--cards N]
mg_fixtures() {
  local seed=1 media="" cards="" targets=() t
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --) shift ;;
      --seed | --media | --cards)
        if [ "$#" -lt 2 ]; then
          mg__fixtures_usage "$1 needs a value"
          return 0
        fi
        case "$1" in
          --seed) seed="$2" ;;
          --media) media="$2" ;;
          --cards) cards="$2" ;;
        esac
        shift 2
        ;;
      -*)
        mg__fixtures_usage "unknown option '$1'"
        return 0
        ;;
      worst-cases)
        while IFS= read -r t; do targets[${#targets[@]}]="$t"; done < <(mg__fixtures_names "$MG_FIXTURES_WORST" .json)
        shift
        ;;
      recipes)
        while IFS= read -r t; do targets[${#targets[@]}]="$t"; done < <(mg__fixtures_names "$MG_FIXTURES_RECIPES" .yaml)
        shift
        ;;
      *)
        targets[${#targets[@]}]="$1"
        shift
        ;;
    esac
  done
  case "$seed" in '' | *[!0-9]*)
    mg__fixtures_usage "--seed takes a non-negative integer, not '$seed'"
    return 0
    ;;
  esac
  case "$media" in '' | files | references) ;; *)
    mg__fixtures_usage "--media is files or references, not '$media'"
    return 0
    ;;
  esac
  if [ "${#targets[@]}" -eq 0 ]; then
    mg__fixtures_usage "no target"
    return 0
  fi
  # Classify every target before building anything.
  local kinds=() kind
  for t in "${targets[@]}"; do
    if [ -f "$MG_FIXTURES_WORST/$t.json" ]; then
      kind=worst-case
    elif [ -f "$MG_FIXTURES_RECIPES/$t.yaml" ]; then
      kind=recipe
    elif printf '%s' "$t" | grep -Eqi '^[1-9][0-9]*[km]?$'; then
      kind=size
    else
      mg__fixtures_usage "unknown target '$t' (worst cases: $(mg__fixtures_names "$MG_FIXTURES_WORST" .json | tr '\n' ' ')recipes: $(mg__fixtures_names "$MG_FIXTURES_RECIPES" .yaml | tr '\n' ' '))"
      return 0
    fi
    kinds[${#kinds[@]}]="$kind"
  done

  if ! (cd core && cargo build --profile fixtures -p meguri-cli); then
    mg_failure fixtures-build "building meguri-cli (profile fixtures) failed; see log.txt"
    mg_status fail
    return 0
  fi
  local bin
  bin="$(cd core && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)/fixtures/meguri-cli"
  local shape
  if command -v sha256sum >/dev/null 2>&1; then
    shape="$(sha256sum <"$MG_FIXTURES_SHAPE" | cut -c1-16)"
  else
    shape="$(shasum -a 256 <"$MG_FIXTURES_SHAPE" | cut -c1-16)"
  fi

  local i dir out rc passed=0 failed=0 summaries="$MG_RUN_DIR/summaries.jsonl" args elapsed peak repro
  : >"$summaries"
  i=0
  for t in "${targets[@]}"; do
    kind="${kinds[$i]}"
    i=$((i + 1))
    args=(generate)
    case "$kind" in
      worst-case)
        args+=(worst-case "$t")
        dir="build/fixtures/$shape-$seed-$t"
        [ -z "$cards" ] || args+=(--cards "$cards")
        ;;
      recipe)
        args+=(recipe "$t")
        dir="build/fixtures/recipes/$t-$seed"
        ;;
      size)
        args+=("$t")
        dir="build/fixtures/$shape-$seed-$(printf '%s' "$t" | tr 'A-Z' 'a-z')"
        ;;
    esac
    [ -z "$media" ] || [ "$kind" = recipe ] || args+=(--media "$media")
    args+=(--seed "$seed" --out "$dir" --cache)
    repro="tools/mg fixtures $t --seed $seed${media:+ --media $media}"
    out="$MG_RUN_DIR/$t.json"
    echo "fixtures: $t ($kind) -> $dir" >&2
    "$bin" "${args[@]}" >"$out"
    rc=$?
    if [ "$rc" -ne 0 ] || ! jq -e .manifest >/dev/null 2>&1 <"$out"; then
      mg_failure "fixtures/$t" "meguri-cli generate exited $rc; see log.txt" "$repro"
      failed=$((failed + 1))
      continue
    fi
    jq -c --arg target "$t" --arg kind "$kind" '{target: $target, kind: $kind} + .' <"$out" >>"$summaries"
    mg_artifact fixture "$dir" "$t"
    elapsed="$(jq -r .elapsed_ms <"$out")"
    peak="$(jq -r '.peak_rss_kb // 0' <"$out")"
    if [ "$(jq -r .cached <"$out")" = false ] && [ "$elapsed" -gt "$MG_FIXTURES_BUDGET_MS" ]; then
      mg_failure "fixtures/$t/time" "generation took ${elapsed} ms, over the ${MG_FIXTURES_BUDGET_MS} ms budget" "$repro"
      failed=$((failed + 1))
    elif [ "$peak" -gt "$MG_FIXTURES_BUDGET_KB" ]; then
      mg_failure "fixtures/$t/memory" "peak resident memory ${peak} kB, over the ${MG_FIXTURES_BUDGET_KB} kB budget" "$repro"
      failed=$((failed + 1))
    else
      passed=$((passed + 1))
    fi
  done
  mg_detail fixtures "$(jq -sc . "$summaries")"
  mg_count total $((passed + failed))
  mg_count passed "$passed"
  mg_count failed "$failed"
  if [ "$failed" -eq 0 ]; then
    mg_message "fixtures: $passed generated or reused under build/fixtures"
    mg_status pass
  else
    mg_message "fixtures: $failed of $((passed + failed)) failed"
    mg_status fail
  fi
}

# The day a recipe's collection is pinned on (F9.2: 120 days of history).
MG_FIXTURES_RECIPE_DAYS=120
# F11: the large-field note's answer is longer than 1 MiB.
MG_FIXTURES_LARGE_ANSWER=1048576

mg__fixtures_check_usage() {
  mg_failure usage "$1; usage: tools/mg fixtures-check [target...] [--seed N]" "tools/mg fixtures-check 10k"
  mg_status error
}

# mg__fixtures_check_one <summary json> <today of profiles>: one oracle check
# and F11's assertions on its report. Prints "pass" or "fail" last.
mg__fixtures_check_one() {
  local summary="$1" profile_today="$2" target kind dir abs today worst result report manifest
  local args=() problems note repro
  target="$(jq -r .target <<<"$summary")"
  kind="$(jq -r .kind <<<"$summary")"
  dir="$(jq -r .dir <<<"$summary")"
  case "$dir" in /*) abs="$dir" ;; *) abs="$MG_ROOT/$dir" ;; esac
  manifest="$(jq -c .manifest <<<"$summary")"
  today="$profile_today"
  [ "$kind" != recipe ] || today="$MG_FIXTURES_RECIPE_DAYS"
  worst='{}'
  [ "$kind" != worst-case ] || worst="$(jq -c . "$MG_FIXTURES_WORST/$target.json")"
  if jq -e '.due_deck' >/dev/null <<<"$worst"; then
    args+=(--deck Due)
  fi
  if jq -e '.large_fields' >/dev/null <<<"$worst"; then
    note="$(jq -r '.large_field_notes[0] // empty' <<<"$summary")"
    if [ -z "$note" ]; then
      mg_failure "fixtures-check/$target/render" "meguri-cli generate listed no large-field note" "tools/mg fixtures-check $target"
      echo fail
      return 0
    fi
    args+=(--render-note "$note")
  fi
  # args may be empty: macOS bash 3.2 calls an empty array unbound under set -u.
  repro="tools/mg oracle check $dir${args[0]+ ${args[*]}}"
  echo "fixtures-check: $target -> $repro" >&2
  result="$("$MG_TOOLS/mg" oracle check "$abs" ${args[0]+"${args[@]}"} </dev/null)"
  result="${result##*$'\n'}"
  if [ -z "$result" ] || [ ! -f "$result" ]; then
    mg_failure "fixtures-check/$target/oracle" "tools/mg oracle check wrote no result; see log.txt" "$repro"
    echo fail
    return 0
  fi
  mg_artifact oracle-check "$result" "$target"
  report="$(jq -c '.details.check // empty' "$result")"
  if [ -z "$report" ]; then
    mg_failure "fixtures-check/$target/oracle" "oracle check: $(jq -r '.status + ": " + ([.failures[] | .id + " " + .message] | join("; "))' "$result")" "$repro"
    echo fail
    return 0
  fi
  # K9's status (Check Database problems, changed tables), then F11. A report
  # these expressions cannot read (jq fails) is a failure, never a pass.
  if ! problems="$(jq -rn --argjson r "$report" --argjson m "$manifest" --argjson w "$worst" \
    --argjson today "$today" --argjson large "$MG_FIXTURES_LARGE_ANSWER" --arg note "${note:-}" '
    ([$r.check_db | if has("error") then "Check Database found the file corrupt"
       else to_entries[] | select(.value > 0) | "Check Database reported \(.value) of \(.key)" end])
    + [$r.changed_tables[] | select(. != "col" and . != "config") | "Check Database changed the \(.) table"]
    + [("notes", "cards", "revlog", "notetypes", "decks", "presets", "tags") as $k
       | select($r.counts[$k] != $m.counts[$k])
       | "the oracle counts \($r.counts[$k]) \($k), the manifest \($m.counts[$k])"]
    + [select($r.counts.graves != 0) | "the oracle counts \($r.counts.graves) graves"]
    + [select($r.timing.days_elapsed != $today) | "the oracle is on day \($r.timing.days_elapsed), the collection pinned on day \($today)"]
    + (if $w.due_deck then
         ([$m.counts.cards, $w.due_deck.review_limit] | min) as $want
         | [select(($r.decks.Due.review // null) != $want) | "deck Due shows \($r.decks.Due.review // "no") reviews, want \($want)"]
       else [] end)
    + (if $w.large_fields then
         ($r.render[$note] // null) as $cards
         | if $cards == null or ($cards | length) == 0 then ["the oracle rendered no card of note \($note)"]
           else [$cards[] | select(.status != "ok" or .answer_bytes <= $large)
             | "card \(.card) of note \($note) rendered \(.status) with \(.answer_bytes) answer bytes"] end
       else [] end)
    | .[]')"; then
    mg_failure "fixtures-check/$target/report" "could not read the oracle check report $result; see log.txt" "$repro"
    echo fail
    return 0
  fi
  if [ -n "$problems" ]; then
    while IFS= read -r line; do
      mg_failure "fixtures-check/$target" "$line" "$repro"
    done <<<"$problems"
    echo fail
  else
    echo pass
  fi
}

# The app-hosted suite that launches each recipe through -MeguriFixture at
# its pinned instant (U11 Verification: UI recipes load through the launch
# option).
MG_FIXTURES_LAUNCH_SUITE=MeguriSnapshotTests/RecipeFixtureTests

# mg__fixtures_launch_run <dirs, ':'-separated> <out prefix>: builds the
# Snapshot plan for the Mac and runs the launch suite on the recipes in the
# app process (off-screen windows only, as tools/mg snapshots runs it);
# writes <prefix>.json {result, passed, failed, skipped, messages}: the
# suite's result, its passed and failed cases (one per recipe directory,
# the Swift Testing arguments), skipped tests and failure messages.
# Returns 1, with the failure recorded, when the suite could not run.
mg__fixtures_launch_run() {
  local dirs="$1" prefix="$2" rc tests
  mg_app_build mac Debug Snapshot || return 1
  mg_host_lease || return 1
  mg_app_mac_guard || return 1
  rm -rf "$prefix.xcresult"
  TEST_RUNNER_MEGURI_RECIPE_FIXTURES="$dirs" xcodebuild test-without-building -project app/Meguri.xcodeproj -scheme Meguri \
    -testPlan Snapshot -configuration Debug -destination "platform=macOS,arch=arm64" -derivedDataPath "$MG_APP_DD" \
    -resultBundlePath "$prefix.xcresult" -collect-test-diagnostics never "-only-testing:$MG_FIXTURES_LAUNCH_SUITE" >"$prefix.log" 2>&1
  rc=$?
  mg_artifact log "$prefix.log" "xcodebuild test $MG_FIXTURES_LAUNCH_SUITE"
  tests="$(xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null)" || tests=null
  if ! xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -ce --argjson tests "${tests:-null}" '[$tests | .. | objects | select(.nodeType? == "Arguments") | .result] as $cases
      | {result, passed: ($cases | map(select(. == "Passed")) | length), failed: ($cases | map(select(. == "Failed")) | length),
        skipped: .skippedTests, messages: [$tests | .. | objects | select(.nodeType? == "Failure Message") | .name | .[0:600]]}' >"$prefix.json"; then
    mg_failure fixtures-check/launch "xcodebuild test-without-building could not run $MG_FIXTURES_LAUNCH_SUITE (exit $rc); see $(basename "$prefix").log" \
      "tools/mg fixtures-check recipes"
    return 1
  fi
}

# mg__fixtures_check_launch <summaries>: the launch suite on every recipe
# of the run; each recipe is one passing case, and a skip is a failure.
# Sets MG_FIXTURES_LAUNCH to pass or fail. Not run in a subshell: the host
# lease it takes lasts until the job ends.
mg__fixtures_check_launch() {
  local summaries="$1" dirs n prefix="$MG_RUN_DIR/launch" problems line repro="tools/mg fixtures-check recipes"
  MG_FIXTURES_LAUNCH=fail
  dirs="$(jq -r --arg root "$MG_ROOT" 'select(.kind == "recipe") | .dir | if startswith("/") then . else "\($root)/\(.)" end' "$summaries")"
  n="$(printf '%s\n' "$dirs" | grep -c .)"
  dirs="$(printf '%s\n' "$dirs" | paste -sd: -)"
  echo "fixtures-check: $n recipes -> $MG_FIXTURES_LAUNCH_SUITE" >&2
  mg__fixtures_launch_run "$dirs" "$prefix" || return 0
  mg_artifact launch "$prefix.json" "$MG_FIXTURES_LAUNCH_SUITE results"
  if ! problems="$(jq -r --argjson n "$n" '
    .messages
    + [select(.result != "Passed") | "the launch suite ended \(.result), \(.failed) cases failed"]
    + [select(.skipped != 0) | "\(.skipped) launch cases were skipped: a skipped check is no check"]
    + [select(.passed != $n) | "\(.passed) launch cases passed for \($n) recipes"]
    | .[]' "$prefix.json")"; then
    mg_failure fixtures-check/launch "could not read the launch results $prefix.json; see log.txt" "$repro"
    return 0
  fi
  if [ -n "$problems" ]; then
    while IFS= read -r line; do
      mg_failure fixtures-check/launch "$line" "$repro"
    done <<<"$problems"
    return 0
  fi
  MG_FIXTURES_LAUNCH=pass
}

# tools/mg fixtures-check [target...] [--seed N]
mg_fixtures_check() {
  local seed=1 targets=() gen status summaries outcome passed=0 failed=0 span profile_today what
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --) shift ;;
      --seed)
        [ "$#" -ge 2 ] || { mg__fixtures_check_usage "--seed needs a value"; return 0; }
        seed="$2"
        shift 2
        ;;
      -*) mg__fixtures_check_usage "unknown option '$1'"; return 0 ;;
      *) targets[${#targets[@]}]="$1"; shift ;;
    esac
  done
  case "$seed" in '' | *[!0-9]*) mg__fixtures_check_usage "--seed takes a non-negative integer, not '$seed'"; return 0 ;; esac
  [ "${#targets[@]}" -gt 0 ] || targets=(10k 100k worst-cases recipes)

  gen="$("$MG_TOOLS/mg" fixtures "${targets[@]}" --seed "$seed" </dev/null)"
  gen="${gen##*$'\n'}"
  if [ -z "$gen" ] || [ ! -f "$gen" ]; then
    mg_failure fixtures "tools/mg fixtures wrote no result; see log.txt" "tools/mg fixtures ${targets[*]} --seed $seed"
    mg_status error
    return 0
  fi
  mg_artifact fixtures "$gen" "tools/mg fixtures result"
  status="$(jq -r .status "$gen")"
  if [ "$status" != pass ]; then
    mg_failure fixtures "tools/mg fixtures: $(jq -r .message "$gen")" "tools/mg fixtures ${targets[*]} --seed $seed"
    mg_message "fixtures-check: generating the collections failed"
    if [ "$status" = fail ]; then mg_status fail; else mg_status error; fi
    return 0
  fi
  # F4.3: profiles and worst cases are pinned on the user shape's span day.
  span="$(jq -r '.totals.span_days' "$MG_FIXTURES_SHAPE")"
  profile_today="$span"
  [ "$profile_today" -ge 1 ] || profile_today=1
  [ "$profile_today" -le 36500 ] || profile_today=36500

  summaries="$MG_RUN_DIR/summaries.jsonl"
  jq -c '.details.fixtures[]' "$gen" >"$summaries"
  while IFS= read -r summary; do
    outcome="$(mg__fixtures_check_one "$summary" "$profile_today")"
    outcome="${outcome##*$'\n'}"
    if [ "$outcome" = pass ]; then
      passed=$((passed + 1))
    else
      failed=$((failed + 1))
    fi
  done <"$summaries"
  # The recipes launched in the app: one more check.
  what="the oracle check of each collection"
  if jq -e 'select(.kind == "recipe")' "$summaries" >/dev/null; then
    mg__fixtures_check_launch "$summaries"
    what="$what and the recipes' launch check"
    if [ "$MG_FIXTURES_LAUNCH" = pass ]; then
      passed=$((passed + 1))
    else
      failed=$((failed + 1))
    fi
  fi
  mg_count total $((passed + failed))
  mg_count passed "$passed"
  mg_count failed "$failed"
  if [ "$failed" -eq 0 ]; then
    mg_message "fixtures-check: $passed checks pass ($what)"
    mg_status pass
  else
    mg_message "fixtures-check: $failed of $((passed + failed)) checks fail ($what)"
    mg_status fail
  fi
}
