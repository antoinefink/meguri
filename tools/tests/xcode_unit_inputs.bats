#!/usr/bin/env bats
# All producer/native/SSH execution below is controlled test code only.
bats_require_minimum_version 1.5.0

setup() {
  export REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
  source "$REPO_ROOT/tools/tests/xcode_unit_inputs_fixture.sh"
  unit_fixture_setup "$BATS_TEST_TMPDIR/repo with spaces"
  export UNIT_SCRIPT="$MG_ROOT/tools/scripts/xcode-unit-inputs.py"
  export UNIT_GUEST="$BATS_TEST_TMPDIR/guest"
  mkdir -p "$UNIT_GUEST" "$BATS_TEST_TMPDIR/guest-bin"
  # Named-root translation models an SSH guest filesystem. The Python
  # receiver/launcher is real; this is not VM execution evidence.
  cat >"$BATS_TEST_TMPDIR/ssh-double" <<'PY'
#!/usr/bin/env python3
import os, subprocess, sys
command = sys.argv[1].replace(os.environ['MG_ROOT'], os.environ['UNIT_GUEST'] + os.environ['MG_ROOT'])
env = {k: v for k, v in os.environ.items() if not k.startswith(('MEGURI_', 'TEST_RUNNER_'))}
env['PATH'] = os.environ['UNIT_BIN'] + ':' + env['PATH']
result = subprocess.run(command, shell=True, env=env, stdout=subprocess.PIPE)
sys.stdout.buffer.write(result.stdout.replace(os.environ['UNIT_GUEST'].encode(), b''))
raise SystemExit(result.returncode)
PY
  export UNIT_BIN="$BATS_TEST_TMPDIR/guest-bin"
  cat >"$UNIT_BIN/xcodebuild" <<'SH'
#!/bin/bash
printf '%s\n' "$TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE" "$TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI" "$*" >"$UNIT_LAUNCH"
exit 17
SH
  chmod +x "$BATS_TEST_TMPDIR/ssh-double" "$UNIT_BIN/xcodebuild"
  export UNIT_LAUNCH="$BATS_TEST_TMPDIR/launched"
  # The checkout exists in the guest, as VM_INPUTS provides it in the lane.
  mkdir -p "$UNIT_GUEST$MG_ROOT"
}

prepare() { run unit_fixture_prepare; }
deliver() { run python3 "$UNIT_SCRIPT" deliver "$MG_RUN_DIR" "$BATS_TEST_TMPDIR/ssh-double"; }
launch() {
  local command
  command="$(python3 "$UNIT_SCRIPT" command "$MG_RUN_DIR")" || return
  run "$BATS_TEST_TMPDIR/ssh-double" "$command test-without-building -xctestrun unchanged.xctestrun -destination platform=macOS,arch=arm64 -collect-test-diagnostics never"
}

@test "Mac Unit invokes both real producer functions and retains source and publication identities" {
  prepare
  [ "$status" -eq 0 ]
  [ "$(cat "$UNIT_CALLS")" = 'rust-producer
native-cargo' ]
  python3 - <<'PY'
import hashlib, json, os, stat
from pathlib import Path
run = Path(os.environ['MG_RUN_DIR'])
r = json.loads((run / 'xcode-unit-inputs.json').read_text())
assert r['run'] == str(run) and r['source']['dirty'] is False
for k in ('fixture', 'helper'):
    p = Path(r[k]['path'])
    assert r[k]['identity'] == {'sha256': hashlib.sha256(p.read_bytes()).hexdigest(), 'bytes': p.stat().st_size}
    assert p.stat().st_nlink == 1 and r[k]['mode'] == stat.S_IMODE(p.stat().st_mode)
assert (run / 'initial-adoption-producer.json').read_bytes() == Path(r['producer']['path']).read_bytes()
assert json.loads((run / 'cloud-service-code.json').read_text())['compiler'] == os.environ['UNIT_COMPILER']
PY
}

@test "Mac Unit refuses failed missing skipped dirty or mismatched real publications" {
  for flag in UNIT_PRODUCER_FAIL UNIT_MISSING_FIXTURE UNIT_SKIPPED UNIT_DIRTY_RECEIPT UNIT_WRONG_REVISION UNIT_CARGO_FAIL UNIT_WARNING; do
    unit_fixture_setup "$MG_ROOT" "$flag"
    export "$flag=1"
    prepare
    [ "$status" -ne 0 ]
    [[ "$output" == *status=fail* ]]
    [ ! -e "$MG_RUN_DIR/xcode-unit-inputs.json" ]
    [ ! -e "$UNIT_LAUNCH" ]
    unset "$flag"
  done
}

@test "Mac Unit rejects dirty source and foreign run scope before invoking producers" {
  echo dirty >>"$MG_ROOT/tools/mg"
  prepare
  [ "$status" -ne 0 ]
  [ ! -e "$UNIT_CALLS" ]
  unit_fixture_setup "$MG_ROOT" foreign
  export MG_RUN_ID=other
  prepare
  [ "$status" -ne 0 ]
  [ ! -e "$UNIT_CALLS" ]
}

@test "Mac Unit delivers exact fixed owned guest bytes and sets runner env only at launch" {
  prepare
  [ "$status" -eq 0 ]
  deliver
  [ "$status" -eq 0 ]
  [ ! -e "$UNIT_LAUNCH" ]
  python3 - <<'PY'
import json, os, stat
from pathlib import Path
run = Path(os.environ['MG_RUN_DIR'])
r = json.loads((run / 'xcode-unit-delivery.json').read_text())
for i, (src, leaf, mode) in enumerate((('initial-adoption-fixture.json', 'initial-adoption-fixture.json', 0o400),
                                      ('cloud-service-code/meguri-cli', 'meguri-cli', 0o500))):
    p = Path(os.environ['UNIT_GUEST'] + str(run / 'vm-unit-inputs' / leaf))
    assert p.read_bytes() == (run / src).read_bytes()
    assert stat.S_IMODE(p.stat().st_mode) == mode and p.stat().st_nlink == 1
    assert r['pins'][i][1] == p.stat().st_ino
PY
  launch
  [ "$status" -eq 17 ]
  [ "$(sed -n '1p' "$UNIT_LAUNCH")" = "$UNIT_GUEST$MG_RUN_DIR/vm-unit-inputs/initial-adoption-fixture.json" ]
  [ "$(sed -n '2p' "$UNIT_LAUNCH")" = "$UNIT_GUEST$MG_RUN_DIR/vm-unit-inputs/meguri-cli" ]
  [ "$(sed -n '3p' "$UNIT_LAUNCH")" = 'test-without-building -xctestrun unchanged.xctestrun -destination platform=macOS,arch=arm64 -collect-test-diagnostics never' ]
}

@test "Mac Unit refuses missing mutated linked or incorrectly permissioned host publications before delivery" {
  for kind in missing bytes link mode; do
    unit_fixture_setup "$MG_ROOT" "$kind"
    prepare
    [ "$status" -eq 0 ]
    target="$MG_RUN_DIR/cloud-service-code/meguri-cli"
    case "$kind" in
      missing) rm "$target" ;;
      bytes) chmod 700 "$target"; echo changed >>"$target"; chmod 500 "$target" ;;
      link) ln "$target" "$MG_RUN_DIR/extra-link" ;;
      mode) chmod 555 "$target" ;;
    esac
    deliver
    [ "$status" -ne 0 ]
    [ ! -e "$MG_RUN_DIR/xcode-unit-delivery.json" ]
    [ ! -e "$UNIT_GUEST$MG_RUN_DIR/vm-unit-inputs" ]
  done
}

@test "Mac Unit refuses guest inode replacement identical bytes mode links and preexisting destinations" {
  for kind in inode bytes mode link symlink; do
    unit_fixture_setup "$MG_ROOT" "$kind"
    prepare
    [ "$status" -eq 0 ]
    deliver
    [ "$status" -eq 0 ]
    target="$UNIT_GUEST$MG_RUN_DIR/vm-unit-inputs/meguri-cli"
    case "$kind" in
      inode) cp "$target" "$target-new"; chmod 500 "$target-new"; mv "$target-new" "$target" ;;
      bytes) chmod 700 "$target"; echo changed >>"$target"; chmod 500 "$target" ;;
      mode) chmod 555 "$target" ;;
      link) ln "$target" "$target-link" ;;
      symlink) mv "$target" "$target-real"; ln -s "$target-real" "$target" ;;
    esac
    launch
    [ "$status" -eq 70 ]
    [ ! -e "$UNIT_LAUNCH" ]
  done
  deliver
  [ "$status" -ne 0 ]
}

@test "Mac Unit missing receipt or mismatched build cannot reach guest delivery" {
  prepare
  [ "$status" -eq 0 ]
  echo '{}' >>"$MG_RUN_DIR/cloud-service-build.jsonl"
  deliver
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/xcode-unit-delivery.json" ]
  rm "$MG_RUN_DIR/xcode-unit-inputs.json"
  deliver
  [ "$status" -ne 0 ]
  [ ! -e "$UNIT_LAUNCH" ]
}

@test "Mac Unit refuses guest ancestor links and caller-selected destinations" {
  prepare
  [ "$status" -eq 0 ]
  outside="$BATS_TEST_TMPDIR/outside"
  mkdir -p "$outside" "$UNIT_GUEST$MG_ROOT/build/mg"
  ln -s "$outside" "$UNIT_GUEST$MG_ROOT/build/mg/xcode-check"
  deliver
  [ "$status" -ne 0 ]
  [ ! -e "$outside/$MG_RUN_ID" ]
  [ ! -e "$MG_RUN_DIR/xcode-unit-delivery.json" ]
  run python3 "$UNIT_SCRIPT" deliver "$MG_RUN_DIR" "$BATS_TEST_TMPDIR/ssh-double" "$outside"
  [ "$status" -eq 70 ]
  [ ! -e "$outside/$MG_RUN_ID" ]
}

@test "Mac Unit preserves unavailable SSH queuing without admitting input mismatches" {
  prepare
  [ "$status" -eq 0 ]
  printf '#!/bin/sh\nexit 255\n' >"$BATS_TEST_TMPDIR/unavailable"
  chmod +x "$BATS_TEST_TMPDIR/unavailable"
  run python3 "$UNIT_SCRIPT" deliver "$MG_RUN_DIR" "$BATS_TEST_TMPDIR/unavailable"
  [ "$status" -eq 72 ]
  [ ! -e "$MG_RUN_DIR/xcode-unit-delivery.json" ]
  [ ! -e "$UNIT_LAUNCH" ]
}
