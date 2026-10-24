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
  unset MEGURI_APP_SIGNING
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
[ "$action" != -showBuildSettings ] || { cat "$FX_PERSONAL_SETTINGS"; exit 0; }
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
if [ "$action" = build ]; then
  p="$dd/Build/Products/$config/Meguri.app"
  w="$p/Contents/PlugIns/MeguriWidget.appex"
  mkdir -p "$p/Contents/MacOS" "$w/Contents/MacOS"
  echo "synthetic app" >"$p/Contents/MacOS/Meguri"
  echo "synthetic widget" >"$w/Contents/MacOS/MeguriWidget"
  echo "{\"CFBundleIdentifier\":\"com.antoinefink.meguri.dev\",\"CFBundleExecutable\":\"Meguri\",\"MeguriCardInspectionBuild\":\"debug\",\"LSUIElement\":true}" >"$p/Contents/Info.plist"
  echo "{\"CFBundleIdentifier\":\"com.antoinefink.meguri.dev.widget\",\"CFBundleExecutable\":\"MeguriWidget\"}" >"$w/Contents/Info.plist"
  if [ "$config" = Release ]; then
    jq ".MeguriCardInspectionBuild = \"personalRelease\" | .LSUIElement = false" "$p/Contents/Info.plist" >"$p/Contents/personal.json"
    mv "$p/Contents/personal.json" "$p/Contents/Info.plist"
    cp "$FX_APP_PROFILE" "$p/Contents/embedded.provisionprofile"
    cp "$FX_WIDGET_PROFILE" "$w/Contents/embedded.provisionprofile"
  fi
  [ "${FX_SOURCE_CHANGE:-0}" = 0 ] || echo "synthetic mutation" >>app/TestPlans/Performance.xctestplan
  exit "${FX_BUILD_RC:-0}"
fi
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

@test "app builds keep adhoc defaults without provisioning arguments" {
  local platform mode
  for platform in mac ios-sim; do
    for mode in default adhoc; do
      unset MEGURI_APP_SIGNING
      [ "$mode" != adhoc ] || export MEGURI_APP_SIGNING=adhoc
      : >"$STUB_LOG"
      mg build-app --platform "$platform"
      [ "$status" -eq 0 ]
      [ "$(field .status)" = pass ]
      called '^xcodebuild build-for-testing .* -testPlan UI -configuration Debug '
      not_called 'MEGURI_SIGNING=|allowProvisioning'
    done
  done
}

@test "app team opt-in adds provisioning arguments to host and nested Snapshot builds" {
  export MEGURI_APP_SIGNING=team
  local platform
  for platform in mac ios-sim; do
    : >"$STUB_LOG"
    mg build-app --platform "$platform"
    [ "$status" -eq 0 ]
    called '^xcodebuild build-for-testing .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$'
  done
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'EOF'
mg_job fx-snapshot-build host fx_snapshot_build "fake caller of the shared Snapshot build"
fx_snapshot_build() {
  mg_app_build mac Debug Snapshot Meguri ARCHS=arm64 || return 0
  mg_status pass
}
EOF
  : >"$STUB_LOG"
  mg fx-snapshot-build
  [ "$status" -eq 0 ]
  called '^xcodebuild build-for-testing .* -testPlan Snapshot .* ARCHS=arm64 MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$'
}

@test "invalid app signing is rejected before lint or any build" {
  local mode
  for mode in '' automatic 'adhoc team'; do
    export MEGURI_APP_SIGNING="$mode"
    : >"$STUB_LOG"
    mg build-app --platform mac
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
    [ ! -s "$STUB_LOG" ]
  done
}

# Synthetic signed facts for the normal build path; never consult OS signing,
# provisioning profiles or hardware. Keep the real entitlement predicate in use.
standalone_facts() {
  export FX_APP_ENT="$BATS_TEST_TMPDIR/app-entitlements.json"
  export FX_WIDGET_ENT="$BATS_TEST_TMPDIR/widget-entitlements.json"
  jq -n --arg fixture "$SB/" '{
    "com.apple.security.app-sandbox":true, "com.apple.security.get-task-allow":true,
    "com.apple.security.network.client":true, "com.apple.security.network.server":true,
    "com.apple.security.files.user-selected.read-write":true, "com.apple.security.files.bookmarks.app-scope":true,
    "com.apple.security.temporary-exception.files.absolute-path.read-only":[$fixture],
    "com.apple.security.application-groups":["fixture.dev"], "keychain-access-groups":["FIXTURE.dev"],
    "com.apple.private.security.storage.AppDataContainers":["fixture.dev"]}' >"$FX_APP_ENT"
  echo '{"com.apple.security.app-sandbox":true,"com.apple.security.get-task-allow":true,"com.apple.security.application-groups":["fixture.dev"]}' >"$FX_WIDGET_ENT"
  mkdir -p "$SB/app/Packages/MeguriCore/Binaries"
  printf 'hash=%s\nconfiguration=Debug\nprofile=dev\nfeatures=ankiweb,test-hooks\nslices=mac,sim\n' \
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa >"$SB/app/Packages/MeguriCore/Binaries/.stamp"
  stub codesign 'echo "codesign $*" >>"$STUB_LOG"
case "$1" in
  --verify) exit "${FX_SIGNATURE_RC:-0}" ;;
  -d)
    [ "${FX_ENTITLEMENTS_RC:-0}" -eq 0 ] || exit 1
    case "$*" in *.appex) cat "$FX_WIDGET_ENT" ;; *) cat "$FX_APP_ENT" ;; esac ;;
  *) exit 97 ;;
esac'
  stub plutil 'echo "plutil $*" >>"$STUB_LOG"
[ "$1 $2 $3 $4" = "-convert json -o -" ] || exit 97
case "$5" in -) cat ;; *) cat "$5" ;; esac'
  stub lipo 'echo "lipo $*" >>"$STUB_LOG"; echo "${FX_ARCH:-arm64}"'
}

personal_facts() {
  standalone_facts
  export MEGURI_APP_SIGNING=team
  export FX_PERSONAL_SETTINGS="$BATS_TEST_TMPDIR/personal-settings.json"
  export FX_APP_PROFILE="$BATS_TEST_TMPDIR/app-profile.plist" FX_WIDGET_PROFILE="$BATS_TEST_TMPDIR/widget-profile.plist"
  jq 'del(.["com.apple.security.get-task-allow"], .["com.apple.security.temporary-exception.files.absolute-path.read-only"], .["com.apple.private.security.storage.AppDataContainers"])
    | .["com.apple.developer.team-identifier"] = "TESTTEAM01"
    | .["com.apple.application-identifier"] = "TESTTEAM01.com.antoinefink.meguri.dev"
    | .["com.apple.security.application-groups"] = ["TESTTEAM01.com.antoinefink.meguri.dev"]
    | .["keychain-access-groups"] = ["TESTTEAM01.com.antoinefink.meguri.dev"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/personal-app.json"
  mv "$BATS_TEST_TMPDIR/personal-app.json" "$FX_APP_ENT"
  jq 'del(.["com.apple.security.get-task-allow"])
    | .["com.apple.developer.team-identifier"] = "TESTTEAM01"
    | .["com.apple.application-identifier"] = "TESTTEAM01.com.antoinefink.meguri.dev.widget"
    | .["com.apple.security.application-groups"] = ["TESTTEAM01.com.antoinefink.meguri.dev"]' "$FX_WIDGET_ENT" >"$BATS_TEST_TMPDIR/personal-widget.json"
  mv "$BATS_TEST_TMPDIR/personal-widget.json" "$FX_WIDGET_ENT"
  python3 - "$FX_APP_ENT" "$FX_APP_PROFILE" "$FX_WIDGET_ENT" "$FX_WIDGET_PROFILE" <<'PY'
import datetime, json, plistlib, sys
for source, target in zip(sys.argv[1::2], sys.argv[2::2]):
    with open(source) as stream:
        grants = json.load(stream)
    grants["get-task-allow"] = True
    with open(target, "wb") as stream:
        plistlib.dump({"Entitlements": grants, "TeamIdentifier": ["TESTTEAM01"],
                      "ExpirationDate": datetime.datetime(2099, 1, 1),
                      "ProvisionedDevices": ["00000000-0000-4000-8000-000000000009"]}, stream)
PY
  jq -n '["Meguri", "MeguriWidget"] | map(. as $target | {target: $target, buildSettings: {
    CONFIGURATION:"Release", MEGURI_BUNDLE_ID_SUFFIX:".dev",
    PRODUCT_BUNDLE_IDENTIFIER:("com.antoinefink.meguri.dev" + (if $target == "Meguri" then "" else ".widget" end)),
    MEGURI_CORE_FEATURES:"ankiweb", MEGURI_SWIFT_CONDITIONS:"ANKIWEB_ENABLED", SWIFT_ACTIVE_COMPILATION_CONDITIONS:"ANKIWEB_ENABLED",
    MEGURI_CARD_INSPECTION_BUILD:"personalRelease", MEGURI_MAC_UI_ELEMENT:"NO",
    CODE_SIGN_ENTITLEMENTS:(if $target == "Meguri" then "Meguri/Meguri-macOS.entitlements" else "MeguriWidget/MeguriWidget-macOS.entitlements" end),
    ENABLE_TESTABILITY:"NO", ENABLE_DEBUG_DYLIB:"NO", ENABLE_HARDENED_RUNTIME:"YES"}})' >"$FX_PERSONAL_SETTINGS"
  printf 'hash=%s\nconfiguration=Release\nprofile=release\nfeatures=ankiweb\nslices=mac,sim\n' \
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa >"$SB/app/Packages/MeguriCore/Binaries/.stamp"
  stub security 'echo "security $*" >>"$STUB_LOG"; [ "$1 $2 $3" = "cms -D -i" ] || exit 97; cat "$4"'
  stub system_profiler 'echo "system_profiler $*" >>"$STUB_LOG"; echo "{\"SPHardwareDataType\":[{\"provisioning_UDID\":\"00000000-0000-4000-8000-000000000009\",\"platform_UUID\":\"00000000-0000-4000-8000-000000000010\"}]}"'
}

@test "personal preview rejects incompatible flags configuration platform and signing before work" {
  local platform config
  for platform in mac ios-sim; do
    for config in Debug Profile Release AppStore; do
      [ "$platform/$config" != mac/Release ] || continue
      mg build-app --platform "$platform" --config "$config" --personal-preview
      [ "$status" -eq 2 ]
      [ ! -s "$STUB_LOG" ]
    done
  done
  mg build-app --platform mac --personal-preview --standalone
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
  mg build-app --platform mac --personal-preview
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
  [ ! -d "$SB/build/app" ]
}

@test "personal preview uses Release core normal build dev identities native launch and hashes without changing test defaults" {
  personal_facts
  local args candidate="$SB/build/app/PersonalPreviewDerivedData/Build/Products/Release/Meguri.app"
  for args in implicit explicit; do
    : >"$STUB_LOG"
    if [ "$args" = explicit ]; then
      mg build-app --personal-preview --platform mac --config Release
    else
      mg build-app --personal-preview --platform mac
    fi
    [ "$status" -eq 0 ] || cat "$STUB_LOG"
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
    called '^xcodebuild -showBuildSettings .* -configuration Release .* MEGURI_BUNDLE_ID_SUFFIX=.dev$'
    called '^xcframework .* --configuration Release$'
    [ "$(grep -n '^xcframework ' "$STUB_LOG" | cut -d: -f1)" -lt "$(grep -n '^xcodebuild -showBuildSettings ' "$STUB_LOG" | cut -d: -f1)" ]
    called '^xcodebuild build .* -configuration Release .* -derivedDataPath build/app/PersonalPreviewDerivedData .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration MEGURI_BUNDLE_ID_SUFFIX=.dev$'
    not_called 'build-for-testing|-testPlan|test-without-building|^(sim-|vm-test|host-lease|open|lsregister)|xcrun (simctl|devicectl)'
    [ "$(field .details.build.personal_preview)" = true ]
    [ "$(field .details.build.standalone)" = false ]
    [ "$(field .details.personal_preview_product.provisioning_verified)" = true ]
    [ "$(field .details.personal_preview_product.test_hooks)" = false ]
    [ "$(field .details.personal_preview_product.card_inspection_build)" = personalRelease ]
    [ "$(field .details.personal_preview_product.executable_sha256)" = "$(shasum -a 256 "$candidate/Contents/MacOS/Meguri" | cut -c1-64)" ]
    [ "$(field .details.personal_preview_product.provisioning_sha256.app)" = "$(shasum -a 256 "$FX_APP_PROFILE" | cut -c1-64)" ]
    [[ "$(field .details.input)" =~ ^[0-9a-f]{64}$ ]]
    not_called '^codesign .*--(sign|remove-signature)'
  done
  : >"$STUB_LOG"
  MEGURI_PERSONAL_PREVIEW=1 MEGURI_BUNDLE_ID_SUFFIX=.dev mg build-app --platform mac --config Release
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
  mg build-app --platform mac --config AppStore
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
  mg build-app --platform ios-sim
  [ "$status" -eq 0 ]
  called '^xcodebuild build-for-testing .* -testPlan UI -configuration Debug .* -derivedDataPath build/app/DerivedData '
  not_called 'MEGURI_BUNDLE_ID_SUFFIX=|^codesign |^security |^system_profiler '
}

@test "personal preview checks resolved target identities and no hooks after core and before app builds" {
  personal_facts
  cp "$FX_PERSONAL_SETTINGS" "$BATS_TEST_TMPDIR/normal-settings.json"
  local mutation
  for mutation in \
    '.[1].buildSettings.PRODUCT_BUNDLE_IDENTIFIER = "com.antoinefink.meguri.widget"' \
    '.[0].buildSettings.MEGURI_CORE_FEATURES = "ankiweb,test-hooks"' \
    '.[0].buildSettings.SWIFT_ACTIVE_COMPILATION_CONDITIONS = "DEBUG ANKIWEB_ENABLED"' \
    '.[0].buildSettings.MEGURI_ENTITLEMENTS_HOOKS_SUFFIX = "-Dev"' \
    '.[1].buildSettings.MEGURI_ENTITLEMENTS_SIGNING_SUFFIX = "-AdHoc"' \
    '.[0].buildSettings.MEGURI_CARD_INSPECTION_BUILD = "debug"'; do
    jq "$mutation" "$BATS_TEST_TMPDIR/normal-settings.json" >"$FX_PERSONAL_SETTINGS"
    : >"$STUB_LOG"
    mg build-app --platform mac --personal-preview
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = personal-preview-settings ]
    called '^xcframework .* --configuration Release$'
    not_called '^xcodebuild build '
  done
}

@test "personal preview rejects fixture scope debug grants stale core invalid provisioning and source changes" {
  local fault
  for fault in fixture debug keychain group stamp profile expired device grant source signature arch; do
    personal_facts
    case "$fault" in
      fixture) jq '.["com.apple.security.temporary-exception.files.absolute-path.read-only"] = ["/"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      debug) jq '.["com.apple.security.get-task-allow"] = true' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      keychain) jq '.["keychain-access-groups"] = ["TESTTEAM01.com.antoinefink.meguri"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      group) jq '.["com.apple.security.application-groups"] = ["TESTTEAM01.com.antoinefink.meguri"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      stamp) sed 's/features=ankiweb/features=ankiweb,test-hooks/' "$SB/app/Packages/MeguriCore/Binaries/.stamp" >"$BATS_TEST_TMPDIR/stamp"; mv "$BATS_TEST_TMPDIR/stamp" "$SB/app/Packages/MeguriCore/Binaries/.stamp" ;;
      profile) echo invalid >"$FX_APP_PROFILE" ;;
      expired|device|grant)
        python3 - "$FX_APP_PROFILE" "$fault" <<'PY'
import datetime, plistlib, sys
with open(sys.argv[1], "rb") as stream:
    profile = plistlib.load(stream)
if sys.argv[2] == "expired":
    profile["ExpirationDate"] = datetime.datetime(2000, 1, 1)
elif sys.argv[2] == "device":
    profile["ProvisionedDevices"] = ["00000000-0000-4000-8000-000000000010"]
else:
    profile["Entitlements"]["keychain-access-groups"] = []
with open(sys.argv[1], "wb") as stream:
    plistlib.dump(profile, stream)
PY
        ;;
    esac
    [ ! -f "$BATS_TEST_TMPDIR/fault.json" ] || mv "$BATS_TEST_TMPDIR/fault.json" "$FX_APP_ENT"
    case "$fault" in
      source) FX_SOURCE_CHANGE=1 mg build-app --platform mac --personal-preview ;;
      signature) FX_SIGNATURE_RC=1 mg build-app --platform mac --personal-preview ;;
      arch) FX_ARCH='arm64 x86_64' mg build-app --platform mac --personal-preview ;;
      *) mg build-app --platform mac --personal-preview ;;
    esac
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = personal-preview-product ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
  done
}

@test "standalone app admits only mac Debug and refuses all other pairs before work" {
  local platform config
  for platform in mac ios-sim; do
    for config in Debug Profile Release AppStore; do
      [ "$platform/$config" != mac/Debug ] || continue
      : >"$STUB_LOG"
      mg build-app --platform "$platform" --config "$config" --standalone
      [ "$status" -eq 2 ]
      [ "$(field '.failures[0].id')" = usage ]
      [ ! -s "$STUB_LOG" ]
      [ ! -d "$SB/build/app" ]
    done
  done
  mg build-app --platform ios-device --standalone
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
  MEGURI_APP_SIGNING=invalid mg build-app --platform mac --standalone
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

@test "standalone app uses normal build independent products and checked metadata then defaults remain test builds" {
  standalone_facts
  local mode candidate="$SB/build/app/StandaloneDerivedData/Build/Products/Debug/Meguri.app"
  for mode in adhoc team; do
    export MEGURI_APP_SIGNING="$mode"
    : >"$STUB_LOG"
    mg build-app --platform mac --config Debug --standalone
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
    called '^lint$'
    called '^xcframework .* --configuration Debug$'
    called '^xcodebuild build -project app/Meguri.xcodeproj -scheme Meguri -configuration Debug -destination platform=macOS,arch=arm64 -derivedDataPath build/app/StandaloneDerivedData '
    not_called 'build-for-testing|-testPlan|test-without-building|^(sim-|vm-test|host-lease)|xcrun (simctl|devicectl)'
    if [ "$mode" = team ]; then
      called '^xcodebuild build .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$'
    else
      not_called 'MEGURI_SIGNING=|allowProvisioning'
    fi
    called "^codesign --verify --deep --strict $candidate$"
    called "^codesign --verify --strict $candidate/Contents/PlugIns/MeguriWidget.appex$"
    not_called '^codesign .*--(sign|remove-signature)'
    [ "$(field '.artifacts[] | select(.kind == "app") | .path')" = "$candidate" ]
    [ "$(field .details.build.app)" = "$candidate" ]
    [ "$(field .details.build.action)" = build ]
    [ "$(field .details.build.standalone)" = true ]
    [ "$(field '.details.standalone_product.architecture | .app + "/" + .widget')" = arm64/arm64 ]
    [ "$(field .details.standalone_product.signature_verified)" = true ]
    [ "$(field .details.standalone_product.executable_sha256)" = "$(shasum -a 256 "$candidate/Contents/MacOS/Meguri" | cut -c1-64)" ]
    [ "$(field '.details.standalone_product.normal_entitlements.app["com.apple.security.temporary-exception.files.absolute-path.read-only"][0]')" = "$SB/" ]
    [[ "$(field .details.input)" =~ ^[0-9a-f]{64}$ ]]
    if [ "$mode" = adhoc ]; then [ ! -d "$SB/build/app/DerivedData" ]; fi
    python3 "$SB/tools/tests/validate_result.py" "$result"
    : >"$STUB_LOG"
    mg build-app --platform mac
    [ "$status" -eq 0 ]
    called '^xcodebuild build-for-testing .* -testPlan UI .* -derivedDataPath build/app/DerivedData '
    not_called '^codesign '
    [ "$(field .details.build.action)" = build-for-testing ]
    [ "$(field .details.build.standalone)" = false ]
    [ "$(field '.details.standalone_product // "absent"')" = absent ]
    [ "$(cat "$candidate/Contents/MacOS/Meguri")" = 'synthetic app' ]
  done
}

@test "standalone app propagates lint core and normal build failures without publication" {
  standalone_facts
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'EOF'
mg_lint_ui_first() {
  echo lint >>"$STUB_LOG"
  [ "${FX_STOP:-}" != lint ] || { mg_failure lint-ui "synthetic lint failure"; mg_status fail; return 1; }
}
mg__xcframework() { echo "xcframework $*" >>"$STUB_LOG"; [ "${FX_STOP:-}" != core ]; }
EOF
  local phase
  for phase in lint core build; do
    : >"$STUB_LOG"
    FX_STOP="$phase" FX_BUILD_RC=1 mg build-app --platform mac --standalone
    [ "$status" -eq 1 ]
    [ "$(field .status)" = fail ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
    not_called '^codesign '
    case "$phase" in
      lint) not_called '^xcframework |^xcodebuild '; [ "$(field '.failures[0].id')" = lint-ui ] ;;
      core) not_called '^xcodebuild '; [ "$(field '.failures[0].id')" = xcframework ] ;;
      build) called '^xcodebuild build '; [ "$(field '.failures[0].id')" = build ] ;;
    esac
  done
}

@test "standalone app rejects actual root read extra paths write exceptions invalid shapes and missing signed facts" {
  standalone_facts
  cp "$FX_APP_ENT" "$BATS_TEST_TMPDIR/normal.json"
  local mutation
  for mutation in \
    '.["com.apple.security.temporary-exception.files.absolute-path.read-only"] = ["/"]' \
    '.["com.apple.security.temporary-exception.files.absolute-path.read-only"] += ["/"]' \
    '.["com.apple.security.temporary-exception.files.absolute-path.read-only"] = "/"' \
    '.["com.apple.security.temporary-exception.files.absolute-path.read-write"] = ["/"]' \
    '.["com.apple.security.app-sandbox"] = false'; do
    jq "$mutation" "$BATS_TEST_TMPDIR/normal.json" >"$FX_APP_ENT"
    mg build-app --platform mac --standalone
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = standalone-product ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
  done
  cp "$BATS_TEST_TMPDIR/normal.json" "$FX_APP_ENT"
  local fault
  for fault in signature entitlements arch stamp; do
    case "$fault" in
      signature) FX_SIGNATURE_RC=1 mg build-app --platform mac --standalone ;;
      entitlements) FX_ENTITLEMENTS_RC=1 mg build-app --platform mac --standalone ;;
      arch) FX_ARCH='arm64 x86_64' mg build-app --platform mac --standalone ;;
      stamp) rm "$SB/app/Packages/MeguriCore/Binaries/.stamp"; mg build-app --platform mac --standalone ;;
    esac
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = standalone-product ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
  done
}

@test "app team opt-in keeps explicit physical build signing arguments single" {
  export MEGURI_APP_SIGNING=team
  mg ui-test --physical-device "$PHONE" --suite MediaTests/testAudio
  [ "$status" -eq 0 ]
  called "^xcodebuild build-for-testing .* -destination platform=iOS,id=$PHONE .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$"
  [ "$(grep '^xcodebuild build-for-testing ' "$STUB_LOG" | grep -o 'MEGURI_SIGNING=team' | wc -l | tr -d ' ')" -eq 1 ]
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

@test "explicit simulator UI suites reject zero tests, all skips, failures and unknown outcomes despite exit zero" {
  local summary
  for summary in '{"result":"Passed","passedTests":0,"failedTests":0,"skippedTests":0}' \
      '{"result":"Passed","passedTests":0,"failedTests":0,"skippedTests":2}' \
      '{"result":"Passed","passedTests":2,"failedTests":1,"skippedTests":0}' \
      '{"result":"Failed","passedTests":2,"failedTests":0,"skippedTests":0}' \
      '{"result":"Unknown","passedTests":2,"failedTests":0,"skippedTests":0}' \
      '{"passedTests":2,"failedTests":0,"skippedTests":0}'; do
    printf '%s\n' "$summary" >"$FX_SUMMARY"
    mg ui-test --platform iphone --device "$SIM" --suite MediaTests
    [ "$status" -eq 1 ]
    [ "$(field .status)" = fail ]
    [ "$(field '.failures[0].id')" = ui-test-results ]
  done
}

@test "explicit simulator UI suites require readable nonnegative integer counts" {
  local summary
  for summary in '' 'not JSON' '{}' \
      '{"result":"Passed","passedTests":"2","failedTests":0,"skippedTests":0}' \
      '{"result":"Passed","passedTests":2.5,"failedTests":0,"skippedTests":0}' \
      '{"result":"Passed","passedTests":2,"failedTests":-1,"skippedTests":0}' \
      '{"result":"Passed","passedTests":2,"failedTests":0,"skippedTests":null}' \
      '{"result":"Passed","passedTests":2,"failedTests":0}' \
      '{"result":"Passed","passedTests":2,"failedTests":0,"skippedTests":0} {"result":"Passed","passedTests":2,"failedTests":0,"skippedTests":0}'; do
    printf '%s\n' "$summary" >"$FX_SUMMARY"
    mg ui-test --platform iphone --device "$SIM" --suite MediaTests
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = ui-test-results ]
  done
  echo '{"result":"Passed","passedTests":2,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
  FX_SUMMARY_RC=1 mg ui-test --platform iphone --device "$SIM" --suite MediaTests
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
}

@test "explicit simulator UI suites pass with actual passed tests and allow partial skips" {
  local skipped
  for skipped in 0 1; do
    jq -n --argjson skipped "$skipped" \
      '{result:"Passed",passedTests:2,failedTests:0,skippedTests:$skipped}' >"$FX_SUMMARY"
    mg ui-test --platform iphone --device "$SIM" --suite MediaTests
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
    [ "$(field '.summary | "\(.passed) \(.failed) \(.skipped)"')" = "2 0 $skipped" ]
  done
}
