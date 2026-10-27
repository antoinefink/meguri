#!/usr/bin/env bats
# Verify tier selection through the real conformance job with isolated runners.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core" "$t/conformance"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  export CALLS="$BATS_TEST_TMPDIR/calls" LIVE_STATUS=pass LIVE_REPORT=yes
  mkdir -p "$SB/stubs"
  cat >"$SB/stubs/cargo" <<'EOF'
#!/bin/sh
echo testkit >>"$CALLS"
printf '%s\n' "$@" >>"$CALLS.cargo-args"
EOF
  chmod +x "$SB/stubs/cargo"
  export PATH="$SB/stubs:$PATH"
  cat >"$SB/tools/mg.d/zz-conformance-fixture.sh" <<'EOF'
fixture_report() {
  jq -n --arg tier "$1" --arg status "$3" \
    '{tier:$tier, totals:{pending:0, reference:0, ($status):1}, failed:(if ($status=="pass" or $status=="reference") then 0 else 1 end), wall_ms:1,
      scenarios:[{scenario:"smoke/fixture", status:$status, fails:($status!="pass" and $status!="reference"), message:"fixture", repro:"fixture"}]}' >"$2"
}
mg_conf__edges_cleanup() { echo edges-cleanup >>"$CALLS"; }
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

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

native_scratch_stubs() {
  export STUB_CLI="$SB/core/stub-cli" CLI_EXIT=0 CLI_WAIT=no
  cat >"$SB/stubs/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "${TMPDIR-unset}" >>"$CALLS.build-tmp"
printf '%s\n' "$PWD" >>"$CALLS.build-cwd"
printf '%s\n' "$*" >>"$CALLS.build-args"
if [ "$1" = build ]; then
  jq -nc --arg exe "$STUB_CLI" '{reason:"compiler-artifact",target:{name:"meguri-cli"},executable:$exe}'
fi
EOF
  cat >"$STUB_CLI" <<'EOF'
#!/bin/sh
printf '%s\n' "$TMPDIR" >>"$CALLS.child-tmp"
printf '%s\n' "$PWD" >>"$CALLS.child-cwd"
prev= tier=t0 report=
for arg in "$@"; do
  [ "$arg" != --live ] || tier=t1
  [ "$prev" != --report ] || report="$arg"
  prev="$arg"
done
if [ "$CLI_WAIT" = yes ]; then
  echo "$$" >"$CALLS.ready"
  exec sleep 300
fi
jq -n --arg tier "$tier" '{tier:$tier,totals:{pending:0,reference:0,pass:1},failed:0,wall_ms:1,
  scenarios:[{scenario:"smoke/fixture",status:"pass",fails:false,message:"fixture",repro:"fixture"}]}' >"$report"
exit "$CLI_EXIT"
EOF
  chmod +x "$SB/stubs/cargo" "$STUB_CLI"
  cat >>"$SB/tools/mg.d/zz-conformance-fixture.sh" <<'EOF'
eval "$(declare -f mg_fixture__real_native | sed '1s/mg_fixture__real_native/mg_conf__cli_native/')"
mg_conf__scratch() {
  echo "$1" >>"$CALLS.scratch"
  [ "${SCRATCH_FAIL:-}" != "$1" ] || return 1
  case "$1" in
    selected) echo "${SCRATCH_SELECTED:-true}" ;;
    prepare) mkdir -p "$MG_RUN_DIR/native-scratch/mount/tmp" ;;
    check) [ -d "$MG_RUN_DIR/native-scratch/mount/tmp" ] ;;
    cleanup)
      [ -d "$MG_RUN_DIR/native-scratch" ] || return 0
      rmdir "$MG_RUN_DIR/native-scratch/mount/tmp" "$MG_RUN_DIR/native-scratch/mount" "$MG_RUN_DIR/native-scratch"
      ;;
  esac
}
mg_conf__record_stale() {
  mg_conf__cli_native conformance run --lane plan --report "$MG_RUN_DIR/stale-before.json" "$@"
}
EOF
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
  ! grep -q TMPDIR "$CALLS.docker-args"
  grep -qx docker-t0 "$CALLS"
  [ ! -e "$run_dir/native-scratch" ]
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
  "$SB/tools/mg" conformance --lane native smoke/fixture >"$out" 2>"$BATS_TEST_TMPDIR/err" 3>&- &
  pid=$!
  for _ in $(seq 100); do
    ready="$(cat "$CALLS.ready" 2>/dev/null)" && [ -n "$ready" ] && break
    sleep 0.1
  done
  [ -n "$ready" ]
  kill -TERM "$pid"
  wait "$pid" || rc=$?
  [ "$rc" -eq 2 ]
  result="$(cat "$out")"
  jq -e '.status=="error" and any(.failures[]; .id=="cancelled")' "$result"
  ! kill -0 "$ready" 2>/dev/null
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
    'test', '-q', '-p', 'meguri-testkit', '--tests', '--no-fail-fast',
]
native = [json.loads(line) for line in pathlib.Path(calls + '.args').read_text().splitlines()]
assert len(native) == 2
assert native[0] == [
    'conformance', 'run', '--conformance', root + '/conformance',
    '--lane', 'macos-native', '--report', run_dir + '/t0-native.json',
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
import sys, yaml
runs = {r['id']: r for r in yaml.safe_load(open(sys.argv[1]))['runs']}
t0, t1 = runs['conformance'], runs['live-oracle']
assert set(t0['triggers']) <= set(runs['core-test']['triggers']), 'core-test must cover every conformance trigger'
assert t0['args'] == ['--lane', 'all']
assert t1['args'][:3] == ['--live-only', '--lane', 'native']
assert all(any(path.startswith(trigger) for trigger in t0['triggers']) for path in t1['triggers'])
assert 'sync-server' not in t1['needs']
# The positive selectors cover every active scenario, and no frozen one.
from pathlib import Path
root = Path(sys.argv[1]).resolve().parents[2] / 'conformance/scenarios'
filters = t1['args'][3:]
assert filters and '*' not in filters
for path in root.rglob('*.yaml'):
    if path.name == 'area.yaml':
        continue
    name = path.relative_to(root).with_suffix('').as_posix()
    frozen = (name.startswith(('sync/conflicts/', 'sync/holdback/', 'sync/media/',
                               'sync/meta/', 'sync/normal/', 'sync/transport/', 'sync/wire/',
                               'smoke/sync/', 'preservation/sync/'))
              or name == 'sched/compat/keys-survive-sync')
    selected = any(name == f or name.startswith(f + '/') for f in filters)
    assert selected != frozen, name
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
