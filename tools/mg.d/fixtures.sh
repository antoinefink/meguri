# Synthetic collections (U11: R1804, R1805, R1963). Sourced by tools/mg; the
# job runs in the dev container.
#
# Builds meguri-cli with the `fixtures` cargo profile and runs `meguri-cli
# generate` into build/fixtures/:
#   <size> and worst cases  <user-shape sha256, 16 hex>-<seed>-<size or name>/
#   recipes                 recipes/<name>-<seed>/
# each holding collection.anki2, collection.media/ and manifest.json. A
# directory whose manifest matches the request is reused (`cached`). Every
# run must stay within the R1804 budget for 1M cards: 10 minutes and 2 GB
# peak memory.

mg_job fixtures docker:dev mg_fixtures \
  "Synthetic collections into build/fixtures: <size> (10k 100k 150k 500k 1m, or a card count) | <worst case> | <recipe> | worst-cases | recipes ... [--seed N] [--media files] [--cards N (worst cases)]"

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
    [ "$media" != files ] || [ "$kind" != size ] || dir="$dir-files"
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
