#!/usr/bin/env bats
# Native helper publication and host headless routing; Cargo/Swift are fakes.
bats_require_minimum_version 1.5.0

setup() {
  export REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
  export MG_ROOT="$BATS_TEST_TMPDIR/repo with spaces"
  export MG_RUN_DIR="$MG_ROOT/build/mg/swift-test/run"
  export CARGO_TARGET_DIR="$BATS_TEST_TMPDIR/native target"
  export FX_EXECUTABLE="$CARGO_TARGET_DIR/debug/meguri-cli"
  export FX_CALLS="$BATS_TEST_TMPDIR/calls"
  mkdir -p "$MG_ROOT/tools/scripts" "$MG_ROOT/core" "$MG_RUN_DIR" \
    "$MG_ROOT/app/Packages/MeguriKit" "$CARGO_TARGET_DIR/debug" "$BATS_TEST_TMPDIR/bin"
  cp "$REPO_ROOT/tools/scripts/swift-cloud-service-helper.py" \
    "$REPO_ROOT/tools/scripts/native-media-review-capture.py" \
    "$REPO_ROOT/tools/scripts/native-media-export-capture.py" "$MG_ROOT/tools/scripts/"
  echo '// synthetic manifest' >"$MG_ROOT/app/Packages/MeguriKit/Package.swift"
  printf '#!/bin/sh\nexit 97\n' >"$FX_EXECUTABLE"
  chmod 755 "$FX_EXECUTABLE"
  : >"$FX_CALLS"
  cat >"$BATS_TEST_TMPDIR/bin/cargo" <<'CARGO'
#!/bin/bash
[ "$*" = 'build -q -p meguri-cli --message-format=json' ] || exit 97
[ "$PWD" = "$MG_ROOT/core" ] || exit 97
[ "$CARGO_TARGET_DIR/debug/meguri-cli" = "$FX_EXECUTABLE" ] || exit 97
echo cargo >>"$FX_CALLS"
[ "${FX_CARGO_FAIL:-0}" = 0 ] || exit 1
python3 - <<'PY'
import json, os
level = os.environ.get('FX_CARGO_DIAGNOSTIC')
if level:
    print(json.dumps({'reason': 'compiler-message', 'message': {
        'level': level, 'message': 'synthetic-compiler-body ' + os.environ['MG_ROOT']}}))
for _ in range(int(os.environ.get('FX_ARTIFACTS', '1'))):
    print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': 'meguri-cli'},
                      'executable': os.environ['FX_EXECUTABLE']}))
if os.environ.get('FX_CARGO_MALFORMED'):
    print('synthetic-invalid-cargo-json')
PY
CARGO
  chmod +x "$BATS_TEST_TMPDIR/bin/cargo"
  export PATH="$BATS_TEST_TMPDIR/bin:$PATH"
  # A prepared path must replace an inherited, untrusted helper setting.
  export MEGURI_CLOUD_SERVICE_CLI=/ambient/must-not-run
}

prepare() {
  run bash -c '
    mg_job() { :; }
    mg_failure() { echo "failure=$1"; }
    mg_status() { echo "status=$1"; }
    mg_artifact() { :; }
    mg_detail() { :; }
    source "$REPO_ROOT/tools/mg.d/bridge.sh"
    mg__swift_cloud_service_helper || exit $?
    [ "$MEGURI_CLOUD_SERVICE_CLI" = "$MG_RUN_DIR/cloud-service-code/meguri-cli" ]
  '
}

lane() {
  run bash -c '
    cd "$MG_ROOT"
    mg_job() { :; }
    mg_failure() { echo "failure=$1:$2"; }
    mg_status() { echo "status=$1"; }
    mg_artifact() { :; }
    mg_detail() { :; }
    mg_count() { :; }
    mg_message() { :; }
    mg_lint_ui_first() { :; }
    source "$REPO_ROOT/tools/mg.d/bridge.sh"
    mg__swift_initial_adoption_fixture() {
      echo initial-fixture >>"$FX_CALLS"
      if [ "${FX_INITIAL_FAIL:-0}" != 0 ]; then
        mg_failure initial-adoption-fixture failed
        mg_status fail
        return 1
      fi
    }
    mg__xcframework() { echo core >>"$FX_CALLS"; }
    swift() {
      echo "swift-$1" >>"$FX_CALLS"
      [ "$MEGURI_CLOUD_SERVICE_CLI" = "$MG_RUN_DIR/cloud-service-code/meguri-cli" ] || return 97
      if [ "$1" = build ]; then return 0; fi
      # Execution is intentionally failed; verify normal exit accounting remains.
      printf "%s\n" "$MEGURI_CLOUD_SERVICE_CLI" >"$MG_RUN_DIR/executed-helper"
      case " $* " in *" --skip-build "*) return 17 ;; *) return 97 ;; esac
    }
    mg_swift_test --package MeguriKit -- --filter CloudServiceProcessTests
  '
}

@test "Swift cloud helper publishes an owned copy and hash receipt using native Cargo cache" {
  # Cargo may hardlink its deps artifact; publication must still make a copy.
  ln "$FX_EXECUTABLE" "$CARGO_TARGET_DIR/debug/deps-artifact"
  prepare
  [ "$status" -eq 0 ]
  [ -x "$MG_RUN_DIR/cloud-service-code/meguri-cli" ]
  [ ! -L "$MG_RUN_DIR/cloud-service-code/meguri-cli" ]
  python3 - <<'PY'
import hashlib, json, os, stat
from pathlib import Path
run = Path(os.environ['MG_RUN_DIR'])
source = Path(os.environ['FX_EXECUTABLE'])
copy = run / 'cloud-service-code/meguri-cli'
r = json.loads((run / 'cloud-service-code.json').read_text())
assert source.stat().st_nlink == 2 and copy.stat().st_nlink == 1
assert (source.stat().st_dev, source.stat().st_ino) != (copy.stat().st_dev, copy.stat().st_ino)
assert copy.stat().st_uid == os.getuid() and stat.S_IMODE(copy.stat().st_mode) == 0o500
assert r['compiler'] == str(source) and r['published'] == str(copy)
assert r['compiled'] == r['executable'] == {'sha256': hashlib.sha256(copy.read_bytes()).hexdigest(), 'bytes': copy.stat().st_size}
assert r['build']['sha256'] == hashlib.sha256((run / 'cloud-service-build.jsonl').read_bytes()).hexdigest()
PY
}

@test "Swift cloud helper refuses zero or multiple Cargo executables" {
  export FX_ARTIFACTS=0
  prepare
  [ "$status" -ne 0 ]
  [[ "$output" == *failure=cloud-service-helper* ]]
  [ ! -e "$MG_RUN_DIR/cloud-service-code" ]
  export FX_ARTIFACTS=2
  prepare
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/cloud-service-code" ]
}

@test "Swift cloud helper refuses linked or writable sources and preexisting publication" {
  mv "$FX_EXECUTABLE" "$FX_EXECUTABLE.real"
  ln -s "$FX_EXECUTABLE.real" "$FX_EXECUTABLE"
  prepare
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/cloud-service-code.json" ]
  rm "$FX_EXECUTABLE"
  mv "$FX_EXECUTABLE.real" "$FX_EXECUTABLE"
  chmod 775 "$FX_EXECUTABLE"
  prepare
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/cloud-service-code.json" ]
  chmod 755 "$FX_EXECUTABLE"
  mkdir "$MG_RUN_DIR/cloud-service-code"
  echo keep >"$MG_RUN_DIR/cloud-service-code/foreign"
  prepare
  [ "$status" -ne 0 ]
  [ "$(cat "$MG_RUN_DIR/cloud-service-code/foreign")" = keep ]
  [ ! -e "$MG_RUN_DIR/cloud-service-code/meguri-cli" ]
}

@test "Swift cloud helper refuses source and destination substitutions during publication" {
  run python3 "$REPO_ROOT/tools/tests/swift_cloud_service_substitution.py"
  [ "$status" -eq 0 ]
}

@test "Swift cloud helper preparation failure stops compilation and test execution" {
  export FX_CARGO_FAIL=1
  lane
  [ "$status" -eq 0 ]
  [[ "$output" == *failure=cloud-service-helper*status=fail* ]]
  [ "$(cat "$FX_CALLS")" = 'initial-fixture
cargo' ]
  [ ! -e "$MG_RUN_DIR/executed-helper" ]
  [ -f "$MG_RUN_DIR/cloud-service-build.jsonl" ]
}

@test "Swift cloud helper publication failure stops headless tests without queued or pass status" {
  export FX_ARTIFACTS=2
  lane
  [[ "$output" == *failure=cloud-service-helper*status=fail* ]]
  [[ "$output" != *status=pass* && "$output" != *queued* ]]
  [ "$(cat "$FX_CALLS")" = 'initial-fixture
cargo' ]
  [ ! -e "$MG_RUN_DIR/executed-helper" ]
}

@test "Swift cloud helper rejects warning and error JSON despite successful Cargo and an artifact" {
  for level in warning error; do
    export FX_CARGO_DIAGNOSTIC="$level"
    : >"$FX_CALLS"
    lane
    [ "$status" -eq 0 ]
    [ "$output" = 'failure=cloud-service-helper:native helper build diagnostics rejected; Swift tests did not start
status=fail' ]
    [ "$(cat "$FX_CALLS")" = 'initial-fixture
cargo' ]
    [ ! -e "$MG_RUN_DIR/cloud-service-code" ]
    [ ! -e "$MG_RUN_DIR/cloud-service-code.json" ]
    [ ! -e "$MG_RUN_DIR/executed-helper" ]
    [ ! -e "$MG_RUN_DIR/swift-build.log" ]
    python3 - <<'PY'
import json, os
from pathlib import Path
messages = [json.loads(line) for line in
            (Path(os.environ['MG_RUN_DIR']) / 'cloud-service-build.jsonl').read_text().splitlines()]
assert len(messages) == 2
assert messages[0]['reason'] == 'compiler-message'
assert messages[0]['message']['level'] == os.environ['FX_CARGO_DIAGNOSTIC']
assert messages[1]['reason'] == 'compiler-artifact'
assert messages[1]['executable'] == os.environ['FX_EXECUTABLE']
PY
  done
}

@test "Swift cloud helper rejects malformed JSON despite successful Cargo and an artifact" {
  export FX_CARGO_MALFORMED=1
  lane
  [ "$status" -eq 0 ]
  [ "$output" = 'failure=cloud-service-helper:native helper build diagnostics rejected; Swift tests did not start
status=fail' ]
  [ "$(cat "$FX_CALLS")" = 'initial-fixture
cargo' ]
  [ -s "$MG_RUN_DIR/cloud-service-build.jsonl" ]
  [ ! -e "$MG_RUN_DIR/cloud-service-code" ]
  [ ! -e "$MG_RUN_DIR/cloud-service-code.json" ]
  [ ! -e "$MG_RUN_DIR/executed-helper" ]
  [ ! -e "$MG_RUN_DIR/swift-build.log" ]
}

@test "Swift cloud helper retains initial fixture failure before native preparation" {
  export FX_INITIAL_FAIL=1
  lane
  [[ "$output" == *failure=initial-adoption-fixture* ]]
  [ "$(cat "$FX_CALLS")" = initial-fixture ]
  [ ! -e "$MG_RUN_DIR/cloud-service-build.jsonl" ]
  [ ! -e "$MG_RUN_DIR/executed-helper" ]
}

@test "Swift cloud helper prepared environment reaches headless skip-build tests with existing failure accounting" {
  lane
  [ "$status" -eq 0 ]
  [ "$(cat "$FX_CALLS")" = 'initial-fixture
cargo
core
swift-build
swift-test' ]
  [ "$(cat "$MG_RUN_DIR/executed-helper")" = "$MG_RUN_DIR/cloud-service-code/meguri-cli" ]
  [[ "$output" == *failure=swift-test-exit*status=fail* ]]
  [ -f "$MG_RUN_DIR/cloud-service-code.json" ]
}

@test "native helper admits only current swift-test or xcode-check run scopes" {
  export MG_JOB=xcode-check MG_RUN_ID=xcode-inputs
  export MG_RUN_DIR="$MG_ROOT/build/mg/xcode-check/$MG_RUN_ID"
  mkdir -p "$MG_RUN_DIR"
  prepare
  [ "$status" -eq 0 ]
  [ -x "$MG_RUN_DIR/cloud-service-code/meguri-cli" ]
  for scope in ui-test arbitrary swift-test/nested xcode-check/nested; do
    export MG_RUN_DIR="$MG_ROOT/build/mg/$scope/foreign"
    mkdir -p "$MG_RUN_DIR"
    prepare
    [ "$status" -ne 0 ]
    [ ! -e "$MG_RUN_DIR/cloud-service-code" ]
  done
  export MG_RUN_DIR="$MG_ROOT/build/mg/xcode-check/not-current"
  mkdir -p "$MG_RUN_DIR"
  prepare
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/cloud-service-code" ]
}
