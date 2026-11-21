#!/usr/bin/env bats
# build-app --real-copy --owner-requested (tools/mg.d/app.sh): Antoine's
# daily-driver Mac app. Same throwaway tools/ checkout and PATH stubs as
# physical_device_ui.bats; no Apple command, profile, LaunchServices record or
# product of this Mac is used. Synthetic team and certificate values only.
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
  export MEGURI_APP_SIGNING=team
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  : >"$STUB_LOG"
  export FX="$BATS_TEST_TMPDIR/fx"
  mkdir -p "$FX/profiles"
  export MEGURI_PROFILE_DIRS="$FX/profiles"
  cat >"$SB/tools/mg.d/zz-real-copy-fakes.sh" <<'EOF'
mg_lint_ui_first() { echo lint >>"$STUB_LOG"; }
mg__xcframework() { echo "xcframework $*" >>"$STUB_LOG"; }
# The identity check itself runs on real products (app_identity_check.py).
mg_app_real_copy_identity() { echo "identity $1" >>"$STUB_LOG"; [ "${FX_IDENTITY_RC:-0}" -eq 0 ]; }
MG_LSREGISTER="$BIN/lsregister"
EOF
  sed -i.bak "s|\$BIN|$BIN|" "$SB/tools/mg.d/zz-real-copy-fakes.sh" && rm -f "$SB/tools/mg.d/zz-real-copy-fakes.sh.bak"
  mkdir -p "$SB/app/Packages/MeguriCore/Binaries"
  printf 'hash=%s\nconfiguration=Release\nprofile=release\nfeatures=ankiweb\nslices=mac,sim\n' \
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa >"$SB/app/Packages/MeguriCore/Binaries/.stamp"
  facts
  stub xcodebuild 'echo "xcodebuild $*" >>"$STUB_LOG"
action="$1"; shift
if [ "$action" = -showBuildSettings ]; then cat "$FX/settings.json"; exit 0; fi
config=""; result=""; dd=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -configuration) config="$2"; shift ;;
    -resultBundlePath) result="$2"; shift ;;
    -derivedDataPath) dd="$2"; shift ;;
  esac
  shift
done
mkdir -p "$result"
p="$dd/Build/Products/$config/Meguri.app"
w="$p/Contents/PlugIns/MeguriWidget.appex"
mkdir -p "$p/Contents/MacOS" "$w/Contents/MacOS"
echo "synthetic app" >"$p/Contents/MacOS/Meguri"
echo "synthetic widget" >"$w/Contents/MacOS/MeguriWidget"
cp "$FX/info.json" "$p/Contents/Info.plist"
echo "{\"CFBundleIdentifier\":\"com.antoinefink.meguri.widget\",\"CFBundleExecutable\":\"MeguriWidget\"}" >"$w/Contents/Info.plist"
cp "$FX/embedded.plist" "$p/Contents/embedded.provisionprofile"
if [ -n "${FX_BUILD_ERROR:-}" ]; then echo "error: $FX_BUILD_ERROR"; exit 65; fi
exit 0'
  stub xcrun 'echo "xcrun $*" >>"$STUB_LOG"
case "$*" in
  "xcresulttool get build-results --path "*) echo "{\"errorCount\":0,\"warningCount\":0}" ;;
  *) exit 97 ;;
esac'
  stub codesign 'echo "codesign $*" >>"$STUB_LOG"
case "$1" in
  --verify) exit 0 ;;
  -d)
    case "$2" in
      --extract-certificates=*) printf "%s" "synthetic certificate" >"${2#*=}0"; exit 0 ;;
      --verbose=4)
        identity=com.antoinefink.meguri
        case "$*" in *.appex) identity="$identity.widget" ;; esac
        printf "Identifier=%s\nTeamIdentifier=TESTTEAM01\n" "$identity" >&2; exit 0 ;;
    esac
    case "$*" in *.appex) cat "$FX/widget-ent.json" ;; *) cat "$FX/app-ent.json" ;; esac ;;
  *) exit 97 ;;
esac'
  stub plutil 'echo "plutil $*" >>"$STUB_LOG"
[ "$1 $2 $3 $4" = "-convert json -o -" ] || exit 97
case "$5" in -) cat ;; *) cat "$5" ;; esac'
  stub lipo 'echo "lipo $*" >>"$STUB_LOG"; echo arm64'
  stub security 'echo "security $*" >>"$STUB_LOG"; [ "$1 $2 $3" = "cms -D -i" ] || exit 97; cat "$4"'
  stub system_profiler 'echo "system_profiler $*" >>"$STUB_LOG"; echo "{\"SPHardwareDataType\":[{\"provisioning_UDID\":\"00000000-0000-4000-8000-000000000009\"}]}"'
  stub pluginkit 'echo "pluginkit $*" >>"$STUB_LOG"'
  stub lsregister 'echo "lsregister $*" >>"$STUB_LOG"
if [ "$1" = -dump ] && [ -n "${FX_STILL_REGISTERED:-}" ]; then echo "path: $FX_STILL_REGISTERED (0x1)"; fi
exit 0'
  stub ditto 'echo "ditto $*" >>"$STUB_LOG"; [ "$1 $2 $3" = "-c -k --keepParent" ] || exit 97; [ -d "$4" ] || exit 1; echo zip >"$5"'
  export PATH="$BIN:$PATH"
}

teardown() {
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures}' "$result"
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }
mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }
called() { grep -Eq -- "$1" "$STUB_LOG" || { cat "$STUB_LOG"; return 1; }; }
not_called() { ! grep -Eq -- "$1" "$STUB_LOG" || { cat "$STUB_LOG"; return 1; }; }
real_copy() { mg build-app --platform mac --real-copy --owner-requested "$@"; }
built() { echo "$SB/build/app/RealCopyDerivedData/Build/Products/Release/Meguri.app"; }

# Synthetic daily-driver facts: resolved settings, signed claims, Info, the
# embedded profile (team TESTTEAM01) and an installed profile as Xcode would
# keep it (team DNQ34W43XX, the real container) for the preflight.
facts() {
  jq -n '["Meguri", "MeguriWidget"] | map(. as $target | {target: $target, buildSettings: {
    CONFIGURATION:"Release", MEGURI_SIGNING:"team", MEGURI_APP_NAME:"Meguri", ASSETCATALOG_COMPILER_APPICON_NAME:"AppIcon",
    MEGURI_CLOUD_CONFIGURATION:"production", MEGURI_CLOUD_CONTAINER:"iCloud.example.daily",
    MEGURI_CLOUD_CONTAINER_development:"iCloud.example.isolated", MEGURI_CLOUD_CONTAINER_production:"iCloud.example.daily",
    MEGURI_CLOUD_ENVIRONMENT:"production", MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT:"Production", MEGURI_CLOUD_PUSH_ENVIRONMENT:"production",
    INFOPLIST_KEY_MeguriCloudContainer:"iCloud.example.daily", INFOPLIST_KEY_MeguriCloudEnvironment:"production",
    MEGURI_CLOUD_SCHEMA_CONFIGURED_development:"YES", MEGURI_CLOUD_SCHEMA_CONFIGURED_production:"NO",
    MEGURI_CLOUD_SCHEMA_CONFIGURED:"NO", MEGURI_CLOUD_SCHEMA_BOOLEAN:"0",
    PRODUCT_BUNDLE_IDENTIFIER:("com.antoinefink.meguri" + (if $target == "Meguri" then "" else ".widget" end)),
    MEGURI_CORE_FEATURES:"ankiweb", MEGURI_SWIFT_CONDITIONS:"ANKIWEB_ENABLED", SWIFT_ACTIVE_COMPILATION_CONDITIONS:"ANKIWEB_ENABLED",
    MEGURI_CARD_INSPECTION_BUILD:"personalRelease", MEGURI_MAC_UI_ELEMENT:"NO",
    CODE_SIGN_ENTITLEMENTS:(if $target == "Meguri" then "Meguri/Meguri-macOS.entitlements" else "MeguriWidget/MeguriWidget-macOS.entitlements" end),
    ENABLE_TESTABILITY:"NO", ENABLE_DEBUG_DYLIB:"NO", ENABLE_HARDENED_RUNTIME:"YES"}})' >"$FX/settings.json"
  jq -n '{
    "com.apple.security.app-sandbox":true, "com.apple.security.network.client":true, "com.apple.security.network.server":true,
    "com.apple.security.files.user-selected.read-write":true, "com.apple.security.files.bookmarks.app-scope":true,
    "com.apple.developer.team-identifier":"TESTTEAM01", "com.apple.application-identifier":"TESTTEAM01.com.antoinefink.meguri",
    "keychain-access-groups":["TESTTEAM01.com.antoinefink.meguri"], "com.apple.security.application-groups":["TESTTEAM01.com.antoinefink.meguri"],
    "com.apple.developer.icloud-container-identifiers":["iCloud.example.daily"], "com.apple.developer.icloud-services":["CloudKit"],
    "com.apple.developer.icloud-container-environment":"Production", "com.apple.developer.aps-environment":"production"}' >"$FX/app-ent.json"
  echo '{"com.apple.security.app-sandbox":true,"com.apple.security.application-groups":["TESTTEAM01.com.antoinefink.meguri"]}' >"$FX/widget-ent.json"
  echo '{"CFBundleIdentifier":"com.antoinefink.meguri","CFBundleExecutable":"Meguri","MeguriCardInspectionBuild":"personalRelease","LSUIElement":false,"MeguriCloudContainer":"iCloud.example.daily","MeguriCloudEnvironment":"production","MeguriCloudSchemaConfigured":false}' >"$FX/info.json"
  profile "$FX/embedded.plist" TESTTEAM01 "$(cat "$FX/app-ent.json")"
  installed_profile production
}

# profile <path> <team> <entitlements JSON>
profile() {
  python3 - "$1" "$2" "$3" <<'PY'
import datetime, json, plistlib, sys
with open(sys.argv[1], "wb") as stream:
    plistlib.dump({"Entitlements": json.loads(sys.argv[3]), "TeamIdentifier": [sys.argv[2]],
                  "Platform": ["OSX"], "DeveloperCertificates": [b"synthetic certificate"],
                  "ExpirationDate": datetime.datetime(2099, 1, 1),
                  "ProvisionedDevices": ["00000000-0000-4000-8000-000000000009"]}, stream)
PY
}

# installed_profile <push environment>: the profile Xcode would install for
# the app (a development profile grants push "development").
installed_profile() {
  profile "$FX/profiles/daily.provisionprofile" DNQ34W43XX "$(jq -n --arg push "$1" '{
    "com.apple.application-identifier":"DNQ34W43XX.com.antoinefink.meguri", "com.apple.developer.team-identifier":"DNQ34W43XX",
    "keychain-access-groups":["DNQ34W43XX.*"], "com.apple.security.application-groups":["DNQ34W43XX.*"],
    "com.apple.developer.icloud-container-identifiers":["iCloud.com.antoinefink.meguri"], "com.apple.developer.icloud-services":"*",
    "com.apple.developer.icloud-container-environment":["Development","Production"], "com.apple.developer.aps-environment":$push}')"
}

@test "the real copy refuses before any work without acknowledgement, team signing or the exact route" {
  local args
  while IFS= read -r args; do
    : >"$STUB_LOG"
    # shellcheck disable=SC2086
    mg build-app $args
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
    [ ! -s "$STUB_LOG" ]
  done <<'EOF'
--platform mac --real-copy
--platform mac --owner-requested
--platform ios-sim --real-copy --owner-requested
--platform mac --config AppStore --real-copy --owner-requested
--platform mac --config Debug --real-copy --owner-requested
--platform mac --real-copy --owner-requested --standalone
--platform mac --real-copy --owner-requested --personal-preview
--platform mac --real-copy --owner-requested --test-plan CloudAgent
EOF
  unset MEGURI_APP_SIGNING
  real_copy
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"MEGURI_APP_SIGNING=team"* ]]
  [ ! -s "$STUB_LOG" ]
  [ ! -d "$SB/build/app" ]
}

@test "the real copy stops before building when no installed profile authorizes it, naming what is missing" {
  installed_profile development
  real_copy
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = real-copy-profile ]
  [[ "$(field '.failures[0].message')" == *"push-production"* ]]
  [[ "$(field '.failures[0].message')" == *"never requests provisioning updates"* ]]
  [ "$(field '.details.daily_driver_profiles.mac.state')" = incomplete ]
  not_called '^(lint|xcframework|xcodebuild|ditto)'
  rm "$FX/profiles/daily.provisionprofile"
  : >"$STUB_LOG"
  real_copy
  [ "$status" -eq 1 ]
  [ "$(field '.details.daily_driver_profiles.mac.state')" = missing ]
  not_called '^(lint|xcframework|xcodebuild|ditto)'
}

@test "the real copy builds Release with installed profiles only, packs it and forgets the built app" {
  real_copy
  [ "$status" -eq 0 ] || cat "$STUB_LOG"
  [ "$(field .status)" = pass ]
  called '^xcodebuild -showBuildSettings -json .* -target Meguri -target MeguriWidget -configuration Release -sdk macosx -arch arm64 MEGURI_SIGNING=team$'
  called '^xcodebuild build .* -configuration Release -destination platform=macOS,arch=arm64 -derivedDataPath build/app/RealCopyDerivedData .* MEGURI_SIGNING=team$'
  not_called 'allowProvisioning|MEGURI_BUNDLE_ID_SUFFIX|build-for-testing|-testPlan|open '
  [ "$(grep -n '^xcodebuild -showBuildSettings' "$STUB_LOG" | cut -d: -f1)" -lt "$(grep -n '^xcodebuild build ' "$STUB_LOG" | cut -d: -f1)" ]
  called "^identity $(built)$"
  called "^ditto -c -k --keepParent $(built) $SB/build/app/real-copy/Meguri.zip$"
  called "^pluginkit -r $(built)/Contents/PlugIns/MeguriWidget.appex$"
  called "^lsregister -u $(built)$"
  [ -f "$SB/build/app/real-copy/Meguri.zip" ]
  [ ! -e "$(built)" ]
  [ "$(field .details.real_copy.built_app_forgotten)" = true ]
  [ "$(field .details.real_copy.zip)" = build/app/real-copy/Meguri.zip ]
  [ "$(field .details.real_copy_product.bundle_id)" = com.antoinefink.meguri ]
  [ "$(field .details.real_copy_product.cloud_environment)" = production ]
  [ "$(field .details.build.real_copy)" = true ]
  [ "$(field '.details.real_copy_cleanup_checks | length')" = 4 ]
  [[ "$(field '.details.real_copy_cleanup_checks[2].command')" == "pluginkit -mAvvv -i com.antoinefink.meguri.widget" ]]
  [[ "$stderr" == *"only the installed copies are registered"* ]]
}

@test "the real copy rejects wrong resolved settings before building" {
  cp "$FX/settings.json" "$FX/good.json"
  local mutation
  for mutation in \
    '.[0].buildSettings.MEGURI_BUNDLE_ID_SUFFIX = ".dev"' \
    '.[1].buildSettings.PRODUCT_BUNDLE_IDENTIFIER = "com.antoinefink.meguri.dev.widget"' \
    '.[0].buildSettings.MEGURI_CLOUD_CONFIGURATION = "development"' \
    '.[0].buildSettings.SWIFT_ACTIVE_COMPILATION_CONDITIONS = "DEBUG ANKIWEB_ENABLED MEGURI_TEST_HOOKS"' \
    '.[0].buildSettings.MEGURI_APP_NAME = "Meguri Dev"' \
    '.[0].buildSettings.ASSETCATALOG_COMPILER_APPICON_NAME = "AppIcon-Dev"' \
    '.[1].buildSettings.MEGURI_ENTITLEMENTS_SIGNING_SUFFIX = "-AdHoc"'; do
    jq "$mutation" "$FX/good.json" >"$FX/settings.json"
    : >"$STUB_LOG"
    real_copy
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = real-copy-settings ]
    not_called '^xcodebuild build '
  done
}

@test "a real copy that fails its checks or its build is still unregistered and deleted" {
  local mutation
  cp "$FX/app-ent.json" "$FX/good-ent.json"
  for mutation in \
    '.["com.apple.security.get-task-allow"] = true' \
    '.["com.apple.application-identifier"] = "TESTTEAM01.com.antoinefink.meguri.dev"' \
    '.["com.apple.developer.icloud-container-environment"] = "Development"' \
    '.["com.apple.security.temporary-exception.files.absolute-path.read-only"] = ["/"]'; do
    jq "$mutation" "$FX/good-ent.json" >"$FX/app-ent.json"
    : >"$STUB_LOG"
    real_copy
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = real-copy-product ]
    called "^lsregister -u $(built)$"
    [ ! -e "$(built)" ]
    not_called '^ditto '
  done
  cp "$FX/good-ent.json" "$FX/app-ent.json"
  : >"$STUB_LOG"
  FX_IDENTITY_RC=1 real_copy
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = real-copy-product ]
  [ ! -e "$(built)" ]
  : >"$STUB_LOG"
  FX_BUILD_ERROR='Provisioning profile "Mac Team Provisioning Profile: com.antoinefink.meguri" does not include the com.apple.developer.aps-environment entitlement.' real_copy
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = real-copy-profile ]
  called "^lsregister -u $(built)$"
  [ ! -e "$(built)" ]
}

@test "the real copy fails when LaunchServices still holds the built app" {
  FX_STILL_REGISTERED="$(built)" real_copy
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = real-copy-forget ]
  [ ! -e "$(built)" ]
}

@test "the profile reader reports authorized, incomplete and missing without touching the account" {
  run python3 "$SB/tools/scripts/daily-driver-profiles.py" --require mac --device 00000000-0000-4000-8000-000000000009
  [ "$status" -eq 0 ]
  [ "$(jq -r .mac.state <<<"$output")" = authorized ]
  run python3 "$SB/tools/scripts/daily-driver-profiles.py" --require mac --device 00000000-0000-4000-8000-00000000ffff
  [ "$status" -eq 1 ]
  [ "$(jq -r '.mac.missing | join(",")' <<<"$output")" = this-device ]
  installed_profile development
  run python3 "$SB/tools/scripts/daily-driver-profiles.py" --require mac
  [ "$status" -eq 1 ]
  [ "$(jq -r '.mac.missing | join(",")' <<<"$output")" = push-production ]
  [ "$(jq -r .ios.state <<<"$output")" = missing ]
  not_called '^security (add|delete|import|find|unlock|set)'
}
