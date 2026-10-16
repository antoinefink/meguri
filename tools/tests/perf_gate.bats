#!/usr/bin/env bats
# Self-tests for `tools/mg perf-gate` (U241) in a sandbox repository holding
# tools/, the real budget catalog, the plan, the interval registry, the UI
# performance tests and milestones.yaml: the catalog check, which perf results
# a run reads (this commit's, the latest per suite and runner), the R1866
# refusals, --require, rebaselining and the device-run report. The
# evaluator's own tests (tools/scripts/gates/tests) run in milestone_gate.bats.

bats_require_minimum_version 1.5.0
# bats file_tags=gate

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
PLAN=docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md
REGISTRY=app/Packages/MeguriCore/Sources/Instrumentation/IntervalNames.swift

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/docs/process" "$t/docs/plans" "$(dirname "$t/$REGISTRY")" "$t/app/MeguriUITests"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp "$REPO_ROOT/.gitignore" "$t/"
  cp "$REPO_ROOT/docs/process/milestones.yaml" "$t/docs/process/"
  cp "$REPO_ROOT/$PLAN" "$t/$PLAN"
  cp "$REPO_ROOT/$REGISTRY" "$t/$REGISTRY"
  cp -R "$REPO_ROOT/app/MeguriUITests/Performance" "$t/app/MeguriUITests/Performance"
  rm -rf "$t/tools/perf/baselines"
  git -C "$t" init -q -b main
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  git -C "$SB" config user.name selftest
  git -C "$SB" config user.email selftest@localhost
  SEQ=0
}

teardown() {
  [ -z "${result:-}" ] || [ ! -f "$result" ] ||
    jq -c '{status, message, failures: [.failures[] | {id, message: (.message // "" | .[0:300])}]}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 20
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }
validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }

# perf_result <job> <perf result JSON> [sha] [dirty]: a finished run of <job>
# on this commit (or <sha>) whose one perf-result artifact is the given JSON.
perf_result() {
  local job="$1" doc="$2" sha="${3:-$(git -C "$SB" rev-parse HEAD)}" dirty="${4:-false}" run path
  SEQ=$((SEQ + 1))
  run="$SB/build/mg/$job/20261016T20000${SEQ}Z-$SEQ"
  mkdir -p "$run/results"
  path="$run/results/$(jq -r .suite <<<"$doc").json"
  printf '%s\n' "$doc" >"$path"
  jq -n --arg job "$job" --arg run "$(basename "$run")" --arg sha "$sha" --argjson dirty "$dirty" --arg p "$path" \
    --arg t "2026-10-16T20:00:0${SEQ}Z" \
    '{job: $job, run_id: $run, status: "pass", dry_run: false, finished_at: $t, commit: {sha: $sha, dirty: $dirty},
      artifacts: [{kind: "perf-result", path: $p}]}' >"$run/result.json"
}

SIM='{"kind": "simulator", "id": "vm/meguri-ui/iPhone 17 Pro/24A434", "model": "iPhone 17 Pro", "os": "iOS 27.0", "os_build": "24A434"}'
IPHONE='{"kind": "device", "id": "iPhone18,1/24A434", "model": "iPhone 17 Pro", "os": "iOS 27.0"}'

# smoke <p50 ms> [configuration] [attachments]: SmokePerfTests' probe (the
# catalog judges it at p50).
smoke() {
  jq -nc --argjson v "$1" --arg c "${2:-Profile}" --argjson a "${3:-0}" --argjson r "$SIM" '{schema_version: 1,
    suite: "SmokePerfTests", kind: "ui", configuration: $c, attachments: $a, runner: $r, results: [
      {name: "probe.syntheticDelay", test: "SmokePerfTests/testProbeIntervalMeasuresItsSyntheticDelay()",
       metric: "duration", unit: "ms", percentile: "p50", value: $v, samples: 5}]}'
}

open_bench() {
  jq -nc --argjson v "$1" --arg f "${2:-150k}" '{schema_version: 1, suite: "open", kind: "wall-clock",
    configuration: "bench", runner: {kind: "mac", id: "Apple M3 Max/macOS 27.0", cpu: "Apple M3 Max"}, results: [
      {name: "collection.open", metric: "duration", unit: "ms", percentile: "p95", value: $v, samples: 100, fixture: $f}]}'
}

@test "perf-gate --check passes on the real catalog" {
  mg perf-gate --check
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  validate "$result"
}

@test "perf-gate --check fails when a budget has no entry or a milestone group is not a catalog group" {
  python3 - "$SB/tools/perf/budgets.yaml" <<'PY'
import sys, yaml
p = sys.argv[1]
data = yaml.safe_load(open(p))
data["budgets"] = [b for b in data["budgets"] if b["id"] != "R1809"]
yaml.safe_dump(data, open(p, "w"), sort_keys=False)
PY
  sed 's/items: \[launch, review, open, sync\]/items: [launch, review, open, nowhere]/' \
    "$SB/docs/process/milestones.yaml" >"$SB/m.yaml" && mv "$SB/m.yaml" "$SB/docs/process/milestones.yaml"
  mg perf-gate --check
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].message | select(contains("R1809 has no catalog entry"))] | length')" -eq 1 ]
  [ "$(field '[.failures[].message | select(contains("nowhere"))] | length')" -ge 1 ]
  validate "$result"
}

@test "perf-gate judges this commit's results only: the latest per suite and runner" {
  perf_result perf-ui "$(smoke 30)" 0000000000000000000000000000000000000001
  perf_result perf-ui "$(smoke 31)" "" true
  perf_result perf-ui "$(smoke 33)"
  perf_result bench-mac "$(open_bench 120)"
  perf_result perf-ui "$(smoke 34)"
  mg perf-gate
  [ "$status" -eq 0 ]
  # The probe has no baseline yet: unchecked, so the run warns.
  [ "$(field .status)" = warn ]
  [ "$(field '.details.inputs | length')" -eq 2 ]
  [ "$(field '.details.groups.open.status')" = pass ]
  [ "$(field '.details.groups.smoke.status')" = unchecked ]
  [ "$(field '.details.groups.launch.status')" = pending ]
  [ "$(field '.details.budgets.R1823')" = pass ]
  [ "$(jq -r '[.evaluations[] | select(.name == "probe.syntheticDelay") | .raw] | join(",")' "$(field '.artifacts[] | select(.path | endswith("evaluation.json")) | .path')")" = 34 ]
  validate "$result"
}

@test "perf-gate fails on a breach and names the budget" {
  perf_result bench-mac "$(open_bench 160)"
  mg perf-gate
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = budget/R1823/core ]
  [[ "$(field '.failures[0].message')" == *"R1823 collection.open p95 160 ms > 150 ms"* ]]
  [ "$(field '.details.groups.open.status')" = fail ]
  validate "$result"
}

@test "perf-gate refuses a Debug result or one with attachments before judging it" {
  perf_result perf-ui "$(smoke 30 Debug)"
  mg perf-gate
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = refused/SmokePerfTests ]
  [[ "$(field '.failures[0].message')" == *"built in Debug"*R1866* ]]
  [ "$(field .summary.total)" -eq 0 ]
  rm -rf "$SB/build/mg/perf-ui"
  perf_result perf-ui "$(smoke 30 Profile 1)"
  mg perf-gate
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[0].message')" == *"kept 1 attachments"* ]]
}

@test "perf-gate --require fails when a required job has no results on this commit" {
  perf_result perf-ui "$(smoke 30)"
  mg perf-gate --require perf-ui --require bench-mac
  [ "$status" -eq 1 ]
  [ "$(field '.failures[] | select(.id == "require/bench-mac") | .id')" = require/bench-mac ]
  [ "$(field '.details.require | join(" ")')" = "perf-ui bench-mac" ]
}

@test "a rebaseline needs a clean tree and a reason, then the +10% rule applies" {
  perf_result perf-ui "$(smoke 40)"
  mg perf-gate --rebaseline
  [ "$status" -eq 2 ]
  [[ "$(field .message)" == *"--rebaseline needs --reason"* ]]
  printf 'x\n' >"$SB/untracked.txt"
  mg perf-gate --rebaseline --reason "first VM simulator baseline"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = dirty-tree ]
  rm "$SB/untracked.txt"
  mg perf-gate --rebaseline --reason "first VM simulator baseline"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  local file="$SB/tools/perf/baselines/vm-meguri-ui-iphone-17-pro-24a434/SmokePerfTests.json"
  [ -f "$file" ]
  [ "$(jq -r .reason "$file")" = "first VM simulator baseline" ]
  [ "$(jq -r .measured_commit "$file")" = "$(git -C "$SB" rev-parse HEAD)" ]
  [ "$(jq -r '.rows[0].value' "$file")" = 40 ]
  git -C "$SB" add tools/perf/baselines
  git -C "$SB" commit -qm "Rebaseline SmokePerfTests on the VM simulator"
  perf_result perf-ui "$(smoke 43.2)"
  mg perf-gate
  [ "$(field .status)" = pass ]
  perf_result perf-ui "$(smoke 44.4)"
  mg perf-gate
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[0].message')" == *"regression +11% > 10%"* ]]
}

@test "a baseline without a reason is an error" {
  mkdir -p "$SB/tools/perf/baselines/x"
  printf '{"schema_version": 1, "runner": {"id": "x"}, "suite": "S", "kind": "ui", "measured_commit": "%s", "reason": "", "rows": []}\n' \
    "$(git -C "$SB" rev-parse HEAD)" >"$SB/tools/perf/baselines/x/S.json"
  mg perf-gate
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [[ "$(field '.failures[0].message')" == *"no reason"* ]]
}

@test "--device-run writes U256's report from device results" {
  local dir="$SB/build/device-results"
  mkdir -p "$dir"
  jq -nc --argjson r "$IPHONE" '{schema_version: 1, suite: "LaunchHomePerfTests", kind: "ui", configuration: "Profile",
    attachments: 0, runner: $r, results: [
      {name: "xctest.application-launch", metric: "duration", unit: "ms", percentile: "p95", value: 540, samples: 5}]}' \
    >"$dir/LaunchHomePerfTests.json"
  mg perf-gate --results build/device-results --device-run M1 --device iphone
  [ "$status" -eq 1 ]
  local report
  report="$(field '.artifacts[] | select(.path | endswith("device-run.json")) | .path')"
  [ -f "$report" ]
  [ "$(jq -r .milestone "$report")" = M1 ]
  [ "$(jq -r .configuration "$report")" = Profile ]
  [ "$(jq -r '.budgets[] | select(.id == "R1818") | .status' "$report")" = fail ]
  [[ "$(field '.failures[0].message')" == *"R1818 xctest.application-launch p95 540 ms > 500 ms"* ]]
  [ "$(field '.details.device_run.runner')" = "iPhone18,1/24A434" ]
  validate "$result"
}

@test "perf-gate refuses arguments that do not go together" {
  mg perf-gate --device-run M1 --device iphone
  [ "$status" -eq 2 ]
  mg perf-gate --check --require perf-ui
  [ "$status" -eq 2 ]
  mg perf-gate --require nightly
  [ "$status" -eq 2 ]
  mg perf-gate --results build/nowhere
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
}
