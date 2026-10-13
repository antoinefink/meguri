# Conformance jobs (specs/conformance/dsl.md D10, D14). Sourced by tools/mg.
#
# Host jobs: T0 runs natively on macOS and in the Linux dev container (KTD24:
# Meguri ships on Apple libm, the oracle runs on glibc), so `conformance`
# drives cargo on the host and the dev service through compose itself. The
# Rust side is meguri-cli's `conformance` command (core/tools/meguri-cli).
# goldens-check and harvest-report need git or the main worktree's oracle/,
# which the container cannot see, so they are host jobs too.

mg_job conformance host mg_conformance \
  "T0 conformance on the committed goldens, macOS native and Linux dev container: [--live] [--lane native|docker|all] [--jobs N] [filter...]"
mg_job goldens-check host mg_goldens_check \
  "Golden freshness, schemas and changed values since a revision: [--base <rev>] (default HEAD)"
mg_job harvest-report host mg_harvest_report \
  "Harvest catalog coverage report (classification x scenario status), the Anki-name leak check and the inventory-at-pin check: [--live-report <conformance --live report.json>]"
mg_job harvest-inventory host mg_harvest_inventory \
  "Spec side: re-inventory Anki's tests at the pinned tag into oracle/harvest/links.csv and the catalog; --supplementary adds main-branch tests (refs/anki); --test runs the inventory's own tests"

mg_conf__usage() {
  mg_failure usage "usage: $1" "tools/mg $MG_JOB --help"
  mg_message "$MG_JOB: $2"
  mg_status error
}

# mg_conf__rel <path under the repo>: the same path relative to the root, as the
# dev container sees it under /work.
mg_conf__rel() { printf '%s\n' "${1#"$MG_ROOT"/}"; }

mg_conf__docker_ok() {
  if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 1
  fi
}

MG_CONF_DEV_BUILT=""

# mg_conf__dev [docker run options...] -- <bash script>: runs the script in the
# dev container from /work, building the image once per run.
mg_conf__dev() {
  local opts=()
  while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
    opts[${#opts[@]}]="$1"
    shift
  done
  [ "${1:-}" = "--" ] && shift
  if [ -z "$MG_CONF_DEV_BUILT" ]; then
    echo "$MG_JOB: building the dev image" >&2
    mg_compose --progress plain build dev >&2 || return 125
    MG_CONF_DEV_BUILT=1
  fi
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" ${opts[0]+"${opts[@]}"} \
    dev bash -c "$1" </dev/null
}

mg_conf__cleanup() {
  local ids
  ids="$(docker ps -aq --filter "label=meguri.mg.run-id=$MG_RUN_ID" 2>/dev/null)"
  # shellcheck disable=SC2086 # one container id per word
  [ -z "$ids" ] || docker rm -f $ids >/dev/null 2>&1
  return 0
}

# mg_conf__cli_native <args...>: meguri-cli on the host (macOS).
mg_conf__cli_native() {
  (cd "$MG_ROOT/core" && cargo run -q -p meguri-cli -- "$@")
}

# mg_conf__quote <args...>: one shell word list for a bash -c script.
mg_conf__quote() {
  local out="" a
  for a in "$@"; do
    out="$out '$(printf '%s' "$a" | sed "s/'/'\\\\''/g")'"
  done
  printf '%s' "$out"
}

# mg_conf__lane_report <lane> <report.json> <rc>: counts and failures from one
# lane's meguri-cli report (T0 or T1: `.fails` follows the report's tier).
mg_conf__lane_report() {
  local lane="$1" report="$2" rc="$3"
  if [ ! -s "$report" ]; then
    mg_failure "$lane/no-report" "the $lane lane exited with code $rc without a report; see log.txt"
    return 1
  fi
  mg_artifact report "$report" "$(jq -r '.tier | ascii_upcase' "$report") report ($lane)"
  jq -c --arg lane "$lane" '.scenarios[] | select(.fails)
    | {id: ($lane + ":" + .scenario),
       message: (.status + ": " + .message
         + (if .first_divergence then " at " + .first_divergence.pointer
              + " (expected " + (.first_divergence.expected | tostring)
              + ", actual " + (.first_divergence.actual | tostring) + ")" else "" end)),
       repro: .repro}' "$report" >>"$MG_STATE/failures.jsonl"
  MG_CONF_LANES="$MG_CONF_LANES$(jq -c --arg lane "$lane" '{($lane): {totals, failed, wall_ms}}' "$report")"$'\n'
}

# tools/mg conformance [--live] [--lane native|docker|all] [--jobs N] [filter...]
mg_conformance() {
  local live=false lane=all jobs=12
  local filters=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --live) live=true; shift ;;
      --lane) lane="${2:-}"; shift 2 || { mg_conf__usage "tools/mg conformance [--live] [--lane native|docker|all] [--jobs N] [filter...]" "--lane needs a value"; return 0; } ;;
      --jobs) jobs="${2:-}"; shift 2 || { mg_conf__usage "tools/mg conformance --jobs N" "--jobs needs a number"; return 0; } ;;
      --help) mg_conf__usage "tools/mg conformance [--live] [--lane native|docker|all] [--jobs N] [filter...]" "help"; return 0 ;;
      --*) mg_conf__usage "tools/mg conformance [--live] [--lane native|docker|all] [--jobs N] [filter...]" "unknown option '$1'"; return 0 ;;
      *) filters[${#filters[@]}]="$1"; shift ;;
    esac
  done
  case "$lane" in native | docker | all) ;; *) mg_conf__usage "tools/mg conformance --lane native|docker|all" "unknown lane '$lane'"; return 0 ;; esac
  case "$jobs" in '' | *[!0-9]* | 0) mg_conf__usage "tools/mg conformance --jobs N" "--jobs needs a positive number"; return 0 ;; esac
  if [ "$live" = true ] && [ "$lane" = docker ]; then
    mg_conf__usage "tools/mg conformance --live [--lane native|all]" "--live drives the oracle from the host and needs the native lane"
    return 0
  fi
  if [ "$lane" != native ] || [ "$live" = true ]; then
    mg_conf__docker_ok || return 0
  fi
  trap 'mg_conf__cleanup; exit 143' TERM
  MG_CONF_LANES=""
  local failed=false rc

  if [ "$live" = true ]; then
    mg_conf__record_stale ${filters[0]+"${filters[@]}"} || failed=true
  fi

  # T0 on each lane: the testkit tests (one test per scenario, the runner's
  # own tests and the 2,000-scenario budget) when no filter is given, then the
  # JSON report of the selected scenarios.
  if [ "$lane" != docker ]; then
    local native="$MG_RUN_DIR/t0-native.json"
    if [ "${#filters[@]}" -eq 0 ]; then
      echo "conformance: testkit tests (macOS native)" >&2
      if ! (cd core && cargo test -q -p meguri-testkit --tests --no-fail-fast) >&2; then
        mg_failure native/testkit "meguri-testkit tests failed natively on macOS; see log.txt" \
          "cd core && cargo test -p meguri-testkit --tests"
        failed=true
      fi
    fi
    mg_conf__cli_native conformance run --conformance "$MG_ROOT/conformance" --lane macos-native \
      --report "$native" ${filters[0]+"${filters[@]}"}
    rc=$?
    mg_conf__lane_report native "$native" "$rc" || failed=true
    [ "$rc" -eq 0 ] || failed=true
  fi
  if [ "$lane" != native ]; then
    local docker_report="$MG_RUN_DIR/t0-docker.json" script
    script="cd core"
    if [ "${#filters[@]}" -eq 0 ]; then
      script="$script && { cargo test -q -p meguri-testkit --tests --no-fail-fast >&2 || echo testkit-failed >\"/work/$(mg_conf__rel "$MG_RUN_DIR")/docker-testkit-failed\"; }"
    fi
    script="$script; cargo run -q -p meguri-cli -- conformance run --conformance /work/conformance --lane linux-docker \
      --report /work/$(mg_conf__rel "$docker_report")$(mg_conf__quote ${filters[0]+"${filters[@]}"})"
    mg_conf__dev -- "$script"
    rc=$?
    if [ -f "$MG_RUN_DIR/docker-testkit-failed" ]; then
      mg_failure docker/testkit "meguri-testkit tests failed in the dev container; see log.txt" \
        "tools/mg core-test -- -E 'package(meguri-testkit)'"
      failed=true
    fi
    mg_conf__lane_report docker "$docker_report" "$rc" || failed=true
    [ "$rc" -eq 0 ] || failed=true
  fi

  if [ "$live" = true ]; then
    mg_conf__live_replay "$jobs" ${filters[0]+"${filters[@]}"} || failed=true
  fi

  mg_detail lanes "$(printf '%s' "$MG_CONF_LANES" | jq -sc 'add // {}')"
  local first
  first="$(ls "$MG_RUN_DIR"/t0-*.json 2>/dev/null | head -n 1)"
  if [ -n "$first" ]; then
    mg_count total "$(jq '.scenarios | length' "$first")"
    mg_count passed "$(jq '[.scenarios[] | select(.status == "pass")] | length' "$first")"
    mg_count failed "$(jq '[.scenarios[] | select(.fails)] | length' "$first")"
    mg_count skipped "$(jq '[.scenarios[] | select(.status != "pass" and (.fails | not))] | length' "$first")"
    mg_count pending "$(jq '.totals.pending' "$first")"
  fi
  local summary
  summary="$(printf '%s' "$MG_CONF_LANES" | jq -sr 'add // {} | to_entries
    | map("\(.key): " + (.value.totals | to_entries | map(select(.value > 0) | "\(.value) \(.key)") | join(", ")))
    | join("; ")')"
  local tiers=T0
  [ "$live" = true ] && tiers="T0 and T1"
  if [ "$failed" = true ] || [ -s "$MG_STATE/failures.jsonl" ]; then
    mg_message "conformance: $tiers failed ($summary)"
    mg_status fail
  else
    mg_message "conformance: $tiers passed ($summary)"
    mg_status pass
  fi
}

# Re-records missing and stale goldens among the selected scenarios through the
# oracle harness, the only writer of goldens (dsl.md D11).
mg_conf__record_stale() {
  local plan="$MG_RUN_DIR/stale-before.json" ids id
  mg_conf__cli_native conformance run --conformance "$MG_ROOT/conformance" --lane plan --report "$plan" "$@" >/dev/null 2>&1
  if [ ! -s "$plan" ]; then
    mg_failure live/plan "could not list missing and stale goldens; see log.txt"
    return 1
  fi
  ids="$(jq -r '.scenarios[] | select(.status == "missing" or .status == "stale") | .scenario' "$plan")"
  if [ -z "$ids" ]; then
    echo "conformance: every selected golden is fresh" >&2
    return 0
  fi
  echo "conformance: re-recording on the oracle: $(printf '%s' "$ids" | tr '\n' ' ')" >&2
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 1
  local args=()
  while IFS= read -r id; do
    [ -n "$id" ] && args[${#args[@]}]="--filter" && args[${#args[@]}]="$id"
  done <<EOF
$ids
EOF
  mg_oracle__harness "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" "$MG_ROOT/conformance/goldens" \
    record --scenarios /in/scenarios --goldens /in/goldens --jobs 12 --report /out/record-report.json "${args[@]}"
  local rc=$?
  mg_artifact report "$MG_RUN_DIR/record-report.json" "goldens re-recorded"
  mg_detail recorded "$(printf '%s' "$ids" | jq -R . | jq -sc .)"
  if [ "$rc" -ne 0 ]; then
    jq -c '.scenarios[]? | select(.status != "recorded")
      | {id: ("record:" + .scenario), message: ("not recorded: " + (.error | tostring)),
         repro: ("tools/mg oracle record " + (.scenario | @sh))}' "$MG_RUN_DIR/record-report.json" \
      >>"$MG_STATE/failures.jsonl" 2>/dev/null
    return 1
  fi
}

# T1 (dsl.md D10): live oracle workers replay the selected scenarios against
# their goldens (any difference or error fails, active or not), and live
# scenarios run as Meguri/oracle chains through a handoff directory the
# workers mount at /handoff.
mg_conf__live_replay() {
  local jobs="$1" live="$MG_RUN_DIR/t1-live.json" handoff="$MG_RUN_DIR/handoff" cmd rc
  shift
  if [ -z "${MG_ORACLE_TAG:-}" ]; then
    mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 1
  fi
  mkdir -p "$handoff"
  cmd="$(jq -nc --arg p "$(mg_compose_project)" --arg f "$MG_ROOT/docker/compose.yml" --arg l "meguri.mg.run-id=$MG_RUN_ID" \
    --arg h "$handoff:/handoff" \
    '["docker", "compose", "-p", $p, "-f", $f, "run", "--rm", "-T", "--no-deps", "--label", $l, "-e", "TZ={tz}",
      "-v", $h, "-e", "ORACLE_HANDOFF=/handoff", "oracle"]')"
  MEGURI_ORACLE_TAG="$MG_ORACLE_TAG" MEGURI_ORACLE_CONTEXT="$MG_ORACLE_DIR" \
    mg_conf__cli_native conformance run --live --conformance "$MG_ROOT/conformance" --lane live-oracle \
    --oracle-cmd "$cmd" --jobs "$jobs" --handoff-dir "$handoff" --report "$live" "$@"
  rc=$?
  mg_conf__lane_report live "$live" "$rc" || return 1
  mg_detail oracle "$(jq -c '.oracle // null' "$live")"
  [ "$rc" -eq 0 ]
}

# The oracle lock file this checkout builds from: its own, or the main
# worktree's (implementer worktrees have no oracle/). Prints nothing if none.
mg_conf__oracle_file() {
  local main
  if [ -f "$MG_ROOT/oracle/$1" ]; then
    printf '%s\n' "$MG_ROOT/oracle/$1"
    return 0
  fi
  main="$(git -C "$MG_ROOT" worktree list --porcelain 2>/dev/null | sed -n '1s/^worktree //p')"
  [ -n "$main" ] && [ -f "$main/oracle/$1" ] && printf '%s\n' "$main/oracle/$1"
  return 0
}

mg_conf__sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum <"$1" | cut -d' ' -f1; else shasum -a 256 <"$1" | cut -d' ' -f1; fi
}

# tools/mg goldens-check [--base <rev>]
mg_goldens_check() {
  local rev=HEAD
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --base) rev="${2:-}"; shift 2 || { mg_conf__usage "tools/mg goldens-check [--base <rev>]" "--base needs a revision"; return 0; } ;;
      *) mg_conf__usage "tools/mg goldens-check [--base <rev>]" "unexpected argument '$1'"; return 0 ;;
    esac
  done
  if ! git -C "$MG_ROOT" rev-parse --verify -q "$rev^{commit}" >/dev/null; then
    mg_conf__usage "tools/mg goldens-check [--base <rev>]" "'$rev' is not a revision"
    return 0
  fi
  mg_conf__docker_ok || return 0
  trap 'mg_conf__cleanup; exit 143' TERM
  local base="$MG_RUN_DIR/base" report="$MG_RUN_DIR/goldens-check.json" lock lock_args="" rc schema_rc
  mkdir -p "$base"
  if git -C "$MG_ROOT" cat-file -e "$rev:conformance/goldens" 2>/dev/null; then
    git -C "$MG_ROOT" archive "$rev" conformance/goldens | tar -xf - -C "$base"
  fi
  mkdir -p "$base/conformance/goldens"
  lock="$(mg_conf__oracle_file requirements.lock)"
  if [ -n "$lock" ]; then
    lock_args="--oracle-lock-sha256 $(mg_conf__sha256 "$lock")"
  else
    echo "goldens-check: no oracle/requirements.lock found; the pin is not compared with it" >&2
  fi
  mg_conf__dev -- "cd core && cargo run -q -p meguri-cli -- conformance goldens-check --conformance /work/conformance \
    --base /work/$(mg_conf__rel "$base")/conformance/goldens $lock_args --report /work/$(mg_conf__rel "$report")"
  rc=$?
  mg_conf__dev -- "cd core && cargo build -q -p meguri-cli && cd .. && python3 -B -I tools/conformance/check_schemas.py \
    --cli /cargo-target/debug/meguri-cli --conformance conformance --report /work/$(mg_conf__rel "$MG_RUN_DIR")/schemas.json"
  schema_rc=$?
  if [ ! -s "$report" ]; then
    mg_failure no-report "goldens-check exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$report" "freshness report"
  jq -c '.problems[] | {id: (.kind + ":" + .scenario), message: .message, repro: .repro}' "$report" \
    >>"$MG_STATE/failures.jsonl"
  if [ -s "$MG_RUN_DIR/schemas.json" ]; then
    mg_artifact report "$MG_RUN_DIR/schemas.json" "schema validation"
    jq -c '.errors[] | {id: ("schema:" + .file), message: .message, repro: "tools/mg goldens-check"}' \
      "$MG_RUN_DIR/schemas.json" >>"$MG_STATE/failures.jsonl"
  elif [ "$schema_rc" -ne 0 ]; then
    mg_failure schema-check "the schema check exited with code $schema_rc without a report; see log.txt"
  fi
  mg_detail changed "$(jq -c '{base: $rev, by_area_and_kind: (.changed | map(.values |= .[:20])),
    goldens: .changed_goldens, added: .added_goldens, removed: .removed_goldens}' --arg rev "$rev" "$report")"
  local problems scenarios changed
  problems="$(jq '.problems | length' "$report")"
  scenarios="$(jq '.scenarios' "$report")"
  changed="$(jq '.changed_goldens | length' "$report")"
  mg_count total "$scenarios"
  mg_count failed "$problems"
  mg_count passed $((scenarios - problems > 0 ? scenarios - problems : 0))
  mg_count changed "$changed"
  if [ -s "$MG_STATE/failures.jsonl" ]; then
    mg_message "goldens-check: $problems freshness problems, $(jq '.errors | length' "$MG_RUN_DIR/schemas.json" 2>/dev/null || echo '?') schema errors; $changed goldens changed since $rev"
    mg_status fail
  else
    mg_message "goldens-check: $scenarios scenarios fresh, schemas valid; $changed goldens changed since $rev"
    mg_status pass
  fi
}

# tools/mg harvest-report [--live-report <report.json>]
mg_harvest_report() {
  local live_report="" live_args=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --live-report) live_report="${2:-}"; shift 2 || { mg_conf__usage "tools/mg harvest-report [--live-report <file>]" "--live-report needs a file"; return 0; } ;;
      *) mg_conf__usage "tools/mg harvest-report [--live-report <file>]" "unexpected argument '$1'"; return 0 ;;
    esac
  done
  if [ -n "$live_report" ]; then
    # Resolved on the host: `latest` links do not exist inside the container.
    if [ -f "$live_report" ]; then
      live_report="$(cd "$(dirname "$live_report")" && pwd -P)/$(basename "$live_report")"
    fi
    case "$live_report" in /*) ;; *) live_report="$PWD/$live_report" ;; esac
    case "$live_report" in
      "$MG_ROOT"/*) [ -f "$live_report" ] || { mg_conf__usage "tools/mg harvest-report --live-report <file>" "no such file '$live_report'"; return 0; } ;;
      *) mg_conf__usage "tools/mg harvest-report --live-report <file>" "the report must be inside the repository (build/mg/conformance/...)"; return 0 ;;
    esac
    live_args=" --live-report /work/$(mg_conf__rel "$live_report")"
  fi
  mg_conf__docker_ok || return 0
  trap 'mg_conf__cleanup; exit 143' TERM
  local json="$MG_RUN_DIR/harvest-report.json" md="$MG_RUN_DIR/harvest-report.md" rc links check_rc=0
  mg_conf__dev -- "cd core && cargo run -q -p meguri-cli -- conformance harvest-report --conformance /work/conformance$live_args \
    --report /work/$(mg_conf__rel "$json") --markdown /work/$(mg_conf__rel "$md")"
  rc=$?
  if [ ! -s "$json" ]; then
    mg_failure no-report "harvest-report exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$md" "harvest coverage (markdown)"
  mg_artifact report "$json" "harvest coverage"
  jq -c '.problems[] | {id: "catalog", message: ., repro: "tools/mg harvest-report"}' "$json" >>"$MG_STATE/failures.jsonl"
  # The leak check reads oracle/harvest/links.csv (spec side): a black box to
  # implementers, run from the main worktree's oracle/ when this one has none.
  # It also fails when links.csv was inventoried at another release than the
  # oracle pin (R1882: a bump re-inventories in the same change).
  links="$(mg_conf__oracle_file harvest/links.csv)"
  if [ -n "$links" ]; then
    local dir
    dir="$(dirname "$links")"
    mg_conf__dev -v "$dir:/oracle-harvest:ro" -- \
      "python3 -B -I /oracle-harvest/inventory.py check --links /oracle-harvest/links.csv --catalog conformance/catalog/harvest.csv \
        --pin conformance/oracle.json"
    check_rc=$?
    [ "$check_rc" -eq 0 ] || mg_failure leak-check \
      "conformance/catalog/harvest.csv names Anki files or tests, disagrees with oracle/harvest/links.csv, or the inventory is not at the oracle pin; see log.txt" \
      "tools/mg harvest-inventory   (spec side), then tools/mg harvest-report"
    mg_detail leak_check "$([ "$check_rc" -eq 0 ] && echo '"pass"' || echo '"fail"')"
  else
    echo "harvest-report: no oracle/harvest/links.csv here or in the main worktree; leak check skipped" >&2
    mg_detail leak_check '"skipped"'
  fi
  mg_detail harvest "$(jq -c '{anki_version, rows_total, rows_at_tag, by_language, supplementary, by_classification}' "$json")"
  local total unclassified
  total="$(jq '.rows_total' "$json")"
  unclassified="$(jq '.by_classification.unclassified.rows // 0' "$json")"
  mg_count total "$total"
  mg_count passed $((total - unclassified))
  mg_count unclassified "$unclassified"
  mg_count failed "$(jq '.problems | length' "$json")"
  local by_lang at_tag supplementary
  by_lang="$(jq -r '.by_language | to_entries | map("\(.key) \(.value | add)") | join(", ")' "$json")"
  at_tag="$(jq '.rows_at_tag' "$json")"
  supplementary="$(jq '.supplementary | add // 0' "$json")"
  if [ -s "$MG_STATE/failures.jsonl" ] || [ "$rc" -ne 0 ]; then
    mg_message "harvest-report: $at_tag tests at anki $(jq -r .anki_version "$json") ($by_lang) and $supplementary supplementary, $unclassified unclassified; problems found"
    mg_status fail
  else
    mg_message "harvest-report: $at_tag tests at anki $(jq -r .anki_version "$json") ($by_lang) and $supplementary supplementary; $((total - unclassified)) classified, $unclassified unclassified"
    mg_status pass
  fi
}

# tools/mg harvest-inventory [--supplementary] [--test]
mg_harvest_inventory() {
  local test_only=false supplementary=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --test) test_only=true; shift ;;
      --supplementary) supplementary=true; shift ;;
      *) mg_conf__usage "tools/mg harvest-inventory [--supplementary] [--test]" "unexpected argument '$1'"; return 0 ;;
    esac
  done
  mg_oracle__own_tree "harvest-inventory rewrites oracle/harvest/links.csv" || return 0
  mg_conf__docker_ok || return 0
  trap 'mg_conf__cleanup; exit 143' TERM
  local tests="python3 -B -I -m unittest discover -s oracle/harvest/tests"
  if [ "$test_only" = true ]; then
    if mg_conf__dev -- "$tests"; then
      mg_message "harvest-inventory: inventory tests passed"
      mg_status pass
    else
      mg_failure inventory-tests "oracle/harvest/tests failed; see log.txt" "tools/mg harvest-inventory --test"
      mg_status fail
    fi
    return 0
  fi
  local version tag main anki main_anki="" extra="" mounts=""
  version="$(sed -n 's/^anki==\([0-9.]*\).*/\1/p' oracle/requirements.in | head -n 1)"
  tag="$(printf '%s' "$version" | awk -F. '{ printf "%d.%02d.%d", $1, $2, $3 }')"
  main="$(git -C "$MG_ROOT" worktree list --porcelain 2>/dev/null | sed -n '1s/^worktree //p')"
  anki="${main:-$MG_ROOT}/refs/anki-$tag"
  if [ ! -d "$anki" ] || [ "$(git -C "$anki" describe --tags --exact-match 2>/dev/null)" != "$tag" ]; then
    mg_failure no-anki-checkout "no Anki checkout at tag $tag in $anki" \
      "git -C refs/anki worktree add ../anki-$tag $tag   (in the main checkout)"
    mg_status error
    return 0
  fi
  if [ "$supplementary" = true ]; then
    main_anki="${main:-$MG_ROOT}/refs/anki"
    local rev
    rev="$(git -C "$main_anki" rev-parse --short=12 HEAD 2>/dev/null)"
    if [ -z "$rev" ] || ! git -C "$main_anki" merge-base --is-ancestor "$tag" HEAD 2>/dev/null; then
      mg_failure no-anki-main "no Anki checkout of main (after $tag) in $main_anki" \
        "git -C refs/anki fetch origin main && git -C refs/anki checkout --detach origin/main   (in the main checkout)"
      mg_status error
      return 0
    fi
    mounts="-v $main_anki:/anki-main:ro"
    extra=" --supplementary /anki-main --supplementary-tag main@$rev"
  fi
  # shellcheck disable=SC2086 # mounts is empty or one -v option
  if mg_conf__dev -v "$anki:/anki:ro" $mounts -- "$tests >&2 && python3 -B -I oracle/harvest/inventory.py inventory --anki /anki --tag $tag$extra \
      --links oracle/harvest/links.csv --catalog conformance/catalog/harvest.csv && python3 -B -I oracle/harvest/inventory.py check \
      --links oracle/harvest/links.csv --catalog conformance/catalog/harvest.csv --pin conformance/oracle.json"; then
    mg_artifact csv "$MG_ROOT/oracle/harvest/links.csv" "links (spec side)"
    mg_artifact csv "$MG_ROOT/conformance/catalog/harvest.csv" "catalog"
    mg_message "harvest-inventory: inventoried Anki $tag$([ "$supplementary" = true ] && echo " and main's supplementary tests"); review and commit oracle/harvest/links.csv and conformance/catalog/harvest.csv together"
    mg_status pass
  else
    mg_failure inventory "the inventory, its tests or its check failed; see log.txt" "tools/mg harvest-inventory"
    mg_status fail
  fi
}
