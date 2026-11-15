#!/usr/bin/env bats
# Verify tier selection through the real conformance job with isolated runners.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core/crates/meguri-testkit" "$t/conformance"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  CONFORMANCE_CANCEL_PID=
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  export CALLS="$BATS_TEST_TMPDIR/calls" LIVE_STATUS=pass LIVE_REPORT=yes
  mkdir -p "$SB/stubs"
  cat >"$SB/stubs/cargo" <<'EOF'
#!/bin/sh
echo testkit >>"$CALLS"
printf '%s\n' "$@" >>"$CALLS.cargo-args"
jq -nc --arg exe "$PWD/stub-testkit" \
  '{reason:"compiler-artifact",profile:{test:true},target:{name:"testkit",kind:["lib"]},executable:$exe}'
echo '{"reason":"build-finished","success":true}'
EOF
  cat >"$SB/core/stub-testkit" <<'EOF'
#!/bin/sh
echo testkit-child >>"$CALLS"
EOF
  chmod +x "$SB/stubs/cargo" "$SB/core/stub-testkit"
  export PATH="$SB/stubs:$PATH"
  cat >"$SB/tools/mg.d/zz-conformance-fixture.sh" <<'EOF'
fixture_report() {
  jq -n --arg tier "$1" --arg status "$3" \
    '{tier:$tier, totals:{pending:0, reference:0, ($status):1}, failed:(if ($status=="pass" or $status=="reference") then 0 else 1 end), wall_ms:1,
      scenarios:[{scenario:"smoke/fixture", status:$status, fails:($status!="pass" and $status!="reference"), message:"fixture", repro:"fixture"}]}' >"$2"
}
mg_conf__edges_cleanup() { echo edges-cleanup >>"$CALLS"; }
mg_conf__scratch() {
  echo "$1" >>"$CALLS.scratch"
  printf '%s\t%s\t%s\n' "$1" "$MG_RUN_ID" "${MG_CONF_NATIVE_OWNER:-}" >>"$CALLS.owners"
  [ "${SCRATCH_FAIL:-}" != "$1" ] || return 1
  case "$1" in
    selected) echo "${SCRATCH_SELECTED:-true}" ;;
    prepare)
      mkdir "$MG_RUN_DIR/native-scratch" || return 1
      mkdir -p "$MG_RUN_DIR/native-scratch/mount/tmp"
      ;;
    check) [ -d "$MG_RUN_DIR/native-scratch/mount/tmp" ] ;;
    cleanup)
      # Cleanup must follow the stopped child, including cancellation.
      if [ -s "$CALLS.ready" ]; then
        ! kill -0 "$(cat "$CALLS.ready")" 2>/dev/null || return 1
      fi
      [ -d "$MG_RUN_DIR/native-scratch" ] || return 0
      rmdir "$MG_RUN_DIR/native-scratch/mount/tmp" "$MG_RUN_DIR/native-scratch/mount" "$MG_RUN_DIR/native-scratch"
      ;;
  esac
}
mg_conf__docker_ok() { echo docker-check >>"$CALLS"; }
mg_conf__record_stale() { echo freshness >>"$CALLS"; }
mg_oracle__actor_scenarios() { echo actors >>"$CALLS"; echo smoke/fixture; }
mg_oracle__sync_faults() { echo faults-up >>"$CALLS"; MG_ORACLE_SYNC_FAULTS=http://fixture; }
mg_oracle__sync_stop() { echo faults-down >>"$CALLS"; }
mg_oracle__tree() { MG_ORACLE_TAG=fixture; MG_ORACLE_DIR="$MG_ROOT/build/oracle-fixture"; }
mg_oracle__build() { echo oracle-build >>"$CALLS"; }
mg_conf__dev() {
  echo docker-t0 >>"$CALLS"
  mg_json_array "$@" >>"$CALLS.docker-args"
  fixture_report t0 "$MG_RUN_DIR/t0-docker.json" pass
}
eval "$(declare -f mg_conf__cli_native | sed '1s/mg_conf__cli_native/mg_fixture__real_native/')"
mg_conf__cli_native() {
  local tier=t0 report="" prev="" arg
  for arg in "$@"; do
    [ "$arg" != --live ] || tier=t1
    [ "$prev" != --report ] || report="$arg"
    prev="$arg"
  done
  echo "$tier" >>"$CALLS"
  mg_json_array "$@" >>"$CALLS.args"
  if [ "$tier" = t1 ]; then
    [ "$LIVE_REPORT" != yes ] || fixture_report t1 "$report" "$LIVE_STATUS"
  else
    fixture_report t0 "$report" pass
  fi
}
EOF
}

teardown() {
  if [ -n "$CONFORMANCE_CANCEL_PID" ]; then
    # These tests own one background job. Resolve it through this shell's job
    # table so teardown cannot signal a recycled PID or a fixture-supplied PID.
    if [ "$(jobs -pr %+ 2>/dev/null)" = "$CONFORMANCE_CANCEL_PID" ]; then
      kill -TERM %+ 2>/dev/null || :
    fi
    wait "$CONFORMANCE_CANCEL_PID" 2>/dev/null || :
    CONFORMANCE_CANCEL_PID=
  fi
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

native_scratch_stubs() {
  export STUB_CLI="$SB/core/stub-cli" STUB_TESTKIT="$SB/core/test binaries" CLI_EXIT=0 CLI_WAIT=no
  export EXPECT_NATIVE_JOBS=2
  mkdir -p "$STUB_TESTKIT" "$SB/conformance/scenarios"
  printf '%s\n' 'package-relative scenarios resolved' >"$SB/conformance/scenarios/native-cwd-sentinel"
  cat >"$SB/stubs/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "${TMPDIR-unset}" >>"$CALLS.build-tmp"
printf '%s\n' "$PWD" >>"$CALLS.build-cwd"
printf '%s\n' "$*" >>"$CALLS.build-args"
if [ "$1" = build ]; then
  jq -nc --arg exe "$STUB_CLI" '{reason:"compiler-artifact",target:{name:"meguri-cli"},executable:$exe}'
elif [ "$1" = test ]; then
  python3 -I -B "$STUB_TESTKIT/artifacts.py"
  exit "${TESTKIT_BUILD_EXIT:-0}"
fi
EOF
  cat >"$STUB_TESTKIT/artifacts.py" <<'PY'
import json, os
from pathlib import Path
root = Path(os.environ["STUB_TESTKIT"])
messages = [{"reason": "compiler-artifact", "profile": {"test": False},
             "target": {"name": "dependency", "kind": ["lib"]}, "executable": None}]
targets = [("unit", "lib"), ("integration", "test"), ("conformance", "test")]
if os.environ.get("TESTKIT_EXTRA_TARGET"):
    targets.append((os.environ["TESTKIT_EXTRA_TARGET"], "test"))
for name, kind in targets:
    messages.append({"reason": "compiler-artifact", "profile": {"test": True},
                     "target": {"name": name, "kind": [kind]}, "executable": str(root / name)})
mode = os.environ.get("TESTKIT_ARTIFACT_CASE", "valid")
if mode == "zero":
    messages = messages[:1]
elif mode == "missing":
    del messages[-1]["executable"]
elif mode in ("null", "invalid-type", "missing-file", "nonexec", "directory", "relative"):
    messages[-1]["executable"] = {"null": None, "invalid-type": [], "missing-file": str(root / "absent"),
        "nonexec": str(root / "artifacts.py"), "directory": str(root), "relative": "conformance"}[mode]
elif mode == "profile":
    del messages[-1]["profile"]
elif mode == "target":
    del messages[-1]["target"]
elif mode == "duplicate":
    messages.append(messages[-1])
if mode != "incomplete":
    messages.append({"reason": "build-finished", "success": mode != "failed"})
for message in messages:
    print(json.dumps(message))
if mode == "malformed":
    print("{broken")
PY
  cat >"$STUB_TESTKIT/unit" <<'EOF'
#!/bin/sh
name="${0##*/}"
printf '%s\n' "$name" >>"$CALLS.tests"
printf '%s\n' "$TMPDIR" >>"$CALLS.test-tmp"
printf '%s\n' "$PWD" >>"$CALLS.test-cwd"
printf '%s\n' "$*" >>"$CALLS.test-args"
[ "$#" -eq 1 ] && [ "$1" = "--test-threads=$EXPECT_NATIVE_JOBS" ] || exit 91
[ "$PWD" = "${STUB_CLI%/*}/crates/meguri-testkit" ] || exit 92
case "$TMPDIR" in */native-scratch/mount/tmp) [ -d "$TMPDIR" ] || exit 93 ;; *) exit 93 ;; esac
mkdir "$CALLS.active" || exit 94
printf 'start %s\n' "$name" >>"$CALLS.test-order"
trap 'printf "exit %s\n" "$name" >>"$CALLS.test-order"; rmdir "$CALLS.active"' EXIT
# Give an incorrectly parallel launcher time to overlap.
sleep 0.05
if [ "$name" = conformance ]; then
  # The harness=false target discovers scenarios relative to the package root.
  sentinel="$(cat ../../../conformance/scenarios/native-cwd-sentinel)" || exit 1
  [ "$sentinel" = 'package-relative scenarios resolved' ] || exit 1
  printf '%s\n' "$sentinel" >>"$CALLS.scenarios-resolved"
fi
if [ "${TESTKIT_WAIT:-no}" = yes ]; then
  echo "$$" >"$CALLS.ready"
  exec sleep 300
fi
[ "${TESTKIT_FAIL:-}" != "$name" ]
EOF
  cp "$STUB_TESTKIT/unit" "$STUB_TESTKIT/integration"
  cp "$STUB_TESTKIT/unit" "$STUB_TESTKIT/conformance"
  cat >"$STUB_CLI" <<'EOF'
#!/bin/sh
printf '%s\n' "$TMPDIR" >>"$CALLS.child-tmp"
printf '%s\n' "$PWD" >>"$CALLS.child-cwd"
prev= tier=t0 report= jobs=
for arg in "$@"; do
  [ "$arg" != --live ] || tier=t1
  [ "$prev" != --report ] || report="$arg"
  [ "$prev" != --jobs ] || jobs="$arg"
  prev="$arg"
done
if [ "$tier" = t0 ]; then
  [ "$jobs" = "$EXPECT_NATIVE_JOBS" ] || exit 95
  [ ! -d "$CALLS.active" ] || exit 94
fi
[ "$PWD" = "${STUB_CLI%/*}" ] || exit 92
case "$TMPDIR" in */native-scratch/mount/tmp) [ -d "$TMPDIR" ] || exit 93 ;; *) exit 93 ;; esac
if [ "$CLI_WAIT" = yes ]; then
  echo "$$" >"$CALLS.ready"
  exec sleep 300
fi
jq -n --arg tier "$tier" '{tier:$tier,totals:{pending:0,reference:0,pass:1},failed:0,wall_ms:1,
  scenarios:[{scenario:"smoke/fixture",status:"pass",fails:false,message:"fixture",repro:"fixture"}]}' >"$report"
exit "$CLI_EXIT"
EOF
  chmod +x "$SB/stubs/cargo" "$STUB_CLI" "$STUB_TESTKIT/unit" \
    "$STUB_TESTKIT/integration" "$STUB_TESTKIT/conformance"
  cat >>"$SB/tools/mg.d/zz-conformance-fixture.sh" <<'EOF'
eval "$(declare -f mg_fixture__real_native | sed '1s/mg_fixture__real_native/mg_conf__cli_native/')"
mg_conf__record_stale() {
  mg_conf__cli_native conformance run --lane plan --jobs "${MG_CONF_NATIVE_JOBS:-2}" --report "$MG_RUN_DIR/stale-before.json" "$@"
}
EOF
}

native_selector_stubs() {
  native_scratch_stubs
  export TESTKIT_EXTRA_TARGET=cloud_checks
  cp "$STUB_TESTKIT/unit" "$STUB_TESTKIT/cloud_checks"
  cat >>"$STUB_TESTKIT/cloud_checks" <<'EOF'
rc=$?
case "${TESTKIT_SUMMARY:-pass}" in
  pass) echo 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s' ;;
  empty) echo 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s' ;;
  failed) echo 'test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s' ;;
  multiple)
    echo 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s'
    echo 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s' ;;
  malformed) echo 'test result: ok. invalid counts' ;;
  missing) : ;;
esac
exit "$rc"
EOF
}

@test "native selector runs exactly the known whole binary with scratch counts artifacts and focused arguments" {
  native_selector_stubs
  local jobs expected run_dir
  for jobs in 1 2 7; do
    expected=2
    [ "$jobs" != 1 ] || expected=1
    export EXPECT_NATIVE_JOBS="$expected"
    mg conformance --lane native --jobs "$jobs" --native-test cloud_checks
    [ "$status" -eq 0 ]
    run_dir="$(dirname "$result")"
    jq -e --arg jobs "$jobs" --argjson n "$expected" '
      .status=="pass" and .args==["--lane","native","--jobs",$jobs,"--native-test","cloud_checks"]
      and .details.focused=={scope:"diagnostic-only",native_test:"cloud_checks",full_conformance:false}
      and .details.native_concurrency==$n and (.details | has("lanes") | not)
      and .summary=={total:2,passed:2,failed:0,skipped:0}
      and (.message | contains("diagnostic-only") and contains("not full native/all conformance"))
      and any(.artifacts[]; .kind=="test-log" and (.label | contains("diagnostic-only")))' "$result"
    [ -s "$run_dir/native-testkit-build.jsonl" ]
    [ -s "$run_dir/native-testkit-executables" ]
    [ ! -e "$run_dir/native-build.jsonl" ]
    [ ! -e "$run_dir/t0-native.json" ]
    [ ! -e "$run_dir/native-scratch" ]
    grep -Fxq -- "conformance: native binary exit $STUB_TESTKIT/cloud_checks rc=0" "$run_dir/log.txt"
  done
  [ "$(cat "$CALLS.tests")" = $'cloud_checks\ncloud_checks\ncloud_checks' ]
  [ "$(cat "$CALLS.test-args")" = $'--test-threads=1\n--test-threads=2\n--test-threads=2' ]
  [ "$(grep -Fxc -- "$SB/core/crates/meguri-testkit" "$CALLS.test-cwd")" -eq 3 ]
  [ "$(grep -cx prepare "$CALLS.scratch")" -eq 3 ]
  [ "$(grep -cx cleanup "$CALLS.scratch")" -eq 3 ]
  [ ! -e "$CALLS.child-tmp" ]
  [ "$(cat "$CALLS")" = $'edges-cleanup\nedges-cleanup\nedges-cleanup' ]
}

@test "native selector rejects missing options paths duplicate selectors and incompatible modes before runners" {
  mg conformance --lane native --native-test ""
  [ "$status" -eq 2 ]
  local args
  for args in '--native-test' '--native-test --jobs' '--native-test /tmp/cloud_checks' \
    '--native-test ../cloud_checks' '--native-test ./cloud_checks' '--native-test cloud_checks/name' \
    '--native-test cloud_checks --native-test cloud_checks' '--native-test cloud_checks' \
    '--lane all --native-test cloud_checks' '--lane docker --native-test cloud_checks' \
    '--lane linux --native-test cloud_checks' '--lane all --lane native --native-test cloud_checks' \
    '--lane native --live --native-test cloud_checks' '--lane native --live-only --native-test cloud_checks' \
    '--lane native --native-test cloud_checks smoke/fixture' '--lane native smoke/fixture --native-test cloud_checks'; do
    # Each fixture is intentionally a fixed space-separated argument list.
    # shellcheck disable=SC2086
    mg conformance $args
    [ "$status" -eq 2 ]
    jq -e '.status=="error" and any(.failures[]; .id=="usage")' "$result"
    [ ! -e "$CALLS" ]
    [ ! -e "$CALLS.cargo-args" ]
    [ ! -e "$CALLS.scratch" ]
  done
}

@test "native selector matches Cargo target names exactly and refuses absent or invalid executables" {
  native_selector_stubs
  local name mode
  for name in unknown cloud cloud_check cloud_checks-extra; do
    mg conformance --lane native --native-test "$name"
    [ "$status" -eq 2 ]
    jq -e '.status=="error" and .details.focused.scope=="diagnostic-only" and any(.failures[]; .id=="native/testkit-build")' "$result"
    [ ! -s "$(dirname "$result")/native-testkit-executables" ]
    [ ! -e "$CALLS.tests" ]
  done
  for mode in zero missing missing-file malformed incomplete duplicate; do
    export TESTKIT_ARTIFACT_CASE="$mode"
    mg conformance --lane native --native-test cloud_checks
    [ "$status" -eq 2 ]
    [ ! -s "$(dirname "$result")/native-testkit-executables" ]
    [ ! -e "$CALLS.tests" ]
  done
}

@test "native selector fails on nonzero exit empty failing missing or ambiguous result counts" {
  native_selector_stubs
  export TESTKIT_FAIL=cloud_checks
  mg conformance --lane native --native-test cloud_checks
  [ "$status" -eq 1 ]
  jq -e '.status=="fail" and .summary.passed==2 and any(.failures[]; .id=="native/testkit" and (.repro | contains("--native-test")))' "$result"
  unset TESTKIT_FAIL
  local mode
  for mode in empty failed missing malformed multiple; do
    export TESTKIT_SUMMARY="$mode"
    mg conformance --lane native --native-test cloud_checks
    [ "$status" -eq 1 ]
    jq -e '.status=="fail" and (.message | contains("diagnostic-only"))' "$result"
    [ ! -e "$(dirname "$result")/native-scratch" ]
  done
  [ ! -e "$CALLS.child-tmp" ]
}

@test "native scenario children alone inherit exact run TMPDIR including freshness and offline live replay" {
  native_scratch_stubs
  local inherited="${TMPDIR-unset}"
  mg conformance --live --lane all smoke/fixture
  [ "$status" -eq 0 ]
  local run_dir="$(dirname "$result")"
  [ "$(grep -cx prepare "$CALLS.scratch")" -eq 1 ]
  [ "$(grep -cx check "$CALLS.scratch")" -eq 3 ]
  [ "$(grep -Fxc -- "$run_dir/native-scratch/mount/tmp" "$CALLS.child-tmp")" -eq 3 ]
  [ "$(grep -Fxc -- "$inherited" "$CALLS.build-tmp")" -eq 3 ]
  [ "$(grep -Fxc -- "$SB/core" "$CALLS.build-cwd")" -eq 3 ]
  [ "$(grep -Fxc -- "$SB/core" "$CALLS.child-cwd")" -eq 3 ]
  [ "$(grep -Fxc -- "conformance: native binary start $STUB_CLI" "$run_dir/log.txt")" -eq 3 ]
  [ "$(grep -Fxc -- "conformance: native binary exit $STUB_CLI rc=0" "$run_dir/log.txt")" -eq 3 ]
  ! grep -q TMPDIR "$CALLS.docker-args"
  grep -qx docker-t0 "$CALLS"
  [ ! -e "$run_dir/native-scratch" ]
}

@test "unfiltered native testkit explicitly bounds every harness and runs targets serially on the shared owner" {
  native_scratch_stubs
  local inherited="${TMPDIR-unset}"
  mg conformance --live --lane all
  [ "$status" -eq 0 ]
  local run_dir="$(dirname "$result")"
  [ "$(cat "$CALLS.tests")" = $'unit\nintegration\nconformance' ]
  [ "$(grep -cx -- '--test-threads=2' "$CALLS.test-args")" -eq 3 ]
  [ "$(cat "$CALLS.test-order")" = $'start unit\nexit unit\nstart integration\nexit integration\nstart conformance\nexit conformance' ]
  jq -e '.details.native_concurrency==2' "$result"
  jq -e '(.details | has("focused") | not) and .summary=={total:1,passed:1,failed:0,skipped:0,pending:0,reference:0}' "$result"
  python3 - "$run_dir/log.txt" "$STUB_TESTKIT" <<'PY'
import pathlib, sys
log = pathlib.Path(sys.argv[1]).read_text()
markers = [f'conformance: native binary {event} {sys.argv[2]}/{name}' +
           (' rc=0' if event == 'exit' else ' test-threads=2')
           for name in ('unit', 'integration', 'conformance') for event in ('start', 'exit')]
positions = [log.index(marker) for marker in markers]
assert positions == sorted(positions)
PY
  [ "$(grep -Fxc -- "$run_dir/native-scratch/mount/tmp" "$CALLS.test-tmp")" -eq 3 ]
  [ "$(grep -Fxc -- "$SB/core/crates/meguri-testkit" "$CALLS.test-cwd")" -eq 3 ]
  [ "$(cat "$CALLS.scenarios-resolved")" = 'package-relative scenarios resolved' ]
  [ "$(grep -Fxc -- "$inherited" "$CALLS.build-tmp")" -eq 4 ]
  [ "$(grep -Fxc -- "$SB/core" "$CALLS.build-cwd")" -eq 4 ]
  grep -qx 'test -q -p meguri-testkit --tests --no-run --message-format=json' "$CALLS.build-args"
  [ "$(grep -cx prepare "$CALLS.scratch")" -eq 1 ]
  [ "$(grep -cx check "$CALLS.scratch")" -eq 6 ]
  [ "$(grep -Fxc -- "$run_dir/native-scratch/mount/tmp" "$CALLS.child-tmp")" -eq 3 ]
  [ "$(grep -Fxc -- "$SB/core" "$CALLS.child-cwd")" -eq 3 ]
  python3 - "$CALLS.owners" <<'PY'
import re, sys
rows = [line.rstrip("\n").split("\t") for line in open(sys.argv[1])]
owned = [row for row in rows if row[0] != "selected"]
assert {row[0] for row in owned} == {"prepare", "check", "cleanup"}
assert len({(row[1], row[2]) for row in owned}) == 1
assert all(re.fullmatch("[0-9a-f]{32}", row[2]) for row in owned)
PY
  [ -s "$run_dir/native-testkit-build.jsonl" ]
  [ -s "$run_dir/native-build.jsonl" ]
  [ ! -e "$run_dir/native-scratch" ]
}

@test "native testkit failure continues all remaining binaries and CLI before final failure" {
  native_scratch_stubs
  export TESTKIT_FAIL=unit
  mg conformance --lane native
  [ "$status" -eq 1 ]
  jq -e '.status=="fail" and any(.failures[]; .id=="native/testkit" and .repro=="tools/mg conformance --lane native")' "$result"
  [ "$(cat "$CALLS.tests")" = $'unit\nintegration\nconformance' ]
  [ "$(grep -cx -- '--test-threads=2' "$CALLS.test-args")" -eq 3 ]
  [ "$(cat "$CALLS.test-order")" = $'start unit\nexit unit\nstart integration\nexit integration\nstart conformance\nexit conformance' ]
  grep -Fq "conformance: native binary exit $STUB_TESTKIT/unit rc=1" "$(dirname "$result")/log.txt"
  [ ! -d "$CALLS.active" ]
  [ "$(grep -cx prepare "$CALLS.scratch")" -eq 1 ]
  [ "$(grep -cx check "$CALLS.scratch")" -eq 4 ]
  [ "$(wc -l <"$CALLS.child-tmp")" -eq 1 ]
  grep -Fxq -- "conformance: native binary exit $STUB_CLI rc=0" "$(dirname "$result")/log.txt"
  [ ! -e "$(dirname "$result")/native-scratch" ]
}

@test "native jobs one lowers both harness and actual T0 workers while larger jobs stay capped" {
  native_scratch_stubs
  local jobs expected
  for jobs in 1 7; do
    expected=2
    [ "$jobs" != 1 ] || expected=1
    export EXPECT_NATIVE_JOBS="$expected"
    mg conformance --lane native --jobs "$jobs"
    [ "$status" -eq 0 ]
    jq -e --argjson n "$expected" '.details.native_concurrency==$n' "$result"
    [ ! -e "$(dirname "$result")/native-scratch" ]
  done
  [ "$(grep -cx -- '--test-threads=1' "$CALLS.test-args")" -eq 3 ]
  [ "$(grep -cx -- '--test-threads=2' "$CALLS.test-args")" -eq 3 ]
}

@test "native testkit refuses zero malformed incomplete or invalid artifacts before any binary executes" {
  native_scratch_stubs
  local mode
  for mode in zero malformed incomplete missing null invalid-type missing-file nonexec directory relative profile target duplicate failed; do
    export TESTKIT_ARTIFACT_CASE="$mode"
    mg conformance --lane native
    [ "$status" -eq 2 ]
    jq -e '.status=="error" and any(.failures[]; .id=="native/testkit-build")' "$result"
    [ ! -e "$CALLS.tests" ]
    [ ! -e "$CALLS.child-tmp" ]
    [ ! -s "$(dirname "$result")/native-testkit-executables" ]
    [ ! -e "$(dirname "$result")/native-scratch" ]
  done
  ! grep -qx prepare "$CALLS.scratch"
  [ "$(grep -cx cleanup "$CALLS.scratch")" -eq 14 ]
}

@test "native testkit compilation failure preserves host environment and cleans earlier replay scratch" {
  native_scratch_stubs
  export TESTKIT_BUILD_EXIT=9
  local inherited="${TMPDIR-unset}"
  mg conformance --live --lane native
  [ "$status" -eq 2 ]
  jq -e '.status=="error" and any(.failures[]; .id=="native/testkit-build")' "$result"
  [ ! -e "$CALLS.tests" ]
  [ "$(grep -Fxc -- "$inherited" "$CALLS.build-tmp")" -eq 2 ]
  [ "$(grep -cx prepare "$CALLS.scratch")" -eq 1 ]
  [ "$(grep -cx cleanup "$CALLS.scratch")" -eq 1 ]
  [ ! -e "$(dirname "$result")/native-scratch" ]
}

@test "native testkit scratch check failure cleans without executing or falling back" {
  native_scratch_stubs
  export SCRATCH_FAIL=check
  mg conformance --lane native
  [ "$status" -eq 2 ]
  jq -e '.status=="error" and any(.failures[]; .id=="native/scratch")' "$result"
  [ ! -e "$CALLS.tests" ]
  [ ! -e "$CALLS.child-tmp" ]
  [ "$(grep -cx prepare "$CALLS.scratch")" -eq 1 ]
  [ "$(grep -cx cleanup "$CALLS.scratch")" -eq 1 ]
  [ ! -e "$(dirname "$result")/native-scratch" ]
}

@test "separate native testkit runs use distinct scratch roots and owner tokens" {
  native_scratch_stubs
  mg conformance --lane native
  [ "$status" -eq 0 ]
  local first="$(dirname "$result")"
  mg conformance --lane native
  [ "$status" -eq 0 ]
  local second="$(dirname "$result")"
  [ "$first" != "$second" ]
  [ "$(grep -Fxc -- "$first/native-scratch/mount/tmp" "$CALLS.test-tmp")" -eq 3 ]
  [ "$(grep -Fxc -- "$second/native-scratch/mount/tmp" "$CALLS.test-tmp")" -eq 3 ]
  python3 - "$CALLS.owners" <<'PY'
import sys
rows = [line.rstrip("\n").split("\t") for line in open(sys.argv[1])]
tokens = [row[2] for row in rows if row[0] == "prepare"]
assert len(tokens) == len(set(tokens)) == 2
PY
  [ ! -e "$first/native-scratch" ]
  [ ! -e "$second/native-scratch" ]
}

@test "native process failure cleans scratch and preserves failure" {
  native_scratch_stubs
  export CLI_EXIT=7
  mg conformance --lane native smoke/fixture
  [ "$status" -eq 1 ]
  [ ! -e "$(dirname "$result")/native-scratch" ]
  grep -qx cleanup "$CALLS.scratch"
}

@test "native scratch setup error stays error without fallback or repeat allocation" {
  native_scratch_stubs
  export SCRATCH_FAIL=prepare
  mg conformance --live --lane native smoke/fixture
  [ "$status" -eq 2 ]
  jq -e '.status=="error" and any(.failures[]; .id=="native/scratch")' "$result"
  [ "$(grep -cx prepare "$CALLS.scratch")" -eq 1 ]
  [ ! -e "$CALLS.child-tmp" ]
}

@test "MG cancellation cleans native scratch after its scenario process stops" {
  native_scratch_stubs
  export CLI_WAIT=yes
  local out="$BATS_TEST_TMPDIR/out" pid rc=0 ready=
  "$SB/tools/mg" conformance --lane native smoke/fixture </dev/null >"$out" 2>"$BATS_TEST_TMPDIR/err" 3>&- &
  pid=$!
  CONFORMANCE_CANCEL_PID=$pid
  for _ in $(seq 100); do
    ready="$(cat "$CALLS.ready" 2>/dev/null)" && [ -n "$ready" ] && break
    sleep 0.1
  done
  [ -n "$ready" ]
  kill -TERM "$pid"
  wait "$pid" || rc=$?
  CONFORMANCE_CANCEL_PID=
  [ "$rc" -eq 2 ]
  result="$(cat "$out")"
  jq -e '.status=="error" and any(.failures[]; .id=="cancelled")' "$result"
  ! kill -0 "$ready" 2>/dev/null
  [ ! -e "$(dirname "$result")/native-scratch" ]
  grep -qx cleanup "$CALLS.scratch"
}

@test "MG cancellation stops native testkit before owner cleanup and never starts later targets" {
  native_scratch_stubs
  export TESTKIT_WAIT=yes
  local out="$BATS_TEST_TMPDIR/out" pid rc=0 ready=
  "$SB/tools/mg" conformance --lane native </dev/null >"$out" 2>"$BATS_TEST_TMPDIR/err" 3>&- &
  pid=$!
  CONFORMANCE_CANCEL_PID=$pid
  for _ in $(seq 100); do
    ready="$(cat "$CALLS.ready" 2>/dev/null)" && [ -n "$ready" ] && break
    sleep 0.1
  done
  [ -n "$ready" ]
  kill -TERM "$pid"
  wait "$pid" || rc=$?
  CONFORMANCE_CANCEL_PID=
  [ "$rc" -eq 2 ]
  result="$(cat "$out")"
  jq -e '.status=="error" and any(.failures[]; .id=="cancelled")' "$result"
  ! kill -0 "$ready" 2>/dev/null
  [ "$(cat "$CALLS.tests")" = unit ]
  grep -Fq -- "conformance: native binary start $STUB_TESTKIT/unit test-threads=2" "$(dirname "$result")/log.txt"
  [ ! -e "$CALLS.child-tmp" ]
  [ ! -e "$(dirname "$result")/native-scratch" ]
  grep -qx cleanup "$CALLS.scratch"
}

@test "Docker-only and dry native conformance never allocate native scratch" {
  native_scratch_stubs
  mg conformance --lane docker smoke/fixture
  [ "$status" -eq 0 ]
  [ ! -e "$CALLS.scratch" ]
  [ ! -e "$CALLS.build-tmp" ]
  mg conformance --dry-run --lane native smoke/fixture
  [ "$status" -eq 0 ]
  [ ! -e "$CALLS.scratch" ]
  [ ! -e "$CALLS.child-tmp" ]
}

@test "empty native selection and metadata commands allocate no scratch" {
  native_scratch_stubs
  export SCRATCH_SELECTED=false
  mg conformance --lane native nonexistent
  # Empty selections need no scratch.
  ! grep -Eq '^(prepare|check|cleanup)$' "$CALLS.scratch"
  [ ! -e "$CALLS.child-tmp" ]
  cat >>"$SB/tools/mg.d/zz-conformance-fixture.sh" <<'EOF'
mg_job fx-native-metadata host fx_native_metadata "fixture metadata"
fx_native_metadata() { mg_conf__cli_native conformance to-json missing.yaml; }
EOF
  rm "$CALLS.scratch"
  mg fx-native-metadata
  [ "$status" -eq 0 ]
  [ ! -e "$CALLS.scratch" ]
}

@test "native scratch helper ownership and hdiutil lifecycle stubs" {
  run python3 -I -B -m unittest discover -s "$REPO_ROOT/tools/tests/conformance_scratch" -v
  printf '%s\n' "$output"
  [ "$status" -eq 0 ]
}

@test "conformance live-only runs fresh T1 without T0 tests or replays" {
  mg conformance --live-only --lane native --jobs 3 'smoke/a b'
  [ "$status" -eq 0 ]
  [ "$(jq -r .message "$result" | cut -d' ' -f2)" = T1 ]
  jq -e '.summary.passed==1 and .details.lanes.live.totals.pass==1 and (.details.lanes|keys)==["live"]' "$result"
  [ "$(grep -cx t1 "$CALLS")" -eq 1 ]
  ! grep -Eq '^(testkit|t0|docker-t0)$' "$CALLS"
  ! grep -qx freshness "$CALLS"
  grep -qx faults-up "$CALLS"
  grep -qx faults-down "$CALLS"
  jq -e 'index("--jobs") as $i | .[$i+1]=="3" and .[-1]=="smoke/a b"' "$CALLS.args"
  [ ! -e "$(dirname "$result")/t0-native.json" ]
  [ ! -e "$(dirname "$result")/t0-docker.json" ]
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "conformance live replays a selection holding a heatmap scenario on the add-on image, Meguri's edges on the base" {
  cat >>"$SB/tools/mg.d/zz-conformance-fixture.sh" <<'EOF'
mg_oracle__heatmap_scenarios() { echo "heatmap-scan $*" >>"$CALLS"; printf '%s' "${HEATMAP_IDS:-}"; }
mg_oracle__heatmap_build() {
  echo heatmap-build >>"$CALLS"
  MG_ORACLE_HEATMAP_TAG="$MG_ORACLE_TAG-heatmap-fixture" MG_ORACLE_HEATMAP_ID=sha256:heatmap
}
eval "$(declare -f mg_conf__cli_native | sed '1s/mg_conf__cli_native/mg_fixture__tiers_cli/')"
mg_conf__cli_native() { printf '%s\n' "${MEGURI_ORACLE_TAG:-}" >>"$CALLS.tag"; mg_fixture__tiers_cli "$@"; }
EOF
  export HEATMAP_IDS=stats/heatmap/today-cell
  mg conformance --live-only --lane native stats
  [ "$status" -eq 0 ]
  grep -qx 'heatmap-scan stats' "$CALLS"
  [ "$(grep -cx heatmap-build "$CALLS")" -eq 1 ]
  [ "$(cat "$CALLS.tag")" = fixture-heatmap-fixture ]
  jq -e 'index("--meguri-edge-cmd") as $i | .[$i+1] | fromjson | index("--image") as $j | .[$j+1] == "meguri-oracle:fixture"' "$CALLS.args"

  rm -f "$CALLS" "$CALLS.tag" "$CALLS.args"
  unset HEATMAP_IDS
  mg conformance --live-only --lane native smoke/fixture
  [ "$status" -eq 0 ]
  grep -qx 'heatmap-scan smoke/fixture' "$CALLS"
  ! grep -qx heatmap-build "$CALLS"
  [ "$(cat "$CALLS.tag")" = fixture ]
}

@test "conformance live still runs native and Docker T0 before T1" {
  mg conformance --live --lane all
  [ "$status" -eq 0 ]
  grep -qx testkit "$CALLS"
  grep -qx freshness "$CALLS"
  grep -qx t0 "$CALLS"
  grep -qx docker-t0 "$CALLS"
  grep -qx t1 "$CALLS"
  jq -e '(.details.lanes|keys)==["docker","live","native"]' "$result"
  python3 - "$CALLS" "$(dirname "$result")" "$SB" <<'PY'
import json, pathlib, shlex, sys
calls, run_dir, root = sys.argv[1:]
assert pathlib.Path(calls + '.cargo-args').read_text().splitlines() == [
    'test', '-q', '-p', 'meguri-testkit', '--tests', '--no-run', '--message-format=json',
]
native = [json.loads(line) for line in pathlib.Path(calls + '.args').read_text().splitlines()]
assert len(native) == 2
assert native[0] == [
    'conformance', 'run', '--conformance', root + '/conformance',
    '--lane', 'macos-native', '--jobs', '2', '--report', run_dir + '/t0-native.json',
]
docker = [json.loads(line) for line in pathlib.Path(calls + '.docker-args').read_text().splitlines()]
assert len(docker) == 1 and len(docker[0]) == 2 and docker[0][0] == '--'
tokens = shlex.shlex(docker[0][1], posix=True, punctuation_chars=True)
tokens.whitespace_split = True
# Only the actual corpus CLI runs in Docker; core-test owns its unit suite.
assert list(tokens) == [
    'cd', 'core', ';', 'cargo', 'run', '-q', '-p', 'meguri-cli', '--',
    'conformance', 'run', '--conformance', '/work/conformance',
    '--lane', 'linux-docker', '--report',
    '/work/' + str(pathlib.Path(run_dir).relative_to(root)) + '/t0-docker.json',
]
PY
}

@test "conformance live-only propagates T1 failures and missing reports" {
  for LIVE_STATUS in error stale missing; do
    export LIVE_STATUS
    mg conformance --live-only --lane native
    [ "$status" -eq 1 ]
    jq -e '.summary.failed==1 and any(.failures[]; .id=="live:smoke/fixture")' "$result"
  done
  export LIVE_REPORT=no
  mg conformance --live-only --lane native
  [ "$status" -eq 1 ]
  jq -e 'any(.failures[]; .id=="live/no-report")' "$result"
}

@test "conformance live-only rejects the Docker lane before starting runners" {
  mg conformance --live-only --lane docker
  [ "$status" -eq 2 ]
  [ ! -e "$CALLS" ]
}

@test "gate pairs T1-only replay with fresh dual-lane T0 coverage" {
  run python3 - "$REPO_ROOT/tools/gate/tiers.yaml" <<'PY'
import re, sys, yaml
from pathlib import Path
runs = {r['id']: r for r in yaml.safe_load(open(sys.argv[1]))['runs']}
t0, t1 = runs['conformance'], runs['live-oracle']
assert set(t0['triggers']) <= set(runs['core-test']['triggers']), 'core-test must cover every conformance trigger'
assert t0['args'] == ['--lane', 'all']
assert t1['args'][:3] == ['--live-only', '--lane', 'native']
assert all(any(path.startswith(trigger) for trigger in t0['triggers']) for path in t1['triggers'])
assert 'sync-server' not in t1['needs']
# D9.1 uses top-level area metadata and character globs, including across '/'.
def is_active(name, areas):
    area = name.split('/')[0]
    return any(re.fullmatch(''.join('.*' if c == '*' else '.' if c == '?' else re.escape(c)
                                   for c in area + '/' + pattern), name, re.DOTALL)
               for pattern in areas.get(area, []))

def scenario_id(path):
    if path.name in ('area.yaml', 'area.yml') or path.name.startswith('.'):
        return None
    return path.with_suffix('').as_posix() if path.suffix in ('.yaml', '.yml', '.json') else None

def check_selection(name, active, selected):
    frozen = (name.startswith(('sync/conflicts/', 'sync/holdback/', 'sync/media/',
                               'sync/meta/', 'sync/normal/', 'sync/transport/', 'sync/wire/',
                               'smoke/sync/', 'preservation/sync/'))
              or name == 'sched/compat/keys-survive-sync')
    if frozen:
        assert not selected, f'{name}: frozen scenario selected (active={active})'
    elif active:
        assert selected, f'{name}: active nonfrozen scenario lacks a T1 selector'

# Synthetic cases stay in memory; no real area or scenario is activated.
for filename, areas, expected in [
    ('example/nested/deep/case.yaml', {'example': ['nested/*']}, True),
    ('example/nested/case.yml', {'example': ['nested/cas?']}, True),
    ('example/nested/case.json', {'example': ['nested/cas?']}, True),
    ('example/nested/cases.yaml', {'example': ['nested/cas?']}, False),
    ('example/nested/case.yaml', {'example': ['case']}, False),
    ('missing/nested/case.yaml', {'example': ['*']}, False),
]:
    name = scenario_id(Path(filename))
    assert name == filename.rsplit('.', 1)[0], f'{filename}: supported scenario suffix ignored'
    assert is_active(name, areas) == expected, f'{filename}: activation mismatch for {areas}'
for filename in ('example/area.yaml', 'example/area.yml', 'example/case.txt'):
    assert scenario_id(Path(filename)) is None, f'{filename}: metadata or unsupported suffix included'
for active in (False, True):
    check_selection('sync/normal/example', active, False)
    try:
        check_selection('sync/normal/example', active, True)
    except AssertionError:
        pass
    else:
        raise AssertionError(f'frozen selection allowed with active={active}')

# The positive selectors cover every active scenario, and no frozen one.
root = Path(sys.argv[1]).resolve().parents[2] / 'conformance/scenarios'
filters = t1['args'][3:]
assert filters and '*' not in filters
areas = {}
for directory in root.iterdir():
    if not directory.is_dir():
        continue
    metadata = [directory / name for name in ('area.yaml', 'area.yml') if (directory / name).is_file()]
    assert len(metadata) <= 1, f'{directory}: duplicate area metadata'
    if metadata:
        areas[directory.name] = yaml.safe_load(metadata[0].read_text()).get('active', [])
for path in root.rglob('*'):
    if not path.is_file() or path.is_symlink():
        continue
    name = scenario_id(path.relative_to(root))
    if name is None:
        continue
    selected = any(name == f or name.startswith(f + '/') for f in filters)
    check_selection(name, is_active(name, areas), selected)
assert runs['conformance-route-transport']['disabled']
PY
  [ "$status" -eq 0 ]
}

@test "conformance reports references separately from product success and skipped work" {
  export LIVE_STATUS=reference
  mg conformance --live-only --lane native
  [ "$status" -eq 0 ]
  jq -e '.summary.total==1 and .summary.passed==0 and .summary.failed==0 and .summary.skipped==0 and .summary.reference==1 and .summary.pending==0 and .details.lanes.live.totals.reference==1' "$result"
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "conformance native T1 receives a direct host helper argv" {
  mg conformance --live-only --lane native
  [ "$status" -eq 0 ]
  jq -e 'index("--meguri-edge-cmd") as $i | .[$i+1] | fromjson |
    .[0:3]==["python3","-I","-B"] and (.[3]|endswith("/tools/mg.d/conformance-edge.py")) and
    index("--run")!=null and index("--project")!=null and index("--root")!=null' "$CALLS.args"
  grep -qx edges-cleanup "$CALLS"
}

@test "generic MG cleanup never bypasses a failed whole-route sweep" {
  cat >>"$SB/tools/mg.d/zz-conformance-fixture.sh" <<'EOF'
mg_job fx-route-cleanup host fx_route_cleanup "fixture route cleanup"
mg_conf__edges_cleanup() { echo edge-sweep >>"$CALLS"; [ "$ROUTE_REMOVE" = yes ]; }
fx_route_cleanup() {
  mkdir -p "$MG_RUN_DIR/edge-owners"
  if mg__remove_run_containers; then mg_status pass; else
    mg_failure reserved "route removal unproved"
    mg_status error
  fi
}
EOF
  cat >"$SB/stubs/docker" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$CALLS.docker"
case "$1" in
  ps) printf '%s\n' own-edge own-worker ordinary ;;
  inspect) printf '%s\n' '[{"Id":"own-edge","Config":{"Labels":{"meguri.mg.edge-owner":"held"}}},{"Id":"own-worker","Config":{"Labels":{"meguri.mg.edge-owner":"held"}}},{"Id":"ordinary","Config":{"Labels":{}}}]' ;;
esac
EOF
  chmod +x "$SB/stubs/docker"
  export ROUTE_REMOVE=no
  mg fx-route-cleanup
  [ "$status" -eq 2 ]
  grep -qx edge-sweep "$CALLS"
  [ ! -e "$CALLS.docker" ]
  export ROUTE_REMOVE=yes
  mg fx-route-cleanup
  [ "$status" -eq 0 ]
  grep -qx 'stop -t 10 ordinary' "$CALLS.docker"
  grep -qx 'rm -f ordinary' "$CALLS.docker"
  ! grep -Eq '^(stop|rm).*own-' "$CALLS.docker"
}
