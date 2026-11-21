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
# The identity checks run on real products (app_identity_check.py, xcode-check).
mg_app_personal_identity() { echo "identity $1" >>"$STUB_LOG"; }
mg_app_real_copy_identity() { echo "identity $1" >>"$STUB_LOG"; }
# Generated dispatcher evidence only. cloud_agent_injection_test.py exercises
# the real preparation helper with generated descriptors/command doubles.
mg_app_cloud_agent_build() {
  echo cloud-agent-prepare >>"$STUB_LOG"
  xcodebuild "$@"
}
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
if [ "$action" = -showBuildSettings ]; then
  case "$*" in
    *-derivedDataPath\ build/app/StandaloneDerivedData*)
      jq --arg signing "${FX_RESOLVED_SIGNING:-${MEGURI_APP_SIGNING:-adhoc}}" \
        "map(.buildSettings.MEGURI_SIGNING = \$signing)" "$FX_STANDALONE_SETTINGS"
      exit "${FX_SETTINGS_RC:-0}" ;;
    *) cat "$FX_PERSONAL_SETTINGS"; exit 0 ;;
  esac
fi
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
  jq --slurpfile cloud "$FX_CLOUD_INFO" ". + \$cloud[0]" "$p/Contents/Info.plist" >"$p/Contents/cloud.json"
  mv "$p/Contents/cloud.json" "$p/Contents/Info.plist"
  if [ "$config" = Release ]; then
    jq --slurpfile cloud "$FX_CLOUD_INFO" ". + \$cloud[0] | .MeguriCardInspectionBuild = \"personalRelease\" | .LSUIElement = false" "$p/Contents/Info.plist" >"$p/Contents/personal.json"
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

trace_app_build() {
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'EOF'
mg_app_build() {
  { printf 'app-build'; printf ' <%s>' "$@"; printf '\n'; } >>"$STUB_LOG"
  mg_app_build_action build-for-testing "$@"
}
mg_app_input_digest() { echo input-digest >>"$STUB_LOG"; return 1; }
EOF
}

@test "build-app explicit CloudAgent forwards the positional plan and records the actual build choice" {
  trace_app_build
  local platform config_args dd
  export MG_APP_TEST_PLAN=UI
  for platform in mac ios-sim; do
    for config_args in implicit explicit; do
      : >"$STUB_LOG"
      if [ "$config_args" = explicit ]; then
        mg build-app --test-plan CloudAgent --config Debug --platform "$platform"
      else
        mg build-app --platform "$platform" --test-plan CloudAgent
      fi
      [ "$status" -eq 0 ]
      [ "$(field .status)" = pass ]
      called "^app-build <$platform> <Debug> <CloudAgent>$"
      dd=CloudAgentSimDerivedData
      [ "$platform" != mac ] || dd=CloudAgentMacDerivedData
      called "^xcodebuild build-for-testing .* -testPlan CloudAgent -configuration Debug .* -derivedDataPath build/app/$dd "
      [ "$(field .details.build.test_plan)" = CloudAgent ]
      [ "$(field .details.build.action)" = build-for-testing ]
      called '^cloud-agent-prepare$'
      not_called 'input-digest|test-without-building|^(sim-|vm-test|host-lease|open)|xcrun (simctl|devicectl)'
    done
  done
}

@test "build-app default and explicit UI ignore the ambient test plan for builds" {
  trace_app_build
  local platform selection config
  export MG_APP_TEST_PLAN=CloudAgent
  for platform in mac ios-sim; do
    for config in Debug Profile; do
      for selection in default explicit; do
        : >"$STUB_LOG"
        if [ "$selection" = explicit ]; then
          mg build-app --platform "$platform" --config "$config" --test-plan UI
        else
          mg build-app --platform "$platform" --config "$config"
        fi
        [ "$status" -eq 0 ]
        [ "$(field .status)" = pass ]
        called "^app-build <$platform> <$config> <UI>$"
        called "^xcodebuild build-for-testing .* -testPlan UI -configuration $config .* -derivedDataPath build/app/DerivedData "
        [ "$(field .details.build.test_plan)" = UI ]
        not_called 'input-digest|test-without-building|^(sim-|vm-test|host-lease|open)|xcrun (simctl|devicectl)'
      done
    done
  done
}

@test "build-app CloudAgent platform roots retain the first inventory across the peer build" {
  trace_app_build
  mg build-app --platform mac --test-plan CloudAgent
  [ "$status" -eq 0 ]
  local first="$SB/build/app/CloudAgentMacDerivedData/Build/Products"
  local manifest="$BATS_TEST_TMPDIR/first-inventory"
  find "$first" -type f -exec shasum -a 256 {} + | sort >"$manifest"
  [ -s "$manifest" ]
  mg build-app --platform ios-sim --test-plan CloudAgent
  [ "$status" -eq 0 ]
  find "$first" -type f -exec shasum -a 256 {} + | sort >"$manifest.after"
  cmp "$manifest" "$manifest.after"
  [ -d "$SB/build/app/CloudAgentSimDerivedData/Build/Products" ]
  [ ! -d "$SB/build/app/DerivedData" ]
  not_called 'test-without-building|^(sim-|vm-test|host-lease|open)|xcrun (simctl|devicectl)'
}

@test "build-app failure reproduction preserves explicit CloudAgent and default UI plans" {
  trace_app_build
  local platform plan expected
  for platform in mac ios-sim; do
    for plan in CloudAgent UI; do
      : >"$STUB_LOG"
      expected="tools/mg build-app --platform $platform --config Debug"
      if [ "$plan" = CloudAgent ]; then
        MG_APP_TEST_PLAN=UI FX_BUILD_RC=1 mg build-app --platform "$platform" --test-plan CloudAgent
        expected="$expected --test-plan CloudAgent"
      else
        MG_APP_TEST_PLAN=CloudAgent FX_BUILD_RC=1 mg build-app --platform "$platform"
      fi
      [ "$status" -eq 1 ]
      [ "$(field .status)" = fail ]
      [ "$(field '.failures[0].id')" = build ]
      [ "$(field '.failures[0].repro')" = "$expected" ]
      called "^xcodebuild build-for-testing .* -testPlan $plan -configuration Debug "
    done
  done
}

@test "build-app team CloudAgent simulator certificate selection stays scoped" {
  trace_app_build
  local mode platform plan config expected selection
  for mode in team adhoc; do
    export MEGURI_APP_SIGNING="$mode"
    for selection in 'ios-sim CloudAgent Debug yes' 'mac CloudAgent Debug no' 'ios-sim UI Debug no' 'ios-sim UI Profile no'; do
      read -r platform plan config expected <<<"$selection"
      : >"$STUB_LOG"
      # Fail the generated compiler double before freezing: inspect the real
      # native argv without fabricating signed products or an acceptance pass.
      FX_BUILD_RC=1 mg build-app --platform "$platform" --config "$config" --test-plan "$plan"
      [ "$status" -eq 1 ]
      [ "$(field '.failures[0].id')" = build ]
      if [ "$mode/$expected" = team/yes ]; then
        called '^xcodebuild build-for-testing .* -testPlan CloudAgent .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration CODE_SIGN_IDENTITY=Apple Development$'
        [ "$(grep '^xcodebuild build-for-testing ' "$STUB_LOG" | grep -o 'CODE_SIGN_IDENTITY=' | wc -l | tr -d ' ')" -eq 1 ]
      else
        not_called 'CODE_SIGN_IDENTITY='
      fi
      not_called 'test-without-building|^(sim-|vm-test|host-lease|open)|xcrun (simctl|devicectl)'
    done
  done
}

@test "build-app CloudAgent rejects preview modes in either option order before build work" {
  trace_app_build
  local platform preview config order
  for platform in mac ios-sim; do
    for preview in --standalone --personal-preview; do
      for config in Debug Release; do
        for order in plan-first plan-last; do
          if [ "$order" = plan-first ]; then
            mg build-app --test-plan CloudAgent "$preview" --config "$config" --platform "$platform"
          else
            mg build-app --platform "$platform" --config "$config" "$preview" --test-plan CloudAgent
          fi
          [ "$status" -eq 2 ]
          [ "$(field '.failures[0].id')" = usage ]
          [ ! -s "$STUB_LOG" ]
        done
      done
      mg build-app --platform "$platform" "$preview" --test-plan CloudAgent
      [ "$status" -eq 2 ]
      [ ! -s "$STUB_LOG" ]
    done
  done
  [ ! -d "$SB/build/app" ]
}

@test "build-app CloudAgent rejects non-Debug configurations and other platforms before build work" {
  trace_app_build
  local platform config
  for platform in mac ios-sim; do
    for config in Profile Release AppStore; do
      mg build-app --platform "$platform" --test-plan CloudAgent --config "$config"
      [ "$status" -eq 2 ]
      [ "$(field '.failures[0].id')" = usage ]
      [ ! -s "$STUB_LOG" ]
    done
  done
  for platform in ios-device iphone ipad watchos 'mac ios-sim' ''; do
    mg build-app --test-plan CloudAgent --config Debug --platform "$platform"
    [ "$status" -eq 2 ]
    [ ! -s "$STUB_LOG" ]
  done
  mg build-app --test-plan CloudAgent
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
  [ ! -d "$SB/build/app" ]
}

@test "build-app rejects missing empty compound and unsupported test plans before build work" {
  trace_app_build
  local plan
  for plan in '' Snapshot Performance 'UI CloudAgent' cloudagent; do
    mg build-app --platform mac --test-plan "$plan"
    [ "$status" -eq 2 ]
    [ ! -s "$STUB_LOG" ]
  done
  mg build-app --platform mac --test-plan
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
  [ ! -d "$SB/build/app" ]
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
  export FX_STANDALONE_SETTINGS="$BATS_TEST_TMPDIR/standalone-settings.json"
  export FX_CLOUD_INFO="$BATS_TEST_TMPDIR/cloud-info.json"
  echo '{"MeguriCloudContainer":"iCloud.example.isolated","MeguriCloudEnvironment":"development","MeguriCloudSchemaConfigured":false}' >"$FX_CLOUD_INFO"
  jq -n '[{target:"Meguri", buildSettings: {
    CONFIGURATION:"Debug", MEGURI_BUNDLE_ID_SUFFIX:".dev", PRODUCT_BUNDLE_IDENTIFIER:"com.antoinefink.meguri.dev",
    MEGURI_CLOUD_CONFIGURATION:"development", MEGURI_CLOUD_CONTAINER:"iCloud.example.isolated",
    MEGURI_CLOUD_CONTAINER_development:"iCloud.example.isolated", MEGURI_CLOUD_CONTAINER_production:"iCloud.example.daily",
    MEGURI_CLOUD_ENVIRONMENT:"development", MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT:"Development", MEGURI_CLOUD_PUSH_ENVIRONMENT:"development",
    INFOPLIST_KEY_MeguriCloudContainer:"iCloud.example.isolated", INFOPLIST_KEY_MeguriCloudEnvironment:"development",
    MEGURI_CLOUD_SCHEMA_CONFIGURED_development:"NO", MEGURI_CLOUD_SCHEMA_CONFIGURED_production:"NO",
    MEGURI_CLOUD_SCHEMA_CONFIGURED:"NO", MEGURI_CLOUD_SCHEMA_BOOLEAN:"0"}}]' >"$FX_STANDALONE_SETTINGS"
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

standalone_team_grants() {
  jq '.["com.apple.developer.icloud-container-identifiers"] = ["iCloud.example.isolated"]
    | .["com.apple.developer.icloud-services"] = ["CloudKit"]
    | .["com.apple.developer.icloud-container-environment"] = "Development"
    | .["com.apple.developer.aps-environment"] = "development"' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/team-app.json"
  mv "$BATS_TEST_TMPDIR/team-app.json" "$FX_APP_ENT"
}

personal_facts() {
  standalone_facts
  export MEGURI_APP_SIGNING=team
  export FX_PERSONAL_SETTINGS="$BATS_TEST_TMPDIR/personal-settings.json"
  export FX_CLOUD_INFO="$BATS_TEST_TMPDIR/cloud-info.json"
  echo '{"MeguriCloudContainer":"iCloud.example.isolated","MeguriCloudEnvironment":"development","MeguriCloudSchemaConfigured":false}' >"$FX_CLOUD_INFO"
  export FX_APP_PROFILE="$BATS_TEST_TMPDIR/app-profile.plist" FX_WIDGET_PROFILE="$BATS_TEST_TMPDIR/widget-profile.plist"
  jq 'del(.["com.apple.security.get-task-allow"], .["com.apple.security.temporary-exception.files.absolute-path.read-only"], .["com.apple.private.security.storage.AppDataContainers"])
    | .["com.apple.developer.team-identifier"] = "TESTTEAM01"
    | .["com.apple.application-identifier"] = "TESTTEAM01.com.antoinefink.meguri.dev"
    | .["com.apple.security.application-groups"] = ["TESTTEAM01.com.antoinefink.meguri.dev"]
    | .["keychain-access-groups"] = ["TESTTEAM01.com.antoinefink.meguri.dev"]
    | .["com.apple.developer.icloud-container-identifiers"] = ["iCloud.example.isolated"]
    | .["com.apple.developer.icloud-services"] = ["CloudKit"]
    | .["com.apple.developer.icloud-container-environment"] = "Development"
    | .["com.apple.developer.aps-environment"] = "development"' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/personal-app.json"
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
    MEGURI_CLOUD_CONFIGURATION:"development", MEGURI_CLOUD_CONTAINER:"iCloud.example.isolated",
    MEGURI_CLOUD_CONTAINER_development:"iCloud.example.isolated", MEGURI_CLOUD_CONTAINER_production:"iCloud.example.daily",
    MEGURI_CLOUD_ENVIRONMENT:"development", MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT:"Development", MEGURI_CLOUD_PUSH_ENVIRONMENT:"development",
    INFOPLIST_KEY_MeguriCloudContainer:"iCloud.example.isolated", INFOPLIST_KEY_MeguriCloudEnvironment:"development",
    MEGURI_CLOUD_SCHEMA_CONFIGURED_development:"NO", MEGURI_CLOUD_SCHEMA_CONFIGURED_production:"NO",
    MEGURI_CLOUD_SCHEMA_CONFIGURED:"NO", MEGURI_CLOUD_SCHEMA_BOOLEAN:"0",
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
    called '^xcodebuild -showBuildSettings .* -configuration Release .* MEGURI_BUNDLE_ID_SUFFIX=.dev MEGURI_CLOUD_CONFIGURATION=development$'
    called '^xcframework .* --configuration Release$'
    [ "$(grep -n '^xcframework ' "$STUB_LOG" | cut -d: -f1)" -lt "$(grep -n '^xcodebuild -showBuildSettings ' "$STUB_LOG" | cut -d: -f1)" ]
    called '^xcodebuild build .* -configuration Release .* -derivedDataPath build/app/PersonalPreviewDerivedData .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration MEGURI_BUNDLE_ID_SUFFIX=.dev MEGURI_CLOUD_CONFIGURATION=development$'
    not_called 'build-for-testing|-testPlan|test-without-building|^(sim-|vm-test|host-lease|open|lsregister)|xcrun (simctl|devicectl)'
    [ "$(field .details.build.personal_preview)" = true ]
    [ "$(field .details.build.test_plan)" = UI ]
    [ "$(field .details.build.standalone)" = false ]
    [ "$(field .details.personal_preview_product.provisioning_verified)" = true ]
    [ "$(field .details.personal_preview_product.test_hooks)" = false ]
    [ "$(field .details.personal_preview_product.cloud_environment)" = development ]
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
    '.[0].buildSettings.MEGURI_CARD_INSPECTION_BUILD = "debug"' \
    '.[0].buildSettings.MEGURI_CLOUD_CONFIGURATION = "production"' \
    '.[0].buildSettings.MEGURI_CLOUD_CONTAINER = "iCloud.example.daily"' \
    '.[0].buildSettings.MEGURI_CLOUD_ENVIRONMENT = "production"' \
    '.[0].buildSettings.MEGURI_CLOUD_PUSH_ENVIRONMENT = "production"' \
    '.[0].buildSettings.MEGURI_BUNDLE_ID_SUFFIX = ""' \
    '.[1].buildSettings.MEGURI_CLOUD_CONFIGURATION = "production"'; do
    jq "$mutation" "$BATS_TEST_TMPDIR/normal-settings.json" >"$FX_PERSONAL_SETTINGS"
    : >"$STUB_LOG"
    mg build-app --platform mac --personal-preview
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = personal-preview-settings ]
    called '^xcframework .* --configuration Release$'
    not_called '^xcodebuild build '
  done
}

# Mutate only the generated profile; signed product claims stay independent.
personal_profile_cloud_grant() {
  python3 - "$FX_APP_PROFILE" "$1" "$2" <<'PY'
import json, plistlib, sys
with open(sys.argv[1], "rb") as stream:
    profile = plistlib.load(stream)
if sys.argv[3] == "missing":
    profile["Entitlements"].pop(sys.argv[2], None)
else:
    profile["Entitlements"][sys.argv[2]] = json.loads(sys.argv[3])
with open(sys.argv[1], "wb") as stream:
    plistlib.dump(profile, stream)
PY
}

@test "personal preview accepts documented profile service and environment authorizations" {
  personal_facts
  local services environment
  for services in '"*"' '["CloudKit"]' '["CloudKit","CloudDocuments"]' '["CloudDocuments","CloudKit"]'; do
    for environment in '"Development"' '["Development"]' '["Development","Production"]' '["Production","Development"]'; do
      personal_profile_cloud_grant com.apple.developer.icloud-services "$services"
      personal_profile_cloud_grant com.apple.developer.icloud-container-environment "$environment"
      mg build-app --platform mac --personal-preview
      [ "$status" -eq 0 ]
      [ "$(field .status)" = pass ]
      [ "$(field .details.personal_preview_product.provisioning_verified)" = true ]
      not_called '^codesign .*--(sign|remove-signature)'
    done
  done
}

@test "personal preview rejects malformed or non authorizing profile cloud grants" {
  local key value values
  for key in com.apple.developer.icloud-services com.apple.developer.icloud-container-environment \
    com.apple.developer.icloud-container-identifiers com.apple.developer.aps-environment; do
    case "$key" in
      com.apple.developer.icloud-services)
        values=(missing '"CloudKit"' '"CloudDocuments"' '"**"' '""' '[]' '["CloudDocuments"]' '["*"]'
          '["CloudKit","*"]' '["CloudKit","CloudKit"]' '["CloudKit","Unknown"]' '["cloudkit"]'
          '["CloudKit",true]' '["CloudKit",["CloudDocuments"]]' true '{}') ;;
      com.apple.developer.icloud-container-environment)
        values=(missing '"Production"' '"development"' '"*"' '""' '[]' '["Production"]' '["*"]'
          '["Development","Development"]' '["Development","Unknown"]' '["development"]'
          '["Development",false]' '["Development",["Production"]]' false '{}') ;;
      com.apple.developer.icloud-container-identifiers)
        values=(missing '["*"]' '["iCloud.example.*"]' '["iCloud.example.daily"]'
          '["iCloud.example.isolated","iCloud.example.daily"]') ;;
      com.apple.developer.aps-environment)
        values=(missing '"production"' '["development"]' '"*"') ;;
    esac
    for value in "${values[@]}"; do
      personal_facts
      personal_profile_cloud_grant "$key" "$value"
      mg build-app --platform mac --personal-preview
      [ "$status" -eq 1 ]
      [ "$(field '.failures[0].id')" = personal-preview-product ]
      [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
    done
  done
}

@test "personal preview keeps exact signed cloud claims with authorizing profile grants" {
  local mutation
  for mutation in \
    '.["com.apple.developer.icloud-services"] = ["CloudDocuments"]' \
    '.["com.apple.developer.icloud-services"] = ["*"]' \
    '.["com.apple.developer.icloud-services"] = "*"' \
    '.["com.apple.developer.icloud-services"] = ["CloudKit","CloudDocuments"]' \
    'del(.["com.apple.developer.icloud-services"])' \
    '.["com.apple.developer.icloud-container-environment"] = "Production"' \
    '.["com.apple.developer.icloud-container-environment"] = ["Development"]' \
    '.["com.apple.developer.icloud-container-environment"] = "development"' \
    'del(.["com.apple.developer.icloud-container-environment"])' \
    '.["com.apple.developer.icloud-container-identifiers"] = ["iCloud.example.daily"]' \
    '.["com.apple.developer.icloud-container-identifiers"] = ["*"]' \
    '.["com.apple.developer.icloud-container-identifiers"] = ["iCloud.example.isolated","iCloud.example.daily"]' \
    'del(.["com.apple.developer.icloud-container-identifiers"])' \
    '.["com.apple.developer.aps-environment"] = "production"' \
    '.["com.apple.developer.aps-environment"] = ["development"]' \
    'del(.["com.apple.developer.aps-environment"])' \
    '.["com.apple.developer.ubiquity-container-identifiers"] = ["iCloud.example.isolated"]'; do
    personal_facts
    personal_profile_cloud_grant com.apple.developer.icloud-services '"*"'
    personal_profile_cloud_grant com.apple.developer.icloud-container-environment '["Production","Development"]'
    jq "$mutation" "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/signed-fault.json"
    mv "$BATS_TEST_TMPDIR/signed-fault.json" "$FX_APP_ENT"
    mg build-app --platform mac --personal-preview
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = personal-preview-product ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
  done
}

@test "personal preview rejects fixture scope debug grants stale core invalid provisioning and source changes" {
  local fault mutation
  for fault in fixture debug keychain group stamp profile expired device grant platform certificates profile-debug signer source signature arch cloud-container cloud-environment cloud-push widget-cloud info-container info-environment info-string info-ready profile-cloud; do
    personal_facts
    case "$fault" in
      fixture) jq '.["com.apple.security.temporary-exception.files.absolute-path.read-only"] = ["/"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      debug) jq '.["com.apple.security.get-task-allow"] = true' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      keychain) jq '.["keychain-access-groups"] = ["TESTTEAM01.com.antoinefink.meguri"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      group) jq '.["com.apple.security.application-groups"] = ["TESTTEAM01.com.antoinefink.meguri"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      cloud-container) jq '.["com.apple.developer.icloud-container-identifiers"] = ["iCloud.example.daily"]' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      cloud-environment) jq '.["com.apple.developer.icloud-container-environment"] = "Production"' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      cloud-push) jq '.["com.apple.developer.aps-environment"] = "production"' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/fault.json" ;;
      widget-cloud)
        jq '.["com.apple.developer.icloud-services"] = ["CloudKit"]' "$FX_WIDGET_ENT" >"$BATS_TEST_TMPDIR/widget-fault.json"
        mv "$BATS_TEST_TMPDIR/widget-fault.json" "$FX_WIDGET_ENT" ;;
      info-container|info-environment|info-string|info-ready)
        case "$fault" in
          info-container) mutation='.MeguriCloudContainer = "iCloud.example.daily"' ;;
          info-environment) mutation='.MeguriCloudEnvironment = "production"' ;;
          info-string) mutation='.MeguriCloudSchemaConfigured = "NO"' ;;
          info-ready) mutation='.MeguriCloudSchemaConfigured = true' ;;
        esac
        jq "$mutation" "$FX_CLOUD_INFO" >"$BATS_TEST_TMPDIR/cloud-fault.json"
        mv "$BATS_TEST_TMPDIR/cloud-fault.json" "$FX_CLOUD_INFO" ;;
      stamp) sed 's/features=ankiweb/features=ankiweb,test-hooks/' "$SB/app/Packages/MeguriCore/Binaries/.stamp" >"$BATS_TEST_TMPDIR/stamp"; mv "$BATS_TEST_TMPDIR/stamp" "$SB/app/Packages/MeguriCore/Binaries/.stamp" ;;
      profile) echo invalid >"$FX_APP_PROFILE" ;;
      expired|device|grant|platform|certificates|profile-debug|profile-cloud)
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
elif sys.argv[2] == "profile-cloud":
    profile["Entitlements"]["com.apple.developer.icloud-container-identifiers"] = ["iCloud.example.daily"]
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
    [ "$mode" != team ] || standalone_team_grants
    : >"$STUB_LOG"
    mg build-app --platform mac --config Debug --standalone
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
    called '^lint$'
    called '^xcframework .* --configuration Debug$'
    called '^xcodebuild build -project app/Meguri.xcodeproj -scheme Meguri -configuration Debug -destination platform=macOS,arch=arm64 -derivedDataPath build/app/StandaloneDerivedData '
    called '^xcodebuild -showBuildSettings -json -project app/Meguri.xcodeproj -scheme Meguri -configuration Debug -destination platform=macOS,arch=arm64 -derivedDataPath build/app/StandaloneDerivedData'
    not_called 'build-for-testing|-testPlan|test-without-building|^(sim-|vm-test|host-lease)|xcrun (simctl|devicectl)'
    if [ "$mode" = team ]; then
      called '^xcodebuild build .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$'
      called '^xcodebuild -showBuildSettings .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$'
    else
      not_called 'MEGURI_SIGNING=|allowProvisioning'
    fi
    called "^codesign --verify --deep --strict $candidate$"
    called "^codesign --verify --strict $candidate/Contents/PlugIns/MeguriWidget.appex$"
    not_called '^codesign .*--(sign|remove-signature)'
    [ "$(field '.artifacts[] | select(.kind == "app") | .path')" = "$candidate" ]
    [ "$(field .details.build.app)" = "$candidate" ]
    [ "$(field .details.build.action)" = build ]
    [ "$(field .details.build.test_plan)" = UI ]
    [ "$(field .details.build.standalone)" = true ]
    [ "$(field '.details.standalone_product.architecture | .app + "/" + .widget')" = arm64/arm64 ]
    [ "$(field .details.standalone_product.signature_verified)" = true ]
    [ "$(field .details.standalone_product.cloud_configuration_verified)" = true ]
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

@test "standalone cloud proof rejects built Info types destinations and protected app or widget grants" {
  standalone_facts
  cp "$FX_CLOUD_INFO" "$BATS_TEST_TMPDIR/cloud-normal.json"
  local mutation target
  for mutation in \
    '.MeguriCloudSchemaConfigured = "NO"' \
    '.MeguriCloudSchemaConfigured = 0' \
    'del(.MeguriCloudSchemaConfigured)' \
    '.MeguriCloudSchemaConfigured = true' \
    '.MeguriCloudContainer = "iCloud.example.daily"' \
    '.MeguriCloudEnvironment = "production"'; do
    jq "$mutation" "$BATS_TEST_TMPDIR/cloud-normal.json" >"$FX_CLOUD_INFO"
    mg build-app --platform mac --standalone
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = standalone-product ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
  done
  cp "$BATS_TEST_TMPDIR/cloud-normal.json" "$FX_CLOUD_INFO"
  for target in "$FX_APP_ENT" "$FX_WIDGET_ENT"; do
    cp "$target" "$BATS_TEST_TMPDIR/grants-normal.json"
    jq '.["com.apple.developer.icloud-services"] = ["CloudKit"]' "$target" >"$BATS_TEST_TMPDIR/grants-bad.json"
    mv "$BATS_TEST_TMPDIR/grants-bad.json" "$target"
    mg build-app --platform mac --standalone
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = standalone-product ]
    [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
    cp "$BATS_TEST_TMPDIR/grants-normal.json" "$target"
  done
  MEGURI_APP_SIGNING=team mg build-app --platform mac --standalone
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = standalone-product ]
  [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
}

@test "standalone cloud proof uses effective signing and fails closed on unavailable settings" {
  standalone_facts
  standalone_team_grants
  FX_RESOLVED_SIGNING=team mg build-app --platform mac --standalone
  [ "$status" -eq 0 ]
  [ "$(field .details.standalone_product.cloud_configuration_verified)" = true ]
  not_called 'MEGURI_SIGNING=|allowProvisioning'
  FX_SETTINGS_RC=1 mg build-app --platform mac --standalone
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = standalone-product ]
  [ "$(field '[.artifacts[] | select(.kind == "app")] | length')" -eq 0 ]
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
# These existing fakes model F11.1 only; F11.2 dispatch has its own fakes below.
mg_ui_test__review_selected() { return 1; }
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

native_review_dispatch_stubs() {
  mkdir -p "$SB/build/fixtures/recipes/local-review-1/collection.media"
  printf '%s\n' '{"passedTests":1,"failedTests":0,"skippedTests":0,"result":"Passed"}' >"$FX_SUMMARY"
  test_tree '[{nodeType:"Test Case",nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExport()",result:"Passed"}]'
  local app_home="/Users/meguri/Library/Developer/CoreSimulator/Devices/$SIM/data/Containers/Data/Application"
  export FX_CONTAINER="$app_home/00000000-0000-4000-8000-000000000004"
  FX_REAL_PYTHON="$(command -v python3)"
  export FX_REAL_PYTHON
  cat >"$BIN/python3" <<'EOF'
#!/bin/bash
case "$1" in
  */tools/scripts/native-media-review-capture.py)
    action="$2"; dir="$3"
    echo "review-helper $action $dir" >>"$STUB_LOG"
    case "$action" in
      start) mkdir "$dir/source-preparation" "$dir/native-review-export" ;;
      decoder-root) exec "$FX_REAL_PYTHON" "$@" ;;
      bind)
        printf '{"freeze":{"sha256":"%064d","bytes":100},"native_filename":"Meguri-Media-00000000000000000000000000000001.colpkg"}\n' 0 >"$dir/native-review-binding.json" ;;
      stage-paths)
        printf '%s\n' "$dir/expected-replay" "$dir/source-preparation" "$dir/native-review-binding.json" "${dir%/build/mg/ui-test/*}/build/fixtures/recipes/local-review-1" ;;
      receipts) [ "${FX_REVIEW_RECEIPT_REFUSED:-0}" = 0 ] ;;
      proof) echo '{"external_content_client_proof":"passed"}' >"$dir/native-review-proof.json" ;;
      simulator|attestation|mac) [ "${FX_REVIEW_CAPTURE_REFUSED:-0}" = 0 ] ;;
      *) exit 97 ;;
    esac
    exit $? ;;
  *) exec "$FX_REAL_PYTHON" "$@" ;;
esac
EOF
  chmod +x "$BIN/python3"
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'EOF'
mg_app_build() { echo "review-build $*" >>"$STUB_LOG"; }
mg_mac_ui_begin() { MG_MAC_RUNNER=vm; }
mg_mac_ui_take() { :; }
mg_app_log_now() { echo synthetic-time; }
mg_app_mac_input_refused() { return 1; }
mg_compose() { echo "review-decoder $*" >>"$STUB_LOG"; }
mg_conf__dev() { echo "review-rust $*" >>"$STUB_LOG"; }
mg_vm_fixture() { echo "review-stage $*" >>"$STUB_LOG"; }
mg_mac_vm_copy_in() { echo "review-copy-in $*" >>"$STUB_LOG"; }
mg_simctl() { echo "review-container $*" >>"$STUB_LOG"; echo "$FX_CONTAINER"; }
mg_mac_vm_ssh() { echo "review-capture $*" >>"$STUB_LOG"; }
mg_sim_fetch() { echo "review-fetch $*" >>"$STUB_LOG"; }
mg_app_xcodebuild_test() {
  echo "review-test $*" >>"$STUB_LOG"
  echo "review-input ${TEST_RUNNER_MEGURI_NATIVE_REVIEW_CASE:-unset} ${TEST_RUNNER_MEGURI_NATIVE_REVIEW_PLATFORM:-unset} ${TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_SHA256:-unset} ${TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_BYTES:-unset}" >>"$STUB_LOG"
  mkdir -p "$1.xcresult"
  return "${FX_RESULT:-0}"
}
mg_package_interop_check() { echo "review-package-proof $*" >>"$STUB_LOG"; mg_status pass; }
EOF
}

review_run() {
  mg ui-test --device "$SIM" --suite CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExport "$@"
}

@test "native review exact dispatch stages frozen states preparation and actual parent before launch" {
  native_review_dispatch_stubs
  review_run
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  local dir="$(dirname "$result")" stage_line test_line
  called "^review-stage $dir/expected-replay$"
  called "^review-stage $dir/source-preparation$"
  called "^review-stage $SB/build/fixtures/recipes/local-review-1$"
  called "^review-stage $dir/native-review-binding.json$"
  called "review-decoder .*prepare-native-review --root /work --run /work/build/mg/ui-test/$(basename "$dir") .*--platform iphone"
  called "review-decoder .* -u $(id -u):$(id -g) -v $SB/tools/scripts/package-interop-check.py:/decoder.py:ro -v $dir/decoder-root:/work:ro -v $SB/build/fixtures/recipes/local-review-1:/work/build/fixtures/recipes/local-review-1:ro -v $dir/source-preparation:/work/build/mg/ui-test/$(basename "$dir")/source-preparation package-interop-decoder"
  not_called "review-decoder .* -v $SB:/work"
  called '^review-rust -- cd core && cargo build -q -p meguri-cli$'
  called "^review-rust -u $(id -u):$(id -g) -v $dir/empty-cargo-home:/cargo-home:ro -v [^ ]+_cargo-target:/cargo-target:ro -- python3 /work/tools/scripts/native-media-review-capture.py derive '/work/build/mg/ui-test/$(basename "$dir")' 'iphone' '[a-f0-9]{40}'$"
  not_called 'review-rust .*cargo run'
  not_called 'review-rust .*(chown|chmod)'
  stage_line="$(grep -n '^review-stage ' "$STUB_LOG" | tail -1 | cut -d: -f1)"
  test_line="$(grep -n '^review-test ' "$STUB_LOG" | cut -d: -f1)"
  [ "$stage_line" -lt "$test_line" ]
  called '^review-input [^ ]+ iphone [a-f0-9]{64} 100$'
  called "^review-package-proof --package $dir/native-review-export/Meguri-Media-[a-f0-9]{32}.colpkg --expected $dir/expected-replay/review --now 2030-01-15T10:00:00\+01:00 --tz Europe/Paris$"
  not_called '^media-'
  : >"$STUB_LOG"
  TEST_RUNNER_MEGURI_NATIVE_REVIEW_CASE=ambient mg ui-test --device "$SIM" --suite CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExportExtra
  [ "$status" -eq 0 ]
  not_called '^review-(helper|stage|decoder|rust|capture|package-proof)'
  called '^review-input unset unset unset unset$'
}

@test "native review refuses failed skipped missing or duplicate methods before capture and package proof" {
  native_review_dispatch_stubs
  FX_RESULT=1 review_run
  [ "$status" -ne 0 ]
  not_called '^review-(container|capture|package-proof)'
  : >"$STUB_LOG"
  echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":1}' >"$FX_SUMMARY"
  review_run
  [ "$status" -ne 0 ]
  not_called '^review-(container|capture|package-proof)'
  echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
  for nodes in '[]' '[{nodeType:"Test Case",nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExport()",result:"Passed"},{nodeType:"Test Case",nodeIdentifier:"CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExport()",result:"Passed"}]'; do
    : >"$STUB_LOG"
    test_tree "$nodes"
    review_run
    [ "$status" -ne 0 ]
    not_called '^review-(container|capture|package-proof)'
  done
}

@test "native review container path and receipt refusal prevent proof" {
  native_review_dispatch_stubs
  FX_CONTAINER=/tmp/unowned review_run
  [ "$status" -ne 0 ]
  not_called '^review-(capture|package-proof)'
  : >"$STUB_LOG"
  FX_REVIEW_RECEIPT_REFUSED=1 review_run
  [ "$status" -ne 0 ]
  called '^review-helper receipts '
  not_called '^review-package-proof '
  : >"$STUB_LOG"
  review_run --config Profile
  [ "$status" -ne 0 ]
  not_called '^review-(helper|test|decoder|rust|capture|package-proof)'
}

@test "native review helper refuses an escaped run before source capture" {
  mkdir -p "$SB/build/mg/ui-test" "$SB/build/mg/escaped"
  run python3 "$SB/tools/scripts/native-media-review-capture.py" start "$SB/build/mg/ui-test/../escaped" mac
  [ "$status" -ne 0 ]
  [ ! -e "$SB/build/mg/escaped/native-review-source.json" ]
}

@test "native review decoder tree is owned empty and matches the actual narrow mount paths" {
  run python3 - "$SB" <<'PY'
import importlib.util
import os
import stat
import sys
from pathlib import Path
from unittest.mock import patch

root = Path(sys.argv[1])
def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, root / 'tools/scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
capture = load('review_capture', 'native-media-review-capture.py')
decoder = load('package_decoder', 'package-interop-check.py')
parent = root / 'build/fixtures/recipes/local-review-1'
parent.mkdir(parents=True)
(parent / 'sentinel').write_bytes(b'actual parent, never copied')

for case in ('success', 'occupied-tree', 'occupied-output', 'output-link', 'parent-link',
             'run-mode', 'output-mode', 'foreign-owner'):
    run = root / 'build/mg/ui-test' / case
    run.mkdir(parents=True)
    output = run / 'source-preparation'
    output.mkdir(mode=0o700)
    if case == 'occupied-tree':
        (run / 'decoder-root').mkdir()
        (run / 'decoder-root/retained').write_bytes(b'keep')
    elif case == 'occupied-output':
        (output / 'retained').write_bytes(b'keep')
    elif case == 'output-link':
        output.rename(run / 'retained-output')
        output.symlink_to(run / 'retained-output', target_is_directory=True)
    elif case == 'parent-link':
        parent.rename(parent.with_name('retained-parent'))
        parent.symlink_to(parent.with_name('retained-parent'), target_is_directory=True)
    elif case == 'run-mode':
        run.chmod(0o722)
    elif case == 'output-mode':
        output.chmod(0o722)
    try:
        with patch.object(capture.os, 'getuid', return_value=os.getuid() + (case == 'foreign-owner')):
            capture.decoder_root(run)
    except (ValueError, OSError):
        assert case != 'success', case
    else:
        assert case == 'success', case
        skeleton = run / 'decoder-root'
        relative = 'build/mg/ui-test/' + run.name + '/source-preparation'
        expected = {'build', 'build/fixtures', 'build/fixtures/recipes',
                    'build/fixtures/recipes/local-review-1', 'build/mg', 'build/mg/ui-test',
                    'build/mg/ui-test/' + run.name, relative}
        assert {p.relative_to(skeleton).as_posix() for p in skeleton.rglob('*')} == expected
        for directory in [skeleton, *skeleton.rglob('*')]:
            info = directory.lstat()
            assert stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
            assert stat.S_IMODE(info.st_mode) == 0o700
        # Use the real preparer's unchanged ancestor guard on both mount paths.
        for name in ('build/fixtures/recipes/local-review-1', relative):
            fd = decoder.open_dir(skeleton / name, skeleton)
            assert os.listdir(fd) == []
            os.close(fd)
        # Docker overlays only these real leaves; it never sees parent copies.
        assert list(output.iterdir()) == []
        assert list((run / 'empty-cargo-home').iterdir()) == []
        assert not list((skeleton / 'build/fixtures/recipes/local-review-1').iterdir())
        try:
            capture.decoder_root(run)
        except (ValueError, OSError):
            pass
        else:
            raise AssertionError('reused tree admitted')
        (skeleton / 'build/mg').chmod(0o722)
        try:
            with capture.owned_directory(skeleton / relative):
                pass
        except (ValueError, OSError):
            pass
        else:
            raise AssertionError('owned ancestor mode admitted')
    if case == 'parent-link':
        parent.unlink()
        parent.with_name('retained-parent').rename(parent)
    assert (parent / 'sentinel').read_bytes() == b'actual parent, never copied'
    if case == 'occupied-tree':
        assert (run / 'decoder-root/retained').read_bytes() == b'keep'
    if case == 'occupied-output':
        assert (output / 'retained').read_bytes() == b'keep'
PY
  [ "$status" -eq 0 ]
}

@test "native review publishes a single-link owned CLI and refuses source or destination changes" {
  run python3 - "$SB" <<'PY'
import importlib.util
import json
import os
import stat
import sys
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import patch

root = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('review_capture', root / 'tools/scripts/native-media-review-capture.py')
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
revision = 'a' * 40
original = b'compiled own CLI fixture'
foreign = b'changed own CLI fixture!'

for case in ('success', 'source-mutation', 'source-substitution', 'destination-substitution',
             'occupied', 'directory-link', 'foreign-owner', 'compiler-mode', 'compiler-link', 'size-bound'):
    run = root / 'build/mg/ui-test' / ('code-' + case)
    run.mkdir(parents=True)
    cache = root / 'compiled-fixture' / case
    cache.mkdir(parents=True)
    compiler = cache / 'meguri-cli'
    compiler.write_bytes(original)
    compiler.chmod(0o755)
    os.link(compiler, cache / 'deps-link')
    before = compiler.stat()
    destination = run / 'run-code/meguri-cli'
    if case == 'occupied':
        destination.parent.mkdir()
        destination.write_bytes(b'keep')
    elif case == 'directory-link':
        (run / 'retained-code').mkdir()
        destination.parent.symlink_to(run / 'retained-code', target_is_directory=True)
    elif case == 'compiler-mode':
        compiler.chmod(0o777)
    elif case == 'compiler-link':
        compiler.unlink()
        compiler.symlink_to(cache / 'deps-link')
    real_read, real_write = os.read, os.write
    fired = False
    source_reads = 0
    def intercept_read(fd, count):
        global fired, source_reads
        info = os.fstat(fd)
        if (info.st_dev, info.st_ino) == (before.st_dev, before.st_ino):
            source_reads += 1
            # First hash completes before publication begins.
            if not fired and source_reads == 3 and case in ('source-mutation', 'source-substitution'):
                fired = True
                if case == 'source-substitution':
                    compiler.rename(cache / 'retained-compiler')
                compiler.write_bytes(foreign)
                compiler.chmod(0o755)
        return real_read(fd, count)
    def intercept_write(fd, data):
        global fired
        count = real_write(fd, data)
        if case == 'destination-substitution' and not fired:
            fired = True
            destination.rename(destination.with_name('retained-publication'))
            destination.write_bytes(foreign)
            destination.chmod(0o500)
        return count
    with ExitStack() as stack:
        stack.enter_context(patch.object(capture, 'COMPILED', compiler))
        stack.enter_context(patch.object(capture, 'revision', return_value=revision))
        stack.enter_context(patch.object(capture.os, 'read', side_effect=intercept_read))
        stack.enter_context(patch.object(capture.os, 'write', side_effect=intercept_write))
        if case == 'foreign-owner':
            stack.enter_context(patch.object(capture.os, 'getuid', return_value=os.getuid() + 1))
        if case == 'size-bound':
            stack.enter_context(patch.object(capture, 'CODE_LIMIT', len(original) - 1))
        try:
            with capture.compiled_code() as (fd, verify, pin, actual):
                binary = capture.publish_code(run, fd, verify, pin, actual)
        except (ValueError, OSError):
            assert case != 'success', case
            assert not (run / 'native-review-code.json').exists()
        else:
            assert case == 'success', case
            info = binary.stat()
            assert info.st_uid == os.getuid() and info.st_nlink == 1
            assert stat.S_IMODE(info.st_mode) == 0o500
            assert binary.read_bytes() == compiler.read_bytes() == original
            assert capture.code_signature(compiler.stat()) == capture.code_signature(before)
            assert compiler.stat().st_nlink == 2
            code = {'revision': revision, 'build_profile': 'dev', 'executable': actual}
            record = capture.published_code(run, code)
            assert record['compiled'] == record['executable'] == actual == capture.identity(binary)
            assert record['compiler_pin'] == pin
            # Ordinary input identity must continue to refuse Cargo's hardlink.
            try:
                capture.identity(compiler)
            except ValueError:
                pass
            else:
                raise AssertionError('input hardlink guard changed')
            for change in ('mode', 'hardlink', 'bytes'):
                if change == 'mode':
                    binary.chmod(0o700)
                elif change == 'hardlink':
                    binary.chmod(0o500)
                    os.link(binary, run / 'extra-code-link')
                else:
                    (run / 'extra-code-link').unlink()
                    binary.chmod(0o700)
                    binary.write_bytes(foreign)
                    binary.chmod(0o500)
                try:
                    capture.published_code(run, code)
                except (ValueError, OSError):
                    pass
                else:
                    raise AssertionError('changed publication admitted: ' + change)
    if case in ('source-mutation', 'source-substitution', 'destination-substitution'):
        assert fired, case
    if case == 'occupied':
        assert destination.read_bytes() == b'keep'
    if case == 'destination-substitution':
        assert destination.read_bytes() == foreign
PY
  [ "$status" -eq 0 ]
}

@test "native review derivation invokes the published CLI with the exact runtime inputs" {
  run python3 - "$SB" <<'PY'
import importlib.util
import json
import os
import sys
from pathlib import Path
from unittest.mock import patch

root = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('review_capture', root / 'tools/scripts/native-media-review-capture.py')
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
run = root / 'build/mg/ui-test/actual-code-path'
run.mkdir(parents=True)
compiler = root / 'compiler-fixture/meguri-cli'
compiler.parent.mkdir()
compiler.write_bytes(b'own compiled CLI fixture')
compiler.chmod(0o755)
os.link(compiler, compiler.with_name('deps-link'))
revision = 'a' * 40
calls = []
def execute(args, **kwargs):
    calls.append(args)
    binary = run / 'run-code/meguri-cli'
    assert args == [str(binary), 'internal-native-review', str(root), run.name, 'mac',
                    revision, '101', '102', '103', '104']
    assert kwargs == {'check': True}
    assert binary.stat().st_nlink == 1 and binary.stat().st_uid == os.getuid()
    record = capture.read(run / 'native-review-code.json')
    assert record['compiled'] == capture.identity(binary)
    expected = run / 'expected-replay'
    expected.mkdir()
    capture.write(expected / 'input.json', {'code': {'revision': revision, 'build_profile': 'dev',
                  'executable': capture.identity(binary)}})
with patch.object(capture, 'COMPILED', compiler), \
     patch.object(capture, 'Path', side_effect=lambda value: root if value == '/work' else Path(value)), \
     patch.object(capture, 'revision', return_value=revision), \
     patch.object(capture.subprocess, 'run', side_effect=execute):
    capture.derive(run, 'mac', revision)
assert len(calls) == 1
assert capture.read(run / 'native-review-rust.json') == capture.read(run / 'expected-replay/input.json')['code']
assert compiler.stat().st_nlink == 2 and compiler.read_bytes() == b'own compiled CLI fixture'
PY
  [ "$status" -eq 0 ]
}

@test "native review Mac capture filters transformed XCTest names and refuses foreign attachment proof" {
  run python3 - "$SB/tools/scripts/native-media-review-capture.py" "$BATS_TEST_TMPDIR/review-attachments" <<'PY'
import fnmatch
import importlib.util
import json
import sys
from copy import deepcopy
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('review_capture', sys.argv[1])
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
base = Path(sys.argv[2])
filename = 'Meguri-Media-' + '2' * 32 + '.colpkg'
names = [filename, 'native-review-app.json', 'native-review-import.json',
         'native-review-accepted.json', 'native-review-closed.json', 'native-review-reopened.json']
human_uuid = '00000000-0000-4000-8000-000000000008'
file_uuid = '00000000-0000-4000-8000-000000000007'

for case in ('success', 'attestation-only', 'iphone-attestation-only', 'wrong-method',
             'wrong-name', 'wrong-index', 'invalid-human-uuid', 'invalid-file-uuid',
             'extra-attachment', 'extra-record', 'extra-file', 'failure', 'missing-failure',
             'empty-manifest', 'occupied-directory'):
    run = base / case
    output = run / 'native-review-export'
    output.mkdir(parents=True)
    attestation_only = case in ('attestation-only', 'iphone-attestation-only')
    platform = 'iphone' if case == 'iphone-attestation-only' else 'mac'
    bundle = run / (platform + '.xcresult')
    bundle.mkdir()
    binding = {'platform': platform, 'native_filename': filename, 'case': case}
    (run / 'native-review-binding.json').write_text(json.dumps(binding))
    payloads = {name: json.dumps(binding if name == 'native-review-app.json'
                                else {'receipt': name}).encode() for name in names}
    payloads[filename] = b'owned native package'
    archive = []
    for name in names:
        path = Path(name)
        entry = {'exportedFileName': file_uuid + path.suffix,
                 'suggestedHumanReadableName': path.stem + '_0_' + human_uuid + path.suffix,
                 'isAssociatedWithFailure': False}
        archive.append((capture.METHOD, entry, payloads[name]))
        # These share extensions or near-matching stems but belong to no owned filename.
        for stem in ('prefix-' + path.stem, path.stem + '-other', 'unrelated'):
            foreign = dict(entry, suggestedHumanReadableName=stem + '_0_' + human_uuid + path.suffix)
            archive.append((capture.METHOD, foreign, b'foreign attachment'))
        archive.append(('OtherTests/testOther()', entry, b'foreign method'))

    calls = []
    expected_names = ['native-review-app.json'] if attestation_only else names
    if case == 'occupied-directory':
        occupied = run / ('native-review-attachments-' + Path(filename).stem)
        occupied.mkdir()
        (occupied / 'preserved').write_bytes(b'failed capture evidence')

    def export(args, **kwargs):
        index = len(calls)
        calls.append(args)
        name = expected_names[index]
        directory = run / ('native-review-attachments-' + Path(name).stem)
        assert args[:10] == ['xcrun', 'xcresulttool', 'export', 'attachments', '--path', str(bundle),
                            '--output-path', str(directory), '--test-id', capture.METHOD]
        assert len(args) == 12 and args[10] == '--filter'
        assert kwargs == {'check': True, 'stdout': capture.subprocess.DEVNULL,
                          'stderr': capture.subprocess.DEVNULL}
        assert not list(directory.iterdir())
        # Model xcresulttool's filename glob against XCTest's transformed human name,
        # never against the original filename or independent exported UUID filename.
        selected = [(deepcopy(entry), data) for method, entry, data in archive
                    if method == args[9] and fnmatch.fnmatchcase(entry['suggestedHumanReadableName'], args[11])]
        manifest = [{'testIdentifier': args[9], 'attachments': [entry for entry, _ in selected]}] if selected else []
        assert len(selected) == 1, (case, args[11], selected)
        entry = manifest[0]['attachments'][0]
        if case == 'wrong-method':
            manifest[0]['testIdentifier'] = 'OtherTests/testOther()'
        elif case == 'wrong-name':
            entry['suggestedHumanReadableName'] = 'prefix-' + entry['suggestedHumanReadableName']
        elif case == 'wrong-index':
            entry['suggestedHumanReadableName'] = entry['suggestedHumanReadableName'].replace('_0_', '_1_')
        elif case == 'invalid-human-uuid':
            entry['suggestedHumanReadableName'] = entry['suggestedHumanReadableName'].replace(human_uuid, 'not-a-uuid')
        elif case == 'invalid-file-uuid':
            entry['exportedFileName'] = 'not-a-uuid' + Path(name).suffix
        elif case == 'extra-attachment':
            manifest[0]['attachments'].append(dict(entry))
        elif case == 'extra-record':
            manifest.append({'testIdentifier': 'OtherTests/testOther()', 'attachments': []})
        elif case == 'failure':
            entry['isAssociatedWithFailure'] = True
        elif case == 'missing-failure':
            del entry['isAssociatedWithFailure']
        elif case == 'empty-manifest':
            manifest = []
        for attachment, data in selected:
            (directory / attachment['exportedFileName']).write_bytes(data)
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        if case == 'extra-file':
            (directory / 'unrelated-screen.png').write_bytes(b'foreign screenshot')

    try:
        with patch.object(capture.subprocess, 'run', side_effect=export):
            capture.mac_capture(run, bundle, attestation_only=attestation_only)
    except (ValueError, OSError):
        assert case not in ('success', 'attestation-only', 'iphone-attestation-only'), case
        assert not list(output.iterdir()), case
        assert len(calls) == (0 if case == 'occupied-directory' else 1), case
        if case == 'occupied-directory':
            assert (occupied / 'preserved').read_bytes() == b'failed capture evidence'
    else:
        assert case in ('success', 'attestation-only', 'iphone-attestation-only'), case
        assert {path.name for path in output.iterdir()} == set(expected_names)
        for name in expected_names:
            assert (output / name).read_bytes() == payloads[name]
        assert [args[11] for args in calls] == [Path(name).stem + '_0_*' + Path(name).suffix
                                               for name in expected_names]
        assert len(calls) == (1 if attestation_only else 6)
        assert {path.name for path in run.glob('native-review-attachments-*')} == {
            'native-review-attachments-' + Path(name).stem for name in expected_names}
PY
  [ "$status" -eq 0 ]
}

@test "native review full receipt proof binds one native R6 activation across four phases" {
  run python3 - "$SB" <<'PY'
import importlib.util
import sys
from copy import deepcopy
from pathlib import Path
from unittest.mock import patch
from uuid import UUID

root = Path(sys.argv[1])
module = importlib.util.spec_from_file_location('receipt_fixture', root / 'tools/scripts/native-media-review-capture.py')
helper = importlib.util.module_from_spec(module)
module.loader.exec_module(helper)

def uuid(number):
    return str(UUID(int=number, version=4)).upper()

# DTO-shaped generated data; proof reads, hashes and writes remain real.
counts = {'incoming': 3, 'added': 3, 'reused': 0, 'trashed': 0, 'decodedBytes': 96}
media_files = {'collection.media/' + name: {'sha256': format(n, '064x'), 'bytes': 32}
               for n, name in enumerate(helper.MEDIA, 41)}
answer = {'owner': uuid(31), 'coordinator': uuid(32), 'session': 29, 'generation': 7,
          'deck': 41, 'card': 43, 'rating': 'easy', 'elapsed': 1000,
          'wall': 2000000000000, 'cap': 60, 'token': list(range(32))}
visit = {key: answer[key] for key in ('coordinator', 'session', 'generation', 'card')}
visit.update(update=3, side='answer', document=uuid(33))
journal = {'generation': 7, 'root': '/fixture/collection/collection.media',
           'names': [{'name': name, 'caseKey': name.lower()} for name in helper.MEDIA]}
activation = {'generation': 7, 'media': counts, 'backup': None, 'retention': 'r6-' + uuid(35)}
template = {
    'version': 1, 'platform': 'mac', 'failure': None, 'collection': '/fixture/collection',
    'inspection': {'sha256': 'f' * 64, 'notes': 1, 'cards': 1, 'reviews': 2,
                   'schema': 18, 'replacing': False, 'mediaCount': 3, 'mediaBytes': 96},
    'activation': activation, 'prepared': counts, 'journal': journal,
    'owner': uuid(31), 'admission': answer, 'completion': 'accepted', 'ended': False,
    'selectedDeck': 41, 'selectedDeckName': 'Deck 41', 'selectionCalls': 1,
    'image': {'binding': visit, 'name': 'meguri-review.png', 'width': 4, 'height': 4,
              'x': 0, 'y': 0, 'displayWidth': 4, 'displayHeight': 4},
    'audio': {'binding': visit, 'name': 'meguri-review.wav', 'index': 0,
              'success': True, 'samples': 24, 'duration': 2,
              'firstPosition': .1, 'lastPosition': 1.9,
              'firstObservedAt': 20, 'lastObservedAt': 21.8, 'completedAt': 22},
    'closedFiles': None, 'seeds': {'inspection_and_initial_activation': 103}}
phases = {phase: deepcopy(template) for phase in helper.PHASES}
for phase, receipt in phases.items():
    receipt['phase'] = 'activated' if phase == 'import' else phase
phases['import'].update(admission=None, completion=None, image=None, audio=None,
                        selectedDeck=None, selectedDeckName=None, selectionCalls=0)
phases['closed'].update(ended=True, closedFiles={
    **media_files, 'collection.anki2': {'sha256': 'e' * 64, 'bytes': 768}})
phases['reopened'].update(ended=True, owner=uuid(34), reopenedJournal=deepcopy(journal),
                          seeds={'inspection_and_initial_activation': 103, 'reopen': 104})

scenarios = []
def add(label, succeeds, change=lambda records: None, replay='r6-' + uuid(36)):
    records = deepcopy(phases)
    change(records)
    scenarios.append((label, succeeds, records, replay))

def references(records, reference):
    for receipt in records.values():
        receipt['activation']['retention'] = reference

add('replay-differs', True)
add('replay-equals', True, replay=activation['retention'])
# R6 suffixes are opaque; these are components, never filesystem paths.
for number, reference in enumerate(('r6-', 'r6-λ 47.\n', 'r6-' + 'z' * 4096)):
    add('component-valid-' + str(number), True,
        lambda records, ref=reference: references(records, ref))
for number, reference in enumerate((None, True, 47, [], {}, '', '.', '..', uuid(37),
                                     'r6-/47', 'r6-\\47', 'r6-\0' + '47', '../r6-47')):
    add('component-invalid-' + str(number), False,
        lambda records, ref=reference: references(records, ref))

def remove_references(records):
    for receipt in records.values():
        del receipt['activation']['retention']
add('reference-absent', False, remove_references)

for phase in helper.PHASES[1:]:
    def replace_reference(records, target=phase):
        records[target]['activation']['retention'] = 'r6-' + uuid(38)
    def remove_reference(records, target=phase):
        del records[target]['activation']['retention']
    def replace_generation(records, target=phase):
        # Keep the local journal valid so continuity must reject this activation.
        records[target]['activation']['generation'] += 1
        records[target]['journal']['generation'] += 1
    add(phase + '-reference-changed', False, replace_reference)
    add(phase + '-reference-absent', False, remove_reference)
    add(phase + '-generation-changed', False, replace_generation)

# Four existing proof boundaries suffice alongside the new R6 coverage.
add('phase-owner', False, lambda records: records['closed'].update(owner=uuid(39)))
add('reopen-seed', False, lambda records: records['reopened']['seeds'].update(reopen=105))
add('media-hash', False, lambda records: records['closed']['closedFiles'][
    'collection.media/meguri-review.png'].update(sha256='d' * 64))
def audio_incomplete(records):
    for phase in helper.PHASES[1:]:
        evidence = records[phase]['audio']
        evidence['completedAt'] = evidence['lastObservedAt']
add('audio-completion', False, audio_incomplete)

# Shared JSON reader bound only (16 MiB). Native phase/transport has its
# separate 128 KiB ceiling, covered by the existing native tests.
def oversized_record(records):
    records['import']['activation']['retention'] = 'r6-' + 'z' * (16 * 1024 * 1024)
add('json-reader-bound', False, oversized_record)

for number, (label, succeeds, records, replay) in enumerate(scenarios):
    run = root / 'receipt-fixture' / str(number)
    output = run / 'native-review-export'
    frozen = run / 'expected-replay'
    for directory in (output, frozen, run / 'source-preparation'):
        directory.mkdir(parents=True, mode=0o700)
    inputs = {'run': 'fixture/' + str(number), 'platform': 'mac',
              'package': {'identity': {'sha256': 'f' * 64}}, 'prepared_files': media_files,
              'chosen': {'deck_id': 41, 'card_id': 43, 'deck_name': 'Deck 41'},
              'wall': {'now_ms': 2000000000000}, 'answer': {'preset_cap_answer_time_secs': 60}}
    helper.write(frozen / 'input.json', inputs)
    helper.write(frozen / 'replay.json', {'media_retention_reference': replay})
    helper.write(frozen / 'freeze.json', {'files': {
        name: helper.identity(frozen / name) for name in ('input.json', 'replay.json')}})
    frozen_before = {path: helper.identity(path) for path in frozen.iterdir()}
    binding = {'freeze': frozen_before[frozen / 'freeze.json'],
               'input': frozen_before[frozen / 'input.json'],
               'native_filename': 'Meguri-Media-' + UUID(uuid(40)).hex + '.colpkg'}
    helper.write(run / 'source-preparation/preparation.json', {
        'source_facts': {'row_counts': {'revlog': 2}, 'media_bytes': 96}})
    helper.write(run / 'native-review-binding.json', binding)
    helper.write(output / 'native-review-app.json', binding)
    (output / binding['native_filename']).write_bytes(b'fixture 40')
    for phase, receipt in records.items():
        receipt.update(run=inputs['run'], freezeIdentity=binding['freeze'], input=binding['input'])
        helper.write(output / ('native-review-' + phase + '.json'), receipt)
    phase_before = {phase: helper.identity(output / ('native-review-' + phase + '.json'))
                    for phase in helper.PHASES}
    try:
        # Only unrelated frozen-source/build authority is replaced.
        with patch.object(helper, 'recheck', return_value=(inputs, binding)):
            helper.proof_receipts(run)
    except (ValueError, OSError, KeyError, TypeError) as error:
        assert not succeeds, (label, str(error))
        assert not (run / 'native-review-capture.json').exists(), label
        if label == 'json-reader-bound':
            assert str(error) == 'record too large', str(error)
    else:
        assert succeeds, label
        proof = helper.read(run / 'native-review-capture.json')
        assert proof['receipts'] == phase_before, label
        assert proof['package'] == helper.identity(output / binding['native_filename']), label
        assert proof['external_content_client_proof'] == 'pending', label
    assert {path: helper.identity(path) for path in frozen.iterdir()} == frozen_before, label
    assert {phase: helper.identity(output / ('native-review-' + phase + '.json'))
            for phase in helper.PHASES} == phase_before, label
PY
  [ "$status" -eq 0 ]
}

@test "native review frozen helper accepts retained Rust work but keeps expected inputs exact" {
  run python3 - "$SB" <<'PY'
import importlib.util
import json
import os
import sys
from pathlib import Path
from unittest.mock import patch

root = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('review_capture', root / 'tools/scripts/native-media-review-capture.py')
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)
run = root / 'build/mg/ui-test/layout-case'
expected = run / 'expected-replay'
prepared = run / 'source-preparation/fixture'
parent = root / 'build/fixtures/recipes/local-review-1'
revision = 'a' * 40

def put(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)

def record(path, value):
    put(path, json.dumps(value, sort_keys=True).encode())

def source(directory, media):
    (directory / 'collection.media').mkdir(parents=True)
    names = ['collection.anki2', 'manifest.json'] + ['collection.media/' + name for name in media]
    for name in names:
        put(directory / name, ('layout fixture ' + name).encode())
    return {name: capture.identity(directory / name) for name in names}

parent_ids = source(parent, [])
prepared_ids = source(prepared, capture.MEDIA)
put(root / 'app/Fixtures/CollectionEntry/EmptyCollection/.gitkeep', b'')
rust_source = root / 'core/crates/meguri-testkit/src/native_review.rs'
put(rust_source, b'own source identity fixture')
preparation = run / 'source-preparation/preparation.json'
record(preparation, {'layout': 'before-launch'})
put(expected / 'input.colpkg', b'package identity fixture')
put(expected / 'replay.json', b'replay identity fixture')
for stage in ('source', 'import', 'review'):
    source(expected / stage, capture.MEDIA)
relative = run.relative_to(root).as_posix()
value = {
    'run': relative, 'platform': 'mac',
    'code': {'build_profile': 'dev', 'revision': revision, 'replay_source': capture.identity(rust_source)},
    'preparation': {'path': relative + '/source-preparation/preparation.json', 'identity': capture.identity(preparation)},
    'parent_source': 'build/fixtures/recipes/local-review-1', 'parent_files': parent_ids,
    'prepared_source': relative + '/source-preparation/fixture', 'prepared_files': prepared_ids,
    'package': {'path': relative + '/expected-replay/input.colpkg'},
    'seeds': {'source_open': 101, 'package_write': 102, 'inspection_and_initial_activation': 103, 'reopen': 104},
    'answer': {'rating': 'easy', 'elapsed_ms': 1000, 'count': 1, 'preset_cap_answer_time_secs': 60},
    'no_old_database': True,
}
record(expected / 'input.json', value)
names = ['input.json', 'input.colpkg', 'replay.json']
for stage in ('source', 'import', 'review'):
    names += [stage + '/collection.anki2', stage + '/manifest.json']
    names += [stage + '/collection.media/' + name for name in capture.MEDIA]
frozen_ids = {name: capture.identity(expected / name) for name in names}
record(expected / 'freeze.json', {'schema_version': 1, 'run': relative, 'platform': 'mac',
                                'stage': 'expected-states-before-app', 'files': frozen_ids})

def refuses():
    try:
        capture.frozen(run)
    except (ValueError, OSError):
        return
    raise AssertionError('invalid layout admitted')

with patch.object(capture, 'ROOT', root), patch.object(capture, 'revision', return_value=revision):
    initial_value, initial_files = capture.frozen(run)
    assert initial_value == value and len(frozen_ids) == 18
    work = expected / 'work'
    work.mkdir(mode=0o700)
    put(work / 'collection.anki2', b'producer scratch, outside expected input')
    (work / 'unread').symlink_to(root / 'absent-scratch-target')
    assert capture.frozen(run) == (initial_value, initial_files)
    assert not any('/work/' in path for path in initial_files)
    put(work / 'collection.anki2', b'changed scratch')
    assert capture.frozen(run) == (initial_value, initial_files)

    with patch.object(capture.os, 'getuid', return_value=os.getuid() + 1):
        refuses()
    work.chmod(0o722)
    refuses()
    work.chmod(0o700)
    retained = run / 'retained-work'
    work.rename(retained)
    work.symlink_to(retained, target_is_directory=True)
    refuses()
    work.unlink()
    put(work, b'not a directory')
    refuses()
    work.unlink()
    retained.rename(work)

    for extra in (expected / 'other-work', expected / 'review/collection.media/extra'):
        put(extra, b'unexpected')
        refuses()
        extra.unlink()
    put(expected / 'review/collection.anki2', b'changed frozen bytes')
    refuses()
    put(expected / 'review/collection.anki2', b'layout fixture collection.anki2')
    assert capture.frozen(run) == (initial_value, initial_files)
    put(parent / 'collection.anki2', b'changed parent bytes')
    refuses()
PY
  [ "$status" -eq 0 ]
}

@test "personal preview permits typed readiness after verified development setup" {
  personal_facts
  jq 'map(.buildSettings.MEGURI_CLOUD_SCHEMA_CONFIGURED_development = "YES"
    | .buildSettings.MEGURI_CLOUD_SCHEMA_CONFIGURED = "YES"
    | .buildSettings.MEGURI_CLOUD_SCHEMA_BOOLEAN = "1")' "$FX_PERSONAL_SETTINGS" >"$BATS_TEST_TMPDIR/ready-settings.json"
  mv "$BATS_TEST_TMPDIR/ready-settings.json" "$FX_PERSONAL_SETTINGS"
  jq '.MeguriCloudSchemaConfigured = true' "$FX_CLOUD_INFO" >"$BATS_TEST_TMPDIR/ready-info.json"
  mv "$BATS_TEST_TMPDIR/ready-info.json" "$FX_CLOUD_INFO"
  mg build-app --platform mac --personal-preview
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
}

# Drive fixtures replace only attachment export and the image-tools checker;
# destination selection, team build, test invocation and result checks stay real.
physical_drive_facts() {
  mkdir -p "$SB/build/drive-input"
  export FX_DRIVE_SCRIPT="$SB/build/drive-input/inspect.json"
  printf '%s\n' '{"name":"physical-inspect","steps":[{"launch":{}},{"snapshot":"initial"}]}' >"$FX_DRIVE_SCRIPT"
  echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' >"$FX_SUMMARY"
  test_tree '[{nodeType:"Test Case", nodeIdentifier:"AgentDriverTests/testDriveScript()", result:"Passed"}]'
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'FAKES'
# Preserve the production physical test helper, inspecting only transport facts.
eval "$(declare -f mg_app_xcodebuild_test | sed '1s/mg_app_xcodebuild_test/fx_original_test/')"
mg_app_xcodebuild_test() {
  [ -z "${TEST_RUNNER_MEGURI_DRIVE_SCRIPT+x}" ] || return 2
  [ "${TEST_RUNNER_MEGURI_DRIVE_REQUIRED:-}" = 1 ] || return 2
  [ "${TEST_RUNNER_MEGURI_DRIVE_FRAMES:-}" = 0 ] || return 2
  python3 - <<'PY'
import base64, os
from pathlib import Path
payload = base64.b64decode(os.environ['TEST_RUNNER_MEGURI_DRIVE_SCRIPT_BASE64'], validate=True)
assert payload == (Path(os.environ['MG_RUN_DIR']) / 'physical-script.json').read_bytes()
assert os.environ['TEST_RUNNER_MEGURI_DRIVE_KEEP_APP_RUNNING'] == ('1' if os.environ.get('FX_PRESERVE_FIXTURE') else '0')
# The original file changing after validation cannot change the delivered bytes.
if os.environ.get('FX_MUTATE_SCRIPT'):
    Path(os.environ['FX_DRIVE_SCRIPT']).write_text('changed source')
assert b'physical-inspect' in payload
PY
  [ "$?" -eq 0 ] || return 2
  echo inline-transport-verified >>"$STUB_LOG"
  fx_original_test "$@"
  local rc=$?
  if [ "${FX_PRESERVE_CASE:-}" = transport ]; then
    echo 'Connection lost to selected device' >>"$1.log"
  fi
  return "$rc"
}
mg_drive__export_attachments() {
  echo drive-export >>"$STUB_LOG"
  mkdir -p "$2"
  python3 - "$2" <<'PY'
import json, os
from pathlib import Path
import sys
out = Path(sys.argv[1])
mode = os.environ.get('FX_DRIVE_CASE', '')
log = {'script':'physical-drive','path':'','platform':'ios','steps':[],
       'kept_running':bool(os.environ.get('FX_PRESERVE_FIXTURE')),
       'activation_completed':bool(os.environ.get('FX_PRESERVE_FIXTURE'))}
attachments = []
captures = [(1, 'launch', 'after'), (2, 'snapshot', 'initial')]
if os.environ.get('FX_DRIVE_FOREGROUND_CYCLE'):
    captures = [(1, 'launch', 'after'), (2, 'foregroundCycle', 'before'), (2, 'foregroundCycle', 'after')]
for index, kind, phase in captures:
    capture_kind = 'foreground-cycle' if kind == 'foregroundCycle' else kind
    name = f'drive-{index:02}-{capture_kind}-{phase}'
    capture = {'phase':phase, 'hierarchy':name+'.json', 'screenshot':name+'.png'}
    if log['steps'] and log['steps'][-1]['index'] == index:
        log['steps'][-1]['captures'].append(capture)
    else:
        log['steps'].append({'index':index,'kind':kind,'status':'ok','captures':[capture], 'started':0,'ended':1})
    for ext in ('.json', '.png'):
        (out / (name+ext)).write_text('{}' if ext == '.json' else 'synthetic-png')
        attachments.append({'suggestedHumanReadableName':name+ext, 'exportedFileName':name+ext})
if mode == 'step-failed': log['steps'][1].update(status='failed', message='drive step failed')
if mode == 'no-captures': log['steps'][0]['captures'] = []
if mode == 'truncated-log': log['steps'] = log['steps'][:1]
if mode == 'malformed-log': log = {}
if mode == 'missing-capture': (out / 'drive-02-snapshot-initial.png').unlink()
if mode != 'missing-log':
    (out / 'drive-log.json').write_text(json.dumps(log))
    attachments.append({'suggestedHumanReadableName':'drive-log.json', 'exportedFileName':'drive-log.json'})
(out / 'manifest.json').write_text(json.dumps([{'attachments':attachments}]))
PY
}
mg_image_tools() {
  echo hierarchy-check >>"$STUB_LOG"
  [ "${FX_DRIVE_CASE:-}" != missing-hierarchy ] || return 2
  shift 2
  [ "$1" = --report ] || return 2
  local report="$2"; shift 2
  report="$MG_ROOT/${report#/work/}"
  python3 - "$report" "$@" <<'PY'
import json, os, sys
issues = []
if os.environ.get('FX_DRIVE_CASE') == 'hierarchy-failure':
    issues = [{'kind':'duplicate-id','identifier':'synthetic.id','message':'synthetic issue',
               'frame':{'x':0,'y':0,'width':10,'height':10}}]
with open(sys.argv[1], 'w') as f:
    json.dump({'files':[{'path':p,'issues':issues} for p in sys.argv[2:]]}, f)
PY
  [ "${FX_DRIVE_CASE:-}" != hierarchy-failure ]
}
FAKES
}

physical_drive() { mg drive "$FX_DRIVE_SCRIPT" --physical-device "$PHONE" --platform iphone --config Debug "$@"; }

# Exercise the real prepare/admit/complete helpers with synthetic public-tool
# responses; the Python suite separately checks sealed products and missing proof.
physical_preservation_facts() {
  physical_drive_facts
  export FX_PRESERVE_FIXTURE=1 FX_REAL_PYTHON="$(command -v python3)"
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'FAKES'
mg_app_input_digest() { printf '%064d\n' 0; }
FAKES
  cat >"$BIN/python3" <<'WRAPPER'
#!/bin/bash
if [ "$1 $2" = '-B -I' ] && [[ "$3" == */tools/scripts/physical-drive.py ]]; then
  exec "$FX_REAL_PYTHON" -B - "$@" <<'PYCODE'
import importlib.util, os, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location('physical', sys.argv[3])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
sys.argv = sys.argv[3:]
action = sys.argv[1]
root = Path(sys.argv[sys.argv.index('--root') + 1])
with open(os.environ['STUB_LOG'], 'a') as log:
    log.write('physical-helper ' + action + '\n')
base = root / 'build/app/DerivedData/Build/Products'
runner = base / 'Debug-iphoneos/MeguriUITests-Runner.app'
artifacts = {'products': {kind: {'path': str(path), 'build': '1'} for kind, path in (
    ('app', base / 'Debug-iphoneos/Meguri.app'), ('runner', runner),
    ('test', runner / 'PlugIns/MeguriUITests.xctest'))},
    'xctestrun': str(base / 'Meguri_UI_iphoneos27.0-arm64.xctestrun')}
generated = {'MeguriUITests': {'BlueprintName':'MeguriUITests','IsUITestBundle':True,
    'TestHostBundleIdentifier':module.RUNNER, 'ProductModuleName':'MeguriUITests',
    'TestingEnvironmentVariables':{'SYNTHETIC_METADATA':'retained'}}}
module.local_artifacts = lambda *_: (artifacts, generated)
def query(device, output, arguments):
    case = os.environ.get('FX_PRESERVE_CASE', '')
    if arguments[:3] == ['device','install','app']:
        with open(os.environ['STUB_LOG'], 'a') as log: log.write('bootstrap-install\n')
        return {}
    if arguments[:3] == ['device','process','launch']:
        with open(os.environ['STUB_LOG'], 'a') as log: log.write('normal-final-launch\n')
        return {'process':{'processIdentifier':17}}
    if arguments[:3] == ['device','info','apps']:
        identifier = module.RUNNER if module.RUNNER in arguments[-1] else module.APP
        url = 'file:///private/synthetic/' + ('Runner.app/' if identifier == module.RUNNER else 'Meguri.app/')
        if action == 'complete' and case == 'runner-changed' and identifier == module.RUNNER:
            url = 'file:///private/replaced/Runner.app/'
        return {'apps':[{'bundleIdentifier':identifier,'bundleVersion':'1','builtByDeveloper':True,'url':url}]}
    if arguments[:3] == ['device','info','processes']:
        if action == 'complete' and case == 'process-missing': return {'runningProcesses':[]}
        return {'runningProcesses':[{'processIdentifier':17,'executable':'file:///private/synthetic/Meguri.app/Meguri'}]}
    raise AssertionError('unexpected synthetic query')
module.device_query = query
result = module.main()
# Faults occur after the real completion helper succeeds, at the publication
# boundary. They must not be mistaken for successful but null jq extractions.
if action == 'complete' and result == 0:
    case = os.environ.get('FX_PUBLICATION_CASE', '')
    evidence = Path(sys.argv[sys.argv.index('--output') + 1]) / 'evidence.json'
    if case == 'missing-evidence': evidence.unlink()
    elif case == 'invalid-json': evidence.write_text('PRIVATE_COMPLETION_SENTINEL')
    elif case == 'empty-evidence': evidence.write_text('')
    elif case == 'null-evidence': evidence.write_text('null')
    elif case == 'multiple-evidence': evidence.write_text(evidence.read_text() + '\nnull')
    elif case in ('missing-field', 'wrong-type', 'unproved-completion', 'unknown-continuity'):
        value = module.read_json(evidence)
        if case == 'missing-field': del value['continuity']
        elif case == 'wrong-type': value['activated_existing'] = 'true'
        elif case == 'unproved-completion': value['installed_test_bundle_verified'] = False
        elif case == 'unknown-continuity': value['continuity']['status'] = 'unknown'
        evidence.write_text(module.json.dumps(value))
sys.exit(result)
PYCODE
fi
exec "$FX_REAL_PYTHON" "$@"
WRAPPER
  chmod +x "$BIN/python3"
  # The production physical guard reads this generated development identity.
  mg run --physical-device "$PHONE" --platform iphone --config Debug
  [ "$status" -eq 0 ]
  called '^bootstrap-install$'
  called '^normal-final-launch$'
  : >"$STUB_LOG"
}

# Capture the receipt inputs before publication, then inject only the requested
# synthetic I/O/export fault. All test execution and completion remain real seams.
physical_publication_facts() {
  physical_preservation_facts
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'FAKES'
eval "$(declare -f mg_drive__job | sed '1s/mg_drive__job/fx_publication_original_job/')"
mg_drive__job() {
  fx_publication_original_job "$@"
  jq -s '{total:0, passed:0, failed:0, skipped:0} + (add // {})' "$MG_STATE/counts.jsonl" >"$MG_RUN_DIR/pre-completion-counts.json"
  jq -s '.' "$MG_STATE/failures.jsonl" >"$MG_RUN_DIR/pre-completion-failures.json"
  FX_PUBLICATION_READY=1
  case "${FX_PUBLICATION_CASE:-}" in
    merge) printf 'null\n' >"$MG_RUN_DIR/manifest.json" ;;
    detail-parse)
      jq '.steps = null' "$MG_RUN_DIR/manifest.json" >"$MG_RUN_DIR/synthetic-manifest.json"
      command mv "$MG_RUN_DIR/synthetic-manifest.json" "$MG_RUN_DIR/manifest.json"
      ;;
  esac
}
mv() {
  if [ "${FX_PUBLICATION_CASE:-}" = rename ] && [ "$1" = "$MG_RUN_DIR/manifest-completed.json" ]; then
    echo completion-rename-refused >>"$STUB_LOG"
    return 1
  fi
  command mv "$@"
}
eval "$(declare -f mg_detail | sed '1s/mg_detail/fx_publication_original_detail/')"
mg_detail() {
  if [ "${FX_PUBLICATION_READY:-0}" = 1 ]; then
    case "${FX_PUBLICATION_CASE:-}/$1" in
      detail-drive/drive|detail-activation/activated_existing|detail-continuity/continuity)
        echo completion-detail-refused >>"$STUB_LOG"
        return 1
        ;;
    esac
  fi
  fx_publication_original_detail "$@"
}
FAKES
}

physical_publication_failed() {
  [ "$status" -ne 0 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[] | select(.id == "physical-completion-publication")] | length')" -eq 1 ]
  local dir="${result%/result.json}"
  jq -e --slurpfile counts "$dir/pre-completion-counts.json" \
    --slurpfile failures "$dir/pre-completion-failures.json" '
    .summary == $counts[0] and
    .failures[:($failures[0] | length)] == $failures[0] and
    (.failures | length) == (($failures[0] | length) + 1)' "$result"
  [ -f "$dir/physical/admission.json" ]
  [ -f "$dir/physical-completion.log" ]
  [ -f "$dir/drive-log.json" ]
  called '^physical-helper complete$'
  not_called 'build-for-testing|bootstrap-install|normal-final-launch'
  [[ "$output$stderr$(cat "$result")" != *PRIVATE_COMPLETION_SENTINEL* ]]
}

@test "physical preservation completion rename failure fails the receipt and retains earlier counts failures and private evidence" {
  physical_publication_facts
  local mode dir
  for mode in '' step-failed; do
    FX_PUBLICATION_CASE=rename FX_DRIVE_CASE="$mode" physical_drive --keep-app-running
    physical_publication_failed
    dir="${result%/result.json}"
    [ -f "$dir/physical/evidence.json" ]
    [ -f "$dir/manifest-completed.json" ]
    jq -e '.activated_existing == false and .continuity == null' "$dir/manifest.json"
    jq -e '.activated_existing == true and .continuity.status == "verified"' "$dir/manifest-completed.json"
    if [ "$mode" = step-failed ]; then
      [ "$(jq 'length' "$dir/pre-completion-failures.json")" -gt 0 ]
      [ "$(field .summary.failed)" -gt 0 ]
    fi
  done
  called '^completion-rename-refused$'
}

@test "physical preservation completion missing malformed or incomplete evidence cannot pass despite helper success" {
  physical_publication_facts
  local mode
  for mode in missing-evidence invalid-json empty-evidence null-evidence multiple-evidence \
      missing-field wrong-type unproved-completion unknown-continuity; do
    FX_PUBLICATION_CASE="$mode" physical_drive --keep-app-running
    physical_publication_failed
    [ "$(field .details.activated_existing)" = false ]
    [ "$(field .details.continuity)" = unknown ]
  done
}

@test "physical preservation completion merge parse and refreshed detail export failures fail the receipt" {
  physical_publication_facts
  local mode
  for mode in merge detail-parse detail-drive detail-activation detail-continuity; do
    FX_PUBLICATION_CASE="$mode" physical_drive --keep-app-running
    physical_publication_failed
    [ -f "${result%/result.json}/physical/evidence.json" ]
  done
  called '^completion-detail-refused$'
}

@test "physical preservation executes installed destination artifacts and completes both brackets without preparation" {
  physical_preservation_facts
  physical_drive --keep-app-running
  [ "$status" -eq 0 ]
  [ "$(field '.details.kept_running')" = true ]
  [ "$(field '.details.activated_existing')" = true ]
  [ "$(field '.details.continuity')" = verified ]
  [ "$(field '.details.drive.device')" = null ]
  called '^physical-helper admit$'
  called '^physical-helper complete$'
  called '^xcodebuild test-without-building -xctestrun .*physical/destination.xctestrun -destination platform=iOS,id='
  not_called 'build-for-testing|bootstrap-install|normal-final-launch|allowProvisioning| -project | -scheme | -testPlan |^(lint|xcframework|sim-|vm-|open)'
  local destination="${result%/result.json}/physical/destination.xctestrun"
  "$FX_REAL_PYTHON" - "$destination" <<'PYCODE'
import plistlib, sys
with open(sys.argv[1], 'rb') as stream: target = plistlib.load(stream)['MeguriUITests']
assert target['UseDestinationArtifacts'] is True
assert target['TestBundleDestinationRelativePath'] == '__TESTHOST__/PlugIns/MeguriUITests.xctest'
assert target['TestingEnvironmentVariables'] == {'SYNTHETIC_METADATA':'retained'}
assert not any(k in target for k in ('TestBundlePath','TestHostPath','UITargetAppPath'))
PYCODE
  # A subsequent inspection still uses the original normal-launch witness.
  physical_drive --keep-app-running
  [ "$status" -eq 0 ]
  [ "$(field '.details.continuity')" = verified ]
  [[ "$stderr" != *"$PHONE"* ]]
  [[ "$stderr" != *'/private/synthetic'* ]]
}

@test "physical preservation retains continuity after a failed selected step capture hierarchy or XCTest without passing the drive" {
  physical_preservation_facts
  local mode
  for mode in step-failed missing-capture hierarchy-failure; do
    FX_DRIVE_CASE="$mode" physical_drive --keep-app-running
    [ "$status" -ne 0 ]
    [ "$(field '.details.continuity')" = verified ]
    [ "$(field '.details.activated_existing')" = true ]
  done
  echo '{"result":"Failed","passedTests":0,"failedTests":1,"skippedTests":0}' >"$FX_SUMMARY"
  test_tree '[{nodeType:"Test Case",nodeIdentifier:"AgentDriverTests/testDriveScript()",result:"Failed"}]'
  FX_XCODE_RC=1 physical_drive --keep-app-running
  [ "$status" -ne 0 ]
  [ "$(field '.details.continuity')" = verified ]
  called '^physical-helper complete$'
  not_called 'build-for-testing|bootstrap-install|normal-final-launch'
}

@test "physical preservation cannot pass missing completion process changed runner installation or interrupted transport" {
  physical_preservation_facts
  local mode
  for mode in process-missing runner-changed transport; do
    FX_PRESERVE_CASE="$mode" physical_drive --keep-app-running
    [ "$status" -ne 0 ]
    [ "$(field '.details.activated_existing')" = false ]
    [ "$(field '.details.continuity')" != verified ]
  done
  not_called 'build-for-testing|bootstrap-install|normal-final-launch'
}

@test "physical preservation public metadata destination contract and continuity seam tests" {
  run python3 -B "$SB/tools/tests/physical_drive_test.py"
  [ "$status" -eq 0 ]
}

@test "physical preservation rejects unsupported opt-ins and launch policy before any work" {
  physical_drive_facts
  local args
  for args in '--platform mac' '--config Profile' '--record' '--device fake'; do
    : >"$STUB_LOG"
    physical_drive --keep-app-running $args
    [ "$status" -ne 0 ]
    not_called '^(lint|xcframework|host-lease|sim-|vm-|xcodebuild|xcrun)'
  done
  mg drive "$FX_DRIVE_SCRIPT" --keep-app-running
  [ "$status" -ne 0 ]
  mg run --keep-app-running --physical-device "$PHONE"
  [ "$status" -ne 0 ]
  printf '%s\n' '{"name":"x","steps":[{"launch":{"arguments":[]}}]}' >"$FX_DRIVE_SCRIPT"
  physical_drive --keep-app-running
  [ "$status" -ne 0 ]
  not_called '^(lint|xcframework|host-lease|sim-|vm-|xcodebuild|xcrun)'
}

@test "physical preservation missing trusted preparation refuses without build installation or runner" {
  physical_drive_facts
  physical_drive --keep-app-running
  [ "$status" -ne 0 ]
  [ "$(field '.failures[0].id')" = physical-preservation ]
  [ "$(field '.details.kept_running')" = true ]
  [ "$(field '.details.activated_existing')" = false ]
  [ "$(field '.details.continuity')" = unknown ]
  not_called '^(lint|xcframework|host-lease|sim-|vm-|xcodebuild|xcrun)'
  [[ "$stderr" != *"$PHONE"* ]]
  [[ "$stderr" != *physical-inspect* ]]
}

@test "physical normal run refuses launch overrides before discovery or build" {
  local args
  for args in '--route sample/sheet' '--window-size default' '-- -MeguriFixture private'; do
    mg run --physical-device "$PHONE" --platform iphone --config Debug $args
    [ "$status" -ne 0 ]
    not_called '^(lint|xcframework|host-lease|sim-|vm-|xcodebuild|xcrun)'
  done
}

@test "physical drive routes frozen inline bytes through team Debug and exact selected test without VM or simulator" {
  physical_drive_facts
  FX_MUTATE_SCRIPT=1 TEST_RUNNER_MEGURI_DRIVE_KEEP_APP_RUNNING=1 TEST_RUNNER_MEGURI_DRIVE_SCRIPT=/private/host-path physical_drive
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.drive.hierarchy_checked)" = true ]
  called '^host-lease --no-session$'
  called '^xcodebuild build-for-testing .* -configuration Debug -destination platform=iOS,id=.* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration'
  called '^xcodebuild test-without-building .* -destination platform=iOS,id=.* -only-testing:MeguriUITests/AgentDriverTests/testDriveScript$'
  called '^inline-transport-verified$'
  called '^hierarchy-check$'
  not_called '^(sim-|vm-|open)|xcrun (simctl|devicectl)|physical-script.base64|private/host-path'
  [ "$(cat "$FX_DRIVE_SCRIPT")" = 'changed source' ]
}

@test "physical drive rejects unsupported modes conflicts and malformed identifiers before discovery" {
  physical_drive_facts
  local args
  for args in '--device fake' '--platform mac' '--platform iphone-small' '--config Profile' '--config Release' '--record' '--physical-device bad'; do
    : >"$STUB_LOG"
    # Intentional splitting of fixed synthetic options.
    physical_drive $args
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ ! -s "$STUB_LOG" ]
  done
  mg drive "$FX_DRIVE_SCRIPT" --physical-device invalid
  [ "$status" -eq 2 ]
  mg drive --physical-device "$PHONE"
  [ "$status" -eq 2 ]
  mg ui-tree --physical-device "$PHONE" --route sample/sheet
  [ "$status" -eq 2 ]
  mg screenshot --physical-device "$PHONE" --route sample/sheet
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

@test "recorded foreground cycles refuse at the common drive entry before discovery runner or recorder without payload leaks" {
  physical_drive_facts
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'FAKES'
# Bypass the CLI to exercise the entry used by callers such as mac-ui.
mg_job foreground-cycle-entry host fx_foreground_cycle_entry "Synthetic common drive entry"
fx_foreground_cycle_entry() { mg_drive__job "$1" "$2" "" Debug yes; }
mg_app_build() { echo unexpected-build >>"$STUB_LOG"; return 1; }
mg_mac_ui_begin() { echo unexpected-mac-discovery >>"$STUB_LOG"; return 1; }
mg_sim_lease() { echo unexpected-sim-lease >>"$STUB_LOG"; return 1; }
mg_drive_run() { echo unexpected-runner >>"$STUB_LOG"; return 1; }
mg_record__start_recorder() { echo unexpected-recorder >>"$STUB_LOG"; return 1; }
mg_drive__screen_start() { echo unexpected-screen-recorder >>"$STUB_LOG"; return 1; }
FAKES
  local step kind entry cycle documents
  local innocuous='{"name":"PRIVATE_SENTINEL","steps":[{"launch":{}}]}'
  for step in '{"foregroundCycle":true}' '{"foregroundCycle":false}' \
      '{"foregroundCycle":null}' '{"foregroundCycle":"PRIVATE_SENTINEL"}' \
      '{"foregroundCycle":true,"transition":"PRIVATE_SENTINEL"}' \
      '{"foregroundCycle":true,"tap":"PRIVATE_SENTINEL"}' \
      '{"\u0066oregroundCycle":{}}'; do
    cycle="$(printf '{"name":"PRIVATE_SENTINEL","steps":[{"launch":{}},%s]}' "$step")"
    for documents in "$cycle" "$cycle"$'\n'"$innocuous" "$innocuous"$'\n'"$cycle"; do
      printf '%s\n' "$documents" >"$FX_DRIVE_SCRIPT"
      for kind in iphone mac; do
        for entry in drive foreground-cycle-entry; do
          : >"$STUB_LOG"
          if [ "$entry" = drive ]; then
            mg drive "$FX_DRIVE_SCRIPT" --platform "$kind" --record
          else
            mg foreground-cycle-entry "$FX_DRIVE_SCRIPT" "$kind"
          fi
          [ "$status" -eq 2 ]
          [ "$(field .status)" = error ]
          [ "$(field '.failures[0].id')" = usage ]
          [[ "$output$stderr$(cat "$result")" != *PRIVATE_SENTINEL* ]]
          [ ! -s "$STUB_LOG" ]
        done
      done
    done
  done
  # A single noncycle document reaches the existing stop-before-work stubs.
  printf '%s\n' "$innocuous" >"$FX_DRIVE_SCRIPT"
  for kind in iphone mac; do
    for entry in drive foreground-cycle-entry; do
      : >"$STUB_LOG"
      if [ "$entry" = drive ]; then
        mg drive "$FX_DRIVE_SCRIPT" --platform "$kind" --record
      else
        mg foreground-cycle-entry "$FX_DRIVE_SCRIPT" "$kind"
      fi
      [ "$(field '[.failures[] | select(.id == "usage")] | length')" -eq 0 ]
      if [ "$kind" = iphone ]; then
        called '^sim-begin$'
        called '^unexpected-build$'
      else
        called '^unexpected-mac-discovery$'
      fi
      not_called 'unexpected-(sim-lease|runner|recorder|screen-recorder)'
      [[ "$output$stderr$(cat "$result")" != *PRIVATE_SENTINEL* ]]
    done
  done
}

@test "physical drive accepts a literal true foreground cycle with kept-running continuity" {
  physical_preservation_facts
  printf '%s\n' '{"name":"physical-inspect","steps":[{"launch":{}},{"foregroundCycle":true}]}' >"$FX_DRIVE_SCRIPT"
  FX_DRIVE_FOREGROUND_CYCLE=1 physical_drive --keep-app-running
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.continuity)" = verified ]
  [ "$(field .details.activated_existing)" = true ]
  [ "$(field '.details.drive.steps[1].kind')" = foregroundCycle ]
  [ "$(jq -c '.steps[1]' "$(dirname "$result")/physical-script.json")" = '{"foregroundCycle":true}' ]
  called '^inline-transport-verified$'
  called '^physical-helper complete$'
  not_called 'build-for-testing|bootstrap-install|normal-final-launch|^(sim-|vm-)|xcrun (simctl|devicectl)'
}

@test "physical drive refuses malformed foreground cycles before discovery or runner work" {
  physical_drive_facts
  local value extra
  for value in false 0 1 1.0 -1 '"true"' '{}' '[]' null; do
    printf '{"name":"PRIVATE_SENTINEL","steps":[{"launch":{}},{"foregroundCycle":%s}]}' "$value" >"$FX_DRIVE_SCRIPT"
    physical_drive
    [ "$status" -eq 2 ]
    [[ "$output$stderr$(cat "$result")" != *PRIVATE_SENTINEL* ]]
    [ ! -s "$STUB_LOG" ]
  done
  for extra in '"extra":null' '"tap":"x"' '"transition":"cycle"' '"launch":{}'; do
    printf '{"name":"PRIVATE_SENTINEL","steps":[{"launch":{}},{"foregroundCycle":true,%s}]}' "$extra" >"$FX_DRIVE_SCRIPT"
    physical_drive
    [ "$status" -eq 2 ]
    [[ "$output$stderr$(cat "$result")" != *PRIVATE_SENTINEL* ]]
    [ ! -s "$STUB_LOG" ]
  done
}

@test "physical drive rejects model OS and nonphysical destinations and queues unavailable without fallback" {
  physical_drive_facts
  physical_drive --platform ipad
  [ "$status" -eq 2 ]
  mg drive "$FX_DRIVE_SCRIPT" --physical-device "$SIM"
  [ "$status" -eq 2 ]
  jq 'map(.operatingSystemVersion = "26.0")' "$FX_DEVICES" >"$FX_DEVICES.tmp"
  mv "$FX_DEVICES.tmp" "$FX_DEVICES"
  physical_drive
  [ "$status" -eq 2 ]
  echo '[]' >"$FX_DEVICES"
  physical_drive
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  not_called '^(lint|xcframework|host-lease|sim-|vm-|xcodebuild)|xcrun (simctl|devicectl)'
}

@test "physical drive refuses private invalid JSON oversize raw launches and motion before discovery without echoing input" {
  physical_drive_facts
  local json launch
  for launch in 'null' '[]' '{"fixture":null}' '{"route":""}' '{"arguments":[]}' '{"animations":false}' '{"PRIVATE_SENTINEL":null}'; do
    printf '{"name":"PRIVATE_SENTINEL","steps":[{"launch":{}},{"launch":%s}]}' "$launch" >"$FX_DRIVE_SCRIPT"
    physical_drive
    [ "$status" -eq 2 ]
    [[ "$output$stderr$(cat "$result")" != *PRIVATE_SENTINEL* ]]
    [ ! -s "$STUB_LOG" ]
  done
  for json in 'PRIVATE_SENTINEL' '{"name":"x","steps":[{"launch":{}},{"resize":"default"}]}' \
      '{"name":"x","steps":[{"launch":{},"transition":null}]}' \
      '{"name":"x","steps":[{"launch":{}},{"snapshot":"x","transition":"move"}]}' \
      '{"name":"x","PRIVATE_SENTINEL":null,"steps":[{"launch":{}}]}'; do
    printf '%s' "$json" >"$FX_DRIVE_SCRIPT"
    physical_drive
    [ "$status" -eq 2 ]
    [[ "$output$stderr$(cat "$result")" != *PRIVATE_SENTINEL* ]]
    [ ! -s "$STUB_LOG" ]
  done
  python3 - "$FX_DRIVE_SCRIPT" <<'PY'
from pathlib import Path
import sys
Path(sys.argv[1]).write_bytes(b' ' * 16385)
PY
  physical_drive
  [ "$status" -eq 2 ]
  rm "$FX_DRIVE_SCRIPT"
  physical_drive
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

@test "physical drive cannot pass absent skipped failed or malformed selected XCTest results" {
  physical_drive_facts
  local tree
  for tree in '[]' '[{nodeType:"Test Case",nodeIdentifier:"OtherTests/testOther()",result:"Passed"}]' \
      '[{nodeType:"Test Case",nodeIdentifier:"AgentDriverTests/testDriveScript()",result:"Skipped"}]' \
      '[{nodeType:"Test Case",nodeIdentifier:"AgentDriverTests/testDriveScript()",result:"Failed"}]'; do
    test_tree "$tree"
    physical_drive
    [ "$status" -ne 0 ]
    [ "$(field .status)" != pass ]
  done
  test_tree '[{nodeType:"Test Case",nodeIdentifier:"AgentDriverTests/testDriveScript()",result:"Passed"}]'
  printf 'not JSON' >"$FX_SUMMARY"
  physical_drive
  [ "$status" -ne 0 ]
  [ "$(field .status)" != pass ]
}

@test "physical drive cannot pass failed steps missing captures logs or hierarchy even when XCTest passes" {
  physical_drive_facts
  local mode
  for mode in step-failed missing-log missing-capture malformed-log no-captures truncated-log missing-hierarchy hierarchy-failure; do
    FX_DRIVE_CASE="$mode" physical_drive
    [ "$status" -ne 0 ]
    [ "$(field .status)" != pass ]
    called '^inline-transport-verified$'
    not_called '^(sim-|vm-)|xcrun (simctl|devicectl)'
  done
}

@test "physical drive accepts the matching synthetic iPad destination" {
  physical_drive_facts
  jq --arg p "$PHONE" 'map(if .identifier == $p then .modelCode = "iPad99,1" else . end)' "$FX_DEVICES" >"$FX_DEVICES.tmp"
  mv "$FX_DEVICES.tmp" "$FX_DEVICES"
  physical_drive --platform ipad
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  called '^inline-transport-verified$'
  not_called '^(sim-|vm-)|xcrun (simctl|devicectl)'
}

@test "physical personal preview rejects conflated arguments and contradictory configuration before discovery" {
  local args
  for args in '--config Debug' '--config Profile' '--config AppStore' '--config' \
      '--device fake' '--route sample/sheet' '--seconds 5' '--window-size default' \
      '--keep-app-running' '--record' '-- -MeguriFixture private' '--'; do
    mg run --personal-preview --platform iphone --physical-device "$PHONE" $args
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
    [ ! -s "$STUB_LOG" ]
    case "$args" in --config*) [[ "$(field '.failures[0].message')" == *'explicit --config must be Release'* ]] ;; esac
  done
  mg run --personal-preview --platform iphone
  [ "$status" -eq 2 ]
  mg run --personal-preview --physical-device "$PHONE"
  [ "$status" -eq 2 ]
  mg run --personal-preview --platform mac --physical-device "$PHONE"
  [ "$status" -eq 2 ]
  mg run --personal-preview --platform iphone-small --physical-device "$PHONE"
  [ "$status" -eq 2 ]
  mg run --platform iphone --physical-device "$PHONE" --config Release
  [ "$status" -eq 2 ]
  mg drive --personal-preview --platform iphone --physical-device "$PHONE"
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

# Preserve the real build/settings boundary; stop at the synthetic compiler
# failure, before signed-product verification or any installer/device process.
@test "physical personal preview selects team Release device core isolated products and exact inventory under host lease" {
  personal_facts
  jq 'map(.buildSettings |= (.CODE_SIGN_ENTITLEMENTS |= sub("macOS"; "iOS")) + {
    PLATFORM_NAME:"iphoneos", MEGURI_SIGNING:"team", DEVELOPMENT_TEAM:"TESTTEAM01",
    MEGURI_TEAM_ID:"TESTTEAM01", CODE_SIGN_IDENTITY:"Apple Development"} | del(.buildSettings.ENABLE_HARDENED_RUNTIME))' \
    "$FX_PERSONAL_SETTINGS" >"$BATS_TEST_TMPDIR/device-settings.json"
  mv "$BATS_TEST_TMPDIR/device-settings.json" "$FX_PERSONAL_SETTINGS"
  local kind config
  for kind in iphone ipad; do
    jq --arg p "$PHONE" --arg model "$( [ "$kind" = iphone ] && echo iPhone99,1 || echo iPad99,1 )" \
      'map(if .identifier == $p then .modelCode = $model else . end)' "$FX_DEVICES" >"$FX_DEVICES.tmp"
    mv "$FX_DEVICES.tmp" "$FX_DEVICES"
    for config in implicit explicit; do
      : >"$STUB_LOG"
      if [ "$config" = implicit ]; then
        FX_BUILD_RC=1 mg run --personal-preview --platform "$kind" --physical-device "$PHONE"
      else
        FX_BUILD_RC=1 mg run --config Release --physical-device "$PHONE" --platform "$kind" --personal-preview
      fi
      [ "$status" -eq 1 ]
      [ "$(field '.failures[0].id')" = build ]
      [ "$(grep -c '^xcrun xcdevice list --timeout 20$' "$STUB_LOG")" -eq 2 ]
      called '^host-lease --no-session$'
      called '^xcframework .* --configuration Release --slices device$'
      called '^xcodebuild -showBuildSettings .* -sdk iphoneos -arch arm64 '
      called "^xcodebuild build .* -configuration Release -destination platform=iOS,id=$PHONE -derivedDataPath build/app/PersonalDevicePreviewDerivedData .* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration MEGURI_BUNDLE_ID_SUFFIX=.dev MEGURI_CLOUD_CONFIGURATION=development$"
      not_called 'build-for-testing|-testPlan|test-without-building|^(sim-|vm-|open|system_profiler)|xcrun (simctl|devicectl)'
      [ "$(field .details.build.personal_preview)" = true ]
      [ "$(field .details.build.config)" = Release ]
      [ "$(field .details.build.messages)" = null ]
    done
  done
  : >"$STUB_LOG"
  mg run --personal-preview --platform iphone --physical-device "$SIM"
  [ "$status" -eq 2 ]
  mg run --personal-preview --platform iphone --physical-device "$PHONE"
  [ "$status" -eq 2 ] # inventory now identifies the selected device as iPad
  not_called '^(lint|xcframework|host-lease|sim-|vm-|xcodebuild)'
}

@test "physical personal preview profile binds iOS development signer push group and selected device with optional debugger grant" {
  personal_facts
  jq 'del(.["com.apple.application-identifier"], .["com.apple.developer.aps-environment"])
    | .["application-identifier"] = "TESTTEAM01.com.antoinefink.meguri.dev"
    | .["com.apple.security.application-groups"] = ["group.com.antoinefink.meguri.dev"]
    | .["aps-environment"] = "development"' "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/ios-entitlements.json"
  mv "$BATS_TEST_TMPDIR/ios-entitlements.json" "$FX_APP_ENT"
  python3 - "$FX_APP_ENT" "$FX_APP_PROFILE" "$PHONE" <<'PY'
import json, plistlib, sys
from pathlib import Path
path = Path(sys.argv[2])
profile = plistlib.loads(path.read_bytes())
profile.update(Platform=['iOS'], ProvisionedDevices=[sys.argv[3]], Entitlements=json.loads(Path(sys.argv[1]).read_text()))
profile['Entitlements']['get-task-allow'] = True
path.write_bytes(plistlib.dumps(profile))
PY
  cat >>"$SB/tools/mg.d/zz-physical-fakes.sh" <<'FAKES'
mg_job fx-device-profile host fx_device_profile "Synthetic iOS preview provisioning check"
fx_device_profile() {
  if mg_app_personal_profile "$MG_ROOT/build/fixture.app" "$(cat "$FX_APP_ENT")" "$PHONE" ios; then
    mg_status pass
  else
    mg_status fail
  fi
}
FAKES
  mkdir -p "$SB/build/fixture.app"
  local debugger
  for debugger in absent true false; do
    jq --arg mode "$debugger" 'if $mode == "absent" then del(.["get-task-allow"]) else .["get-task-allow"] = ($mode == "true") end' \
      "$FX_APP_ENT" >"$BATS_TEST_TMPDIR/debugger.json"
    mv "$BATS_TEST_TMPDIR/debugger.json" "$FX_APP_ENT"
    cp "$FX_APP_PROFILE" "$SB/build/fixture.app/embedded.mobileprovision"
    mg fx-device-profile
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
  done
  python3 - "$FX_APP_PROFILE" <<'PY'
import plistlib, sys
from pathlib import Path
path = Path(sys.argv[1])
profile = plistlib.loads(path.read_bytes())
profile['ProvisionedDevices'] = ['other-synthetic-device']
path.write_bytes(plistlib.dumps(profile))
PY
  cp "$FX_APP_PROFILE" "$SB/build/fixture.app/embedded.mobileprovision"
  mg fx-device-profile
  [ "$status" -ne 0 ]
  not_called '^system_profiler|xcrun (simctl|devicectl)|xcodebuild '
}

# The owner-requested real copy on a phone (run --real-copy --owner-requested).
real_copy_device_facts() {
  personal_facts
  export MEGURI_PROFILE_DIRS="$BATS_TEST_TMPDIR/profiles"
  mkdir -p "$MEGURI_PROFILE_DIRS"
  jq 'map(.target as $target | .buildSettings |= (. + {
      MEGURI_BUNDLE_ID_SUFFIX:"", MEGURI_CLOUD_CONFIGURATION:"production", MEGURI_CLOUD_CONTAINER:"iCloud.example.daily",
      MEGURI_CLOUD_ENVIRONMENT:"production", MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT:"Production", MEGURI_CLOUD_PUSH_ENVIRONMENT:"development",
      INFOPLIST_KEY_MeguriCloudContainer:"iCloud.example.daily", INFOPLIST_KEY_MeguriCloudEnvironment:"production",
      PRODUCT_BUNDLE_IDENTIFIER:("com.antoinefink.meguri" + (if $target == "Meguri" then "" else ".widget" end)),
      MEGURI_APP_NAME:"Meguri", ASSETCATALOG_COMPILER_APPICON_NAME:"AppIcon", MEGURI_SIGNING:"team", PLATFORM_NAME:"iphoneos",
      CODE_SIGN_ENTITLEMENTS:(if $target == "Meguri" then "Meguri/Meguri-iOS.entitlements" else "MeguriWidget/MeguriWidget-iOS.entitlements" end),
      CODE_SIGN_STYLE:"Manual",
      PROVISIONING_PROFILE_SPECIFIER:(if $target == "Meguri" then "AAAAAAAA-1111-2222-3333-444444444444" else "BBBBBBBB-1111-2222-3333-444444444444" end)})
    | del(.buildSettings.ENABLE_HARDENED_RUNTIME))' "$FX_PERSONAL_SETTINGS" >"$BATS_TEST_TMPDIR/real-settings.json"
  mv "$BATS_TEST_TMPDIR/real-settings.json" "$FX_PERSONAL_SETTINGS"
}

# real_copy_device_profiles: installed iOS development profiles for the
# daily app and widget covering $PHONE (synthetic team DNQ34W43XX).
real_copy_device_profiles() {
  python3 - "$MEGURI_PROFILE_DIRS" "$PHONE" <<'PY'
import datetime, plistlib, sys
for name, ident, grants in (
    ("app", "DNQ34W43XX.com.antoinefink.meguri", {
        "keychain-access-groups": ["DNQ34W43XX.*"], "com.apple.security.application-groups": ["group.com.antoinefink.meguri"],
        "com.apple.developer.icloud-container-identifiers": ["iCloud.com.antoinefink.meguri"], "com.apple.developer.icloud-services": "*",
        "com.apple.developer.icloud-container-environment": ["Development", "Production"], "aps-environment": "development"}),
    ("widget", "DNQ34W43XX.com.antoinefink.meguri.widget", {"com.apple.security.application-groups": ["group.com.antoinefink.meguri"]})):
    grants.update({"application-identifier": ident, "get-task-allow": True})
    with open(sys.argv[1] + "/" + name + ".mobileprovision", "wb") as stream:
        plistlib.dump({"Entitlements": grants, "TeamIdentifier": ["DNQ34W43XX"], "Platform": ["iOS"],
                      "ExpirationDate": datetime.datetime(2099, 1, 1),
                      "UUID": ("A" if name == "app" else "B") * 8 + "-1111-2222-3333-444444444444",
                      "ProvisionedDevices": [sys.argv[2]]}, stream)
PY
}

@test "the phone real copy refuses before discovery without acknowledgement or with launch options" {
  local args
  for args in '--real-copy --platform iphone' '--owner-requested --platform iphone' \
      '--real-copy --owner-requested --personal-preview --platform iphone' '--real-copy --owner-requested --seconds 5 --platform iphone' \
      '--real-copy --owner-requested --platform iphone --route sample/sheet' '--real-copy --owner-requested --platform iphone --config Debug' \
      '--real-copy --owner-requested --platform mac'; do
    mg run $args --physical-device "$PHONE"
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
    [ ! -s "$STUB_LOG" ]
  done
}

@test "the phone real copy stops at its profile check when the app or widget profile for the device is missing" {
  real_copy_device_facts
  mg run --real-copy --owner-requested --platform iphone --physical-device "$PHONE"
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = real-copy-profile ]
  not_called '^(xcframework|xcodebuild)|devicectl'
  real_copy_device_profiles
  rm "$MEGURI_PROFILE_DIRS/widget.mobileprovision"
  : >"$STUB_LOG"
  mg run --real-copy --owner-requested --platform iphone --physical-device "$PHONE"
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = real-copy-profile ]
  [ "$(field '.details.daily_driver_profiles.ios_widget.state')" = missing ]
  not_called '^(xcframework|xcodebuild)|devicectl'
}

@test "the phone real copy builds Release with development push from the pinned device profiles and never registers or launches" {
  real_copy_device_facts
  real_copy_device_profiles
  FX_BUILD_RC=1 mg run --real-copy --owner-requested --platform iphone --physical-device "$PHONE"
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = build ]
  called '^host-lease --no-session$'
  called '^xcframework .* --configuration Release --slices device$'
  called '^xcodebuild -showBuildSettings .* -sdk iphoneos -arch arm64 MEGURI_SIGNING=team MEGURI_CLOUD_PUSH_ENVIRONMENT=development MEGURI_DEVICE_PROFILE_STYLE=Manual MEGURI_DEVICE_PROFILE_SPECIFIER=AAAAAAAA-1111-2222-3333-444444444444 MEGURI_WIDGET_DEVICE_PROFILE_STYLE=Manual MEGURI_WIDGET_DEVICE_PROFILE_SPECIFIER=BBBBBBBB-1111-2222-3333-444444444444$'
  called "^xcodebuild build .* -configuration Release -destination platform=iOS,id=$PHONE -derivedDataPath build/app/RealCopyDeviceDerivedData .* MEGURI_SIGNING=team MEGURI_CLOUD_PUSH_ENVIRONMENT=development MEGURI_DEVICE_PROFILE_STYLE=Manual MEGURI_DEVICE_PROFILE_SPECIFIER=AAAAAAAA-1111-2222-3333-444444444444 MEGURI_WIDGET_DEVICE_PROFILE_STYLE=Manual MEGURI_WIDGET_DEVICE_PROFILE_SPECIFIER=BBBBBBBB-1111-2222-3333-444444444444$"
  not_called 'allowProvisioning|MEGURI_BUNDLE_ID_SUFFIX|build-for-testing|-testPlan|devicectl|process launch'
  [ "$(field .details.build.real_copy)" = true ]
  [ "$(field .details.build.messages)" = null ]
  # Wrong resolved settings stop before the build.
  jq '.[0].buildSettings.MEGURI_CLOUD_PUSH_ENVIRONMENT = "production"' "$FX_PERSONAL_SETTINGS" >"$BATS_TEST_TMPDIR/bad.json"
  cp "$BATS_TEST_TMPDIR/bad.json" "$FX_PERSONAL_SETTINGS"
  : >"$STUB_LOG"
  mg run --real-copy --owner-requested --platform iphone --physical-device "$PHONE"
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = real-copy-settings ]
  not_called '^xcodebuild build '
}
