#!/usr/bin/env bats
# Self-tests for the test tiers, merge gate, quarantine and scheduled runs
# (U19: R1950-R1953, R1880, R1876, R1937, R1991; AE82 ordering). Each test
# runs a sandbox repository with the gate's own families (gate, merge,
# quarantine, nightly, milestone, session, and host, oracle and conformance
# for their helpers) and a tiers.yaml of made-up runs: `t-job <name>` reports
# what $FX/<name>.* says (status, a status sequence, failure ids, counts,
# details), a fake `similarity` scan flags a file on demand, the fake oracle
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
      gate.sh | merge.sh | quarantine.sh | nightly.sh | milestone.sh | session.sh | host.sh | oracle.sh | conformance.sh) ;;
      *) rm -f "$f" ;;
    esac
  done
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
  - {job: session, why: sessions}
  - {job: oracle, why: its tree helpers}
  - {job: conformance, why: its oracle file lookup}
  - {job: goldens-check, why: conformance.sh}
  - {job: harvest-report, why: conformance.sh}
  - {job: harvest-inventory, why: conformance.sh}
  - {job: mac-ui, why: host.sh}
EOF
  cat >"$t/tools/mg.d/zz-fakes.sh" <<'EOF'
mg_job t-job host t_job "fake run: t-job <name> [args...]; reports what \$FX/<name>.* says"
mg_job similarity host t_similarity "fake similarity scan: flags core/x.rs when \$FX/similarity.flag exists"
mg_job t-oracle host t_oracle "fake oracle-image run (conformance --live): t-oracle <name> [args...]"
mg_job t-oracle-own host t_oracle_own "fake oracle test: t-oracle-own <name> [args...]"
mg_job t-scan-selftest host t_scan_selftest "fake similarity-selftest"
mg_job t-oracle-files host t_oracle_files "fake oracle metadata check: t-oracle-files <name> <file under oracle/> <its conformance side>"
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
    queued) mg_detail queued true; status=warn ;;
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
  # Every per-change run runs in full at a milestone.
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
  [ "$(jq -r '[.runs[] | select(.stage == "canary") | .id] | join(" ")' "$out")" = "pin-canary canary-scenarios" ]
  for id in core-test conformance goldens-check lint-ui core-lint core-deny swift-test-core snapshots a11y-iphone similarity perf-smoke notices; do
    jq -e --arg id "$id" '.runs[] | select(.id == $id and .tier == "T0")' "$out" >/dev/null || { echo "no T0 run $id"; return 1; }
  done
  jq -e '.runs[] | select(.id == "live-oracle" and .tier == "T1" and .oracle)' "$out" >/dev/null
  jq -e '.runs[] | select(.id == "core-fuzz" and .tier == "T2" and (.args | index("{seed}")))' "$out" >/dev/null
  jq -e '.runs[] | select(.id == "private-suite" and .tier == "T3" and .where == "main")' "$out" >/dev/null
  jq -e '.runs[] | select(.id == "harvest-coverage" and (.require | map(.id) | index("R1880-unclassified")))' "$out" >/dev/null
  # Harvest coverage reads the live run's T1 report, the file conformance --live writes.
  jq -e '.runs[] | select(.id == "harvest-coverage" and .args == ["--live-report", "{artifact:live-oracle:t1-live.json}"])' "$out" >/dev/null
  grep -qF 'live="$MG_RUN_DIR/t1-live.json"' "$REPO_ROOT/tools/mg.d/conformance.sh"
  # The scanner's own tests run the branch's scanner, in a full checkout of the tip.
  jq -e '.runs[] | select(.id == "similarity-selftest" and .where == "tip" and .merge_args == null)' "$out" >/dev/null
  # The jobs that compare the branch with the lock and the harvest inventory
  # under oracle/ read that tree's files.
  [ "$(jq -r '[.runs[] | select(.oracle_files) | .id] | join(" ")' "$out")" = "goldens-check harvest-report harvest-coverage" ]
  # A pin bump in a sparse worktree whose main checkout has the old oracle/:
  # gate --local runs the rest and leaves those two to the merge.
  printf 'conformance/oracle.json\0conformance/catalog/harvest.csv\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode local \
    --oracle-tree differs --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  [ "$(jq -r '[.runs[] | select(.outcome == "deferred-tip") | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "goldens-check harvest-report" ]
  jq -e '.runs[] | select(.id == "conformance" and .action == "run")' "$BATS_TEST_TMPDIR/plan.json" >/dev/null

  # The sched change of the plan's scenario selects sched's live suites and the scan.
  printf 'core/crates/meguri-sched/src/answer.rs\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode full \
    --main-capable yes --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  jq -e '.runs[] | select(.id == "live-oracle" and .action == "run" and (.args | index("sched")) and (.args | index("fp-probe")))' \
    "$BATS_TEST_TMPDIR/plan.json" >/dev/null
  jq -e '.runs[] | select(.id == "similarity" and .action == "run")' "$BATS_TEST_TMPDIR/plan.json" >/dev/null
  # A docs-only change runs nothing but the static checks.
  printf 'docs/process/notes.md\0' >"$BATS_TEST_TMPDIR/changed"
  python3 -B -I "$REPO_ROOT/tools/gate/gate.py" plan --config "$out" --changed "$BATS_TEST_TMPDIR/changed" --mode full \
    --main-capable yes --var base=b --var tip=t --out "$BATS_TEST_TMPDIR/plan.json"
  [ "$(jq -r '[.runs[] | select(.action == "run") | .id] | join(" ")' "$BATS_TEST_TMPDIR/plan.json")" = "static static-tree" ]
}
