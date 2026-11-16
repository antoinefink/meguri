#!/usr/bin/env bats
# Self-tests for the test tiers, merge gate, quarantine and scheduled runs
# (U19: R1950-R1953, R1880, R1876, R1937, R1991; AE82 ordering) and the
# catch-up of runs the machine's state queued (U19b). Each test runs a
# sandbox repository with the gate's own families (gate, merge, catch-up,
# quarantine, nightly, milestone, session, and host, oracle and conformance
# for their helpers) and a tiers.yaml of made-up runs: `t-job <name>` reports
# what $FX/<name>.* says (status, a status sequence, failure ids, counts,
# details, queued parts), a fake doctor lists $FX/doctor.missing as missing
# user steps, a fake `similarity` scan flags a file on demand, the fake oracle
# jobs pick their oracle/ with the real mg_oracle__tree and
# mg_oracle__own_tree, and the fake oracle metadata check finds its oracle/
# file with the real mg_conf__oracle_file.
# Implementer worktrees are sparse without oracle/, as `tools/mg worktree new`
# makes them. The planning, evaluation, static checks, receipts, escalations,
# tip checkouts and fast-forwards are the real ones. A test that checks the
# real tiers.yaml against the real job registry closes the file.

bats_require_minimum_version 1.5.0
# bats file_tags=gate

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
TRAILERS=$'Role: implementer\nSpec: sched/answer#A1\nSession: s-20261016T000000Z-0badcafe'

setup_file() {
  local t="$BATS_FILE_TMPDIR/template" f
  mkdir -p "$t/core" "$t/docs" "$t/conformance/scenarios/smoke" "$t/conformance/scenarios/sched" "$t/conformance/catalog" \
    "$t/oracle/driver" "$t/oracle/harvest"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp "$REPO_ROOT/.gitignore" "$t/"
  for f in "$t"/tools/mg.d/*.sh; do
    case "$(basename "$f")" in
      gate.sh | merge.sh | catch-up.sh | quarantine.sh | nightly.sh | milestone.sh | milestone-gate.sh | session.sh | host.sh | oracle.sh | conformance.sh) ;;
      *) rm -f "$f" ;;
    esac
  done
  # The milestones `tools/mg milestone` accepts, in order (U240r), and the
  # runs each one's gate reads (U240): M0's suites, M1R's conf and the oracle
  # check that applies from M1R, M1's ui. harvest takes live's report, and
  # the device-run steps list the T4 device run. The plan texts are empty, so
  # nothing needs a plan excerpt.
  mkdir -p "$t/docs/process"
  {
    cat <<'EOF'
version: 1
plan: docs/plan.md
global:
  - {id: oracle-check, kind: suite, run: otest, from: M1R}
milestones:
  - id: M0
    name: Foundations
    plan: {gate: "", user_steps: ""}
    units: []
    suites:
      - {id: static, run: static}
      - {id: unit, run: unit}
      - {id: harvest, run: harvest}
  - id: M1R
    name: Reviews first
    plan: {gate: "", user_steps: ""}
    units: []
    suites:
      - {id: conf, run: conf}
    user_steps:
      - {id: device-run-mac, description: Device run on the Mac, evidence: docs/process/milestone-records/device-runs/M1R/mac.json, validator: {type: device-run, device: mac}}
  - id: M1
    name: Daily driver
    plan: {gate: "", user_steps: ""}
    units: []
    suites:
      - {id: ui, run: ui}
    user_steps:
      - {id: device-run-mac, description: Device run on the Mac, evidence: docs/process/milestone-records/device-runs/M1/mac.json, validator: {type: device-run, device: mac}}
EOF
    printf '  - {id: %s, name: Later, plan: {gate: "", user_steps: ""}, units: []}\n' M2 M3 M4 M5
  } >"$t/docs/process/milestones.yaml"
  printf 'id: smoke/a\n' >"$t/conformance/scenarios/smoke/a.yaml"
  printf 'id: sched/b\n' >"$t/conformance/scenarios/sched/b.yaml"
  printf '# notes\n' >"$t/docs/notes.md"
  printf '[workspace]\n' >"$t/core/Cargo.toml"
  printf 'FROM scratch\n' >"$t/oracle/Dockerfile"
  printf 'print("op")\n' >"$t/oracle/driver/ops.py"
  # The pin's lock and the harvest inventory, and what the conformance side
  # holds of them (goldens-check and harvest-report compare the two).
  printf 'anki==25.09\n' | tee "$t/oracle/requirements.lock" >"$t/conformance/oracle.lock"
  printf 'test_a,25.09\n' | tee "$t/oracle/harvest/links.csv" >"$t/conformance/catalog/harvest.csv"
  printf 'main\n' >"$t/tools/similarity/marker.txt"
  cat >"$t/tools/gate/tiers.yaml" <<'EOF'
version: 1
tiers:
  T0: {when: every change}
  T1: {when: core changes}
  T2: {when: nightly}
  T3: {when: milestones}
  T4: {when: milestones, on devices}
runs:
  - {id: static, job: static-checks, args: [--base, "{base}"], tier: T0, stage: static, placement: host, class: static, budget_s: 60}
  - {id: static-tree, job: static-checks, args: [--base, "{base}"], merge_args: [--rev, "{tip}", --base, "{base}"], tier: T0, stage: static, placement: host, class: static, budget_s: 60, local: false, where: main}
  - {id: pin-canary, job: t-job, args: [pin-canary], tier: T0, stage: canary, placement: docker:dev, class: unit, triggers: [core/], budget_s: 60}
  - {id: unit, job: t-job, args: [unit], tier: T0, placement: docker:dev, class: unit, triggers: [core/], budget_s: 60}
  - {id: conf, job: t-job, args: [conf], tier: T0, placement: [host, docker:dev], class: conformance, triggers: [core/, conformance/], budget_s: 60}
  - {id: ui, job: t-job, args: [ui], tier: T0, placement: vm, needs: [simulator], class: ui, triggers: [app/], budget_s: 60}
  - {id: slow, job: t-job, args: [slow], tier: T0, placement: host, class: tooling, triggers: [tools/slow/], budget_s: 1}
  - {id: sim, job: similarity, args: [--base, "{base}"], merge_args: [--rev, "{tip}", --base, "{base}"], tier: T0, placement: [host, docker:similarity], needs: [refs], class: similarity, triggers: [core/, app/], budget_s: 60, local: false, where: main}
  - {id: stest, job: t-scan-selftest, tier: T0, placement: [host, docker:similarity], needs: [refs], class: tooling, triggers: [tools/similarity/], budget_s: 60, local: false, where: tip}
  - {id: later, job: notices, tier: T0, placement: host, class: static, triggers: [core/], budget_s: 60, pending: U16}
  - {id: goldens, job: t-oracle-files, args: [goldens, requirements.lock, conformance/oracle.lock], tier: T0, placement: [host, docker:dev], class: conformance, triggers: [conformance/], oracle_files: true, budget_s: 60}
  - {id: catalog, job: t-oracle-files, args: [catalog, harvest/links.csv, conformance/catalog/harvest.csv], tier: T0, placement: [host, docker:dev], class: conformance, triggers: [conformance/catalog/], oracle_files: true, budget_s: 60}
  - id: live
    job: t-oracle
    args: [live]
    tier: T1
    placement: [host, docker:oracle]
    needs: [oracle]
    oracle: true
    local: false
    class: conformance
    budget_s: 60
    filters:
      core/: [smoke]
      core/crates/meguri-sched/: [sched]
      conformance/: ["*"]
      tools/mg.d/oracle.sh: ["*"]
  - {id: otest, job: t-oracle-own, args: [otest], tier: T1, placement: [host, docker:oracle], needs: [oracle], oracle: true, local: false, class: tooling, triggers: [oracle/, tools/mg.d/oracle.sh], budget_s: 60}
  - {id: fuzz, job: t-job, args: [fuzz, --seed, "{seed}"], tier: T2, schedule: nightly, placement: docker:fuzz, class: unit, budget_s: 60}
  - {id: soak, job: t-job, args: [soak], tier: T2, schedule: weekly, placement: [host, docker:oracle], needs: [oracle, sync-server], class: sync, budget_s: 60}
  - id: harvest
    job: t-job
    args: [harvest, --live-report, "{artifact:live:t1-live.json}"]
    tier: T3
    placement: host
    class: conformance
    budget_s: 60
    require:
      - {id: R1880-unclassified, summary: unclassified, max: 0}
      - {id: R1880-in-scope, detail: harvest.by_classification.in-scope.statuses, only_keys: [pass], release: true}
  - {id: audit, job: release-audit, tier: T3, release_only: true, placement: host, class: static, budget_s: 60, pending: U17}
  - {id: device, job: device-run, tier: T4, placement: host, needs: [device], class: perf, budget_s: 60, user_step: device-run}
utilities:
  - {job: gate, why: runs the tiers}
  - {job: merge, why: merges}
  - {job: escalation, why: escalations}
  - {job: quarantine, why: quarantine}
  - {job: nightly, why: T2}
  - {job: milestone, why: T3}
  - {job: milestone-gate, why: milestone.sh reads the manifest's milestones}
  - {job: session, why: sessions}
  - {job: oracle, why: its tree helpers}
  - {job: conformance, why: its oracle file lookup}
  - {job: conformance-route-test, why: conformance.sh route infrastructure harness}
  - {job: goldens-check, why: conformance.sh}
  - {job: harvest-report, why: conformance.sh}
  - {job: harvest-inventory, why: conformance.sh}
  - {job: mac-ui, why: host.sh}
  - {job: catch-up, why: re-runs queued runs}
  - {job: doctor, why: reads host state}
EOF
  cat >"$t/tools/mg.d/zz-fakes.sh" <<'EOF'
mg_job t-job host t_job "fake run: t-job <name> [args...]; reports what \$FX/<name>.* says"
mg_job similarity host t_similarity "fake similarity scan: flags core/x.rs when \$FX/similarity.flag exists"
mg_job t-oracle host t_oracle "fake oracle-image run (conformance --live): t-oracle <name> [args...]"
mg_job t-oracle-own host t_oracle_own "fake oracle test: t-oracle-own <name> [args...]"
mg_job t-scan-selftest host t_scan_selftest "fake similarity-selftest"
mg_job t-oracle-files host t_oracle_files "fake oracle metadata check: t-oracle-files <name> <file under oracle/> <its conformance side>"
mg_job doctor host t_doctor "fake doctor: lists \$FX/doctor.missing as missing user steps"
t_doctor() {
  local s
  echo "doctor @$(basename "$MG_ROOT")" >>"$STUB_LOG"
  if [ -f "$FX/doctor.missing" ]; then
    while read -r s; do mg_user_step "$s" missing true "made-up $s" "made-up"; done <"$FX/doctor.missing"
  fi
  # Steps doctor cannot verify are listed too; they hold nothing back.
  mg_user_step uitest-automation unverified true "made-up" "made-up"
  mg_status "$(cat "$FX/doctor.status" 2>/dev/null || echo pass)"
}
# $FX/<name>@<checkout basename>.* overrides $FX/<name>.* in that checkout.
t_job() {
  local name="$1" root f status=pass line
  shift
  root="$(basename "$MG_ROOT")"
  echo "t-job $name${*:+ $*} @$root" >>"$STUB_LOG"
  f="$FX/$name"
  { [ -e "$FX/$name@$root.status" ] || [ -e "$FX/$name@$root.seq" ]; } && f="$FX/$name@$root"
  if [ -s "$f.seq" ]; then
    status="$(head -n 1 "$f.seq")"
    tail -n +2 "$f.seq" >"$f.seq.tmp" && mv "$f.seq.tmp" "$f.seq"
  elif [ -f "$f.status" ]; then
    status="$(cat "$f.status")"
  fi
  [ ! -f "$FX/$name.write" ] || printf 'written by a job\n' >"$MG_ROOT/$(cat "$FX/$name.write")"
  [ ! -f "$FX/$name.sleep" ] || sleep "$(cat "$FX/$name.sleep")"
  if [ -f "$FX/$name.counts" ]; then
    while read -r k v; do mg_count "$k" "$v"; done <"$FX/$name.counts"
  fi
  if [ -f "$FX/$name.details" ]; then
    jq -c 'to_entries[] | {(.key): .value}' "$FX/$name.details" >>"$MG_STATE/details.jsonl"
  fi
  # $FX/<name>.rows: harvest rows [{scenario, classification, status}], as
  # harvest-report counts them: a --live-report's statuses replace `live` and
  # `pending` for the scenarios it ran.
  if [ -f "$FX/$name.rows" ]; then
    local live="" prev="" a
    for a in "$@"; do
      [ "$prev" != --live-report ] || live="$a"
      prev="$a"
    done
    jq -c --slurpfile t1 "${live:-/dev/null}" '
      ([$t1[0].scenarios[]? | {(.scenario): .status}] | add // {}) as $ran
      | map(if (.status == "live" or .status == "pending") and $ran[.scenario] then .status = $ran[.scenario] else . end)
      | {harvest: {by_classification: (group_by(.classification) | map({key: .[0].classification,
          value: {rows: length, statuses: (group_by(.status) | map({key: .[0].status, value: length}) | from_entries)}})
          | from_entries)}}' "$FX/$name.rows" >>"$MG_STATE/details.jsonl"
    mg_count unclassified "$(jq '[.[] | select(.classification == "unclassified")] | length' "$FX/$name.rows")"
  fi
  case "$status" in
    queued)
      # $FX/<name>.queued: "<user step> <part>" lines, recorded the way a job
      # blocked by the machine's state records them (mg_queued_part).
      if [ -f "$FX/$name.queued" ]; then
        while read -r line; do
          mg_user_step "${line%% *}" missing true "made-up ${line%% *}" "made-up"
          mg_queued_part "${line%% *}" "${line#* }" "made-up: ${line%% *}"
        done <"$FX/$name.queued"
      fi
      mg_detail queued true
      status=warn
      ;;
    fail | error)
      if [ -f "$FX/$name.failures" ]; then
        while IFS= read -r line; do mg_failure "$line" "made-up failure $line"; done <"$FX/$name.failures"
      else
        mg_failure "$name-$status" "made-up $status"
      fi
      ;;
  esac
  mg_message "t-job $name: $status"
  mg_status "$status"
}
# The oracle jobs' contracts (tools/mg.d/oracle.sh, conformance.sh): an
# oracle-image job uses this checkout's oracle/, or the main checkout's when
# that is the branch's (mg_oracle__tree), and otherwise fails as `conformance
# --live` does: status fail with oracle-tree-differs among its failures. Its
# T1 report is $FX/<name>.t1. `oracle test` refuses a checkout without its own
# oracle/ (mg_oracle__own_tree: error, no-oracle-tree).
t_oracle() {
  local root
  root="$(basename "$MG_ROOT")"
  if ! mg_oracle__tree; then
    echo "t-oracle $* @$root refused" >>"$STUB_LOG"
    mg_message "t-oracle $1: failed"
    mg_status fail
    return 0
  fi
  echo "t-oracle $* @$root oracle=$([ "$MG_ORACLE_DIR" = "$MG_ROOT/oracle" ] && echo own || echo main)" >>"$STUB_LOG"
  if [ -f "$FX/$1.t1" ]; then cp "$FX/$1.t1" "$MG_RUN_DIR/t1-live.json"; else echo '{"tier": "t1", "scenarios": []}' >"$MG_RUN_DIR/t1-live.json"; fi
  mg_artifact report "$MG_RUN_DIR/t1-live.json" "T1 report (live)"
  t_job "$@"
}
t_oracle_own() {
  local root
  root="$(basename "$MG_ROOT")"
  mg_oracle__own_tree "test copies oracle/ into its host checks" || { echo "t-oracle-own $* @$root refused" >>"$STUB_LOG"; return 0; }
  echo "t-oracle-own $* @$root" >>"$STUB_LOG"
  t_job "$@"
}
# The oracle metadata checks' contract (goldens-check, harvest-report): they
# read a file of this checkout's oracle/, or of the main checkout's when this
# one has none (mg_conf__oracle_file), and fail when the branch's conformance
# side disagrees with it (pin_mismatch, the leak check).
t_oracle_files() {
  local file
  file="$(mg_conf__oracle_file "$2")"
  echo "t-oracle-files $1 @$(basename "$MG_ROOT") oracle=$(case "$file" in "$MG_ROOT"/*) echo own ;; "") echo none ;; *) echo main ;; esac)" >>"$STUB_LOG"
  if [ -n "$file" ] && cmp -s "$file" "$MG_ROOT/$3"; then
    mg_status pass
  else
    mg_failure "pin_mismatch:$1" "$3 does not match oracle/$2"
    mg_status fail
  fi
}
# similarity-selftest's contract: refused in a role worktree, the checkout's
# own scanner tests, the corpus from MG_SIMILARITY_REFS or refs/ (calibration
# skipped, warn, without one).
t_scan_selftest() {
  local refs="${MG_SIMILARITY_REFS:-$MG_ROOT/refs}"
  case "$(mg_session_role)" in
    implementer | fixer) mg_failure implementer-worktree "refused in a role worktree"; mg_status error; return 0 ;;
  esac
  echo "t-scan-selftest @$(basename "$MG_ROOT") marker=$(cat "$MG_ROOT/tools/similarity/marker.txt") refs=$refs" >>"$STUB_LOG"
  if [ -d "$refs/anki" ]; then mg_status pass; else mg_status warn; fi
}
t_similarity() {
  echo "similarity $* @$(basename "$MG_ROOT")" >>"$STUB_LOG"
  if [ -f "$FX/similarity.flag" ]; then
    printf '# implementer view\ncore/x.rs lines 1-9: struct 12%%\n' >"$MG_RUN_DIR/implementer.md"
    printf '{"flags": [{"path": "core/x.rs"}]}\n' >"$MG_RUN_DIR/implementer.json"
    printf 'reviewer view, with the matching corpus file\n' >"$MG_RUN_DIR/reviewer.md"
    mg_artifact similarity-implementer "$MG_RUN_DIR/implementer.md" "implementer view"
    mg_artifact similarity-implementer-json "$MG_RUN_DIR/implementer.json"
    mg_artifact similarity-reviewer "$MG_RUN_DIR/reviewer.md" "reviewer view"
    mg_failure "similarity:core/x.rs:struct" "core/x.rs lines 1-9: struct 12%; rewrite this region from the spec"
    mg_status fail
  else
    mg_status pass
  fi
}
EOF
  git -C "$t" init -q -b trunk
  # Sparse settings stay per worktree, as in the real repository.
  git -C "$t" config extensions.worktreeConfig true
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/main"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  git -C "$SB" config user.name selftest
  git -C "$SB" config user.email selftest@localhost
  BASE0="$(git -C "$SB" rev-parse HEAD)"
  FX="$BATS_TEST_TMPDIR/fx"
  export FX STUB_LOG="$BATS_TEST_TMPDIR/stub.log" MG_GATE_NOW=2026-10-16T12:00:00Z
  export HOME="$BATS_TEST_TMPDIR/home"
  mkdir -p "$FX" "$HOME"
  # The similarity corpus, untracked in the main checkout only.
  mkdir -p "$SB/refs/anki"
  : >"$STUB_LOG"
}

teardown() {
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{job, status, message, failures: [.failures[] | {id, message: (.message // "" | .[0:400])}]}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 20
}

# mg <args>: the main checkout's tools/mg; mg_in <dir> <args>: another one's.
mg() { mg_in "$SB" "$@"; }
mg_in() {
  local dir="$1"
  shift
  run --separate-stderr "$dir/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }
receipt() { jq -r '.artifacts[] | select(.kind == "receipt") | .path' "$result"; }
rq() { jq -r "$@" "$(receipt)"; }
outcome() { rq --arg id "$1" '.runs[] | select(.id == $id) | .outcome'; }
validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }
called() { grep -c -- "$1" "$STUB_LOG" || true; }

# commit_in <dir> <path> <contents> [message]: one commit changing one file.
commit_in() {
  mkdir -p "$(dirname "$1/$2")"
  printf '%s\n' "$3" >"$1/$2"
  git -C "$1" add -- "$2"
  git -C "$1" commit -qm "${4:-change $2}"
}

# fx <name> <status> [failure ids...]
fx() {
  local name="$1" status="$2"
  shift 2
  printf '%s\n' "$status" >"$FX/$name.status"
  rm -f "$FX/$name.failures"
  [ "$#" -eq 0 ] || printf '%s\n' "$@" >"$FX/$name.failures"
}

# worktree <dir> <branch> [role]: a worktree on a new branch from trunk,
# marked with a role and, for implementers and fixers, sparse without oracle/,
# the way tools/mg worktree new makes it.
worktree() {
  git -C "$SB" worktree add -q -b "$2" "$1" trunk
  case "${3:-}" in
    implementer | fixer) git -C "$1" sparse-checkout set --no-cone '/*' '!/oracle/' ;;
  esac
  [ -z "${3:-}" ] || jq -n --arg r "$3" '{role: $r, session: "s-20261016T000000Z-0badcafe"}' \
    >"$(git -C "$1" rev-parse --absolute-git-dir)/meguri-session.json"
}

# --- T0 selection --------------------------------------------------------

@test "a change touching only docs/ runs the static checks only and passes in seconds" {
  commit_in "$SB" docs/notes.md "# more notes"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(field .status)" = pass ]
  [ "$(rq '[.runs[] | select(.result != null) | .id] | join(" ")')" = "static static-tree" ]
  [ "$(outcome unit)" = not-triggered ]
  [ "$(outcome live)" = not-triggered ]
  [ ! -s "$STUB_LOG" ]
  [ "$(field .duration_ms)" -lt 30000 ]
  [ "$(rq .pass)" = true ]
  [ "$(rq .merge_ready)" = true ]
  [ "$(rq .commit)" = "$(git -C "$SB" rev-parse HEAD)" ]
  validate "$result"
}

@test "a change under core/crates/meguri-sched/ adds the T1 live suites for sched and the similarity scan" {
  commit_in "$SB" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(called "^t-job live sched smoke @main$")" = 1 ]
  [ "$(called "^similarity --base $BASE0 @main$")" = 1 ]
  [ "$(called '^t-job unit @main$')" = 1 ]
  [ "$(outcome live)" = pass ]
  [ "$(outcome later)" = pending ]
  [ "$(rq '.runs[] | select(.id == "later") | .message')" = "job lands with U16" ]
  # T0 before T1, canaries before both.
  [ "$(grep -n 'pin-canary' "$STUB_LOG" | cut -d: -f1)" -lt "$(grep -n 't-job unit' "$STUB_LOG" | cut -d: -f1)" ]
  [ "$(grep -n 't-job unit' "$STUB_LOG" | cut -d: -f1)" -lt "$(grep -n 't-job live' "$STUB_LOG" | cut -d: -f1)" ]
}

@test "a pin bump fails at the canary first and the gate stops there (AE82 ordering)" {
  commit_in "$SB" core/Cargo.lock 'name = "rand"
version = "0.9.5"' "Bump rand

$TRAILERS"
  fx pin-canary fail 'layering-check::canary_pins$cargo_lock_matches_the_pins'
  mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(rq '.blocking[0].id')" = run:pin-canary ]
  [ "$(rq '.blocking[1].id')" = 't-job:layering-check::canary_pins$cargo_lock_matches_the_pins' ]
  [ "$(called 't-job pin-canary')" = 1 ]
  [ "$(called 't-job unit')" = 0 ]
  [ "$(called 't-job live')" = 0 ]
  [ "$(called '^similarity')" = 0 ]
  [ "$(outcome unit)" = skipped-after-canary ]
  [ "$(outcome live)" = skipped-after-canary ]
  [ "$(rq '[.blocking[] | select(.id | startswith("run:"))] | length')" = 1 ]
  validate "$result"
}

@test "a failing T0 run keeps T1 from starting and fails the gate" {
  commit_in "$SB" core/crates/meguri-text/src/lib.rs "pub fn f() {}" "Change text

$TRAILERS"
  fx conf fail "native:text/fold/x"
  mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  [ "$(outcome conf)" = fail ]
  [ "$(outcome live)" = skipped-after-t0 ]
  [ "$(called 't-job live')" = 0 ]
  [ "$(called 't-job unit')" = 1 ]
  jq -e '.failures[] | select(.id == "t-job:native:text/fold/x")' "$result"
}

@test "gate --local runs every run but the similarity scan and oracle-image runs, and its receipt never merges" {
  commit_in "$SB" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  mg gate --local --base "$BASE0"
  [ "$status" -eq 0 ]
  [ "$(outcome sim)" = skipped-local ]
  [ "$(outcome live)" = skipped-local ]
  [ "$(outcome static-tree)" = skipped-local ]
  [ "$(outcome unit)" = pass ]
  [ "$(called '^similarity')" = 0 ]
  [ "$(rq .mode)" = local ]
  [ "$(rq .merge_ready)" = false ]
}

@test "in an implementer worktree the main-checkout runs are deferred to tools/mg merge" {
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  commit_in "$wt" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  mg_in "$wt" gate --base "$BASE0"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(outcome sim)" = deferred-main ]
  [ "$(outcome static-tree)" = deferred-main ]
  [ "$(rq '.runs[] | select(.id == "sim") | .merge_args | join(" ")')" = "--rev $(git -C "$wt" rev-parse HEAD) --base $BASE0" ]
  [ "$(rq '.incomplete | join(" ")')" = "static-tree sim" ]
  [ "$(rq .merge_ready)" = false ]
  [ "$(called '^similarity')" = 0 ]
}

@test "a queued VM run passes with a note, a run over its budget is reported, a job that writes to the tree fails the gate" {
  commit_in "$SB" app/Meguri/View.swift "struct V {}" "Change a view

$TRAILERS"
  fx ui queued
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(outcome ui)" = queued ]

  commit_in "$SB" tools/slow/x.sh "true"
  printf '1.5\n' >"$FX/slow.sleep"
  mg gate --base "$BASE0"
  [ "$(rq '.runs[] | select(.id == "slow") | .over_budget')" = true ]
  [ "$(field .status)" = warn ]

  rm -f "$FX/slow.sleep"
  printf 'stray.txt\n' >"$FX/slow.write"
  mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  jq -e '.failures[] | select(.id == "tree-changed")' "$result"
}

# --- flakes and quarantine (R1953) ---------------------------------------

@test "a flaky test fails the gate once with no retry; quarantined with evidence and approval it passes until its deadline" {
  local t='meguri-sync::transport$resumes_after_reset' failed passed
  commit_in "$SB" core/crates/meguri-sync/src/lib.rs "pub fn f() {}" "Change sync

$TRAILERS"
  printf 'fail\npass\n' >"$FX/unit.seq"
  printf '%s\n' "$t" >"$FX/unit.failures"
  mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  [ "$(called 't-job unit')" = 1 ]
  jq -e --arg t "t-job:$t" '.failures[] | select(.id == $t)' "$result"
  failed="$(field .run_id)"

  mg gate --base "$BASE0"
  [ "$status" -eq 0 ]
  [ "$(called 't-job unit')" = 2 ]
  passed="$(field .run_id)"

  # Without Antoine's approval, or without evidence of one commit passing
  # and failing, the entry is refused.
  mg quarantine add --run unit --test "$t" --class sync --owner U105 --reason "drops a reset now and then" \
    --deadline 2026-10-30 --failed "$failed" --passed "$passed"
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[0].message')" == *"approved_by: antoine"* ]]
  mg quarantine add --run unit --test "$t" --class sync --owner U105 --reason "drops a reset now and then" \
    --deadline 2026-10-30 --failed "$failed" --passed "$failed" --approved-by antoine
  [ "$status" -eq 1 ]
  [[ "$(jq -r '[.failures[].message] | join(" | ")' "$result")" == *"no unit run that passed"* ]]
  mg quarantine add --run unit --test "$t" --class unit --owner U105 --reason "flaky" \
    --deadline 2026-10-30 --failed "$failed" --passed "$passed" --approved-by antoine
  [ "$status" -eq 0 ]
  git -C "$SB" diff --quiet -- tools/gate/quarantine.yaml && return 1
  git -C "$SB" checkout -q -- tools/gate/quarantine.yaml

  mg quarantine add --run unit --test "$t" --class sync --owner U105 --reason "drops a reset now and then" \
    --deadline 2026-10-30 --failed "$failed" --passed "$passed" --approved-by antoine
  [ "$status" -eq 0 ] || return 1
  grep -q 'approved_by: antoine' "$SB/tools/gate/quarantine.yaml"
  git -C "$SB" commit -qam "Quarantine a flaky sync test"

  printf 'fail\n' >"$FX/unit.seq"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(field .status)" = warn ]
  [ "$(outcome unit)" = quarantined ]
  [ "$(rq '.quarantine.active[0].failed_here')" = true ]

  # After the deadline the gate fails, whether or not the test failed.
  MG_GATE_NOW=2026-10-31T00:00:00Z mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  jq -e --arg t "quarantine-expired:unit:$t" '.failures[] | select(.id == $t)' "$result"
}

@test "a quarantined canary failure does not stop the gate, and runs a gate did not start keep a receipt from merging" {
  local t='layering-check::canary_pins$cargo_lock_matches_the_pins' failed passed dir
  commit_in "$SB" core/Cargo.lock 'name = "rand"' "Bump rand

$TRAILERS"
  printf 'fail\npass\n' >"$FX/pin-canary.seq"
  printf '%s\n' "$t" >"$FX/pin-canary.failures"
  mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  failed="$(field .run_id)"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ]
  passed="$(field .run_id)"
  mg quarantine add --run pin-canary --test "$t" --class unit --owner U45 --reason "the pin probe races the lock file" \
    --deadline 2026-10-30 --failed "$failed" --passed "$passed" --approved-by antoine
  [ "$status" -eq 0 ]
  git -C "$SB" commit -qam "Quarantine the pin canary"

  printf 'fail\n' >"$FX/pin-canary.seq"
  : >"$STUB_LOG"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(outcome pin-canary)" = quarantined ]
  [ "$(called '^t-job unit @main$')" = 1 ]
  [ "$(called '^t-job conf @main$')" = 1 ]
  [ "$(called '^t-job live smoke @main$')" = 1 ]
  [ "$(rq '[.runs[] | select(.outcome == "skipped-after-canary")] | length')" = 0 ]
  [ "$(rq .merge_ready)" = true ]

  # Required runs the gate did not start never make a receipt merge-ready,
  # even when nothing that ran blocks.
  dir="$(dirname "$result")"
  jq -c 'select(.id == "pin-canary")' "$dir/results.jsonl" >"$BATS_TEST_TMPDIR/results.jsonl"
  jq -c '.runs[] | select(.action == "run" and .id != "pin-canary") | {id, outcome: "skipped-after-canary", message: "not run"}' \
    "$dir/plan.json" >>"$BATS_TEST_TMPDIR/results.jsonl"
  python3 -B -I "$SB/tools/gate/gate.py" evaluate --config "$dir/config.json" --plan "$dir/plan.json" \
    --results "$BATS_TEST_TMPDIR/results.jsonl" --now "$MG_GATE_NOW" --commit "$(git -C "$SB" rev-parse HEAD)" \
    --out "$BATS_TEST_TMPDIR/receipt.json"
  [ "$(jq -r '.runs[] | select(.id == "pin-canary") | .outcome' "$BATS_TEST_TMPDIR/receipt.json")" = quarantined ]
  [ "$(jq .merge_ready "$BATS_TEST_TMPDIR/receipt.json")" = false ]
  jq -e '.incomplete | index("unit")' "$BATS_TEST_TMPDIR/receipt.json"
}

@test "a hand-written quarantine entry without approval or evidence fails the gate" {
  cat >>"$SB/tools/gate/quarantine.yaml" <<'EOF'
  - test: "x$y"
    run: unit
    class: unit
    owner: U90
    reason: flaky
    deadline: 2026-12-01
EOF
  git -C "$SB" commit -qam "Quarantine by hand"
  mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  [[ "$(jq -r '[.failures[] | select(.id | startswith("quarantine-invalid:"))][0].message' "$result")" == *"approved_by: antoine"* ]]
  [[ "$(jq -r '[.failures[] | select(.id | startswith("quarantine-invalid:"))][0].message' "$result")" == *"evidence"* ]]
  mg quarantine check
  [ "$status" -eq 1 ]
}

@test "a quarantined test of a conformance run must be classed conformance" {
  local t="native:render/x" failed passed
  commit_in "$SB" conformance/scenarios/smoke/c.yaml "id: smoke/c"
  printf 'fail\npass\n' >"$FX/conf.seq"
  printf '%s\n' "$t" >"$FX/conf.failures"
  mg gate --base "$BASE0"
  failed="$(field .run_id)"
  mg gate --base "$BASE0"
  passed="$(field .run_id)"
  mg quarantine add --run conf --test "$t" --class unit --owner U60 --reason flaky --deadline 2026-10-30 \
    --failed "$failed" --passed "$passed" --approved-by antoine
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[0].message')" == *"conformance tests"* ]]
}

# --- static checks -------------------------------------------------------

@test "a scenario that names an AnkiWeb host fails the static check (R1876); only U116 and U255 scripts may be allowed" {
  local host
  host="$(printf 'sync.%s.%s' ankiweb net)"
  commit_in "$SB" conformance/scenarios/smoke/leak.yaml "id: smoke/leak
endpoint: https://$host/"
  mg gate --base "$BASE0"
  [ "$status" -eq 1 ]
  jq -e '.failures[] | select(.id == "static-checks:ankiweb-hosts:conformance/scenarios/smoke/leak.yaml:2")' "$result"

  mg static-checks --check ankiweb-hosts
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "ankiweb-hosts:conformance/scenarios/smoke/leak.yaml:2" ]
  # The committed tree is scanned the same way (the merge's --rev run).
  mg static-checks --check ankiweb-hosts --rev HEAD
  [ "$status" -eq 1 ]

  printf 'conformance/scenarios/smoke/leak.yaml U105\n' >>"$SB/tools/gate/static-checks/ankiweb-allow.txt"
  mg static-checks --check ankiweb-hosts
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = ankiweb-hosts-error ]
  # A host written as a regular expression is a host too.
  git -C "$SB" checkout -q -- tools/gate/static-checks/ankiweb-allow.txt
  git -C "$SB" rm -q conformance/scenarios/smoke/leak.yaml
  printf 're = r"%s\\.%s"\n' ankiweb net >"$SB/tools/probe.py"
  mg static-checks --check ankiweb-hosts
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "ankiweb-hosts:tools/probe.py:1" ]
}

@test "the trailer check fails a core/ commit that skipped the commit-msg hook" {
  commit_in "$SB" core/crates/meguri-text/src/lib.rs "pub fn f() {}" "Change text without trailers"
  local bad
  bad="$(git -C "$SB" rev-parse --short=12 HEAD)"
  commit_in "$SB" core/crates/meguri-text/src/more.rs "pub fn g() {}" "Change text

$TRAILERS"
  mg static-checks --base "$BASE0"
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = "trailers:$bad" ]
  [[ "$(field '.failures[0].message')" == *"need a Role: trailer"* ]]
  mg static-checks --base "$BASE0" --rev HEAD
  [ "$status" -eq 1 ]
  validate "$result"
}

@test "the no-retries check fails a test plan or a script that reruns failing tests (R1953)" {
  mkdir -p "$SB/app/TestPlans"
  printf '{"defaultOptions": {"testRepetitionMode": "none"}, "testTargets": [{"target": {"name": "T"}}]}\n' >"$SB/app/TestPlans/Unit.xctestplan"
  mg static-checks --check no-retries
  [ "$status" -eq 0 ]
  printf '{"defaultOptions": {}, "testTargets": [{"target": {"name": "T"}, "testRepetitionMode": "retryOnFailure"}]}\n' >"$SB/app/TestPlans/Unit.xctestplan"
  mg static-checks --check no-retries
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "no-retries:app/TestPlans/Unit.xctestplan:0" ]
  rm "$SB/app/TestPlans/Unit.xctestplan"
  mkdir -p "$SB/core/.config"
  printf '[profile.default]\nretries = 2\n' >"$SB/core/.config/nextest.toml"
  mg static-checks --check no-retries
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "no-retries:core/.config/nextest.toml:2" ]
  printf '[profile.default]\nretries = 0\n' >"$SB/core/.config/nextest.toml"
  printf 'cargo nextest run --retr%s 3\n' ies >"$SB/tools/run-tests.sh"
  mg static-checks --check no-retries
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "no-retries:tools/run-tests.sh:1" ]
}

# --- merge, escalations (R1951, R1957, R1991) ------------------------------

@test "merge fast-forwards the integration branch only on a passing receipt for the branch tip" {
  local wt="$BATS_TEST_TMPDIR/U90" tip
  worktree "$wt" unit/U90 implementer
  commit_in "$wt" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  tip="$(git -C "$wt" rev-parse HEAD)"
  fx unit fail
  mg merge unit/U90
  [ "$status" -eq 1 ]
  [ "$(git -C "$SB" rev-parse trunk)" = "$BASE0" ]
  [ "$(rq .pass)" = false ]
  [ "$(rq .commit)" = "$tip" ]
  [ "$(outcome unit)" = fail ]
  [[ "$(field .message)" == *"trunk not moved"* ]]

  fx unit pass
  : >"$STUB_LOG"
  mg merge unit/U90
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(git -C "$SB" rev-parse trunk)" = "$tip" ]
  [ "$(field .details.merged)" = true ]
  # The worktree ran the worktree runs; the main checkout scanned the tip.
  [ "$(called "^t-job unit @U90$")" -ge 1 ]
  [ "$(called "^similarity --rev $tip --base $BASE0 @main$")" = 1 ]
  [ "$(outcome sim)" = pass ]
  [ "$(outcome static-tree)" = pass ]
  [ "$(rq .merge_ready)" = true ]
  # Nothing was queued, so nothing waits for catch-up.
  [ "$(rq '.queued | length')" = 0 ]
  [ ! -e "$SB/build/mg/catch-up/queue.jsonl" ]
  validate "$result"
}

@test "merge does not move the integration branch when a job of the worktree's gate wrote to the tested tree" {
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  commit_in "$wt" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  printf 'stray.txt\n' >"$FX/unit.write"
  mg merge unit/U90
  [ "$status" -eq 1 ]
  [ "$(git -C "$SB" rev-parse trunk)" = "$BASE0" ]
  [ "$(rq .merge_ready)" = false ]
  [[ "$(field .message)" == *"trunk not moved"* ]]
  # The worktree gate's own verdict reaches the merge's receipt...
  jq -e '.failures[] | select(.id == "tree-changed")' "$result"
  [ "$(rq '[.receipt_blocking[].id] | join(" ")')" = "tree-changed worktree-changed" ]
  # ...and the merge checks the worktree again before it moves anything.
  jq -e '.failures[] | select(.id == "worktree-changed")' "$result"
}

@test "merge refuses a branch that does not contain the integration head, a dirty worktree, and runs only in the main checkout" {
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  commit_in "$wt" docs/a.md "a"
  commit_in "$SB" docs/b.md "b"
  mg merge unit/U90
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = not-fast-forward ]
  git -C "$wt" merge -q --no-edit trunk
  printf 'x\n' >"$wt/docs/dirty.md"
  mg merge unit/U90
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = worktree-not-at-tip ]
  rm "$wt/docs/dirty.md"
  mg_in "$wt" merge unit/U90
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = not-main ]
}

@test "a similarity flag sends only the implementer view to the worktree and records an escalation" {
  local wt="$BATS_TEST_TMPDIR/U90" tip feedback
  worktree "$wt" unit/U90 implementer
  commit_in "$wt" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  tip="$(git -C "$wt" rev-parse HEAD)"
  : >"$FX/similarity.flag"
  mg merge unit/U90
  [ "$status" -eq 1 ]
  ! git -C "$SB" merge-base --is-ancestor "$tip" trunk
  feedback="$(field .details.implementer_feedback)"
  [[ "$feedback" == "$wt/build/mg/merge-feedback/"* ]]
  [ -f "$feedback/implementer.md" ]
  [ -f "$feedback/implementer.json" ]
  [ -z "$(find "$wt/build" -name 'reviewer*')" ]
  ! jq -e '.artifacts[] | select(.kind | test("reviewer"))' "$result"
  jq -e '.failures[] | select(.id == "similarity:similarity:core/x.rs:struct")' "$result"
  [ "$(jq -s '[.[] | select(.kind == "approval" and .unit == "U90" and .state == "open")] | length' "$SB/docs/process/provenance/escalations.jsonl")" = 1 ]
  # The escalation is committed on trunk, which moves by that commit only.
  [ "$(git -C "$SB" diff --name-only "$BASE0" trunk)" = docs/process/provenance/escalations.jsonl ]
}

@test "a unit with an open escalation (red after its sixth round) stays unmerged while a unit that does not depend on it merges" {
  local a="$BATS_TEST_TMPDIR/U90" b="$BATS_TEST_TMPDIR/U91" id
  mg escalation add --kind blocked-red --unit U90 --summary "no green gate after six review rounds"
  [ "$status" -eq 0 ]
  id="$(field .details.escalation.id)"
  worktree "$a" unit/U90 implementer
  worktree "$b" unit/U91 implementer
  commit_in "$a" docs/a.md "a"
  commit_in "$b" docs/b.md "b"
  mg merge unit/U90
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "escalation:$id" ]
  ! git -C "$SB" merge-base --is-ancestor unit/U90 trunk
  mg merge unit/U91
  [ "$status" -eq 0 ]
  git -C "$SB" merge-base --is-ancestor unit/U91 trunk

  mg escalation list --open
  [ "$(field '.details.escalations | length')" = 1 ]
  mg escalation resolve "$id" --by antoine --resolution "split the unit"
  [ "$status" -eq 0 ]
  mg escalation list --open
  [ "$(field '.details.escalations | length')" = 0 ]
  git -C "$a" merge -q --no-edit trunk
  mg merge unit/U90
  [ "$status" -eq 0 ]
  mg escalation add --kind nonsense --unit U90 --summary x
  [ "$status" -eq 2 ]
}

@test "merge reads the escalations at the branch tip: one opened only on the branch blocks it, one resolved only there does not" {
  local a="$BATS_TEST_TMPDIR/U90" b="$BATS_TEST_TMPDIR/U91" id
  # Opened in the unit's worktree: committed on unit/U90 only.
  worktree "$a" unit/U90 implementer
  commit_in "$a" docs/a.md "a"
  mg_in "$a" escalation add --kind stop-condition --unit U90 --summary "a Goal Capsule stop condition"
  [ "$status" -eq 0 ]
  id="$(field .details.escalation.id)"
  [ -z "$(git -C "$SB" show trunk:docs/process/provenance/escalations.jsonl 2>/dev/null)" ]
  mg merge unit/U90
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = "escalation:$id" ]
  [ "$(git -C "$SB" rev-parse trunk)" = "$BASE0" ]

  # Opened on trunk, resolved in the unit's worktree after it took trunk in.
  mg escalation add --kind approval --unit U91 --summary "an allowlist entry"
  [ "$status" -eq 0 ]
  id="$(field .details.escalation.id)"
  worktree "$b" unit/U91 implementer
  commit_in "$b" docs/b.md "b"
  mg_in "$b" escalation resolve "$id" --by antoine --resolution "approved in chat"
  [ "$status" -eq 0 ]
  mg escalation list --open --unit U91
  [ "$(field '.details.escalations | length')" = 1 ]
  mg merge unit/U91
  [ "$status" -eq 0 ]
  [ "$(git -C "$SB" rev-parse trunk)" = "$(git -C "$b" rev-parse HEAD)" ]
}

@test "merge refuses a branch whose escalation ledger does not parse, rather than reading it as empty" {
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  commit_in "$wt" docs/process/provenance/escalations.jsonl '<<<<<<< ours
{"id": "E-1", "kind": "approval", "state": "open", "unit": "U90", "summary": "s"}
>>>>>>> theirs'
  mg merge unit/U90
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = escalation-ledger ]
  [ "$(git -C "$SB" rev-parse trunk)" = "$BASE0" ]
}

# --- runs queued by the machine's state, and their catch-up (U19b) ----------

QUEUE=build/mg/catch-up/queue.jsonl

# queue_merge <dir> <unit>: a unit whose app/ change runs `ui`, merged from a
# new worktree on trunk. Whatever $FX/ui.* says decides the run.
queue_merge() {
  worktree "$1" "unit/$2" implementer
  commit_in "$1" "app/Meguri/$2.swift" "struct V {}" "Change a view

$TRAILERS"
  mg merge "unit/$2"
}

# queue_states: each queue entry's state, in the order the entries came.
queue_states() {
  jq -rs 'reduce .[] as $l ({order: [], by: {}}; (if .by[$l.id] then . else .order += [$l.id] end) | .by[$l.id] += $l)
    | [.order[] as $id | .by[$id].state] | join(" ")' "$SB/$QUEUE"
}

@test "a complete clean direct-master unit gate queues blocked runs for catch-up" {
  commit_in "$SB" app/view.swift 'struct View {}' $'direct unit\n\nRole: infra\nUnit: U150'
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ]
  [ "$(rq .merge_ready)" = true ]
  [ -f "$SB/$QUEUE" ]
  [ "$(queue_states)" = queued ]
  [ "$(entry 0 .unit)" = U150 ]
  [ "$(entry 0 .commit)" = "$(git -C "$SB" rev-parse HEAD)" ]
  fx ui pass
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(queue_states)" = done ]
}

@test "partial local and dirty direct-master gates never enqueue acceptance work" {
  commit_in "$SB" app/view.swift 'struct View {}' $'direct unit\n\nRole: infra\nUnit: U150'
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  mg gate --base "$BASE0" --only ui
  [ "$status" -eq 0 ]
  [ ! -f "$SB/$QUEUE" ]
  mg gate --base "$BASE0" --local
  [ "$status" -eq 0 ]
  [ ! -f "$SB/$QUEUE" ]
  printf '\n// WIP\n' >>"$SB/app/view.swift"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ]
  [ "$(rq .dirty)" = true ]
  [ ! -f "$SB/$QUEUE" ]
}

# entry <n> <jq filter>: the filter on the n-th queue entry (from 0), its
# lines merged in order.
entry() {
  jq -rs --argjson n "$1" "reduce .[] as \$l ({order: [], by: {}}; (if .by[\$l.id] then . else .order += [\$l.id] end) | .by[\$l.id] += \$l)
    | .by[.order[\$n]] | $2" "$SB/$QUEUE"
}

@test "a run the locked host blocked is queued: its receipt names the user step and the parts, does not block, and the merge queues it for catch-up" {
  local wt="$BATS_TEST_TMPDIR/U150" tip
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$wt" U150
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  tip="$(git -C "$wt" rev-parse HEAD)"
  [ "$(git -C "$SB" rev-parse trunk)" = "$tip" ]
  [ "$(field .status)" = warn ]
  [ "$(outcome ui)" = queued ]
  [ "$(rq '.blocking | length')" = 0 ]
  [ "$(rq '.runs[] | select(.id == "ui") | "\(.user_steps | join(",")) \([.queued_parts[].part] | join(","))"')" = "host-session the devices phase" ]
  [ "$(rq '.queued[] | "\(.id) \(.job) \(.args | join(" ")) \(.user_steps | join(",")) \([.parts[].part] | join(","))"')" = "ui t-job ui host-session the devices phase" ]
  [[ "$(field .message)" == *"queued: ui (host-session)"* ]]

  # One entry, with what catch-up needs to run it again and whom it concerns.
  [ "$(jq -s length "$SB/$QUEUE")" = 1 ]
  [ "$(entry 0 '"\(.state) \(.job) \(.args | join(" ")) \(.unit) \(.branch) \(.user_steps | join(",")) \([.parts[].part] | join(","))"')" = \
    "queued t-job ui U150 unit/U150 host-session the devices phase" ]
  [ "$(entry 0 .commit)" = "$tip" ]
  [ "$(entry 0 .merge)" = "$(field .run_id)" ]
  [ "$(jq -r .status "$(entry 0 .result)")" = warn ]
  validate "$result"
}

@test "catch-up waits while doctor lists the user step, then runs each queued run once on the head for every unit that queued it" {
  local a="$BATS_TEST_TMPDIR/U150" b="$BATS_TEST_TMPDIR/U241" head
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$a" U150
  [ "$status" -eq 0 ]
  queue_merge "$b" U241
  [ "$status" -eq 0 ]
  [ "$(queue_states)" = "queued queued" ]
  head="$(git -C "$SB" rev-parse trunk)"

  # The Mac is still locked: nothing runs and the entries stay.
  printf 'host-session\n' >"$FX/doctor.missing"
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(called '^doctor @main$')" = 1 ]
  [ "$(called '^t-job ui')" = 0 ]
  [ "$(queue_states)" = "queued queued" ]
  [ "$(field '[.details.waiting[] | "\(.unit):\(.user_steps | join(","))"] | join(" ")')" = "U150:host-session U241:host-session" ]

  # Unlocked: one run on the head for both entries, which are done.
  rm "$FX/doctor.missing"
  fx ui pass
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(called '^t-job ui @main$')" = 1 ]
  [ "$(queue_states)" = "done done" ]
  [ "$(field '.details.runs | length')" = 1 ]
  [ "$(field '.details.runs[0] | "\(.job) \(.args | join(" ")) \(.status) \(.entries | length)"')" = "t-job ui pass 2" ]
  [ "$(jq -r .commit.sha "$(field '.details.runs[0].result')")" = "$head" ]
  [ "$(entry 1 .head)" = "$head" ]
  [ "$(entry 1 .catch_up)" = "$(field .run_id)" ]

  # Nothing is left to run.
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(called '^t-job ui @main$')" = 1 ]
  validate "$result"
}

@test "catch-up reuses a passing clean head result even when its former user step is missing again" {
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$BATS_TEST_TMPDIR/U150" U150
  [ "$status" -eq 0 ]
  fx ui pass
  mg t-job ui
  [ "$status" -eq 0 ]
  local proof="$result"
  printf 'host-session\n' >"$FX/doctor.missing"
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(called '^t-job ui')" = 0 ]
  [ "$(queue_states)" = done ]
  [ "$(field '.details.runs[0].result')" = "$proof" ]
  [ "$(field '.details.runs[0].reused')" = true ]
  [ "$(field '.summary.reused')" = 1 ]
  [ "$(field '.summary.waiting')" = 0 ]
  [ "$(entry 0 .reused)" = true ]
}

@test "catch-up refuses reuse if HEAD or cleanliness changes during evidence validation" {
  cat >>"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
jq() {
  local rc last="${!#}"
  command jq "$@"
  rc=$?
  if [ -f "$FX/race-proof" ] && [ "$last" = "$(cat "$FX/race-proof")" ]; then
    rm "$FX/race-proof"
    case "$(cat "$FX/race-mode")" in
      commit) git -C "$MG_ROOT" -c core.hooksPath=/dev/null commit -q --allow-empty -m 'Concurrent commit' ;;
      dirty) printf 'concurrent change\n' >>"$MG_ROOT/docs/notes.md" ;;
      untracked) printf 'concurrent file\n' >"$MG_ROOT/concurrent.txt" ;;
    esac
  fi
  return "$rc"
}
EOF
  git -C "$SB" add tools/mg.d/zz-fakes.sh
  git -C "$SB" commit -q -m 'Install validation race fixture'
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$BATS_TEST_TMPDIR/U150" U150
  cp "$SB/$QUEUE" "$FX/queued.original"
  local mode
  for mode in commit dirty untracked; do
    cp "$FX/queued.original" "$SB/$QUEUE"
    fx ui pass
    mg t-job ui
    [ "$status" -eq 0 ]
    printf '%s\n' "$result" >"$FX/race-proof"
    printf '%s\n' "$mode" >"$FX/race-mode"
    : >"$STUB_LOG"
    mg catch-up
    [ ! -f "$FX/race-proof" ]
    if [ "$mode" = commit ]; then
      [ "$status" -eq 0 ]
      [ "$(called '^t-job ui')" = 1 ]
      [ "$(field '.details.runs[0].reused')" = false ]
    else
      [ "$status" -eq 2 ]
      [ "$(called '^t-job ui')" = 0 ]
      [ "$(field '.failures[0].id')" = main-dirty ]
      [ "$(queue_states)" = queued ]
    fi
    git -C "$SB" checkout -q -- docs/notes.md
    rm -f "$SB/concurrent.txt"
  done
}

@test "catch-up reruns after the head changes rather than reusing an older passing result" {
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$BATS_TEST_TMPDIR/U150" U150
  fx ui pass
  mg t-job ui
  [ "$status" -eq 0 ]
  commit_in "$SB" docs/next.md 'a different commit'
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(called '^t-job ui')" = 1 ]
  [ "$(queue_states)" = done ]
  [ "$(field '.details.runs[0].reused')" = false ]
}

@test "catch-up does not reuse a result with different arguments or a later failed run" {
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$BATS_TEST_TMPDIR/U150" U150
  cp "$SB/$QUEUE" "$FX/queued.original"
  fx ui pass
  mg t-job ui
  mg t-job other
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(called '^t-job ui')" = 1 ]
  [ "$(field '.details.runs[0].reused')" = false ]
  cp "$FX/queued.original" "$SB/$QUEUE"
  fx ui fail
  mg t-job ui
  [ "$status" -eq 1 ]
  fx ui pass
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(called '^t-job ui')" = 1 ]
  [ "$(field '.details.runs[0].reused')" = false ]
}

@test "catch-up refuses dirty dry queued foreign-root or malformed passing evidence" {
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$BATS_TEST_TMPDIR/U150" U150
  cp "$SB/$QUEUE" "$FX/queued.original"
  fx ui pass
  mg t-job ui
  [ "$status" -eq 0 ]
  local proof="$result" pristine mutation
  pristine="$(cat "$proof")"
  for mutation in '.commit.dirty = true' '.dry_run = true' '.details.queued = true' \
    '.details.queued_parts = [{user_step:"host-session"}]' '.root = "/elsewhere"' \
    '.status = "warn"' '.failures = [{id:"broken",message:"broken",repro:"tools/mg t-job ui"}]' \
    'del(.commit)' 'null' 'del(.run_id)' 'del(.started_at)' 'del(.duration_ms)' \
    'del(.placement)' 'del(.service)' 'del(.message)' 'del(.artifacts)' \
    '.user_steps = [{}]' '.summary.total = -1' '.summary.passed = 1.5' \
    '.run_id = "invalid"' '.started_at = "yesterday"' '.artifacts = [{kind: 1, path: "relative"}]'; do
    cp "$FX/queued.original" "$SB/$QUEUE"
    printf '%s\n' "$pristine" | jq "$mutation" >"$proof"
    ln -sfn "$(basename "$(dirname "$proof")")" "$SB/build/mg/t-job/latest"
    : >"$STUB_LOG"
    mg catch-up
    [ "$status" -eq 0 ]
    [ "$(called '^t-job ui')" = 1 ]
    [ "$(queue_states)" = done ]
    [ "$(field '.details.runs[0].reused')" = false ]
  done
}

@test "a caught-up run that fails records a blocked-red escalation for the unit that queued it; other units still merge" {
  local a="$BATS_TEST_TMPDIR/U150" b="$BATS_TEST_TMPDIR/U91" ledger="$SB/docs/process/provenance/escalations.jsonl"
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$a" U150
  [ "$status" -eq 0 ]
  fx ui fail "ui-broke"
  mg catch-up
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(queue_states)" = failed ]
  [ "$(jq -s '[.[] | select(.kind == "blocked-red" and .unit == "U150" and .state == "open")] | length' "$ledger")" = 1 ]
  [ "$(entry 0 .escalation)" = "$(jq -rs '.[0].id' "$ledger")" ]
  [ "$(jq -rs '.[0].detail.run_dir' "$ledger")" = "$(dirname "$(field '.details.runs[0].result')")" ]
  [ "$(jq -rs '.[0].detail.job' "$ledger")" = t-job ]
  jq -e '.failures[] | select(.id == "rerun:t-job ui" and (.message | contains("U150")))' "$result"
  # The escalation is committed on trunk.
  [ "$(git -C "$SB" log -1 --format=%s trunk)" = "Record escalation $(jq -rs '.[0].id' "$ledger") (blocked-red, U150)" ]

  # Not a pause: a unit that does not depend on it merges, and the failed
  # entry does not run again.
  worktree "$b" unit/U91 implementer
  commit_in "$b" docs/b.md "b"
  mg merge unit/U91
  [ "$status" -eq 0 ]
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(called '^t-job ui')" = 0 ]
}

@test "a caught-up run queued again waits for its new user step; catch-up runs only in the main checkout, on a clean tree, after doctor" {
  local a="$BATS_TEST_TMPDIR/U150"
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$a" U150
  [ "$status" -eq 0 ]

  # Unlocked, the run gets further and is blocked by macOS's dropped input.
  printf 'mac-input test-hooks\n' >"$FX/ui.queued"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(queue_states)" = queued ]
  [ "$(entry 0 '"\(.user_steps | join(",")) \([.parts[].part] | join(","))"')" = "mac-input test-hooks" ]
  printf 'mac-input\n' >"$FX/doctor.missing"
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(called '^t-job ui')" = 0 ]

  # Doctor that could not run decides nothing.
  rm "$FX/doctor.missing"
  printf 'error\n' >"$FX/doctor.status"
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = doctor ]
  [ "$(called '^t-job ui')" = 0 ]
  rm "$FX/doctor.status"

  # A tracked change in the main checkout: the run would not test a commit.
  printf 'changed\n' >>"$SB/docs/notes.md"
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = main-dirty ]
  git -C "$SB" checkout -q -- docs/notes.md
  mg_in "$a" catch-up
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = not-main ]
  [ "$(called '^t-job ui')" = 0 ]
  [ "$(queue_states)" = queued ]
}

@test "a caught-up run whose escalation cannot be recorded stays escalation-pending, and the next catch-up records it without running again" {
  local a="$BATS_TEST_TMPDIR/U150" ledger="$SB/docs/process/provenance/escalations.jsonl" lock first
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$a" U150
  [ "$status" -eq 0 ]

  # Another record holds the ledgers' lock past its wait: no escalation.
  lock="$(git -C "$SB" rev-parse --path-format=absolute --git-common-dir)/meguri-session.lock"
  mkdir "$lock"
  export MG_SESSION_LOCK_SECONDS=1
  fx ui fail ui-broke
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(queue_states)" = escalation-pending ]
  first="$(field '.details.runs[0].result')"
  [ "$(entry 0 '"\(.rerun) \(.rerun_status) \(.units | join(","))"')" = "$first fail U150" ]
  jq -e '.failures[] | select(.id == "escalation:U150" and (.message | contains("escalation-pending")))' "$result"
  jq -e '.failures[] | select(.id == "rerun:t-job ui")' "$result"
  [ ! -e "$ledger" ]

  # The ledger is free again: the escalation is recorded from the saved
  # result, without running anything again.
  rmdir "$lock"
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(called '^t-job ui')" = 0 ]
  [ "$(called '^doctor')" = 0 ]
  [ "$(queue_states)" = failed ]
  [ "$(jq -s '[.[] | select(.kind == "blocked-red" and .unit == "U150" and .state == "open")] | length' "$ledger")" = 1 ]
  [ "$(entry 0 .escalation)" = "$(jq -rs '.[0].id' "$ledger")" ]
  [ "$(jq -rs '.[0].detail | "\(.result) \(.job) \(.entries | length)"' "$ledger")" = "$first t-job 1" ]
  [ "$(field '.details.escalations[0] | "\(.unit) \(.late) \(.escalation)"')" = "U150 true $(jq -rs '.[0].id' "$ledger")" ]
  validate "$result"

  # Recorded once.
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(jq -s length "$ledger")" = 1 ]
}

@test "an escalation appended but not committed is committed by the next catch-up before its clean-tree check, and counts only once committed" {
  local a="$BATS_TEST_TMPDIR/U150" rel=docs/process/provenance/escalations.jsonl ledger hooks id
  ledger="$SB/$rel"
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$a" U150
  [ "$status" -eq 0 ]

  # The main checkout's commits fail (its pre-commit hook refuses them): the
  # escalation is appended to the ledger, but not committed.
  hooks="$(git -C "$SB" rev-parse --path-format=absolute --git-path hooks)"
  mkdir -p "$hooks"
  printf '#!/bin/sh\n[ ! -e "%s" ] || { echo "made-up refusal" >&2; exit 1; }\n' "$FX/refuse-commits" >"$hooks/pre-commit"
  chmod +x "$hooks/pre-commit"
  : >"$FX/refuse-commits"
  fx ui fail ui-broke
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(queue_states)" = escalation-pending ]
  jq -e '.failures[] | select(.id == "escalation:U150" and (.message | contains("could not commit")))' "$result"
  [ "$(jq -s length "$ledger")" = 1 ]
  id="$(jq -rs '.[0].id' "$ledger")"
  ! git -C "$SB" rev-parse -q --verify "HEAD:$rel"

  # Still refused: the escalation in the working tree does not count as
  # recorded, its entries stay escalation-pending, and nothing runs.
  : >"$STUB_LOG"
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = escalation-ledger ]
  [[ "$(field '.failures[0].message')" == *"appended but not committed"* ]]
  [ "$(queue_states)" = escalation-pending ]
  [ "$(called '^t-job ui')" = 0 ]
  [ "$(called '^doctor')" = 0 ]

  # Commits work again: the next catch-up commits the line, then marks the
  # entries failed with that escalation, without running anything again.
  rm "$FX/refuse-commits"
  mg catch-up
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(queue_states)" = failed ]
  [ "$(entry 0 .escalation)" = "$id" ]
  [ "$(field '.details.escalations[0] | "\(.unit) \(.late) \(.escalation)"')" = "U150 true $id" ]
  [ "$(jq -s length "$ledger")" = 1 ]
  [ "$(git -C "$SB" show "HEAD:$rel")" = "$(cat "$ledger")" ]
  [ -z "$(git -C "$SB" status --porcelain -- "$rel")" ]
  [ "$(git -C "$SB" log -1 --format=%s trunk)" = "Record escalation $id (blocked-red, U150)" ]
  [ "$(called '^t-job ui')" = 0 ]
  validate "$result"

  # A ledger line changed by hand is not settled: catch-up stops there.
  printf '%s\n' '{"id": "E-made-up", "state": "open"}' >"$ledger"
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = escalation-ledger ]
  [[ "$(field '.failures[0].message')" == *"differs from HEAD's by more than appended lines"* ]]
  [ "$(git -C "$SB" show "HEAD:$rel" | jq -rs '.[0].id')" = "$id" ]
}

@test "catch-up refuses a queue it cannot read instead of reading it as empty" {
  local a="$BATS_TEST_TMPDIR/U150"
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$a" U150
  [ "$status" -eq 0 ]
  fx ui pass
  : >"$STUB_LOG"

  # A line cut short (an append that did not finish).
  printf '{"id": "Q-cut", "sta' >>"$SB/$QUEUE"
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = queue-unreadable ]
  [[ "$(field '.failures[0].message')" == *"$QUEUE cannot be read"* ]]
  [ "$(called '^doctor')" = 0 ]
  [ "$(called '^t-job ui')" = 0 ]

  # A line that is JSON, but not an entry.
  printf '%s\n' "$(head -n 1 "$SB/$QUEUE")" '["Q-x"]' >"$SB/$QUEUE.new" && mv "$SB/$QUEUE.new" "$SB/$QUEUE"
  mg catch-up
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = queue-unreadable ]
  [[ "$(field '.failures[0].message')" == *'not a queue entry: ["Q-x"]'* ]]
  [ "$(called '^t-job ui')" = 0 ]

  # Repaired, the entry runs.
  head -n 1 "$SB/$QUEUE" >"$SB/$QUEUE.new" && mv "$SB/$QUEUE.new" "$SB/$QUEUE"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(queue_states)" = done ]
}

@test "a catch-up started while another works the queue runs nothing; a dead holder's lock is taken over" {
  local a="$BATS_TEST_TMPDIR/U150" out="$BATS_TEST_TMPDIR/first.out" first dead lock="$SB/build/mg/catch-up/lock"
  fx ui queued
  printf 'host-session the devices phase\n' >"$FX/ui.queued"
  queue_merge "$a" U150
  [ "$status" -eq 0 ]
  fx ui pass
  printf '3\n' >"$FX/ui.sleep"
  : >"$STUB_LOG"
  "$SB/tools/mg" catch-up >"$out" 2>/dev/null 3>&- &
  first=$!
  for _ in $(seq 1 100); do [ "$(called '^t-job ui')" = 1 ] && break; sleep 0.1; done
  [ "$(called '^t-job ui')" = 1 ]

  # The first one is running the entry's job: the second leaves it alone.
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [[ "$(field .message)" == *"another catch-up ($(jq -r .run_id "$lock/holder.json"), pid $first"*"is working the queue; this run did nothing"* ]]
  [ "$(field .details.busy.job)" = catch-up ]
  [ "$(called '^doctor')" = 1 ]
  wait "$first"
  [ "$(jq -r .status "$(tail -n 1 "$out")")" = pass ]
  [ "$(called '^t-job ui')" = 1 ]
  [ "$(queue_states)" = done ]
  [ "$(jq -s length "$SB/$QUEUE")" = 2 ]
  [ ! -e "$lock" ]

  # A catch-up that died holding the lock does not keep others out.
  sleep 0 &
  dead=$!
  wait "$dead" || true
  mkdir "$lock"
  jq -n --argjson pid "$dead" '{job: "catch-up", run_id: "gone", pid: $pid}' >"$lock/holder.json"
  mg catch-up
  [ "$status" -eq 0 ]
  [ "$(field .message)" = "catch-up: nothing is queued" ]
  [ ! -e "$lock" ]
}

# wt_outcome <run id>: the run's outcome in the worktree gate a merge ran.
wt_outcome() {
  jq -r --arg id "$1" '.details.receipt.runs[] | select(.id == $id) | .outcome' \
    "$(jq -r '.artifacts[] | select(.kind == "gate-result") | .path' "$result")"
}

@test "a branch that changes oracle/ gets its oracle runs from the merge, in a full checkout of the tip" {
  local wt="$BATS_TEST_TMPDIR/U90" spec="$BATS_TEST_TMPDIR/U90-spec" tip live
  worktree "$wt" unit/U90 implementer
  worktree "$spec" spec/U90 oracle-maintainer
  commit_in "$spec" oracle/driver/ops.py 'print("a new op")' "Add a driver op"
  git -C "$wt" merge -q --no-edit spec/U90
  [ ! -e "$wt/oracle" ]
  commit_in "$wt" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  tip="$(git -C "$wt" rev-parse HEAD)"
  mg merge unit/U90
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(git -C "$SB" rev-parse trunk)" = "$tip" ]
  # The worktree left them to the merge: the main checkout's oracle/ is not
  # this branch's, so they never ran there.
  [ "$(wt_outcome live)" = deferred-tip ]
  [ "$(wt_outcome otest)" = deferred-tip ]
  [ "$(called '^t-oracle.* @U90 ')" = 0 ]
  [ "$(called '^t-oracle live sched smoke @merge-tip oracle=own$')" = 1 ]
  [ "$(called '^t-oracle-own otest @merge-tip$')" = 1 ]
  [ "$(outcome live)" = pass ]
  [ "$(outcome otest)" = pass ]
  # Their results outlive the checkout, which is gone again.
  live="$(rq '.runs[] | select(.id == "live") | .result')"
  [[ "$live" == "$(dirname "$result")/tip/live/"* ]]
  [ -f "$live" ]
  [ -f "$(jq -r '.artifacts[] | select(.label == "T1 report (live)") | .path' "$live")" ]
  [ ! -e "$SB/build/mg/merge-tip" ]
  [ -z "$(git -C "$SB" worktree list | grep merge-tip)" ]
  validate "$result"
}

@test "a branch that bumps the oracle pin gets its oracle metadata checks from the merge, in a full checkout of the tip" {
  local wt="$BATS_TEST_TMPDIR/U90" spec="$BATS_TEST_TMPDIR/U90-spec" next="$BATS_TEST_TMPDIR/U91" tip
  worktree "$wt" unit/U90 implementer
  worktree "$spec" spec/U90 oracle-maintainer
  # A bump: the new lock and its pin, re-inventoried in the same change (R1882).
  printf 'anki==25.10\n' | tee "$spec/oracle/requirements.lock" >"$spec/conformance/oracle.lock"
  printf 'test_a,25.10\ntest_b,25.10\n' | tee "$spec/oracle/harvest/links.csv" >"$spec/conformance/catalog/harvest.csv"
  git -C "$spec" add -A
  git -C "$spec" commit -qm "Bump the oracle to 25.10"
  git -C "$wt" merge -q --no-edit spec/U90
  commit_in "$wt" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  tip="$(git -C "$wt" rev-parse HEAD)"
  mg merge unit/U90
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(git -C "$SB" rev-parse trunk)" = "$tip" ]
  # The main checkout's oracle/ still had the old lock and inventory, so the
  # worktree left both checks to the merge instead of failing them.
  [ "$(wt_outcome goldens)" = deferred-tip ]
  [ "$(wt_outcome catalog)" = deferred-tip ]
  [ "$(called '^t-oracle-files .* @U90 ')" = 0 ]
  [ "$(called '^t-oracle-files goldens @merge-tip oracle=own$')" = 1 ]
  [ "$(called '^t-oracle-files catalog @merge-tip oracle=own$')" = 1 ]
  [ "$(outcome goldens)" = pass ]
  [ "$(outcome catalog)" = pass ]
  [ "$(rq .merge_ready)" = true ]

  # Integrated, the bump is the main checkout's: the next branch checks
  # against it in its own worktree.
  worktree "$next" unit/U91 implementer
  commit_in "$next" conformance/scenarios/smoke/d.yaml "id: smoke/d"
  : >"$STUB_LOG"
  mg_in "$next" gate --local --base "$tip"
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(outcome goldens)" = pass ]
  [ "$(called '^t-oracle-files goldens @U91 oracle=main$')" = 1 ]
}

@test "oracle test refuses an implementer worktree (no-oracle-tree), so the merge runs it in a full checkout of the tip" {
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  printf '\n# a note\n' >>"$wt/tools/mg.d/oracle.sh"
  git -C "$wt" commit -qam "Touch the oracle jobs"
  mg merge unit/U90
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(called '^t-oracle-own otest @U90 refused$')" = 1 ]
  [ "$(wt_outcome otest)" = needs-oracle-tree ]
  [ "$(called '^t-oracle-own otest @merge-tip$')" = 1 ]
  [ "$(outcome otest)" = pass ]
  # The live suites ran in the worktree: the main checkout's oracle/ is this branch's.
  [ "$(called '^t-oracle live @U90 oracle=main$')" = 1 ]
  [ "$(outcome live)" = pass ]
  [ "$(git -C "$SB" rev-parse trunk)" = "$(git -C "$wt" rev-parse HEAD)" ]
}

@test "the merge's tip checkout serves one merge at a time, and a run that writes to it keeps the branch unmerged" {
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  printf '\n# a note\n' >>"$wt/tools/mg.d/oracle.sh"
  git -C "$wt" commit -qam "Touch the oracle jobs"
  mkdir -p "$SB/build/mg/merge-tip"
  : >"$SB/build/mg/merge-tip/busy"
  mg merge unit/U90
  [ "$status" -eq 1 ]
  jq -e '.failures[] | select(.id == "tip-checkout")' "$result"
  [ "$(outcome otest)" = error ]
  [ -f "$SB/build/mg/merge-tip/busy" ]
  [ "$(git -C "$SB" rev-parse trunk)" = "$BASE0" ]

  rm -rf "$SB/build/mg/merge-tip"
  printf 'stray.txt\n' >"$FX/otest.write"
  mg merge unit/U90
  [ "$status" -eq 1 ]
  [ "$(outcome otest)" = pass ]
  jq -e '.failures[] | select(.id == "tree-changed:tip")' "$result"
  [ "$(git -C "$SB" rev-parse trunk)" = "$BASE0" ]
  [ ! -e "$SB/build/mg/merge-tip" ]
}

@test "an oracle job that refuses its oracle/ needs the branch's tree, whether it fails or errs" {
  commit_in "$SB" core/crates/meguri-sched/src/lib.rs "pub fn f() {}" "Change sched

$TRAILERS"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ]
  local dir r="$BATS_TEST_TMPDIR/live-result.json" o="$BATS_TEST_TMPDIR/otest-result.json"
  dir="$(dirname "$result")"
  # conformance --live when mg_oracle__tree refuses: it runs its other lanes
  # and ends fail; oracle test ends error.
  jq -n '{status: "fail", message: "conformance: T0 and T1 failed",
    failures: [{id: "oracle-tree-differs", message: "this branch changes oracle/"}, {id: "live/plan", message: "could not list"}]}' >"$r"
  jq -n '{status: "error", message: "oracle: refused", failures: [{id: "no-oracle-tree", message: "no oracle/"}]}' >"$o"
  jq -c --arg r "$r" 'if .id == "live" then .result = $r else . end' "$dir/results.jsonl" >"$BATS_TEST_TMPDIR/results.jsonl"
  python3 -B -I "$SB/tools/gate/gate.py" evaluate --config "$dir/config.json" --plan "$dir/plan.json" \
    --results "$BATS_TEST_TMPDIR/results.jsonl" --now "$MG_GATE_NOW" --out "$BATS_TEST_TMPDIR/receipt.json"
  [ "$(jq -r '.runs[] | select(.id == "live") | .outcome' "$BATS_TEST_TMPDIR/receipt.json")" = needs-oracle-tree ]
  [[ "$(jq -r '.blocking[] | select(.id == "run:live") | .message' "$BATS_TEST_TMPDIR/receipt.json")" == *"full checkout of the tip"* ]]

  jq -c --arg o "$o" 'if .id == "live" then .result = $o else . end' "$dir/results.jsonl" >"$BATS_TEST_TMPDIR/results.jsonl"
  python3 -B -I "$SB/tools/gate/gate.py" evaluate --config "$dir/config.json" --plan "$dir/plan.json" \
    --results "$BATS_TEST_TMPDIR/results.jsonl" --now "$MG_GATE_NOW" --out "$BATS_TEST_TMPDIR/receipt.json"
  [ "$(jq -r '.runs[] | select(.id == "live") | .outcome' "$BATS_TEST_TMPDIR/receipt.json")" = needs-oracle-tree ]
}

@test "a branch's similarity self-test runs the branch's scanner in a full checkout of the tip, with the main checkout's corpus" {
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  commit_in "$wt" tools/similarity/marker.txt "branch" "Change the scanner"
  mg_in "$wt" gate --base "$BASE0"
  [ "$(outcome stest)" = deferred-tip ]
  mg merge unit/U90
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(called "^t-scan-selftest @merge-tip marker=branch refs=$SB/refs$")" = 1 ]
  [ "$(called '^t-scan-selftest @main ')" = 0 ]
  [ "$(outcome stest)" = pass ]
}

# --- nightly and milestone runs ----------------------------------------------

@test "nightly runs the nightly T2 runs with one logged seed; --weekly adds the weekly ones (R1937)" {
  mg nightly --seed 77
  [ "$status" -eq 0 ]
  [ "$(called '^t-job fuzz --seed 77 @main$')" = 1 ]
  [ "$(called 't-job soak')" = 0 ]
  [ "$(called 't-job unit')" = 0 ]
  [ "$(field .details.seed)" = 77 ]
  [[ "$(field .message)" == *"seed 77"* ]]
  jq -e '.user_steps[] | select(.id == "nightly-scheduler" and .state == "missing")' "$result"
  validate "$result"

  mg nightly --weekly
  [ "$status" -eq 0 ]
  [ "$(called 't-job soak')" = 1 ]
  [ "$(field '.details.seed | type')" = number ]

  mg nightly --print-schedule
  [ "$status" -eq 0 ]
  # launchd treats a missing calendar key as "every": the weekly entry needs
  # its minute, or it starts again each minute of 04:00-04:59.
  python3 - "$(dirname "$result")" <<'PY'
import plistlib, sys
d = sys.argv[1]
with open(d + "/com.meguri.nightly.plist", "rb") as f:
    nightly = plistlib.load(f)
with open(d + "/com.meguri.nightly.weekly.plist", "rb") as f:
    weekly = plistlib.load(f)
assert nightly["StartCalendarInterval"] == {"Hour": 2, "Minute": 30}, nightly["StartCalendarInterval"]
assert weekly["StartCalendarInterval"] == {"Weekday": 0, "Hour": 4, "Minute": 0}, weekly["StartCalendarInterval"]
assert nightly["ProgramArguments"][1:] == ["nightly"], nightly["ProgramArguments"]
assert weekly["ProgramArguments"][1:] == ["nightly", "--weekly"], weekly["ProgramArguments"]
PY
}

@test "a milestone run fails on one unclassified harvest row (R1880) and waits on device runs" {
  printf 'unclassified 1\n' >"$FX/harvest.counts"
  mg milestone M1
  [ "$status" -eq 1 ]
  jq -e '.failures[] | select(.id == "t-job:R1880-unclassified")' "$result"
  # A milestone's runs run in full (no path triggers): live with every area.
  [ "$(called '^t-job unit @main$')" = 1 ]
  [ "$(called '^t-job live smoke @main$')" = 0 ]
  [ "$(called '^t-job live @main$')" = 1 ]

  printf 'unclassified 0\n' >"$FX/harvest.counts"
  mg milestone M1
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field '.details.awaiting_user[0].id')" = device ]
  # Release-only runs are not part of a milestone run.
  [ -z "$(outcome audit)" ]

  # At release every in-scope scenario passes too.
  printf '{"harvest": {"by_classification": {"in-scope": {"rows": 4, "statuses": {"pass": 3, "pending": 1}}}}}\n' >"$FX/harvest.details"
  mg milestone M1
  [ "$status" -eq 0 ]
  mg milestone release
  [ "$status" -eq 1 ]
  [[ "$(jq -r '.failures[] | select(.id == "t-job:R1880-in-scope") | .message' "$result")" == *"1 pending"* ]]
  [ "$(outcome audit)" = pending ]
}

@test "a release run gives harvest coverage the live oracle run's T1 report, so a harvested live chain that passed counts (R1880)" {
  local report
  printf '%s\n' '[{"scenario": "storage/open/oracle-made-reopen", "classification": "in-scope", "status": "live"},
    {"scenario": "sched/b", "classification": "in-scope", "status": "pass"}]' >"$FX/harvest.rows"
  printf '%s\n' '{"tier": "t1", "scenarios": [{"scenario": "storage/open/oracle-made-reopen", "status": "pass"}]}' >"$FX/live.t1"
  mg milestone release
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(outcome harvest)" = pass ]
  report="$(dirname "$(rq '.runs[] | select(.id == "live") | .result')")/t1-live.json"
  [ -f "$report" ]
  [ "$(called "^t-job harvest --live-report $report @main$")" = 1 ]

  # Without a live pass the chain stays `live`, which a release refuses.
  printf '%s\n' '{"tier": "t1", "scenarios": []}' >"$FX/live.t1"
  mg milestone release
  [ "$status" -eq 1 ]
  [[ "$(jq -r '.failures[] | select(.id == "t-job:R1880-in-scope") | .message' "$result")" == *"1 live"* ]]
}

@test "a run whose {artifact:...} an earlier run did not write is unresolved and fails the run" {
  printf 'unclassified 0\n' >"$FX/harvest.counts"
  printf 'fail\n' >"$FX/unit.status"
  mg milestone M1
  [ "$status" -eq 1 ]
  # T0 failed, so T1 (the live run) did not start and wrote no report.
  [ "$(outcome live)" = skipped-after-t0 ]
  [ "$(outcome harvest)" = unresolved ]
  [ "$(called 't-job harvest')" = 0 ]
  [[ "$(jq -r '.failures[] | select(.id == "run:harvest") | .message' "$result")" == *"{artifact:live:t1-live.json}"* ]]
}

@test "a milestone run fails while a sync test is quarantined" {
  local t='meguri-sync::x$y' failed passed
  commit_in "$SB" core/crates/meguri-sync/src/lib.rs "pub fn f() {}" "Change sync

$TRAILERS"
  printf 'fail\npass\n' >"$FX/unit.seq"
  printf '%s\n' "$t" >"$FX/unit.failures"
  mg gate --base "$BASE0"
  failed="$(field .run_id)"
  mg gate --base "$BASE0"
  passed="$(field .run_id)"
  mg quarantine add --run unit --test "$t" --class sync --owner U105 --reason flaky --deadline 2026-10-30 \
    --failed "$failed" --passed "$passed" --approved-by antoine
  [ "$status" -eq 0 ]
  git -C "$SB" commit -qam "Quarantine"
  mg gate --base "$BASE0"
  [ "$status" -eq 0 ]
  mg milestone M1
  [ "$status" -eq 1 ]
  jq -e --arg t "quarantine-protected:unit:$t" '.failures[] | select(.id == $t)' "$result"
  # From M1R on, every milestone (U240r).
  mg milestone M1R
  [ "$status" -eq 1 ]
  jq -e --arg t "quarantine-protected:unit:$t" '.failures[] | select(.id == $t)' "$result"
}

@test "a milestone run takes the milestone names from the manifest (U240r)" {
  printf 'unclassified 0\n' >"$FX/harvest.counts"
  mg milestone M1R
  [ "$status" -eq 0 ]
  [ "$(field .details.milestone)" = M1R ]
  [ "$(called '^t-job unit @main$')" = 1 ]
  mg milestone M1X
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  [[ "$(field '.failures[0].message')" == *"unknown milestone 'M1X'"*"are M0, M1R, M1, M2, M3, M4, M5"* ]]
  [ "$(called '^t-job unit @main$')" = 1 ]
}

@test "a milestone run runs only the runs its gate reads, never a later milestone's suite (U240)" {
  printf 'unclassified 0\n' >"$FX/harvest.counts"
  # ui is first a suite of M1: any run that started it would fail.
  printf 'fail\n' >"$FX/ui.status"
  mg milestone M1R
  [ "$status" -eq 0 ] || { jq . "$(receipt)"; return 1; }
  [ "$(field .status)" = warn ]
  [ "$(called 't-job ui')" = 0 ]
  [ "$(outcome ui)" = not-triggered ]
  # M0's suites and M1R's own, the live run whose report harvest takes, the
  # oracle check that applies from M1R, and M1R's device run (awaiting).
  [ "$(field '.details.runs | join(" ")')" = "conf device harvest live otest static unit" ]
  [ "$(called '^t-job unit @main$')" = 1 ]
  [ "$(called '^t-job conf @main$')" = 1 ]
  [ "$(called '^t-job live @main$')" = 1 ]
  [ "$(called '^t-oracle-own otest @main$')" = 1 ]
  [ "$(field '.details.awaiting_user | map(.id) | join(" ")')" = device ]
  # A run no milestone lists runs at none.
  [ "$(called '^similarity ')" = 0 ]
  [ "$(outcome sim)" = not-triggered ]

  # M0 has neither M1R's suites, nor the check from M1R, nor a device run.
  : >"$STUB_LOG"
  mg milestone M0
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field '.details.runs | join(" ")')" = "harvest live static unit" ]
  [ "$(called 't-job conf')" = 0 ]
  [ "$(called 't-oracle-own')" = 0 ]
  [ "$(field '.details.awaiting_user')" = "[]" ]

  # M1 runs ui, and fails on it.
  mg milestone M1
  [ "$status" -eq 1 ]
  [ "$(called '^t-job ui @main$')" = 1 ]
  jq -e '.failures[] | select(.id == "t-job:ui-fail")' "$result"
}

@test "M0 blocks an unchanged source flag through the real whole-tree similarity run" {
  # Reuse the existing fake scanner, but take the suite-to-run wiring and
  # arguments from production so this regression fails with the old config.
  python3 - "$REPO_ROOT" "$SB" <<'PY'
from pathlib import Path
import sys
import yaml

root, sandbox = map(Path, sys.argv[1:])
manifest = yaml.safe_load((root / "docs/process/milestones.yaml").read_text())
suite = next(s for m in manifest["milestones"] if m["id"] == "M0"
             for s in m["suites"] if s["id"] == "similarity")
tiers = yaml.safe_load((root / "tools/gate/tiers.yaml").read_text())
scan = next(r for r in tiers["runs"] if r["id"] == suite["run"])
path = sandbox / "tools/gate/tiers.yaml"
config = yaml.safe_load(path.read_text())
config["runs"].append(scan)
path.write_text(yaml.safe_dump(config, sort_keys=False))
path = sandbox / "docs/process/milestones.yaml"
config = yaml.safe_load(path.read_text())
config["milestones"][0]["suites"].append({"id": suite["id"], "run": suite["run"]})
path.write_text(yaml.safe_dump(config, sort_keys=False))
PY
  git -C "$SB" add tools/gate/tiers.yaml docs/process/milestones.yaml
  git -C "$SB" commit -qm "Use the real M0 similarity wiring"
  commit_in "$SB" core/x.rs 'pub fn unchanged() {}' "Add source

$TRAILERS"
  local tip="$(git -C "$SB" rev-parse HEAD)"
  : >"$FX/similarity.flag"
  mg milestone M0
  [ "$status" -eq 1 ]
  [ "$(rq .base)" = "$tip" ]
  [ "$(rq .commit)" = "$tip" ]
  jq -e '.dirty == false and .changed_paths == 0' "$(receipt)"
  [ "$(called "^similarity --rev $tip @main$")" = 1 ]
  jq -e --arg tip "$tip" '.runs[] | select(.job == "similarity" and .outcome == "fail"
    and .args == ["--rev", $tip] and ([.args[] | select(. == "--base")] | length) == 0)' "$(receipt)"
  jq -e '.failures[] | select(.id == "similarity:similarity:core/x.rs:struct")' "$result"
  validate "$result"
}

@test "a milestone run refuses an invalid manifest and runs nothing" {
  printf '  - {id: M6, name: Later, plan: {gate: "", user_steps: ""}, units: [], suites: [{id: gone, run: gone}]}\n' \
    >>"$SB/docs/process/milestones.yaml"
  git -C "$SB" commit -qam "List a suite that is not a tiers run"
  mg milestone M0
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = manifest ]
  [[ "$(field '.failures[0].message')" == *"run gone is not in tools/gate/tiers.yaml"* ]]
  [ ! -s "$STUB_LOG" ]
}

@test "a milestone run refuses a dirty tree or a worktree" {
  printf 'x\n' >"$SB/docs/untracked.md"
  mg milestone M0
  [ "$status" -eq 2 ]
  rm "$SB/docs/untracked.md"
  local wt="$BATS_TEST_TMPDIR/U90"
  worktree "$wt" unit/U90 implementer
  mg_in "$wt" milestone M0
  [ "$status" -eq 2 ]
  mg milestone M9
  [ "$status" -eq 2 ]
}

# --- tiers.yaml validation --------------------------------------------------

@test "the gate refuses a tiers.yaml that leaves a job without a tier or keeps a landed job pending" {
  cat >>"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
mg_job notices host t_job "made-up: U16 landed"
mg_job orphan host t_job "made-up: no tier"
EOF
  mg gate --plan
  [ "$status" -eq 2 ]
  jq -e '.failures[] | select(.message | test("job orphan has no tier"))' "$result"
  jq -e '.failures[] | select(.message | test("job notices exists now; remove `pending: U16`"))' "$result"
}

@test "the gate refuses placements that break R1952 and runs that a merge could not place" {
  python3 - "$SB/tools/gate/tiers.yaml" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("  - {id: ui, job: t-job, args: [ui], tier: T0, placement: vm,", "  - {id: ui, job: t-job, args: [ui], tier: T0, placement: docker:dev,")
s = s.replace("needs: [oracle]\n    oracle: true\n    local: false", "needs: [oracle]\n    oracle: true\n    local: true")
s = s.replace("{id: fuzz, job: t-job, args: [fuzz, --seed, \"{seed}\"], tier: T2, schedule: nightly", "{id: fuzz, job: t-job, args: [fuzz, --seed, \"{sed}\"], tier: T2, schedule: nightly")
s = s.replace("budget_s: 60, local: false, where: tip}", "budget_s: 60, local: true, where: tip}")
s = s.replace("{artifact:live:t1-live.json}", "{artifact:device:t1-live.json}")
open(p, "w").write(s)
PY
  mg gate --plan
  [ "$status" -eq 2 ]
  jq -e '.failures[] | select(.message | test("run ui: needs simulator, which run on the host or the test VM"))' "$result"
  jq -e '.failures[] | select(.message | test("run live: an oracle-image run cannot run in gate --local"))' "$result"
  jq -e '.failures[] | select(.message | test("unknown placeholder \\{sed\\}"))' "$result"
  jq -e '.failures[] | select(.message | test("run stest: a where: tip run cannot run in gate --local"))' "$result"
  jq -e '.failures[] | select(.message | test("run harvest: \\{artifact:device:t1-live.json\\} names run device, which does not run before it"))' "$result"
}

@test "the real tiers.yaml gives every registered tools/mg job a tier, with valid placements and filters" {
  local reg="$BATS_TEST_TMPDIR/registry.tsv" out="$BATS_TEST_TMPDIR/config.json"
  "$REPO_ROOT/tools/mg" --list >"$reg"
  run python3 -B -I "$REPO_ROOT/tools/gate/gate.py" load --tiers "$REPO_ROOT/tools/gate/tiers.yaml" \
    --quarantine "$REPO_ROOT/tools/gate/quarantine.yaml" --registry "$reg" --conformance "$REPO_ROOT/conformance" --out "$out"
  [ "$status" -eq 0 ] || { echo "$output"; return 1; }
  [ "$(jq '.quarantine.invalid | length' "$out")" = 0 ]
  # The canaries run first; T0 covers Rust, Swift, goldens, lint, deny, UI and the similarity scan.
  [ "$(jq -r '[.runs[] | select(.stage == "canary") | .id] | join(" ")' "$out")" = "pin-canary canary-scenarios sync-web-verify" ]
  for id in core-test conformance goldens-check lint-ui core-lint core-deny swift-test-core swift-test-core-profile snapshots a11y-iphone similarity perf-smoke notices; do
    jq -e --arg id "$id" '.runs[] | select(.id == $id and .tier == "T0")' "$out" >/dev/null || { echo "no T0 run $id"; return 1; }
  done
  jq -e '.runs as $runs
    | ($runs[] | select(.id == "swift-test-core")) as $debug
    | ($runs[] | select(.id == "swift-test-core-profile")) as $profile
    | $debug.args == ["--package", "MeguriCore"]
      and $profile.job == "swift-test" and $profile.placement == ["host"]
      and $profile.class == "perf" and $profile.triggers == $debug.triggers
      and $profile.args == ["--package", "MeguriCore", "--config", "Profile", "--", "--filter", "nativeGeneratedCountedTreeLiftAndValueConversionUnderFiveMilliseconds"]' "$out" >/dev/null
  jq -e '.runs[] | select(.id == "live-oracle" and .tier == "T1" and .oracle)' "$out" >/dev/null
  jq -e '.runs[] | select(.id == "core-fuzz" and .tier == "T2" and (.args | index("{seed}")))' "$out" >/dev/null
  jq -e '.runs[] | select(.id == "private-suite" and .tier == "T3" and .where == "main")' "$out" >/dev/null
  jq -e '.runs[] | select(.id == "harvest-coverage" and (.require | map(.id) | index("R1880-unclassified")))' "$out" >/dev/null
  # Harvest coverage reads the live run's T1 report, the file conformance --live writes.
  jq -e '.runs[] | select(.id == "harvest-coverage" and .args == ["--live-report", "{artifact:live-oracle:t1-live.json}"])' "$out" >/dev/null
  grep -qF 'live="$MG_RUN_DIR/t1-live.json"' "$REPO_ROOT/tools/mg.d/conformance.sh"
  # The scanner's own tests run the branch's scanner, in a full checkout of the tip.
  jq -e '.runs[] | select(.id == "similarity-selftest" and .where == "tip" and .merge_args == null)' "$out" >/dev/null
  # Runs that read the branch's oracle/ files, including route transport.
  jq -e '[.runs[] | select(.id == "conformance-route-transport") | {placement, needs, oracle_files}]
    == [{placement: ["host", "docker:oracle"], needs: ["oracle"], oracle_files: true}]' "$out" >/dev/null
  [ "$(jq -r '[.runs[] | select(.oracle_files) | .id] | join(" ")' "$out")" = "goldens-check harvest-report sync-server-reference-selftest conformance-route-transport sync-server-interop sync-server-interop-normal sync-server-reference-smoke sync-server-reference-edges sync-server-reference-normal sync-server-reference-normal-matrix harvest-coverage" ]
  # A pin bump in a sparse worktree whose main checkout has the old oracle/:
  # gate --local runs the rest and leaves those two to the merge.
  printf 'conformance/oracle.json\0conformance/catalog/harvest.csv\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode local \
    --oracle-tree differs --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  [ "$(jq -r '[.runs[] | select(.outcome == "deferred-tip") | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "goldens-check harvest-report" ]
  jq -e '.runs[] | select(.id == "conformance" and .action == "run")' "$BATS_TEST_TMPDIR/plan.json" >/dev/null

  # Server source selects tests and lint; dependency changes also select the audit.
  printf 'server/src/lib.rs\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode full \
    --oracle-tree same --main-capable yes --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  [ "$(jq -r '[.runs[] | select(.action == "run" and (.id | test("^sync-server-(test|lint|deps)$"))) | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "sync-server-test sync-server-lint" ]
  [ "$(jq -r '[.runs[] | select(.action == "run" and .tier == "T1" and (.id | test("^sync-server-(candidate-build|interop|interop-normal)$"))) | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "sync-server-candidate-build sync-server-interop sync-server-interop-normal" ]
  # A fresh project prepares embedded assets before any Rust consumer.
  jq -e 'any(.runs[]; .id == "sync-web-verify" and .action == "run" and .stage == "canary")
    and ([.runs[] | select(.action == "run") | .id] |
      index("sync-web-verify") < index("sync-server-test"))' "$BATS_TEST_TMPDIR/plan.json" >/dev/null
  printf 'server/Cargo.lock\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode full \
    --oracle-tree same --main-capable yes --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  [ "$(jq -r '[.runs[] | select(.action == "run" and (.id | test("^sync-server-(test|lint|deps)$"))) | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "sync-server-test sync-server-lint sync-server-deps" ]

  # A reference driver change selects its selftest and both reference suites.
  printf 'oracle/meguri-sync-server/driver.py\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode full \
    --oracle-tree same --main-capable yes --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  [ "$(jq -r '[.runs[] | select(.action == "run" and (.id | test("^sync-server-reference-(selftest|smoke|edges)$"))) | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "sync-server-reference-selftest sync-server-reference-smoke sync-server-reference-edges" ]
  # Reference-only changes also select a fresh candidate build.
  jq -e 'any(.runs[]; .id == "sync-web-verify" and .action == "run" and .stage == "canary")
    and ([.runs[] | select(.action == "run") | .id] |
      index("sync-web-verify") < index("sync-server-candidate-build"))' "$BATS_TEST_TMPDIR/plan.json" >/dev/null
  jq -e '.runs[] | select(.id == "oracle-test" and .action == "run")' "$BATS_TEST_TMPDIR/plan.json" >/dev/null

  # Dedicated neutral inputs have their own reference coverage. Check actual
  # plans so exclusions cannot suppress an old input in the same change.
  python3 -B -I - "$REPO_ROOT" "$out" "$BATS_TEST_TMPDIR" <<'PY'
import json
from pathlib import Path
import subprocess
import sys

root, config, tmp = map(Path, sys.argv[1:])
old = {"core-test", "conformance", "goldens-check", "harvest-report", "live-oracle"}
reference = {"sync-server-reference-selftest", "sync-server-reference-smoke",
             "sync-server-reference-edges"}
dedicated = [
    "conformance/schema/meguri-sync-server.schema.json",
    "conformance/catalog/meguri-sync-server.json",
    "conformance/fixtures/meguri-sync-server/AGENTS.md",
    "conformance/fixtures/meguri-sync-server/original.json",
    "conformance/fixtures/meguri-sync-server/edges.json",
    "conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-foundation.json",
    "conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-edges.json",
    "conformance/recordings/scenarios/meguri-sync-server/AGENTS.md",
    "conformance/recordings/scenarios/meguri-sync-server/desktop-selection.json",
    "conformance/recordings/scenarios/meguri-sync-server/desktop-edge-selection.json",
    "conformance/recordings/scenarios/meguri-sync-server/candidate-complete-selection.json",
    "conformance/recordings/scenarios/meguri-sync-server/candidate-full-selection.json",
    "conformance/recordings/scenarios/meguri-sync-server/candidate-inputs.md",
]

def selected(paths, mode="full"):
    changed, plan = tmp / "routing-changed", tmp / "routing-plan.json"
    changed.write_bytes(b"".join(p.encode() + b"\0" for p in paths))
    subprocess.run([
        sys.executable, "-B", "-I", str(root / "tools/gate/gate.py"), "plan",
        "--config", str(config), "--changed", str(changed), "--mode", mode,
        "--oracle-tree", "same", "--main-capable", "yes",
        "--var", "base=b", "--var", "tip=t", "--out", str(plan),
    ], check=True)
    return {r["id"] for r in json.loads(plan.read_text())["runs"] if r["action"] == "run"}

def retains(paths, expected, mode="full"):
    actual = selected(paths, mode)
    assert expected <= actual, (mode, paths, "missing", sorted(expected - actual))
    return actual

for path in dedicated:
    actual = retains([path], reference)
    assert not actual & old, (path, "selected old jobs", sorted(actual & old))

for path in ("docker/meguri-sync-reference.Dockerfile", "tools/mg.d/meguri-sync-reference.sh"):
    retains([path], reference)

fixture = "conformance/fixtures/storage/schema-18.anki2"
scenario = "conformance/scenarios/smoke/runner-empty.yaml"
fixture_jobs = {"core-test", "conformance", "goldens-check", "live-oracle"}
scenario_jobs = old | {"canary-scenarios"}
for path, expected in (
    (fixture, fixture_jobs),
    ("conformance/catalog/harvest.csv", {"core-test", "conformance", "goldens-check", "harvest-report"}),
    (scenario, scenario_jobs),
    ("conformance/schema/scenario.schema.json", {"core-test", "conformance", "goldens-check"}),
    ("conformance/recordings/scenarios/sync/wire/records-down/exchanges.jsonl",
     {"core-test", "conformance", "goldens-check"}),
):
    retains([path], expected)

for shared, expected in (
    (fixture, fixture_jobs),
    (scenario, scenario_jobs),
    ("core/crates/meguri-sched/src/answer.rs", {"core-test", "conformance", "live-oracle"}),
    ("tools/mg.d/conformance.sh", old | {"canary-scenarios"}),
    ("tools/mg.d/oracle.sh", {"core-test", "conformance", "live-oracle", "oracle-test"}),
):
    baseline = retains([shared], expected) & old
    for path in dedicated:
        actual = selected([shared, path]) & old
        assert actual == baseline, (shared, path, "old jobs", sorted(actual), "expected", sorted(baseline))

# These modes deliberately execute all paths, even for dedicated-only input.
for mode in ("milestone", "release"):
    retains([dedicated[3]], old | reference | {"oracle-test"}, mode)
PY

  # The sched change of the plan's scenario selects sched's live suites and the scan.
  printf 'core/crates/meguri-sched/src/answer.rs\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode full \
    --main-capable yes --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  jq -e '.runs[] | select(.id == "live-oracle" and .action == "run" and (.args | any(startswith("sched/"))) and (.args | index("fp-probe")))' \
    "$BATS_TEST_TMPDIR/plan.json" >/dev/null
  jq -e '.runs[] | select(.id == "similarity" and .action == "run")' "$BATS_TEST_TMPDIR/plan.json" >/dev/null
  for id in swift-test-core swift-test-core-profile; do
    jq -e --arg id "$id" '.runs[] | select(.id == $id and .action == "run")' "$BATS_TEST_TMPDIR/plan.json" >/dev/null
  done
  # A docs-only change runs nothing but the static checks.
  printf 'docs/process/notes.md\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode full \
    --main-capable yes --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  [ "$(jq -r '[.runs[] | select(.action == "run") | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "static static-tree" ]
}
