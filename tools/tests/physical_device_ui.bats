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
  "xcresulttool export attachments "*) export-native-attachment "$@" ;;
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
if [ "${FX_WIDGET_REFRESH_CHECK:-0}" = 1 ] && [ "$dest" = platform=macOS,arch=arm64 ]; then
  products="$dd/Build/Products/$config"
  widget="$products/Meguri.app/Contents/PlugIns/MeguriWidget.appex"
  [ ! -e "$widget" ] && [ ! -L "$widget" ] || exit 96
  [ "$(cat "$products/MeguriWidget.appex/keep")" = source-product ] || exit 96
  [ "$(cat "$products/Meguri.app/Contents/PlugIns/Other.appex/keep")" = other-plugin ] || exit 96
  echo "widget-copy-refreshed $dd $config" >>"$STUB_LOG"
fi
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
    [ "${FX_WIDGET_NO_PROFILE:-0}" = 1 ] || cp "$FX_WIDGET_PROFILE" "$w/Contents/embedded.provisionprofile"
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

@test "Mac embedded widget refresh selects only the current build directory and configuration" {
  local mode dd config platform build_dir build_config products widget selected args=()
  for mode in normal profile standalone personal ios; do
    unset MEGURI_APP_SIGNING
    dd=DerivedData; config=Debug; platform=mac; args=()
    case "$mode" in
      profile) config=Profile ;;
      standalone) dd=StandaloneDerivedData; standalone_facts; args=(--standalone) ;;
      personal) dd=PersonalPreviewDerivedData; config=Release; personal_facts; args=(--personal-preview) ;;
      ios) platform=ios-sim ;;
    esac
    for build_dir in DerivedData StandaloneDerivedData PersonalPreviewDerivedData; do
      for build_config in Debug Profile Release; do
        products="$SB/build/app/$build_dir/Build/Products/$build_config"
        widget="$products/Meguri.app/Contents/PlugIns/MeguriWidget.appex"
        mkdir -p "$widget" "$products/MeguriWidget.appex" "$products/Meguri.app/Contents/PlugIns/Other.appex"
        echo stale-copy >"$widget/stale"
        echo source-product >"$products/MeguriWidget.appex/keep"
        echo other-plugin >"$products/Meguri.app/Contents/PlugIns/Other.appex/keep"
      done
    done
    : >"$STUB_LOG"
    FX_WIDGET_REFRESH_CHECK=1 mg build-app --platform "$platform" --config "$config" "${args[@]}"
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
    selected="$SB/build/app/$dd/Build/Products/$config"
    if [ "$platform" = mac ]; then
      called "^widget-copy-refreshed build/app/$dd $config$"
      [ ! -e "$selected/Meguri.app/Contents/PlugIns/MeguriWidget.appex/stale" ]
    else
      not_called '^widget-copy-refreshed '
    fi
    for build_dir in DerivedData StandaloneDerivedData PersonalPreviewDerivedData; do
      for build_config in Debug Profile Release; do
        products="$SB/build/app/$build_dir/Build/Products/$build_config"
        [ "$(cat "$products/MeguriWidget.appex/keep")" = source-product ]
        [ "$(cat "$products/Meguri.app/Contents/PlugIns/Other.appex/keep")" = other-plugin ]
        if [ "$platform" != mac ] || [ "$products" != "$selected" ]; then
          [ "$(cat "$products/Meguri.app/Contents/PlugIns/MeguriWidget.appex/stale")" = stale-copy ]
        fi
      done
    done
  done
}

@test "Mac embedded widget refresh refuses symlinked generated paths before Xcode" {
  local component products="$SB/build/app/DerivedData/Build/Products/Debug" link target
  for component in derived app plugins widget; do
    rm -rf "$SB/build/app"
    target="$BATS_TEST_TMPDIR/outside-$component"
    mkdir -p "$target"
    echo preserve >"$target/keep"
    case "$component" in
      derived) link="$SB/build/app/DerivedData" ;;
      app) link="$products/Meguri.app" ;;
      plugins) link="$products/Meguri.app/Contents/PlugIns" ;;
      widget) link="$products/Meguri.app/Contents/PlugIns/MeguriWidget.appex" ;;
    esac
    mkdir -p "$(dirname "$link")"
    ln -s "$target" "$link"
    : >"$STUB_LOG"
    mg build-app --platform mac
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = embedded-widget ]
    [ -L "$link" ]
    [ "$(cat "$target/keep")" = preserve ]
    not_called '^xcodebuild '
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
    case "$2" in
      --extract-certificates=*)
        cert="${FX_LEAF_CERT:-synthetic certificate}"
        case "$*" in *.appex) cert="${FX_WIDGET_LEAF_CERT:-$cert}" ;; esac
        printf "%s" "$cert" >"${2#*=}0"; exit 0 ;;
      --verbose=4)
        identity=com.antoinefink.meguri.dev
        case "$*" in *.appex) identity="${FX_WIDGET_SIGNED_ID:-$identity.widget}" ;; esac
        printf "Identifier=%s\nTeamIdentifier=TESTTEAM01\n" "$identity" >&2; exit 0 ;;
    esac
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
    with open(target, "wb") as stream:
        plistlib.dump({"Entitlements": grants, "TeamIdentifier": ["TESTTEAM01"],
                      "Platform": ["OSX"], "DeveloperCertificates": [b"synthetic certificate"],
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
  for fault in fixture debug keychain group stamp profile expired device grant platform certificates profile-debug signer source signature arch; do
    personal_facts
    case "$fault" in
      fixture) jq '.["com.apple.security.temporary-exception.files.absolute-path.read-only"] = ["/"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      debug) jq '.["com.apple.security.get-task-allow"] = true' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      keychain) jq '.["keychain-access-groups"] = ["TESTTEAM01.com.antoinefink.meguri"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      group) jq '.["com.apple.security.application-groups"] = ["TESTTEAM01.com.antoinefink.meguri"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      stamp) sed 's/features=ankiweb/features=ankiweb,test-hooks/' "$SB/app/Packages/MeguriCore/Binaries/.stamp" >"$BATS_TEST_TMPDIR/stamp"; mv "$BATS_TEST_TMPDIR/stamp" "$SB/app/Packages/MeguriCore/Binaries/.stamp" ;;
      profile) echo invalid >"$FX_APP_PROFILE" ;;
      expired|device|grant|platform|certificates|profile-debug)
        python3 - "$FX_APP_PROFILE" "$fault" <<'PY'
import datetime, plistlib, sys
with open(sys.argv[1], "rb") as stream:
    profile = plistlib.load(stream)
if sys.argv[2] == "expired":
    profile["ExpirationDate"] = datetime.datetime(2000, 1, 1)
elif sys.argv[2] == "device":
    profile["ProvisionedDevices"] = ["00000000-0000-4000-8000-000000000010"]
elif sys.argv[2] == "grant":
    profile["Entitlements"]["keychain-access-groups"] = []
elif sys.argv[2] == "platform":
    profile["Platform"] = ["iOS"]
elif sys.argv[2] == "certificates":
    profile["DeveloperCertificates"] = []
else:
    profile["Entitlements"]["get-task-allow"] = False
with open(sys.argv[1], "wb") as stream:
    plistlib.dump(profile, stream)
PY
        ;;
    esac
    [ ! -f "$BATS_TEST_TMPDIR/fault.json" ] || mv "$BATS_TEST_TMPDIR/fault.json" "$FX_APP_ENT"
    case "$fault" in
      source) FX_SOURCE_CHANGE=1 mg build-app --platform mac --personal-preview ;;
      signature) FX_SIGNATURE_RC=1 mg build-app --platform mac --personal-preview ;;
      signer) FX_LEAF_CERT='different synthetic certificate' mg build-app --platform mac --personal-preview ;;
      arch) FX_ARCH='arm64 x86_64' mg build-app --platform mac --personal-preview ;;
      *) mg build-app --platform mac --personal-preview ;;
    esac
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = personal-preview-product ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
  done
}

@test "personal preview accepts native Mac widget without profile only with exact dev signed identity and authorized app signer" {
  personal_facts
  echo '{"com.apple.security.app-sandbox":true,"com.apple.security.application-groups":["TESTTEAM01.com.antoinefink.meguri.dev"]}' >"$FX_WIDGET_ENT"
  FX_WIDGET_NO_PROFILE=1 mg build-app --platform mac --personal-preview
  [ "$status" -eq 0 ]
  [ "$(field .details.personal_preview_product.provisioning.app)" = verified ]
  [ "$(field .details.personal_preview_product.provisioning.widget)" = not_embedded ]
  [ "$(field .details.personal_preview_product.provisioning_sha256.widget)" = null ]
  [ "$(field .details.personal_preview_product.widget_signer_matches_app)" = true ]
  FX_WIDGET_NO_PROFILE=1 FX_WIDGET_LEAF_CERT='other signer' mg build-app --platform mac --personal-preview
  [ "$status" -eq 1 ]
  FX_WIDGET_NO_PROFILE=1 FX_WIDGET_SIGNED_ID=com.antoinefink.meguri.widget mg build-app --platform mac --personal-preview
  [ "$status" -eq 1 ]
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

# Native media destination capture extends the same ui-test dispatcher. These
# synthetic files stand in for already-generated fixtures and a native save;
# no app, VM, database, media generator or package producer runs here.
native_media_fakes() {
  mkdir -p "$SB/build/fixtures/recipes/typical-1/collection.media"
  echo generated-database >"$SB/build/fixtures/recipes/typical-1/collection.anki2"
  echo '{"recipe":"typical","seed":1}' >"$SB/build/fixtures/recipes/typical-1/manifest.json"
  echo generated-image >"$SB/build/fixtures/recipes/typical-1/collection.media/image.png"
  echo generated-sound >"$SB/build/fixtures/recipes/typical-1/collection.media/sound.wav"
  echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
  test_tree '[{nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Passed"}]'
  export FX_NATIVE_DOCS="$BATS_TEST_TMPDIR/native-documents"
  mkdir -p "$FX_NATIVE_DOCS"
  echo stale-old-package >"$FX_NATIVE_DOCS/Meguri-Collection.colpkg"
  export FX_CONTAINER="/Users/meguri/Library/Developer/CoreSimulator/Devices/$SIM/data/Containers/Data/Application/00000000-0000-4000-8000-000000000004"
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'EOF'
mg_app_build() { echo "media-build $*" >>"$STUB_LOG"; }
mg_mac_ui_begin() { MG_MAC_RUNNER="${FX_RUNNER:-vm}"; }
mg_mac_ui_take() { echo mac-take >>"$STUB_LOG"; }
mg_sim_take() { MG_SIM_RUNNER="${FX_RUNNER:-vm}"; echo sim-take >>"$STUB_LOG"; }
mg_app_log_now() { echo synthetic-time; }
mg_app_mac_input_refused() { return 1; }
mg_compose() {
  echo "media-decoder $*" >>"$STUB_LOG"
  [ "${FX_PREP_FAIL:-0}" = 0 ] || return 1
  case "$*" in
    '--progress plain build package-interop-decoder') return 0 ;;
    'run '*) fake-native-baseline "$MG_ROOT" "$MG_RUN_DIR" "$MG_RUN_ID" "$@" ;;
    *) return 97 ;;
  esac
}
mg_vm_fixture() {
  echo "media-stage $*" >>"$STUB_LOG"
  if [ "${FX_STAGE_QUEUED:-0}" = 1 ]; then
    mg_queued_part mac-vm native-media-export "synthetic unavailable VM"
    mg_status warn
    return 2
  fi
}
mg_mac_vm_copy_in() { echo "media-copy-in $*" >>"$STUB_LOG"; }
mg_app_xcodebuild_test() {
  echo "media-test $*" >>"$STUB_LOG"
  echo "media-handoff ${TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT:-unset}" >>"$STUB_LOG"
  echo "media-name ${TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME:-unset}" >>"$STUB_LOG"
  echo "media-fixture-record ${TEST_RUNNER_MEGURI_NATIVE_MEDIA_FIXTURE_RECORD:-unset}" >>"$STUB_LOG"
  mkdir -p "$1.xcresult"
  if [ -n "${TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME:-}" ] && [ "${FX_MISSING:-0}" = 0 ]; then
    local filename="$TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME"
    [[ "$filename" =~ ^Meguri-Media-[a-f0-9]{32}\.colpkg$ ]] || return 98
    [ "${FX_MEDIA_ORDER:-before}" != before ] || echo old-method-overwrite >"$FX_NATIVE_DOCS/Meguri-Collection.colpkg"
    local saved="$FX_NATIVE_DOCS/$filename"
    if [ "$2" != mac ]; then
      local stem="${filename%.colpkg}"
      [ ! -e "$FX_NATIVE_DOCS/$stem" ] && [ ! -L "$FX_NATIVE_DOCS/$stem" ] || return 99
      mkdir "$FX_NATIVE_DOCS/$stem" || return 99
      echo "media-new-folder $stem" >>"$STUB_LOG"
      saved="$FX_NATIVE_DOCS/$stem/Meguri-Collection.colpkg"
      echo "media-touch-save $stem/Meguri-Collection.colpkg" >>"$STUB_LOG"
    fi
    [ ! -e "$saved" ] || return 99
    echo "media-first-save $filename" >>"$STUB_LOG"
    echo first-save >"$saved"
    echo "media-replace $filename" >>"$STUB_LOG"
    [ "$2" = mac ] || echo "media-touch-replace $stem/Meguri-Collection.colpkg" >>"$STUB_LOG"
    echo native-saved-package >"$saved"
    [ "${FX_MEDIA_ORDER:-before}" != after ] || echo old-method-overwrite >"$FX_NATIVE_DOCS/Meguri-Collection.colpkg"
    if [ "$2" = mac ]; then
      cp "$saved" "$1.xcresult/native-attachment.colpkg"
      echo "media-mac-attachment $filename" >>"$STUB_LOG"
    fi
  fi
  [ "${FX_SOURCE_CHANGED:-0}" = 0 ] || echo changed >>"$MG_ROOT/build/fixtures/recipes/typical-1/collection.anki2"
  if [ "${FX_PREPARED_CHANGED:-0}" != 0 ]; then
    chmod u+w "$MG_RUN_DIR/native-media-baseline/fixture/collection.anki2"
    echo changed >>"$MG_RUN_DIR/native-media-baseline/fixture/collection.anki2"
  fi
  if [ "${FX_RECORD_CHANGED:-0}" != 0 ]; then
    chmod u+w "$MG_RUN_DIR/expected-source.json"
    echo '{}' >"$MG_RUN_DIR/expected-source.json"
  fi
  return "${FX_RESULT:-0}"
}
mg_simctl() { echo "media-simctl $*" >>"$STUB_LOG"; printf '%s\n' "$FX_CONTAINER"; }
mg_mac_vm_ssh() {
  echo "media-ssh $*" >>"$STUB_LOG"
  [ "${FX_CAPTURE_QUEUED:-0}" = 0 ] || return 255
  [ "${FX_MISSING:-0}" = 0 ] || return 1
  case "$1" in *"$(mvl_q "$TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME")") ;; *) return 97 ;; esac
  local stem="${TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME%.colpkg}"
  echo "media-capture-source $stem/Meguri-Collection.colpkg" >>"$STUB_LOG"
  cp "$FX_NATIVE_DOCS/$stem/Meguri-Collection.colpkg" \
    "$TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT/$TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME"
}
mg_sim_fetch() { echo "media-fetch $*" >>"$STUB_LOG"; }
mg_mac_vm_copy_out() { echo "media-copy-out $*" >>"$STUB_LOG"; }
EOF
  # Runner-only stand-in: SQL preparation is exercised with real databases by
  # NativeClock in package_interop_check_test.py inside the pinned dev image.
  cat >"$BIN/fake-native-baseline" <<'EOF'
#!/usr/bin/env python3
import importlib.util
import json
import shutil
import sys
from pathlib import Path
root, run = map(Path, sys.argv[1:3])
run_id = sys.argv[3]
args = sys.argv[4:]
assert args[:6] == ['run', '--rm', '-T', '--no-deps', '--label', 'meguri.mg.run-id=' + run_id]
service = args.index('package-interop-decoder')
assert args[service + 1:] == ['python3', '/decoder.py', 'prepare-native-media', '--expected', '/in', '--output', '/out']
mounts = [args[i + 1] for i, value in enumerate(args[:-1]) if value == '-v']
assert mounts == [str(root / 'tools/scripts/package-interop-check.py') + ':/decoder.py:ro',
                  str(run / 'native-media-parent') + ':/in:ro',
                  str(run / 'native-media-baseline') + ':/out']
spec = importlib.util.spec_from_file_location('capture', root / 'tools/scripts/native-media-export-capture.py')
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
parent = capture.fixture_files(run / 'native-media-parent')
fixture = run / 'native-media-baseline/fixture'
assert list(fixture.parent.iterdir()) == []
shutil.copytree(run / 'native-media-parent', fixture)
with (fixture / 'collection.anki2').open('ab') as stream:
    stream.write(b'fake-runner-only-clock-row')
record = {'parent_files': parent, 'prepared_files': capture.fixture_files(fixture),
          'clock': capture.CLOCK, 'inserted_row': {'KEY': 'localOffset', 'usn': -1,
          'mtime_secs': 1894676400, 'val_hex': '2d3630', 'storage_class': 'blob'}}
(fixture.parent / 'preparation.json').write_text(json.dumps(record))
EOF
  chmod +x "$BIN/fake-native-baseline"
  cat >"$BIN/export-native-attachment" <<'EOF'
#!/usr/bin/env python3
import json
import os
import shutil
import sys
from pathlib import Path

args = sys.argv[1:]
assert len(args) == 11 and args[:4] == ['xcresulttool', 'export', 'attachments', '--path']
bundle = Path(args[4])
run = bundle.parent
filename = json.loads((run / 'expected-source.json').read_text())['native_filename']
stem = Path(filename).stem
method = 'CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()'
exported = run / 'native-media-attachments'
assert bundle == run / 'mac.xcresult'
assert args[5:] == ['--output-path', str(exported), '--test-id', method, '--filter', '*.colpkg']
assert exported.is_dir() and not list(exported.iterdir())
mode = os.environ.get('FX_ATTACHMENT_MODE', 'success')
if mode == 'export-error':
    print('synthetic subprocess output must stay private', file=sys.stderr)
    sys.exit(1)
name = '00000000-0000-4000-8000-000000000007.colpkg'
human_suffix = '_0_00000000-0000-4000-8000-000000000008.colpkg'
payload = exported / name
source = bundle / 'native-attachment.colpkg'
if source.exists() and mode != 'missing-package':
    shutil.copyfile(source, payload)
attachment = {'exportedFileName': name,
              'suggestedHumanReadableName': stem + human_suffix,
              'isAssociatedWithFailure': False, 'configurationName': 'Synthetic',
              'deviceName': 'Synthetic', 'deviceId': 'synthetic', 'timestamp': 1}
manifest = [{'testIdentifier': method, 'attachments': [attachment]}]
if mode == 'wrong-test':
    manifest[0]['testIdentifier'] = 'MediaTests/testAudio()'
elif mode == 'wrong-name':
    attachment['suggestedHumanReadableName'] = 'prefix-' + stem + human_suffix
elif mode == 'wrong-delimiter':
    attachment['suggestedHumanReadableName'] = stem + 'suffix' + human_suffix
elif mode == 'wrong-token':
    attachment['suggestedHumanReadableName'] = 'Meguri-Media-' + 'f' * 32 + human_suffix
elif mode == 'failure-marker':
    attachment['isAssociatedWithFailure'] = True
elif mode == 'missing-marker':
    del attachment['isAssociatedWithFailure']
elif mode == 'duplicate-attachment':
    manifest[0]['attachments'].append(dict(attachment))
elif mode == 'extra-record':
    manifest.append({'testIdentifier': 'MediaTests/testAudio()', 'attachments': []})
elif mode == 'extra-export':
    (exported / 'unrelated-screen.png').write_bytes(b'unrelated-screen')
elif mode in ('escape', 'absolute', 'wrong-extension'):
    attachment['exportedFileName'] = {'escape': '../escape.colpkg', 'absolute': '/tmp/escape.colpkg',
                                      'wrong-extension': name + '.png'}[mode]
elif mode in ('linked-payload', 'hardlink-payload', 'fifo', 'directory', 'empty'):
    payload.unlink()
    if mode == 'linked-payload':
        payload.symlink_to(source)
    elif mode == 'hardlink-payload':
        os.link(source, payload)
    elif mode == 'fifo':
        os.mkfifo(payload)
    elif mode == 'directory':
        payload.mkdir()
    else:
        payload.touch()
elif mode == 'malformed-manifest':
    manifest = {'unexpected': []}
manifest_path = exported / 'manifest.json'
manifest_path.write_text(json.dumps(manifest))
if mode == 'linked-manifest':
    manifest_path.unlink()
    manifest_path.symlink_to(source)
EOF
  chmod +x "$BIN/export-native-attachment"
}

media_run() {
  mg ui-test --device "$SIM" --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace "$@"
}

@test "native media export gates exact selector or whole class and stages before execution" {
  native_media_fakes
  local selector stage_line test_line
  for selector in CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace CollectionEntryJourneyTests; do
    : >"$STUB_LOG"
    mg ui-test --device "$SIM" --suite "$selector"
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
    called "^media-stage $SB/build/mg/ui-test/[^ /]+/native-media-baseline/fixture$"
    called "^media-copy-in $SB/tools/scripts/native-media-export-capture.py $SB/build/mg/ui-test/[^ /]+/native-media-export$"
    stage_line="$(grep -n '^media-stage ' "$STUB_LOG" | cut -d: -f1)"
    test_line="$(grep -n '^media-test ' "$STUB_LOG" | cut -d: -f1)"
    [ "$stage_line" -lt "$test_line" ]
    called '^media-simctl get_app_container '
    called "^media-fetch $SB/build/mg/ui-test/[^ /]+/native-media-export/Meguri-Media-[a-f0-9]{32}\.colpkg$"
    [ "$(field .details.native_media_export.external_content_client_proof)" = pending ]
    [ "$(field .details.native_media_export.e7_4_acceptance)" = pending ]
    [ "$(field .details.native_media_export.native_destination.bytes)" -gt 0 ]
    [[ "$(field .details.native_media_export.native_destination.sha256)" =~ ^[a-f0-9]{64}$ ]]
    [ -f "$(dirname "$result")/expected-source.json" ]
    jq -e '. as $record | (.fixture | test("^build/mg/ui-test/[^/]+/native-media-baseline/fixture$"))
      and .parent.fixture == "build/fixtures/recipes/typical-1" and .manifest_provenance == "parent"
      and (.files | has("manifest.json") and has("collection.anki2") and has("collection.media/image.png") and has("collection.media/sound.wav"))
      and .files["manifest.json"] == .parent.files["manifest.json"]
      and .files["collection.anki2"] != .parent.files["collection.anki2"]
      and .files["collection.media/image.png"] == .parent.files["collection.media/image.png"]
      and .inserted_row == {KEY:"localOffset",usn:-1,mtime_secs:1894676400,val_hex:"2d3630",storage_class:"blob"}' \
      "$(dirname "$result")/expected-source.json"
  done
  for selector in CollectionEntryJourneyTests/testNativeExportCancelSaveReplaceReimportAndDurableReview CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplaceExtra MediaTests; do
    : >"$STUB_LOG"
    TEST_RUNNER_MEGURI_NATIVE_MEDIA_FIXTURE_RECORD=/untrusted/record TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT=/untrusted/output TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME=../untrusted.colpkg mg ui-test --device "$SIM" --suite "$selector"
    [ "$status" -eq 0 ]
    not_called '^media-(stage|copy|simctl|fetch|ssh)'
    called '^media-handoff unset$'
    called '^media-name unset$'
    called '^media-fixture-record unset$'
    [ "$(field .details.native_media_export)" = null ]
  done
}

@test "native media baseline binds frozen expected handoff Docker mounts and cancellation label" {
  native_media_fakes
  TEST_RUNNER_MEGURI_NATIVE_MEDIA_FIXTURE_RECORD=/untrusted/ambient media_run
  [ "$status" -eq 0 ]
  local dir="$(dirname "$result")" expected
  expected="$SB/$(jq -r .fixture "$dir/expected-source.json")"
  [ "$expected" = "$dir/native-media-baseline/fixture" ]
  called "^media-fixture-record $dir/expected-source.json$"
  called "^media-copy-in $dir/expected-source.json$"
  called "^media-stage $expected$"
  called "^media-decoder run --rm -T --no-deps --label meguri.mg.run-id=$(basename "$dir") "
  jq -e --slurpfile source "$dir/expected-source.json" \
    '.fixture_identity == ($source[0] | del(.native_filename)) and .fixture == $source[0].fixture' \
    "$dir/native-media-export.json"
  run python3 - "$SB/tools/scripts/native-media-export-capture.py" "$dir" <<'PY_FREEZE'
import importlib.util
import json
import sys
from pathlib import Path
spec = importlib.util.spec_from_file_location('capture', sys.argv[1])
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
run = Path(sys.argv[2])
record = json.loads((run / 'expected-source.json').read_text())
assert record == dict(capture.fixture_identity(run), native_filename=record['native_filename'])
# The unchanged installer copies file modes into its writable collection.
assert (run / 'native-media-baseline/fixture/collection.anki2').stat().st_mode & 0o200
PY_FREEZE
  [ "$status" -eq 0 ]
  : >"$STUB_LOG"
  FX_PREP_FAIL=1 media_run
  [ "$status" -eq 2 ]
  not_called '^media-(stage|test|copy-in|simctl|ssh|fetch)'
  [ ! -e "$(dirname "$result")/expected-source.json" ]
}

@test "native media baseline rejects prepared fixture or bound record tamper after launch" {
  native_media_fakes
  FX_PREPARED_CHANGED=1 media_run
  [ "$status" -eq 2 ]
  [ "$(field .details.native_media_capture_failure.phase)" = receipt ]
  [ "$(field .details.native_media_export)" = null ]
  [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 0 ]
  FX_RECORD_CHANGED=1 media_run
  [ "$status" -eq 2 ]
  [ "$(field .details.native_media_capture_failure.phase)" = receipt ]
  [ "$(field .details.native_media_export)" = null ]
  [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 0 ]
}

@test "native media export captures only own run package on Mac and iPad" {
  native_media_fakes
  local platform dir filename
  for platform in mac ipad; do
    : >"$STUB_LOG"
    if [ "$platform" = mac ]; then
      mg ui-test --platform mac --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace
      called '^xcrun xcresulttool export attachments --path .*/mac.xcresult --output-path .*/native-media-attachments --test-id CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace\(\) --filter \*\.colpkg$'
      called "^media-copy-in $(dirname "$result")/expected-source.json$"
      [ "$(grep -c '^media-copy-in ' "$STUB_LOG")" -eq 1 ]
      not_called '^media-(copy-out|simctl|fetch|ssh)'
    else
      media_run --platform ipad
      called '^media-fetch .*/native-media-export/Meguri-Media-[a-f0-9]{32}\.colpkg$'
      not_called '^media-copy-out '
    fi
    [ "$status" -eq 0 ]
    dir="$(dirname "$result")"
    filename="$(field .details.native_media_export.native_filename)"
    [[ "$filename" =~ ^Meguri-Media-[a-f0-9]{32}\.colpkg$ ]]
    [ "$(cat "$dir/native-media-export/$filename")" = native-saved-package ]
    if [ "$platform" = mac ]; then
      called "^media-mac-attachment $filename$"
      [ "$(ls -A "$dir/native-media-attachments")" = $'00000000-0000-4000-8000-000000000007.colpkg\nmanifest.json' ]
      local save_line capture_line
      save_line="$(grep -n '^media-mac-attachment ' "$STUB_LOG" | cut -d: -f1)"
      capture_line="$(grep -n '^xcrun xcresulttool export attachments ' "$STUB_LOG" | cut -d: -f1)"
      [ "$save_line" -lt "$capture_line" ]
    else
      called "^media-new-folder ${filename%.colpkg}$"
      called "^media-touch-save ${filename%.colpkg}/Meguri-Collection\.colpkg$"
      called "^media-touch-replace ${filename%.colpkg}/Meguri-Collection\.colpkg$"
      called "^media-capture-source ${filename%.colpkg}/Meguri-Collection\.colpkg$"
      [ "$(cat "$FX_NATIVE_DOCS/${filename%.colpkg}/Meguri-Collection.colpkg")" = native-saved-package ]
      [ "$(cat "$FX_NATIVE_DOCS/Meguri-Collection.colpkg")" = old-method-overwrite ]
    fi
    [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 3 ]
    not_called 'Library/Application Support|Preferences|credentials|host-lease|xcrun simctl'
  done
}

@test "native media export never captures failed skipped cancelled or unexecuted cases" {
  native_media_fakes
  local mode
  for mode in failed skipped cancelled no-tests wrong-case; do
    : >"$STUB_LOG"
    echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
    case "$mode" in
      failed) FX_RESULT=1 media_run ;;
      skipped)
        echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":1}' >"$FX_SUMMARY"
        media_run ;;
      cancelled) FX_RESULT=2 media_run ;;
      no-tests)
        echo '{"result":"Passed","passedTests":0,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
        media_run ;;
      wrong-case)
        test_tree '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Passed"}]'
        media_run ;;
    esac
    [ "$status" -ne 0 ]
    not_called '^media-(simctl|fetch|copy-out|ssh)'
    [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 0 ]
  done
}

@test "native media export validates simulator locations before collection" {
  native_media_fakes
  local path valid="$FX_CONTAINER"
  for path in / /tmp/other "$valid/../other" "$valid/" "$valid"$'\n/tmp/other' \
      "${valid/$SIM/00000000-0000-4000-8000-000000000099}" '/Users/$(touch forbidden)/Library/Developer/CoreSimulator/Devices/x'; do
    : >"$STUB_LOG"
    FX_CONTAINER="$path" media_run
    [ "$status" -ne 0 ]
    not_called '^media-(ssh|fetch|copy-out)'
    [ "$(field .details.native_media_export)" = null ]
  done
}

@test "native media export requires package original source identity and VM capture" {
  native_media_fakes
  local platform
  for platform in mac iphone; do
    : >"$STUB_LOG"
    if [ "$platform" = mac ]; then
      FX_MISSING=1 mg ui-test --platform mac --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace
    else
      FX_MISSING=1 media_run
    fi
    [ "$status" -eq 2 ]
    [ "$(field .details.native_media_export)" = null ]
    [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 0 ]
  done
  : >"$STUB_LOG"
  FX_SOURCE_CHANGED=1 media_run
  [ "$status" -eq 2 ]
  [ "$(field .details.native_media_export)" = null ]
  : >"$STUB_LOG"
  FX_SOURCE_CHANGED=1 mg ui-test --platform mac --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace
  [ "$status" -eq 2 ]
  [ "$(field .details.native_media_capture_failure.phase)" = receipt ]
  [ "$(field .details.native_media_export)" = null ]
  [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 0 ]
  : >"$STUB_LOG"
  FX_RUNNER=host media_run
  [ "$status" -eq 2 ]
  not_called '^media-(stage|test|simctl|ssh|fetch)'
  : >"$STUB_LOG"
  mg ui-test --physical-device "$PHONE" --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace
  [ "$status" -eq 2 ]
  not_called '^media-|^xcrun xcdevice|^host-lease'
}

@test "native media export leaves unavailable VM staging and capture queued" {
  native_media_fakes
  FX_STAGE_QUEUED=1 media_run
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  not_called '^media-test '
  : >"$STUB_LOG"
  FX_CAPTURE_QUEUED=1 media_run
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  not_called '^media-fetch '
  [ "$(field .details.native_media_export)" = null ]
}

@test "native media export source guard rejects open fixtures links and foreign run paths" {
  native_media_fakes
  local recipe="$SB/build/fixtures/recipes/typical-1" outside="$BATS_TEST_TMPDIR/foreign"
  echo pending >"$recipe/collection.anki2-wal"
  media_run
  [ "$status" -eq 2 ]
  not_called '^media-stage |^media-test '
  rm "$recipe/collection.anki2-wal"
  mv "$recipe/collection.media/image.png" "$BATS_TEST_TMPDIR/image.png"
  ln -s "$BATS_TEST_TMPDIR/image.png" "$recipe/collection.media/image.png"
  media_run
  [ "$status" -eq 2 ]
  not_called '^media-stage |^media-test '
  mkdir -p "$outside"
  run python3 "$SB/tools/scripts/native-media-export-capture.py" identity "$outside"
  [ "$status" -ne 0 ]
  [ ! -e "$outside/native-media-export" ]
  local own="$SB/build/mg/ui-test/source-guard"
  mkdir -p "$own"
  ln -s "$outside" "$own/native-media-export"
  run python3 "$SB/tools/scripts/native-media-export-capture.py" identity "$own"
  [ "$status" -ne 0 ]
  [ ! -e "$outside/expected-source.json" ]
}

@test "native media export names remain fresh across runs and either whole-class method order" {
  native_media_fakes
  local platform order filename previous=""
  for platform in mac iphone; do
  for order in before after before after; do
    : >"$STUB_LOG"
    if [ "$order" = before ]; then
      test_tree '[
        {nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeExportCancelSaveReplaceReimportAndDurableReview()", result:"Passed"},
        {nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Passed"}]'
    else
      test_tree '[
        {nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Passed"},
        {nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeExportCancelSaveReplaceReimportAndDurableReview()", result:"Passed"}]'
    fi
    echo '{"result":"Passed","passedTests":2,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
    if [ "$platform" = mac ]; then
      FX_MEDIA_ORDER="$order" TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME=../ignored.colpkg \
        mg ui-test --platform mac --suite CollectionEntryJourneyTests
    else
      FX_MEDIA_ORDER="$order" TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME=../ignored.colpkg \
        mg ui-test --device "$SIM" --suite CollectionEntryJourneyTests
    fi
    [ "$status" -eq 0 ]
    filename="$(field .details.native_media_export.native_filename)"
    [[ "$filename" =~ ^Meguri-Media-[a-f0-9]{32}\.colpkg$ ]]
    [ "$filename" != "$previous" ]
    called "^media-name $filename$"
    called "^media-first-save $filename$"
    called "^media-replace $filename$"
    if [ "$platform" = mac ]; then
      called "^media-mac-attachment $filename$"
      not_called '^media-(ssh|copy-out|simctl|fetch)'
    else
      called "^media-fetch .*/native-media-export/$filename$"
      [ "$(grep -c "^media-new-folder ${filename%.colpkg}$" "$STUB_LOG")" -eq 1 ]
      called "^media-touch-save ${filename%.colpkg}/Meguri-Collection\.colpkg$"
      called "^media-touch-replace ${filename%.colpkg}/Meguri-Collection\.colpkg$"
      called "^media-capture-source ${filename%.colpkg}/Meguri-Collection\.colpkg$"
      [ "$(cat "$FX_NATIVE_DOCS/${filename%.colpkg}/Meguri-Collection.colpkg")" = native-saved-package ]
    fi
    [ "$(jq -r .native_filename "$(dirname "$result")/expected-source.json")" = "$filename" ]
    [ "$(cat "$(dirname "$result")/native-media-export/$filename")" = native-saved-package ]
    [ "$(cat "$FX_NATIVE_DOCS/Meguri-Collection.colpkg")" = old-method-overwrite ]
    previous="$filename"
  done
  done
}

@test "native media capture pins descriptors and rejects deterministic substitution and mutation" {
  run python3 - "$SB/tools/scripts/native-media-export-capture.py" "$BATS_TEST_TMPDIR/capture" <<'PY'
import importlib.util
import os
import sys
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('capture', sys.argv[1])
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
base = Path(sys.argv[2])
filename = 'Meguri-Media-' + '1' * 32 + '.colpkg'
device = '00000000-0000-4000-8000-000000000003'
container = ('/Users/meguri/Library/Developer/CoreSimulator/Devices/' + device
             + '/data/Containers/Data/Application/00000000-0000-4000-8000-000000000004')
assert capture.simulator_source(container, device, filename) == (
    Path(container) / 'Documents/RF2 Import Tests' / Path(filename).stem / 'Meguri-Collection.colpkg')
original = b'native-A'
foreign = b'foreignB'

for invalid in ('Meguri-Collection.colpkg', '../' + filename, filename + '\n',
                filename.replace('1', 'G'), '/tmp/' + filename):
    try:
        capture.package_name(invalid)
    except ValueError:
        pass
    else:
        raise AssertionError('unsafe filename accepted')
    try:
        capture.simulator_source(container, device, invalid)
    except ValueError:
        pass
    else:
        raise AssertionError('unsafe nonce child accepted')

for case in ('success', 'occupied', 'file-replacement', 'file-symlink',
             'directory-replacement', 'directory-symlink', 'same-length-mutation',
             'destination-mutation', 'initial-symlink', 'initial-directory-symlink', 'hardlink', 'fifo'):
    root = base / case
    source_dir = root / 'Documents/RF2 Import Tests' / Path(filename).stem
    output_dir = root / 'own-run/native-media-export'
    source_dir.mkdir(parents=True)
    output_dir.mkdir(parents=True)
    source = source_dir / 'Meguri-Collection.colpkg'
    output = output_dir / filename
    escaped = root / 'escaped.colpkg'
    escaped.write_bytes(foreign)
    source.write_bytes(original)
    before = source.stat()
    if case == 'occupied':
        output.write_bytes(foreign)
    elif case == 'initial-symlink':
        source.unlink()
        source.symlink_to(escaped)
    elif case == 'initial-directory-symlink':
        original_dir = source_dir.with_name('original-folder')
        source_dir.rename(original_dir)
        source_dir.symlink_to(original_dir, target_is_directory=True)
    elif case == 'hardlink':
        os.link(source, root / 'extra-link')
    elif case == 'fifo':
        source.unlink()
        os.mkfifo(source)
    fired = False
    source_reads = []
    read = os.read

    def intercept(fd, count):
        global fired
        metadata = os.fstat(fd)
        is_source = (metadata.st_dev, metadata.st_ino) == (before.st_dev, before.st_ino)
        if not fired and ((is_source and case != 'destination-mutation')
                          or (not is_source and case == 'destination-mutation')):
            fired = True
            if case in ('file-replacement', 'file-symlink'):
                source.rename(source_dir / 'retained.colpkg')
                if case == 'file-symlink':
                    source.symlink_to(escaped)
                else:
                    source.write_bytes(foreign)
            elif case in ('directory-replacement', 'directory-symlink'):
                source_dir.rename(source_dir.with_name('retained-folder'))
                if case == 'directory-symlink':
                    substitute = root / 'substitute'
                    substitute.mkdir()
                    (substitute / source.name).write_bytes(foreign)
                    source_dir.symlink_to(substitute, target_is_directory=True)
                else:
                    source_dir.mkdir()
                    source.write_bytes(foreign)
            elif case == 'same-length-mutation':
                source.write_bytes(foreign)
                # Explicitly different metadata; no clock resolution or sleeps.
                os.utime(source, ns=(before.st_atime_ns, before.st_mtime_ns + 1_000_000_000))
            elif case == 'destination-mutation':
                output.write_bytes(foreign)
        block = read(fd, count)
        if is_source:
            source_reads.append(block)
        return block

    try:
        with patch.object(capture.os, 'read', side_effect=intercept):
            capture.copy_package(source, output)
    except (ValueError, OSError):
        assert case != 'success', case
        if case == 'occupied':
            assert output.read_bytes() == foreign
        else:
            assert not output.exists(), case
    else:
        assert case == 'success', case
        assert output.read_bytes() == original
        assert capture.file_identity(output)['bytes'] == len(original)
    if case.startswith(('file-', 'directory-')):
        assert fired and b''.join(source_reads) == original, case
    if case in ('same-length-mutation', 'destination-mutation'):
        assert fired, case
    assert not (root / 'own-run/native-media-export.json').exists()
PY
  [ "$status" -eq 0 ]
}

@test "native media export Mac refuses absent failed skipped duplicate or unrelated methods before export" {
  native_media_fakes
  local mode
  for mode in failed-run skipped-summary cancelled no-tests wrong-case failed-case skipped-case duplicate missing-tree; do
    : >"$STUB_LOG"
    echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
    test_tree '[{nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Passed"}]'
    case "$mode" in
      failed-run) FX_RESULT=1 mg ui-test --platform mac --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace ;;
      cancelled) FX_RESULT=2 mg ui-test --platform mac --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace ;;
      *)
        case "$mode" in
          skipped-summary) echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":1}' >"$FX_SUMMARY" ;;
          no-tests) echo '{"result":"Passed","passedTests":0,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY" ;;
          wrong-case) test_tree '[{nodeType:"Test Case", nodeIdentifier:"MediaTests/testAudio()", result:"Passed"}]' ;;
          failed-case) test_tree '[{nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Failed"}]' ;;
          skipped-case) test_tree '[{nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Skipped"}]' ;;
          duplicate) test_tree '[{nodeType:"Test Case", nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Passed"}, {nodeType:"Test Case", nodeIdentifier:"MeguriUITests/CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()", result:"Passed"}]' ;;
          missing-tree) echo invalid >"$FX_TESTS" ;;
        esac
        mg ui-test --platform mac --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace
        ;;
    esac
    [ "$status" -ne 0 ]
    not_called '^xcrun xcresulttool export attachments '
    not_called '^media-(ssh|copy-out|simctl|fetch)'
    [ "$(field .details.native_media_export)" = null ]
    [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 0 ]
    [ -z "$(ls -A "$(dirname "$result")/native-media-export")" ]
  done
}

@test "native media export Mac selective attachment capture refuses malformed foreign or unsafe exports" {
  native_media_fakes
  local mode phase
  for mode in export-error wrong-test wrong-name wrong-delimiter wrong-token failure-marker missing-marker \
      duplicate-attachment extra-record extra-export escape absolute wrong-extension linked-payload \
      hardlink-payload fifo directory empty missing-package malformed-manifest linked-manifest; do
    : >"$STUB_LOG"
    FX_ATTACHMENT_MODE="$mode" mg ui-test --platform mac --suite CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace
    [ "$status" -eq 2 ]
    called '^xcrun xcresulttool export attachments --path .*/mac.xcresult --output-path .*/native-media-attachments --test-id CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace\(\) --filter \*\.colpkg$'
    [ "$(grep -c '^xcrun xcresulttool export attachments ' "$STUB_LOG")" -eq 1 ]
    phase=manifest
    case "$mode" in export-error) phase=export ;; esac
    [ "$(field .details.native_media_capture_failure.phase)" = "$phase" ]
    [ "$(field .details.native_media_export)" = null ]
    [ "$(field '[.artifacts[] | select(.kind | startswith("native-media"))] | length')" = 0 ]
    [ -z "$(ls -A "$(dirname "$result")/native-media-export")" ]
    ! grep -q 'synthetic subprocess output must stay private' "$(dirname "$result")/log.txt"
    not_called '^media-(ssh|copy-out|simctl|fetch)'
  done
}

@test "native media export Mac attachment capture pins manifest payload and own-run descriptors" {
  run python3 - "$SB/tools/scripts/native-media-export-capture.py" "$BATS_TEST_TMPDIR/mac-attachments" <<'PY'
import importlib.util
import json
import os
import stat
import sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('capture', sys.argv[1])
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
base = Path(sys.argv[2])
filename = 'Meguri-Media-' + '2' * 32 + '.colpkg'
stem = Path(filename).stem
basename = '00000000-0000-4000-8000-000000000007.colpkg'
human_name = stem + '_0_00000000-0000-4000-8000-000000000008.colpkg'
original = b'native-A'
foreign = b'foreignB'

for case in ('success', 'directory-linkcount-growth', 'file-linkcount-growth',
             'occupied', 'wrong-owner', 'wrong-bundle', 'export-occupied',
             'export-symlink', 'ancestor-symlink', 'bundle-symlink',
             'manifest-symlink', 'file-symlink', 'hardlink', 'fifo',
             'missing-package', 'ancestor-replacement', 'folder-replacement', 'file-replacement',
             'same-length-mutation', 'rehash-mutation', 'destination-mutation',
             'manifest-mutation', 'extra-export'):
    root = base / case
    run = root / 'own-run'
    output_dir = run / 'native-media-export'
    output_dir.mkdir(parents=True)
    output = output_dir / filename
    bundle = run / 'mac.xcresult'
    bundle.mkdir()
    exported = run / 'native-media-attachments'
    source = exported / basename
    manifest_path = exported / 'manifest.json'
    expected = run / 'expected-source.json'
    expected.write_text(json.dumps({'native_filename': filename if case != 'wrong-owner'
                                   else 'Meguri-Media-' + '3' * 32 + '.colpkg'}))
    escaped = root / 'escaped.colpkg'
    escaped.write_bytes(foreign)
    manifest = [{'testIdentifier': capture.METHOD, 'attachments': [{
        'exportedFileName': basename, 'suggestedHumanReadableName': human_name,
        'isAssociatedWithFailure': False}]}]
    if case == 'occupied':
        output.write_bytes(foreign)
    elif case == 'wrong-bundle':
        bundle = root / 'foreign.xcresult'
        bundle.mkdir()
    elif case == 'export-occupied':
        exported.mkdir()
        (exported / 'foreign').write_bytes(foreign)
    elif case == 'export-symlink':
        exported.symlink_to(root, target_is_directory=True)
    elif case in ('ancestor-symlink', 'bundle-symlink'):
        directory = run if case == 'ancestor-symlink' else bundle
        retained = directory.with_name(directory.name + '-retained')
        directory.rename(retained)
        directory.symlink_to(retained, target_is_directory=True)
    before = None
    exported_calls = []
    stat_path = os.stat
    stat_fd = os.fstat
    output_metadata = stat_path(output_dir)
    directory_counts = {(output_metadata.st_dev, output_metadata.st_ino):
                        (output_dir, {filename}, output_metadata.st_nlink)}
    observed_growth = {}

    def export(args, **kwargs):
        global before
        exported_calls.append(args)
        assert args == ['xcrun', 'xcresulttool', 'export', 'attachments', '--path', str(bundle),
                        '--output-path', str(exported), '--test-id', capture.METHOD,
                        '--filter', '*.colpkg']
        assert kwargs == {'check': True, 'stdout': capture.subprocess.DEVNULL,
                          'stderr': capture.subprocess.DEVNULL}
        assert not list(exported.iterdir())
        metadata = stat_path(exported)
        directory_counts[(metadata.st_dev, metadata.st_ino)] = (
            exported, {'manifest.json', basename}, metadata.st_nlink)
        source.write_bytes(original)
        manifest_path.write_text(json.dumps(manifest))
        if case == 'missing-package':
            source.unlink()
        elif case == 'manifest-symlink':
            manifest_path.unlink()
            manifest_path.symlink_to(escaped)
        elif case == 'file-symlink':
            source.unlink()
            source.symlink_to(escaped)
        elif case == 'hardlink':
            os.link(source, root / 'extra-link')
        elif case == 'fifo':
            source.unlink()
            os.mkfifo(source)
        else:
            before = source.stat()

    read = os.read
    fired = False
    source_reads = []
    source_read_calls = 0

    def portable_metadata(metadata):
        identity = (metadata.st_dev, metadata.st_ino)
        if case == 'directory-linkcount-growth' and stat.S_ISDIR(metadata.st_mode) \
                and identity in directory_counts:
            directory, names, initial_links = directory_counts[identity]
            # Only expected regular-file creation changes the simulated link count.
            growth = len(set(os.listdir(directory)) & names)
            observed_growth[directory] = growth
            links = initial_links + growth
        elif case == 'file-linkcount-growth' and fired and before is not None \
                and identity == (before.st_dev, before.st_ino):
            links = metadata.st_nlink + 1
        else:
            return metadata
        fields = {name: getattr(metadata, name) for name in dir(metadata) if name.startswith('st_')}
        fields['st_nlink'] = links
        return SimpleNamespace(**fields)

    def portable_stat(*args, **kwargs):
        return portable_metadata(stat_path(*args, **kwargs))

    def portable_fstat(*args, **kwargs):
        return portable_metadata(stat_fd(*args, **kwargs))

    def intercept(fd, count):
        global fired, source_read_calls
        metadata = os.fstat(fd)
        is_source = before is not None and (metadata.st_dev, metadata.st_ino) == (before.st_dev, before.st_ino)
        if is_source:
            source_read_calls += 1
        delayed = case in ('rehash-mutation', 'destination-mutation')
        if is_source and not fired and (not delayed or source_read_calls == 3):
            fired = True
            if case in ('ancestor-replacement', 'folder-replacement'):
                directory = run if case == 'ancestor-replacement' else exported
                directory.rename(directory.with_name(directory.name + '-retained'))
                exported.mkdir(parents=True)
                source.write_bytes(foreign)
            elif case == 'file-replacement':
                source.rename(exported / 'retained.colpkg')
                source.write_bytes(foreign)
            elif case in ('same-length-mutation', 'rehash-mutation'):
                source.write_bytes(foreign)
                os.utime(source, ns=(before.st_atime_ns, before.st_mtime_ns + 1_000_000_000))
            elif case == 'destination-mutation':
                output.write_bytes(foreign)
            elif case == 'manifest-mutation':
                manifest_path.write_text('[]')
            elif case == 'extra-export':
                (exported / 'unrelated-screen.png').write_bytes(foreign)
        block = read(fd, count)
        if is_source:
            source_reads.append(block)
        return block

    try:
        with patch.object(capture.subprocess, 'run', side_effect=export), \
             patch.object(capture.os, 'read', side_effect=intercept), \
             patch.object(capture.os, 'stat', side_effect=portable_stat), \
             patch.object(capture.os, 'fstat', side_effect=portable_fstat):
            capture.mac_capture(run, filename, bundle)
    except (ValueError, OSError):
        assert case not in ('success', 'directory-linkcount-growth'), case
        if case == 'occupied':
            assert output.read_bytes() == foreign
        else:
            assert not output.exists(), case
        if case == 'ancestor-replacement':
            assert not (root / 'own-run-retained/native-media-export' / filename).exists()
    else:
        assert case in ('success', 'directory-linkcount-growth'), case
        assert output.read_bytes() == original
        assert source.read_bytes() == original
        assert capture.file_identity(output)['bytes'] == len(original)
        if case == 'directory-linkcount-growth':
            assert observed_growth == {exported: 2, output_dir: 1}, observed_growth
    assert escaped.read_bytes() == foreign
    if case in ('wrong-owner', 'wrong-bundle', 'export-occupied', 'export-symlink',
                'ancestor-symlink', 'bundle-symlink'):
        assert not exported_calls, case
    else:
        assert len(exported_calls) == 1, case
    if case in ('ancestor-replacement', 'folder-replacement', 'file-replacement'):
        assert fired and b''.join(source_reads) == original, case
        assert source.read_bytes() == foreign, case
    if case in ('same-length-mutation', 'rehash-mutation', 'destination-mutation', 'manifest-mutation',
                'extra-export', 'file-linkcount-growth'):
        assert fired, case
    assert not (run / 'native-media-export.json').exists()

# CLI ownership is checked before an exporter can inspect a foreign result.
with patch.object(capture, 'ROOT', base), \
     patch.object(capture.subprocess, 'run', side_effect=AssertionError('foreign export')):
    for run_path in (base / 'foreign', base / 'build/mg/ui-test/../foreign'):
        with patch.object(sys, 'argv', ['capture', 'mac', str(run_path), filename, str(run_path / 'mac.xcresult')]):
            try:
                capture.main()
            except (ValueError, OSError):
                pass
            else:
                raise AssertionError('foreign run accepted')
PY
  [ "$status" -eq 0 ]
}
