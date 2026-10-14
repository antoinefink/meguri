# cargo-fuzz jobs. Sourced by tools/mg; they run inside the `fuzz` container
# (docker/dev.Dockerfile stage `fuzz`: the dev image plus a pinned nightly and
# cargo-fuzz, whose toolchain is $MG_FUZZ_TOOLCHAIN).

mg_job core-fuzz docker:fuzz mg_core_fuzz \
  "cargo-fuzz targets in core/fuzz/fuzz_targets, seeded from the text corpus: [--seconds N] (per target, default 600) [target...]"

MG_CORE_FUZZ_SECONDS=600

# mg__fuzz_targets: the target names, one per line.
mg__fuzz_targets() {
  local f
  for f in core/fuzz/fuzz_targets/*.rs; do
    [ -f "$f" ] || continue
    f="${f##*/}"
    printf '%s\n' "${f%.rs}"
  done
}

# mg__fuzz_seed <dir>: one seed file per distinct string input of the
# generated text corpus (conformance/scenarios/text/corpus/); prints the count.
mg__fuzz_seed() {
  local files=() f
  for f in conformance/scenarios/text/corpus/*.json; do
    [ -f "$f" ] && files[${#files[@]}]="$f"
  done
  python3 -I - "$1" ${files[0]+"${files[@]}"} <<'PY'
import hashlib
import json
import os
import sys

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
seeds = set()
for path in sys.argv[2:]:
    with open(path, encoding="utf-8") as f:
        scenario = json.load(f)
    for step in scenario.get("steps", []):
        for request in step.get("observe", []):
            if not isinstance(request, dict) or "text" not in request:
                continue
            for item in request["text"]["inputs"]:
                if isinstance(item, str):
                    seeds.add(item.encode("utf-8"))
for data in seeds:
    with open(os.path.join(out, hashlib.sha1(data).hexdigest()), "wb") as f:
        f.write(data)
print(len(seeds))
PY
}

# tools/mg core-fuzz [--seconds N] [target...]: each target runs for N
# seconds (libFuzzer -max_total_time) from the seeds and fails on any crash,
# panic, timeout or out-of-memory input it finds. Findings are saved under the
# run directory; the target's log has the stack trace.
mg_core_fuzz() {
  local seconds="$MG_CORE_FUZZ_SECONDS" usage="usage: tools/mg core-fuzz [--seconds N] [target...]"
  local targets=() known t
  known="$(mg__fuzz_targets)"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --) shift ;;
      --seconds)
        if [ "$#" -lt 2 ]; then
          mg_failure usage "--seconds needs a number; $usage" "tools/mg core-fuzz"
          mg_status error
          return 0
        fi
        seconds="$2"
        shift 2
        ;;
      -*)
        mg_failure usage "unknown option '$1'; $usage" "tools/mg core-fuzz"
        mg_status error
        return 0
        ;;
      *)
        if ! printf '%s\n' "$known" | grep -qxF -- "$1"; then
          mg_failure usage "unknown target '$1'; targets: $(printf '%s' "$known" | tr '\n' ' ')" "tools/mg core-fuzz"
          mg_status error
          return 0
        fi
        targets[${#targets[@]}]="$1"
        shift
        ;;
    esac
  done
  # A positive whole number of seconds (libFuzzer reads 0 as "no limit"), at
  # most seven digits so shell arithmetic cannot overflow.
  local digits="${seconds#"${seconds%%[!0]*}"}"
  case "$seconds" in
    '' | *[!0-9]*) digits="" ;;
  esac
  if [ -z "$digits" ] || [ "${#digits}" -gt 7 ]; then
    mg_failure usage "--seconds takes a positive whole number of seconds; $usage" "tools/mg core-fuzz"
    mg_status error
    return 0
  fi
  seconds="$digits"
  if [ "${#targets[@]}" -eq 0 ]; then
    while IFS= read -r t; do
      [ -n "$t" ] && targets[${#targets[@]}]="$t"
    done <<EOF
$known
EOF
  fi
  if [ "${#targets[@]}" -eq 0 ]; then
    mg_failure no-targets "core/fuzz/fuzz_targets has no target"
    mg_status fail
    return 0
  fi
  if [ -z "${MG_FUZZ_TOOLCHAIN:-}" ]; then
    mg_failure fuzz-toolchain "MG_FUZZ_TOOLCHAIN is unset; the fuzz image sets it (docker/dev.Dockerfile)"
    mg_status error
    return 0
  fi
  mg_detail environment "$(jq -nc --arg toolchain "$MG_FUZZ_TOOLCHAIN" \
    --arg cargo_fuzz "$(cargo "+$MG_FUZZ_TOOLCHAIN" fuzz --version 2>&1 | head -n 1)" --arg os "$(uname -sm)" \
    '{toolchain: $toolchain, cargo_fuzz: $cargo_fuzz, os: $os}')"

  # Release build with debug assertions, so overflow and other debug checks
  # count as findings.
  local flags=(-O -a)
  if ! (cd core && cargo "+$MG_FUZZ_TOOLCHAIN" fuzz build "${flags[@]}"); then
    mg_failure fuzz-build "building the fuzz targets failed; see log.txt"
    mg_count total "${#targets[@]}"
    mg_count failed "${#targets[@]}"
    mg_message "core-fuzz: the targets did not build"
    mg_status fail
    return 0
  fi

  local seeds="${TMPDIR:-/tmp}/core-fuzz-seeds" n
  rm -rf "$seeds"
  n="$(mg__fuzz_seed "$seeds")" || {
    mg_failure fuzz-seed "writing the seed corpus failed; see log.txt"
    mg_status error
    return 0
  }

  local passed=0 failed=0 stats="[]" target log work found rc runs
  for target in "${targets[@]}"; do
    log="$MG_RUN_DIR/$target.log"
    work="${TMPDIR:-/tmp}/core-fuzz-corpus/$target"
    found="$MG_RUN_DIR/findings/$target"
    rm -rf "$work"
    mkdir -p "$work" "$found"
    echo "core-fuzz: $target for ${seconds}s from $n seeds" >&2
    (
      cd core &&
        cargo "+$MG_FUZZ_TOOLCHAIN" fuzz run "${flags[@]}" "$target" "$work" "$seeds" -- \
          -max_total_time="$seconds" -artifact_prefix="$found/" -print_final_stats=1
    ) >"$log" 2>&1
    rc=$?
    mg_artifact log "$log" "$target"
    runs="$(sed -n 's/^stat::number_of_executed_units: *//p' "$log" | tail -n 1)"
    stats="$(jq -c --arg t "$target" --arg r "${runs:-0}" --argjson rc "$rc" \
      '. + [{target: $t, runs: ($r | tonumber), exit_code: $rc}]' <<<"$stats")"
    if [ -n "$(ls -A "$found")" ]; then
      for t in "$found"/*; do
        mg_artifact finding "$t" "$target"
      done
      mg_failure "fuzz/$target" "$target found $(ls "$found" | wc -l | tr -d ' ') failing input(s) under ${found#"$MG_ROOT"/}; stack trace in ${log#"$MG_ROOT"/}" \
        "tools/mg core-fuzz --seconds $seconds $target"
      failed=$((failed + 1))
    elif [ "$rc" -ne 0 ]; then
      mg_failure "fuzz/$target-exit-$rc" "cargo fuzz run $target exited with code $rc without a finding; see ${log#"$MG_ROOT"/}" \
        "tools/mg core-fuzz --seconds $seconds $target"
      failed=$((failed + 1))
    else
      rmdir "$found"
      passed=$((passed + 1))
    fi
  done
  rmdir "$MG_RUN_DIR/findings" 2>/dev/null
  mg_detail targets "$stats"
  mg_count total "${#targets[@]}"
  mg_count passed "$passed"
  mg_count failed "$failed"
  if [ "$failed" -eq 0 ]; then
    mg_message "core-fuzz: ${#targets[@]} target(s) ran ${seconds}s each without a finding ($(jq -r 'map("\(.target) \(.runs) runs") | join(", ")' <<<"$stats"))"
    mg_status pass
  else
    mg_message "core-fuzz: $failed of ${#targets[@]} target(s) failed"
    mg_status fail
  fi
}
