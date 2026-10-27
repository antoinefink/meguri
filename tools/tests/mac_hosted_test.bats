#!/usr/bin/env bats
# Focused app-hosted routing/reporting. All Apple and VM operations are stubs.
bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
SUITE=NativeAudioFileTests/retainedDescriptorPreparation

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t"
  cp -R "$REPO_ROOT/tools" "$REPO_ROOT/docker" "$t/"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log" FX_RESULT="$BATS_TEST_TMPDIR/result"
  mkdir -p "$FX_RESULT"
  : >"$STUB_LOG"
  echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_RESULT/summary.json"
  set_cases "$SUITE()" Passed
  stub uname 'case "$*" in -s) echo "${FX_HOST_OS:-Darwin}";; *) exit 97;; esac'
  stub xcodebuild 'echo "RAW XCODE $*" >>"$STUB_LOG"; exit 97'
  stub xcrun '
echo "xcrun $*" >>"$STUB_LOG"
case "$*" in
  "xcresulttool get test-results summary --path "*) cat "$FX_RESULT/summary.json" ;;
  "xcresulttool get test-results tests --path "*) cat "$FX_RESULT/tests.json" ;;
  *) exit 97 ;;
esac'
  export PATH="$BIN:$PATH" MEGURI_SIM_ALLOW_HOST=1 MEGURI_MAC_UI_ALLOW_HOST=1
  cat >"$SB/tools/mg.d/zz-hosted-fakes.sh" <<'EOF'
mg_app_input_digest() {
  echo "digest" >>"$STUB_LOG"
  if [ -f "$MG_RUN_DIR/drift" ]; then echo changed; else echo original; fi
}
mg_sim_begin() {
  echo "begin allow-host=$MEGURI_SIM_ALLOW_HOST" >>"$STUB_LOG"
  MG_SIM_RUNNER=vm
  MG_MAC_VM_REASON="fake unavailable"
  [ "${FX_QUEUE:-}" != begin ] || { mg__mac_vm_queued "fake unavailable" "$MG_VM_PART"; return 2; }
}
mg_app_build() {
  echo "build $*" >>"$STUB_LOG"
  mkdir -p "$MG_RUN_DIR/build-mac-Debug-Snapshot.xcresult"
  [ "${FX_DRIFT:-}" != build ] || touch "$MG_RUN_DIR/drift"
  return 0
}
mg_sim_take() {
  echo "take VM" >>"$STUB_LOG"
  [ "${FX_DRIFT:-}" != take ] || touch "$MG_RUN_DIR/drift"
  [ "${FX_QUEUE:-}" != take ] || { mg__mac_vm_unreachable "fake lease unavailable"; return 2; }
}
mg_vm_xcodebuild_test() {
  echo "vm-test $*" >>"$STUB_LOG"
  MG_MAC_VM_RUN_ERROR="" MG_MAC_VM_RUN_QUEUED=""
  echo 'selected test log' >"$1.log"
  if [ "${FX_QUEUE:-}" = run ]; then
    mg__mac_vm_unreachable "fake result transport unavailable"
    MG_MAC_VM_RUN_QUEUED=1
    return 1
  fi
  if [ "${FX_ERROR:-}" = run ]; then
    mg_failure mac-vm-copy "fake invalid transport"
    MG_MAC_VM_RUN_ERROR=1
    return 1
  fi
  mkdir -p "$1.xcresult"
  [ "${FX_DRIFT:-}" != run ] || touch "$MG_RUN_DIR/drift"
  return "${FX_TEST_EXIT:-0}"
}
mg_host_lease() { echo 'FORBIDDEN host lease' >>"$STUB_LOG"; return 97; }
mg_mac_vm_ssh() { echo 'FORBIDDEN snapshot output' >>"$STUB_LOG"; return 97; }
mg_image_tools() { echo 'FORBIDDEN image tools' >>"$STUB_LOG"; return 97; }
mg_snapshots__finish() { echo 'FORBIDDEN snapshots finish' >>"$STUB_LOG"; return 97; }
EOF
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }
set_cases() {
  jq -nc --arg id "$1" --arg r "$2" \
    '{testNodes: [{nodeType: "Test Suite", children: [{nodeType: "Test Case", nodeIdentifier: $id, result: $r}]}]}' >"$FX_RESULT/tests.json"
}
mg() {
  run --separate-stderr "$SB/tools/mg" mac-hosted-test "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }
called() { grep -Eq -- "$1" "$STUB_LOG"; }
not_called() { ! grep -Eq -- "$1" "$STUB_LOG"; }

@test "mac-hosted-test requires one safe exact selector before any work" {
  local bad
  mg
  [ "$status" -eq 2 ]
  mg --suite
  [ "$status" -eq 2 ]
  for bad in '' NativeAudioFileTests all 'NativeAudioFileTests/*' "$SUITE()" \
      "MeguriSnapshotTests/$SUITE" " $SUITE" "$SUITE " "$SUITE/extra" \
      "$SUITE;echo" "$SUITE"$'\nOther/test' '-only-testing:Other/test'; do
    mg --suite "$bad"
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
  done
  mg --suite "$SUITE" --suite "$SUITE"
  [ "$status" -eq 2 ]
  mg --suite "$SUITE" --platform iphone
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

@test "mac-hosted-test guards host platform and signing before VM or build" {
  FX_HOST_OS=Linux mg --suite "$SUITE"
  [ "$status" -eq 2 ]
  MEGURI_APP_SIGNING=invalid mg --suite "$SUITE"
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

@test "mac-hosted-test routes only the owned exact method through Snapshot helper and VM" {
  mg --suite "$SUITE"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.hosted_test.full_suite)" = false ]
  [ "$(field .details.selected_test.executed)" -eq 1 ]
  [ "$(field '.summary | [.total,.passed,.failed,.skipped] | join(",")')" = 1,1,0,0 ]
  called '^begin allow-host=0$'
  called '^build mac Debug Snapshot$'
  called '^take VM$'
  called "^vm-test .* Snapshot macosx platform=macOS,arch=arm64 Debug -only-testing:MeguriSnapshotTests/$SUITE$"
  not_called 'FORBIDDEN|RAW XCODE|simctl|all$'
  [ "$(field '[.artifacts[] | select(.kind == "xcresult")] | length')" -eq 2 ]
  [ -f "$(field '.artifacts[] | select(.kind == "test-counts") | .path')" ]
  [ -f "$(field '.artifacts[] | select(.kind == "log" and (.path | endswith("selected.log"))) | .path')" ]
  [ "${#lines[@]}" -eq 1 ]
  set_cases "MeguriSnapshotTests/$SUITE" Passed
  mg --suite "$SUITE"
  [ "$status" -eq 0 ]
}

@test "mac-hosted-test refuses zero executions and a skipped selected method" {
  echo '{"result":"Passed","passedTests":0,"failedTests":0,"skippedTests":0}' >"$FX_RESULT/summary.json"
  echo '{"testNodes":[]}' >"$FX_RESULT/tests.json"
  mg --suite "$SUITE"
  [ "$status" -eq 1 ]
  [ "$(field .summary.total)" -eq 0 ]
  [ "$(field .details.selected_test.executed)" -eq 0 ]
  echo '{"result":"Passed","passedTests":0,"failedTests":0,"skippedTests":1}' >"$FX_RESULT/summary.json"
  set_cases "$SUITE()" Skipped
  mg --suite "$SUITE"
  [ "$status" -eq 1 ]
  [ "$(field .summary.skipped)" -eq 1 ]
  [ "$(field .details.selected_test.executed)" -eq 0 ]
}

@test "mac-hosted-test refuses wrong extra duplicate or aggregate-only passing cases" {
  set_cases OtherTests/other Passed
  mg --suite "$SUITE"
  [ "$status" -eq 1 ]
  [ "$(field .summary.passed)" -eq 0 ]
  set_cases "$SUITE()" Passed
  jq '.testNodes[0].children += .testNodes[0].children' "$FX_RESULT/tests.json" >"$FX_RESULT/extra.json"
  mv "$FX_RESULT/extra.json" "$FX_RESULT/tests.json"
  mg --suite "$SUITE"
  [ "$status" -eq 1 ]
  [ "$(field .summary.passed)" -eq 2 ]
  jq '.testNodes[0].children[1].nodeIdentifier = "OtherTests/other"' "$FX_RESULT/tests.json" >"$FX_RESULT/extra.json"
  mv "$FX_RESULT/extra.json" "$FX_RESULT/tests.json"
  mg --suite "$SUITE"
  [ "$status" -eq 1 ]
  [ "$(field .summary.passed)" -eq 1 ]
  set_cases "$SUITE()" Passed
  echo '{"result":"Passed","passedTests":2,"failedTests":0,"skippedTests":0}' >"$FX_RESULT/summary.json"
  mg --suite "$SUITE"
  [ "$status" -eq 1 ]
}

@test "mac-hosted-test counts selected failures and refuses nonzero test exits" {
  echo '{"result":"Failed","passedTests":0,"failedTests":1,"skippedTests":0}' >"$FX_RESULT/summary.json"
  set_cases "$SUITE()" Failed
  FX_TEST_EXIT=65 mg --suite "$SUITE"
  [ "$status" -eq 1 ]
  [ "$(field .summary.failed)" -eq 1 ]
  [ "$(field .details.selected_test.executed)" -eq 1 ]
  echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_RESULT/summary.json"
  set_cases "$SUITE()" Passed
  FX_TEST_EXIT=65 mg --suite "$SUITE"
  [ "$status" -eq 1 ]
}

@test "mac-hosted-test unreadable result data is error never a pass" {
  echo invalid >"$FX_RESULT/tests.json"
  mg --suite "$SUITE"
  [ "$status" -eq 2 ]
  echo invalid >"$FX_RESULT/summary.json"
  mg --suite "$SUITE"
  [ "$status" -eq 2 ]
}

@test "mac-hosted-test preserves VM queued and transport error semantics without host fallback" {
  local phase
  for phase in begin take run; do
    FX_QUEUE="$phase" mg --suite "$SUITE"
    [ "$status" -eq 0 ]
    [ "$(field .status)" = warn ]
    [ "$(field .details.queued)" = true ]
    [ "$(field '.failures | length')" -eq 0 ]
  done
  FX_ERROR=run mg --suite "$SUITE"
  [ "$status" -eq 2 ]
  not_called 'FORBIDDEN|RAW XCODE'
}

@test "mac-hosted-test refuses changed source binding at build and receipt boundaries" {
  FX_DRIFT=build mg --suite "$SUITE"
  [ "$status" -eq 2 ]
  not_called '^vm-test '
  FX_DRIFT=take mg --suite "$SUITE"
  [ "$status" -eq 2 ]
  not_called '^vm-test '
  FX_DRIFT=run mg --suite "$SUITE"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = source-binding ]
}

@test "mac-hosted-test dry-run registers host placement without invoking helpers or updating latest" {
  mg --dry-run --suite "$SUITE"
  [ "$status" -eq 0 ]
  [ "$(field .dry_run)" = true ]
  [ "$(field .details.job.placement)" = host ]
  [ "$(field .details.job.function)" = mg_mac_hosted_test ]
  [ ! -s "$STUB_LOG" ]
  [ ! -e "$SB/build/mg/mac-hosted-test/latest" ]
}
