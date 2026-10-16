#!/usr/bin/env bats
# Self-tests for the milestone gate (U240): the evaluator's own tests
# (tools/scripts/gates/tests), the real manifest against the real tiers.yaml,
# job registry and plan, and `tools/mg milestone-gate` in a sandbox
# repository with a made-up manifest, a fake suite job and a fake doctor.

bats_require_minimum_version 1.5.0
# bats file_tags=gate

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template" f m i
  mkdir -p "$t/docs/process" "$t/conformance"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp "$REPO_ROOT/.gitignore" "$t/"
  for f in "$t"/tools/mg.d/*.sh; do
    case "$(basename "$f")" in
      gate.sh | milestone-gate.sh | perf-gate.sh) ;;
      *) rm -f "$f" ;;
    esac
  done
  cat >"$t/tools/gate/tiers.yaml" <<'EOF'
version: 1
tiers:
  T0: {when: every change}
  T1: {when: core changes}
  T2: {when: nightly}
  T3: {when: milestones}
  T4: {when: milestones, on devices}
runs:
  - {id: unit, job: t-job, args: [unit], tier: T0, placement: host, class: unit, budget_s: 60}
  - {id: milestones, job: milestone-gate, args: [--check], tier: T0, placement: host, class: static, budget_s: 60}
utilities:
  - {job: gate, why: runs the tiers}
  - {job: static-checks, why: static checks}
  - {job: doctor, why: reads host state}
  - {job: perf-gate, why: "milestone-gate loads its budget catalog for device runs"}
EOF
  printf 'entries:\n' >"$t/tools/gate/quarantine.yaml"
  cat >"$t/tools/mg.d/zz-fakes.sh" <<'EOF'
mg_job t-job host t_job "fake suite: t-job <name>; its status is \$FX/<name>.status (default pass)"
mg_job doctor host t_doctor "fake doctor: lists the user steps in \$FX/doctor.steps (id state title per line)"
t_job() {
  local status=pass
  [ ! -f "$FX/$1.status" ] || status="$(cat "$FX/$1.status")"
  [ "$status" = pass ] || [ "$status" = warn ] || mg_failure "$1-$status" "made-up $status"
  mg_message "t-job $1: $status"
  mg_status "$status"
}
t_doctor() {
  local id state title
  if [ -f "$FX/doctor.steps" ]; then
    while read -r id state title; do mg_user_step "$id" "$state" true "$title" "do it"; done <"$FX/doctor.steps"
  fi
  mg_message "doctor (fake)"
  mg_status pass
}
EOF
  {
    printf '# Plan\n\n### Delivery Milestones\n\n| Milestone | Units | Closing gate | User steps |\n|---|---|---|---|\n'
    # Only M0 has closing-gate and user-steps text: its suite and user step
    # claim it with their plan excerpts.
    printf '| **M0 Stage M0** | U1 | Gate of M0. | Steps of M0 (R1969) |\n'
    i=2
    for m in M1 M2 M3 M4 M5; do
      printf '| **%s Stage %s** | U%d |  |  |\n' "$m" "$m" "$i"
      i=$((i + 1))
    done
    printf '\n### Unit Index\n\n| U-ID | Title |\n|---|---|\n'
    for i in 1 2 3 4 5 6; do printf '| U%d | unit %d |\n' "$i" "$i"; done
  } >"$t/docs/plan.md"
  {
    printf 'version: 1\nplan: docs/plan.md\nevidence_paths: [docs/process/milestone-records/]\n'
    printf 'global:\n  - {id: quarantine, kind: quarantine}\nmilestones:\n'
    i=1
    for m in M0 M1 M2 M3 M4 M5; do
      if [ "$m" = M0 ]; then
        printf '  - id: M0\n    name: Stage M0\n    plan: {gate: "Gate of M0.", user_steps: "Steps of M0 (R1969)"}\n    units: [U1]\n'
        printf '    suites:\n      - {id: unit, run: unit, plan: [Gate of M0]}\n      - {id: milestones, run: milestones}\n'
        printf '    user_steps:\n      - {id: xcode-license, description: Accept the license, evidence: build/mg/doctor/, validator: {type: setup, step: xcode-license}, plan: ["Steps of M0 (R1969)"]}\n'
      else
        printf '  - id: %s\n    name: Stage %s\n    plan: {gate: "", user_steps: ""}\n    units: [U%d]\n' "$m" "$m" "$i"
      fi
      i=$((i + 1))
    done
  } >"$t/docs/process/milestones.yaml"
  git -C "$t" init -q -b trunk
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -q --allow-empty \
    -m $'Add U1\n\nUnit: U1\nRole: implementer'
}

setup() {
  SB="$BATS_TEST_TMPDIR/main"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  git -C "$SB" config user.name selftest
  git -C "$SB" config user.email selftest@localhost
  FX="$BATS_TEST_TMPDIR/fx"
  mkdir -p "$FX"
  export FX MG_GATE_NOW=2026-11-20T12:00:00Z
}

teardown() {
  [ -z "${result:-}" ] || [ ! -f "$result" ] ||
    jq -c '{job, status, message, failures: [.failures[] | {id, message: (.message // "" | .[0:400])}]}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 20
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }
validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }

@test "the evaluator's own tests pass" {
  run python3 -B -m unittest discover -s "$REPO_ROOT/tools/scripts/gates/tests" -t "$REPO_ROOT/tools/scripts/gates/tests"
  [ "$status" -eq 0 ] || { printf '%s\n' "$output" | grep -E '^(FAIL|ERROR):'; printf '%s\n' "$output" | tail -n 40; return 1; }
}

@test "the real manifest mirrors the plan and names only runs tools/mg milestone executes" {
  local d="$BATS_TEST_TMPDIR"
  "$REPO_ROOT/tools/mg" --list >"$d/registry.tsv"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" load --tiers "$REPO_ROOT/tools/gate/tiers.yaml" \
    --quarantine "$REPO_ROOT/tools/gate/quarantine.yaml" --registry "$d/registry.tsv" \
    --conformance "$REPO_ROOT/conformance" --out "$d/config.json"
  python3 -B -I "$REPO_ROOT/tools/scripts/gates/milestone_gate.py" load \
    --manifest "$REPO_ROOT/docs/process/milestones.yaml" --out "$d/manifest.json"
  run python3 -B -I "$REPO_ROOT/tools/scripts/gates/milestone_gate.py" check \
    --manifest "$d/manifest.json" --config "$d/config.json" --root "$REPO_ROOT"
  [ "$status" -eq 0 ] || { printf '%s\n' "$output" | jq .; return 1; }
  [ "$(jq -c .errors <<<"$output")" = "[]" ]
  # Every unit of the plan's Unit Index is in exactly one milestone.
  [ "$(jq '[.milestones[].units[]] | length' "$d/manifest.json")" -eq \
    "$(jq '[.milestones[].units[]] | unique | length' "$d/manifest.json")" ]
  [ "$(jq '[.milestones[].units[]] | length' "$d/manifest.json")" -eq \
    "$(sed -n '/^### Unit Index/,/^### /p' "$REPO_ROOT/$(jq -r .plan "$d/manifest.json")" | grep -c '^| U[0-9]')" ]
}

@test "the real manifest fails the check when an entry the plan requires is removed or weakened" {
  local d="$BATS_TEST_TMPDIR" line edit expected m1='.milestones[] | select(.id == "M1")'
  "$REPO_ROOT/tools/mg" --list >"$d/registry.tsv"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" load --tiers "$REPO_ROOT/tools/gate/tiers.yaml" \
    --quarantine "$REPO_ROOT/tools/gate/quarantine.yaml" --registry "$d/registry.tsv" \
    --conformance "$REPO_ROOT/conformance" --out "$d/config.json"
  python3 -B -I "$REPO_ROOT/tools/scripts/gates/milestone_gate.py" load \
    --manifest "$REPO_ROOT/docs/process/milestones.yaml" --out "$d/manifest.json"
  while IFS= read -r line; do
    edit="${line%%;;*}" expected="${line#*;;}"
    jq "$edit" "$d/manifest.json" >"$d/broken.json"
    ! cmp -s "$d/manifest.json" "$d/broken.json" || { echo "no change: $edit"; return 1; }
    run python3 -B -I "$REPO_ROOT/tools/scripts/gates/milestone_gate.py" check \
      --manifest "$d/broken.json" --config "$d/config.json" --root "$REPO_ROOT"
    [ "$status" -eq 1 ] || { echo "passed: $edit"; return 1; }
    jq -e --arg e "$expected" 'any(.errors[]; contains($e))' <<<"$output" >/dev/null ||
      { echo "$edit: no error with '$expected'"; jq . <<<"$output"; return 1; }
  done <<EOF
($m1).suites |= map(select(.id != "u246-conformance"));;U246: areas storage, sched
($m1 | .suites[] | select(.id == "private-suite") | .scope.items) -= ["queues"];;binds queues, which its scope lacks
($m1).suites |= map(select(.id != "u253-a11y"));;including U253's sweep of those screens
($m1).user_steps |= map(select(.id != "device-run-ipad"));;no device-run user step on ipad
($m1 | .user_steps[] | select(.id == "trial") | .validator.days) = 6;;do not say 6-day
($m1).suites |= map(select(.id != "perf-gate"));;U241 and U242 groups for launch, review, open and sync
($m1 | .suites[] | select(.id == "bench-mac")).plan = ["U241 and U242 groups for launch, review, open and sync (R1813, R1817, R1818, R1823, R1842–R1844)"];;must bind the same items
($m1 | .user_steps[] | select(.id == "trial") | .validator) = {"type": "report"};;which asks for a trial step of 7 days
($m1 | .user_steps[] | select(.id == "variant-picks") | .validator) = {"type": "report"};;which asks for a variant-pick step
(.milestones[] | select(.id == "M3")).suites |= map(select(.id != "u253-a11y"));;M3: the closing gate text names U253
(.milestones[] | select(.id == "M5")).user_steps |= map(select(.id != "device-run-mac"));;no device-run user step on mac
(.global[] | select(.id == "native-oracle")) |= del(.plan);;the macOS-native oracle check (KTD24)
EOF
}

@test "milestone-gate fails while a suite has no result, and names it" {
  printf 'xcode-license missing Accept the Xcode license\n' >"$FX/doctor.steps"
  mg milestone-gate M0
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | index("suite:unit") != null')" = true ]
  [ "$(field '.details.outcome')" = fail ]
  [ "$(field '.summary.missing')" -eq 2 ]
  [ ! -e "$SB/docs/process/milestone-records/M0.json" ]
  validate "$result"
}

@test "milestone-gate awaits Antoine when only a setup step is missing" {
  printf 'xcode-license missing Accept the Xcode license\n' >"$FX/doctor.steps"
  mg t-job unit
  [ "$status" -eq 0 ]
  mg milestone-gate --check
  [ "$status" -eq 0 ]
  mg milestone-gate M0
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.outcome)" = awaiting_user ]
  [ "$(field '.details.awaiting_user[0].id')" = user-step:xcode-license ]
  [ "$(field '.details.gate_commit')" = "$(git -C "$SB" rev-parse HEAD)" ]
  [ ! -e "$SB/docs/process/milestone-records/M0.json" ]
  validate "$result"
}

@test "milestone-gate passes on one commit and writes the record" {
  mg t-job unit
  mg milestone-gate --check
  mg milestone-gate M0
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.outcome)" = pass ]
  local record="$SB/docs/process/milestone-records/M0.json"
  [ -f "$record" ]
  [ "$(jq -r .gate_commit "$record")" = "$(git -C "$SB" rev-parse HEAD)" ]
  [ "$(jq -r .closed_at "$record")" = 2026-11-20T12:00:00Z ]
  [ "$(jq -r '[.suites[].id] | join(" ")' "$record")" = "unit milestones" ]
  [ "$(jq -r '.user_steps[0].outcome' "$record")" = pass ]
  [[ "$(field .message)" == *"commit docs/process/milestone-records/M0.json"* ]]
  validate "$result"
  # Committing the record (an evidence path) keeps the gate commit.
  git -C "$SB" add docs/process/milestone-records/M0.json
  git -C "$SB" commit -qm "Close M0"
  mg milestone-gate M0
  [ "$(field .status)" = pass ]
  [ "$(field '.details.record.written')" = false ]
}

@test "milestone-gate fails outside the main checkout and writes no record" {
  local wt="$BATS_TEST_TMPDIR/wt"
  git -C "$SB" worktree add -q -b unit/U1 "$wt"
  run --separate-stderr "$wt/tools/mg" t-job unit
  [ "$status" -eq 0 ]
  run --separate-stderr "$wt/tools/mg" milestone-gate --check
  [ "$status" -eq 0 ]
  run --separate-stderr "$wt/tools/mg" milestone-gate M0
  result="${lines[${#lines[@]} - 1]:-}"
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = checkout ]
  [[ "$(field '.failures[0].message')" == *"main checkout"* ]]
  [ ! -e "$wt/docs/process/milestone-records/M0.json" ]
}

@test "a result from a dirty tree fails the gate" {
  printf 'scratch\n' >"$SB/untracked.txt"
  mg t-job unit
  rm "$SB/untracked.txt"
  mg milestone-gate M0
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[] | select(.id == "suite:unit") | .message')" == *dirty* ]]
}

@test "milestone-gate --check fails when the manifest drifts from the plan" {
  sed 's/Steps of M0/Other steps/' "$SB/docs/plan.md" >"$SB/docs/plan.md.new"
  mv "$SB/docs/plan.md.new" "$SB/docs/plan.md"
  mg milestone-gate --check
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [[ "$(field '.failures[0].message')" == *"M0: plan.user_steps is not the table's user steps text"* ]]
  validate "$result"
}

@test "milestone-gate rejects an invalid manifest and bad arguments" {
  printf '  - {id: M6, name: x}\n' >>"$SB/docs/process/milestones.yaml"
  mg milestone-gate M0
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [[ "$(field '.failures[0].message')" == *"M0, M1, M2, M3, M4 and M5"* ]]
  mg milestone-gate M9
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  mg milestone-gate
  [ "$status" -eq 2 ]
}
