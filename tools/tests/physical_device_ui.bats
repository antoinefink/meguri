#!/usr/bin/env bats
# Same throwaway tools/ checkout and PATH stubs as agent_loop/sim_vm.bats.
# Exercise the real build, policy selector/guard, dispatcher and reporting;
# no Apple commands, physical hardware or VM is used by these regressions.
bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/app/TestPlans"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/app/TestPlans/Performance.xctestplan" "$t/app/TestPlans/"
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
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  export FX_DEVICES="$BATS_TEST_TMPDIR/devices.json" FX_SUMMARY="$BATS_TEST_TMPDIR/summary.json"
  export FX_TESTS="$BATS_TEST_TMPDIR/tests.json"
  # Synthetic destination identifiers only, never a user's phone identity.
  export PHONE=00000000-0000000000000001
  SIM=00000000-0000-4000-8000-000000000002
  MAC=00000000-0000-4000-8000-000000000003
  : >"$STUB_LOG"
  jq -n --arg p "$PHONE" --arg s "$SIM" --arg m "$MAC" '[
    {identifier:$p, name:"PRIVATE_PHONE_NAME", serialNumber:"PRIVATE_SERIAL", modelCode:"iPhone99,1",
      platform:"com.apple.platform.iphoneos", simulator:false, operatingSystemVersion:"27.0 (24A000)", available:true},
    {identifier:$s, name:"PRIVATE_SIM_NAME", modelCode:"iPhone99,1",
      platform:"com.apple.platform.iphonesimulator", simulator:true, operatingSystemVersion:"27.0", available:true},
    {identifier:$m, name:"PRIVATE_MAC_NAME", modelCode:"Mac99,1",
      platform:"com.apple.platform.macosx", simulator:false, operatingSystemVersion:"27.0", available:true}]' >"$FX_DEVICES"
  echo '{"result":"Passed","passedTests":2,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
  test_tree '[
    {nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Passed"},
    {nodeType:"Test Case", nodeIdentifier:"MediaTests/testVideo()", result:"Passed"}]'
  cat >"$SB/tools/mg.d/zz-physical-fakes.sh" <<'EOF'
# Physical runs must ignore MG_SIM_RUNNER.
MG_SIM_RUNNER=vm
mg_lint_ui_first() { echo lint >>"$STUB_LOG"; }
mg__xcframework() { echo "xcframework $*" >>"$STUB_LOG"; }
mg_host_lease() { echo "host-lease $*" >>"$STUB_LOG"; [ "$*" = --no-session ]; }
mg_sim_begin() { echo sim-begin >>"$STUB_LOG"; MG_SIM_RUNNER=vm; }
mg_sim_check_given() { echo "sim-check $*" >>"$STUB_LOG"; }
mg_sim_take() { echo sim-take >>"$STUB_LOG"; }
mg_sim_prepare() { echo "sim-prepare $*" >>"$STUB_LOG"; }
mg_vm_xcodebuild_test() {
  echo "vm-test $*" >>"$STUB_LOG"
  mkdir -p "$1.xcresult"
  : >"$1.log"
}
EOF
  stub xcrun 'echo "xcrun $*" >>"$STUB_LOG"
case "$*" in
  "xcdevice list --timeout 20") cat "$FX_DEVICES"; exit "${FX_INVENTORY_RC:-0}" ;;
  "xcresulttool get build-results --path "*) echo "{\"errorCount\":0,\"warningCount\":0}" ;;
  "xcresulttool get test-results summary --path "*) cat "$FX_SUMMARY"; exit "${FX_SUMMARY_RC:-0}" ;;
  "xcresulttool get test-results tests --path "*) cat "$FX_TESTS"; exit "${FX_TESTS_RC:-0}" ;;
  *) exit 97 ;;
esac'
  stub plutil 'echo "plutil $*" >>"$STUB_LOG"
[ "$1 $2 $3 $4 $5" = "-extract CFBundleIdentifier raw -o -" ] || exit 97
cat "$6"'
  stub xcodebuild 'echo "xcodebuild $*" >>"$STUB_LOG"
action="$1"; shift
dest=""; config=""; scheme=""; plan=""; result=""; dd=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -destination) dest="$2"; shift ;;
    -configuration) config="$2"; shift ;;
    -scheme) scheme="$2"; shift ;;
    -testPlan) plan="$2"; shift ;;
    -resultBundlePath) result="$2"; shift ;;
    -derivedDataPath) dd="$2"; shift ;;
  esac
  shift
done
mkdir -p "$result"
if [ "$action" = build-for-testing ]; then
  sdk=iphonesimulator
  case "$dest" in platform=iOS,id=*) sdk=iphoneos ;; esac
  p="$dd/Build/Products"
  mkdir -p "$p/$config-$sdk/Meguri.app"
  echo "${FX_BUNDLE:-com.antoinefink.meguri.dev}" >"$p/$config-$sdk/Meguri.app/Info.plist"
  [ "${FX_POLICY:-}" != wrong-sdk ] || sdk=iphonesimulator
  python3 -c "import plistlib,sys
target = {\"IsUITestBundle\": True, \"TestBundlePath\": \"__TESTROOT__/\" + sys.argv[2] + \"/MeguriUITests.xctest\"}
if sys.argv[3] == \"unsafe\": target[\"UITargetAppMainThreadCheckerEnabled\"] = True
with open(sys.argv[1], \"wb\") as f: plistlib.dump({\"TestConfigurations\":[{\"TestTargets\":[target]}]}, f)" \
    "$p/${scheme}_${plan}_${sdk}27.0-arm64.xctestrun" "$config-$sdk" "${FX_POLICY:-}"
  exit "${FX_BUILD_RC:-0}"
fi
exit "${FX_XCODE_RC:-0}"'
  export PATH="$BIN:$PATH"
}

teardown() {
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures, details}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 15
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }
mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }
called() { grep -Eq -- "$1" "$STUB_LOG" || { cat "$STUB_LOG"; return 1; }; }
not_called() { ! grep -Eq -- "$1" "$STUB_LOG" || { cat "$STUB_LOG"; return 1; }; }
phone_run() { mg ui-test --physical-device "$PHONE" --config Profile --suite MediaTests/testAudio --suite MediaTests/testVideo "$@"; }
test_tree() {
  jq -n "{testNodes:[{nodeType:\"Test Plan\", children:[{nodeType:\"UI test bundle\",
    subTestNodes:[{nodeType:\"Test Suite\", name:\"MediaTests\", subTestNodes:$1}]}]}]}" >"$FX_TESTS"
}

@test "physical ui inventory uses xcdevice array keys and excludes personal fields, Mac and simulator identifiers" {
  mg ui-test --list-physical-devices
  [ "$status" -eq 0 ]
  [ "$(field '.details.devices | length')" -eq 1 ]
  [ "$(field '.details.devices[0].identifier')" = "$PHONE" ]
  [ "$(field '.details.devices[0].modelCode')" = iPhone99,1 ]
  [ "$(field '.details.devices[0].operatingSystemVersion')" = '27.0 (24A000)' ]
  [ "$(field '.details.devices[0].available')" = true ]
  [ "$(field '.details.devices[0] | keys | join(",")')" = available,identifier,modelCode,operatingSystemVersion ]
  ! rg 'PRIVATE_|serialNumber' "$result" "$(dirname "$result")/log.txt"
  ! grep -F "$SIM" "$result"
  ! grep -F "$MAC" "$result"
  called '^xcrun xcdevice list --timeout 20$'
  not_called '^(lint|xcframework|host-lease|sim-|vm-test|xcodebuild|plutil)'
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "physical ui inventory returns an empty inventory and diagnoses malformed or failed discovery" {
  echo '[]' >"$FX_DEVICES"
  mg ui-test --list-physical-devices
  [ "$status" -eq 0 ]
  [ "$(field '.details.devices | length')" -eq 0 ]
  local bad
  for bad in 'not JSON' '{}' '[{}]' '[] []'; do
    printf '%s\n' "$bad" >"$FX_DEVICES"
    mg ui-test --list-physical-devices
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = physical-device-inventory ]
  done
  echo '[]' >"$FX_DEVICES"
  FX_INVENTORY_RC=1 mg ui-test --list-physical-devices
  [ "$status" -eq 2 ]
  not_called '^(lint|xcframework|host-lease|sim-|vm-test|xcodebuild|plutil)'
}

@test "physical ui Profile builds all core slices and iphoneos Performance products then tests only exact selectors on host" {
  phone_run
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .summary.passed)" -eq 2 ]
  [ "$(field .summary.skipped)" -eq 0 ]
  [ "$(field .details.performance_test_policy.status)" = pass ]
  [ "$(field .details.build.platform)" = ios-device ]
  [ "$(field .details.platform)" = iphone ]
  [ "$(field .details.destination_platform)" = ios-device ]
  [[ "$(field .details.build.app)" == *'/Profile-iphoneos/Meguri.app' ]]
  called '^host-lease --no-session$'
  called '^xcframework .* --configuration Profile --slices mac,sim,device$'
  called "^xcodebuild build-for-testing .* -scheme Meguri-Profile -testPlan Performance -configuration Profile -destination platform=iOS,id=$PHONE .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$"
  called "^plutil -extract CFBundleIdentifier raw -o - $SB/build/app/DerivedData/Build/Products/Profile-iphoneos/Meguri.app/Info.plist$"
  called "^xcodebuild test-without-building .* -scheme Meguri-Profile -testPlan Performance -configuration Profile -destination platform=iOS,id=$PHONE .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration -parallel-testing-enabled NO -destination-timeout 60 -test-timeouts-enabled YES -only-testing:MeguriUITests/MediaTests/testAudio -only-testing:MeguriUITests/MediaTests/testVideo$"
  called '^xcrun xcresulttool get test-results tests --path '
  not_called '^(sim-|vm-test)|xcrun (simctl|devicectl)'
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "physical ui defaults to Debug with the UI plan even with ambient Profile scheme settings" {
  MG_APP_TEST_SCHEME=Meguri-Profile MG_APP_TEST_PLAN=Performance \
    mg ui-test --physical-device "$PHONE" --suite MediaTests/testAudio
  [ "$status" -eq 0 ]
  called "^xcodebuild build-for-testing .* -scheme Meguri -testPlan UI -configuration Debug -destination platform=iOS,id=$PHONE .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$"
  called "^xcodebuild test-without-building .* -scheme Meguri -testPlan UI -configuration Debug -destination platform=iOS,id=$PHONE .* MEGURI_SIGNING=team "
  [ "$(field '.details.performance_test_policy // "absent"')" = absent ]
  not_called '^(sim-|vm-test)'
}

@test "physical ui refuses conflicting inventory and run arguments, Mac platform, unsafe config and invalid selectors before discovery or build" {
  local args
  mg ui-test --list-physical-devices --config Debug
  [ "$status" -eq 2 ]
  mg ui-test --suite MediaTests --list-physical-devices
  [ "$status" -eq 2 ]
  mg ui-test --list-physical-devices --physical-device "$PHONE"
  [ "$status" -eq 2 ]
  phone_run --device "$SIM"
  [ "$status" -eq 2 ]
  phone_run --platform mac
  [ "$status" -eq 2 ]
  for args in Release AppStore; do
    phone_run --config "$args"
    [ "$status" -eq 2 ]
  done
  for args in '' auto 'bad/id' '00000000-0000000000000001,platform=macOS' "$PHONE"$'\nextra'; do
    mg ui-test --physical-device "$args" --suite MediaTests
    [ "$status" -eq 2 ]
  done
  phone_run --suite 'MediaTests/testAudio/other'
  [ "$status" -eq 2 ]
  mg ui-test --physical-device "$PHONE"
  [ "$status" -eq 2 ]
  not_called '^(xcrun|lint|xcframework|host-lease|sim-|vm-test|xcodebuild|plutil)'
}

@test "physical ui rejects actual simulator and Mac identifiers without falling back or building" {
  local id
  for id in "$SIM" "$MAC"; do
    mg ui-test --physical-device "$id" --suite MediaTests
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
  done
  not_called '^(lint|xcframework|host-lease|sim-|vm-test|xcodebuild|plutil)'
}

@test "physical ui records unavailable device as queued" {
  jq --arg p "$PHONE" 'map(if .identifier == $p then .available = false else . end)' "$FX_DEVICES" >"$FX_DEVICES.tmp"
  mv "$FX_DEVICES.tmp" "$FX_DEVICES"
  phone_run
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.user_steps[0].id')" = device-developer-mode ]
  echo '[]' >"$FX_DEVICES"
  phone_run
  [ "$status" -eq 0 ]
  [ "$(field .details.queued)" = true ]
  not_called '^(lint|xcframework|host-lease|sim-|vm-test|xcodebuild|plutil)'
}

@test "physical ui rejects cross-family and unknown models before build" {
  phone_run --platform ipad
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  local model
  for model in iPad99,1 Unknown99,1; do
    jq --arg model "$model" '.[0].modelCode = $model' "$FX_DEVICES" >"$FX_DEVICES.tmp"
    mv "$FX_DEVICES.tmp" "$FX_DEVICES"
    phone_run --platform iphone
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
  done
  phone_run --platform ipad
  [ "$status" -eq 2 ]
  not_called '^(lint|xcframework|host-lease|sim-|vm-test|xcodebuild|plutil)'
}

@test "physical ui records the requested iPad axis and device destination" {
  jq '.[0].modelCode = "iPad99,1"' "$FX_DEVICES" >"$FX_DEVICES.tmp"
  mv "$FX_DEVICES.tmp" "$FX_DEVICES"
  phone_run --platform ipad
  [ "$status" -eq 0 ]
  [ "$(field .details.platform)" = ipad ]
  [ "$(field .details.destination_platform)" = ios-device ]
  [ "$(field .details.build.platform)" = ios-device ]
}

@test "physical ui accepts a CoreDevice UUID, refuses old OS, unsafe iphoneos policy and production bundle before tests" {
  jq --arg p "$PHONE" 'map(if .identifier == $p then .operatingSystemVersion = "26.4" else . end)' "$FX_DEVICES" >"$FX_DEVICES.tmp"
  mv "$FX_DEVICES.tmp" "$FX_DEVICES"
  phone_run
  [ "$status" -eq 2 ]
  not_called '^xcodebuild '
  PHONE=00000000-0000-4000-8000-000000000004
  jq --arg p "$PHONE" '.[0].identifier = $p | .[0].operatingSystemVersion = "27.1"' "$FX_DEVICES" >"$FX_DEVICES.tmp"
  mv "$FX_DEVICES.tmp" "$FX_DEVICES"
  FX_POLICY=unsafe phone_run
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = performance-test-policy ]
  not_called '^xcodebuild test-without-building '
  rm -rf "$SB/build/app/DerivedData"
  FX_POLICY=wrong-sdk phone_run
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = performance-test-policy ]
  not_called '^xcodebuild test-without-building '
  FX_BUNDLE=com.antoinefink.meguri phone_run
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = physical-device-bundle ]
  not_called '^xcodebuild test-without-building '
  phone_run
  [ "$status" -eq 0 ]
  called "test-without-building .* -destination platform=iOS,id=$PHONE "
}

@test "physical ui cannot pass zero, failed, skipped or unreadable test results even when xcodebuild exits zero" {
  local summary
  for summary in '{"passedTests":0,"failedTests":0,"skippedTests":0}' \
      '{"passedTests":0,"failedTests":0,"skippedTests":2}' \
      '{"passedTests":2,"failedTests":0,"skippedTests":1}' '{"passedTests":2,"failedTests":1,"skippedTests":0}'; do
    printf '%s\n' "$summary" >"$FX_SUMMARY"
    phone_run
    [ "$status" -eq 1 ]
    [ "$(field .status)" = fail ]
  done
  for summary in '' 'not JSON' '{}' '{"passedTests":"2","failedTests":0,"skippedTests":0}' \
      '{"passedTests":2.5,"failedTests":0,"skippedTests":0}' \
      '{"passedTests":2,"failedTests":0,"skippedTests":0} {"passedTests":2,"failedTests":0,"skippedTests":0}'; do
    printf '%s\n' "$summary" >"$FX_SUMMARY"
    phone_run
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
  done
  echo '{"passedTests":2,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
  FX_SUMMARY_RC=1 phone_run
  [ "$status" -eq 2 ]
  FX_XCODE_RC=65 phone_run
  [ "$status" -eq 1 ]
}

@test "physical ui rejects failed summary and count mismatch" {
  local outcome
  for outcome in Failed Unknown ''; do
    jq -n --arg outcome "$outcome" '{result:$outcome,passedTests:2,failedTests:0,skippedTests:0}' >"$FX_SUMMARY"
    phone_run
    [ "$status" -eq 1 ]
  done
  echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
  phone_run
  [ "$status" -eq 1 ]
}

@test "physical ui requires both requested methods in the test tree" {
  test_tree '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Passed"}]'
  phone_run
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = physical-device-results ]
  test_tree '[
    {nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudioExtra()", result:"Passed"},
    {nodeType:"Test Case", nodeIdentifier:"MediaTests/testVideo()", result:"Passed"}]'
  phone_run
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
}

@test "physical ui accepts exact method identifiers and a class with passed methods" {
  test_tree '[
    {nodeType:"Test Case", nodeIdentifier:"MeguriUITests/MediaTests/testAudio()", result:"Passed"},
    {nodeType:"Test Case", nodeIdentifier:"MediaTests/testVideo", result:"Passed"}]'
  phone_run
  [ "$status" -eq 0 ]
  mg ui-test --physical-device "$PHONE" --suite MediaTests
  [ "$status" -eq 0 ]
  jq 'walk(if type == "object" and has("subTestNodes") then .children = .subTestNodes | del(.subTestNodes) else . end)' \
    "$FX_TESTS" >"$FX_TESTS.tmp"
  mv "$FX_TESTS.tmp" "$FX_TESTS"
  phone_run
  [ "$status" -eq 0 ]
}

@test "physical ui refuses empty, skipped or failed terminal cases" {
  local cases
  for cases in '[]' \
      '[{nodeType:"Test Suite", nodeIdentifier:"MediaTests/testAudio()", result:"Passed"}]' \
      '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Skipped"}]' \
      '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Failed"}]' \
      '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Passed",
        subTestNodes:[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudioExtra()", result:"Passed"}]}]'; do
    test_tree "$cases"
    mg ui-test --physical-device "$PHONE" --suite MediaTests/testAudio
    [ "$status" -eq 1 ]
    [ "$(field .status)" = fail ]
  done
  test_tree '[]'
  mg ui-test --physical-device "$PHONE" --suite MediaTests
  [ "$status" -eq 1 ]
  test_tree '[{nodeType:"Test Case", nodeIdentifier:"MediaTestsExtra/testAudio()", result:"Passed"}]'
  mg ui-test --physical-device "$PHONE" --suite MediaTests
  [ "$status" -eq 1 ]
}

@test "physical ui refuses unreadable and malformed test trees" {
  local tree
  for tree in '' 'not JSON' '[]' '{}' '{"testNodes":{}}' '{"testNodes":[]} {"testNodes":[]}'; do
    printf '%s\n' "$tree" >"$FX_TESTS"
    phone_run
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
  done
  test_tree '[{nodeType:"Test Case", nodeIdentifier:null, result:"Passed"}]'
  phone_run
  [ "$status" -eq 2 ]
  test_tree '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Passed", children:"broken"}]'
  phone_run
  [ "$status" -eq 2 ]
  test_tree '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Passed"}]'
  FX_TESTS_RC=1 phone_run
  [ "$status" -eq 2 ]
}

@test "physical ui extension preserves simulator build and VM test dispatch with only the requested selector" {
  mg ui-test --platform iphone --device "$SIM" --suite MediaTests/testAudio
  [ "$status" -eq 0 ]
  called '^sim-begin$'
  called "^sim-check $SIM$"
  called '^sim-take$'
  called "^sim-prepare $SIM$"
  called '^xcframework .* --configuration Debug$'
  called '^xcodebuild build-for-testing .* -destination generic/platform=iOS Simulator '
  called "^vm-test .* UI iphonesimulator id=$SIM Debug -only-testing:MeguriUITests/MediaTests/testAudio$"
  not_called '^(host-lease|plutil)|^xcrun xcdevice|^xcodebuild test-without-building|allowProvisioningUpdates|--slices|MEGURI_SIGNING=team'
}
