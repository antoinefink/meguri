# App builds and launches (U8, R1961, R1962). Sourced by tools/mg; host jobs.
#
# Default builds go to build/app/DerivedData (build-for-testing of the UI plan, so
# drive, ui-tree and screenshot reuse them). Simulators come from the pool
# (tools/scripts/sim-pool.sh), which lives in the macOS test VM (U8c,
# mac-vm.sh): simulator jobs call mg_sim_begin before they build and
# mg_sim_take after (the VM's lease, the products copied in), and reach the
# simulators through mg_simctl. A --device must be a pool device this
# checkout leased. Mac launches take the host lease (host.sh). The Mac app is
# launched hidden (open -g -j) with -MeguriBackgroundLaunch YES, so it stays
# out of the Dock and never becomes active (U8d; mg_app_open_mac), and only in
# Debug: Profile has no background launch, and Release and AppStore have the
# daily driver's ids (app/AGENTS.md).

mg_job build-app host mg_build_app \
  "Build the app and its tests: --platform ios-sim|mac [--config Debug|Profile|Release|AppStore] [--test-plan UI|CloudAgent] [--standalone|--personal-preview] (lint-ui first, core XCFramework; default build-for-testing with UI plan; CloudAgent: Debug only, no previews; --standalone: Mac Debug automation preview; --personal-preview: Mac Release with development identities and team signing in PersonalPreviewDerivedData; previews never launch)"
mg_job run host mg_run \
  "Install and launch: --platform iphone|iphone-small|ipad|ipad-small|foldable|mac [--device <leased udid> | --physical-device <explicit identifier>] [--config Debug|Profile]; physical requires Debug and a normal launch without overrides; simulator/Mac also accept --route, --window-size, --seconds and -- launch args"

MG_APP_DD=build/app/DerivedData
MG_APP_DEV_ID=com.antoinefink.meguri.dev

# mg_app_products <mac|ios-sim|ios-device> <config>
mg_app_products() {
  case "$1" in
    mac) printf '%s/%s/Build/Products/%s\n' "$MG_ROOT" "$MG_APP_DD" "$2" ;;
    ios-device) printf '%s/%s/Build/Products/%s-iphoneos\n' "$MG_ROOT" "$MG_APP_DD" "$2" ;;
    *) printf '%s/%s/Build/Products/%s-iphonesimulator\n' "$MG_ROOT" "$MG_APP_DD" "$2" ;;
  esac
}

# Xcode can retain an embedded widget signed by an earlier signing mode.
# Remove only its generated copy so the build embeds the current product.
mg_app_remove_embedded_widget() {
  local platform="$1" config="$2" widget path
  case "$platform/$MG_APP_DD" in
    mac/build/app/DerivedData|mac/build/app/StandaloneDerivedData|mac/build/app/PersonalPreviewDerivedData|mac/build/app/CloudAgentMacDerivedData)
      widget="$(mg_app_products mac "$config")/Meguri.app/Contents/PlugIns/MeguriWidget.appex"
      ;;
    ios-sim/build/app/CloudAgentSimDerivedData)
      widget="$(mg_app_products ios-sim "$config")/Meguri.app/PlugIns/MeguriWidget.appex"
      ;;
    *) mg_failure embedded-widget "widget cleanup requires a known app build directory"; mg_status error; return 1 ;;
  esac
  path="$widget"
  while [ "$path" != "$MG_ROOT" ]; do
    if [ -L "$path" ]; then
      mg_failure embedded-widget "refusing widget cleanup through a symbolic link in the build product path"
      mg_status error
      return 1
    fi
    path="${path%/*}"
  done
  if ! rm -rf "$widget"; then
    mg_failure embedded-widget "could not remove the generated embedded widget before the build"
    mg_status error
    return 1
  fi
}

# mg_app_build <mac|ios-sim|ios-device> <config> [plan] [scheme] [build settings...]: UI
# lint first (AE90), core XCFramework and build-for-testing of the plan (UI
# by default; snapshots build Snapshot; perf-ui builds Performance with the
# Meguri-Profile scheme; unit-sim builds Unit with ARCHS=arm64, since package
# test products do not take the app's arm64-only setting). On failure
# records it and returns 1. MEGURI_APP_SIGNING=team opts host builds into
# Xcode-managed team provisioning; the default remains adhoc.
mg_app_build() {
  mg_app_build_action build-for-testing "$@"
}

# Only the explicit native-test build caller selects this preparation boundary.
mg_app_cloud_agent_build() {
  local frameworks
  case "$platform" in
    mac) frameworks="$(mg_app_products mac Debug)/Meguri.app/Contents/Frameworks" ;;
    ios-sim) frameworks="$(mg_app_products ios-sim Debug)/Meguri.app/Frameworks" ;;
    *) return 1 ;;
  esac
  python3 -B -I "$MG_TOOLS/scripts/cloud-agent-injection.py" --build "$MG_RUN_DIR" "$frameworks" -- "$@"
}

# Private app-only Mac profile input, never a public MG argument.
mg_app_mac_agent_profile_scope() {
  [ "$#" -eq 6 ] && [ "$1" = team ] && [ "$2" = build-for-testing ] &&
    [ "$3" = mac ] && [ "$4" = Debug ] && [ "$6" = Meguri ] || return 1
  case "$5" in
    CloudAgent) return 0 ;;
    Snapshot) [ "${MG_JOB:-}" = mac-hosted-test ] || [ "${MG_JOB:-}" = snapshots ] ;;
    UI) [ "${MG_JOB:-}" = drive ] ||
      { [ "${MG_JOB:-}" = ui-test ] &&
        [ "${MG_UI_MAC_PROFILE_SELECTOR:-}" = MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch ]; } ;;
    *) return 1 ;;
  esac
}

mg_app_mac_agent_profile_check() {
  local profile
  [ -n "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ] || return 0
  profile="$(jq -r .file <<<"$MG_CLOUD_AGENT_MAC_PROFILE_BINDING")" || return 1
  printf '%s\n' "$MG_CLOUD_AGENT_MAC_PROFILE_BINDING" | python3 -B -I \
    "$MG_TOOLS/scripts/cloud-agent-mac-profile.py" --root "$MG_ROOT" --file "$profile" --check
}

mg_app_mac_agent_private_settings() {
  "$@" 2>"$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE/resolved-settings.log"
}

mg_app_mac_agent_profile_refusal() {
  local reason
  reason="$(tail -n 1 "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE/validation.log" 2>/dev/null)"
  reason="${reason#App-only Mac CloudAgent profile validation refused: }"
  case "$reason" in
    source-dirty|source-changed|profile-input|profile-cache|profile-settings|profile-target-scope|profile-entitlements|profile-host|profile-signer|profile-team|profile-identity|profile-device|profile-expired|profile-development|profile-grants|profile-signer-platform|profile-changed|guarded-input) ;;
    *) reason=guarded-input ;;
  esac
  mg_failure agent-profile "Approved app-only Mac CloudAgent profile validation refused ($reason)"
}

# Explicit action keeps every nested UI/snapshot/perf caller on its test plan.
# Only explicit preview actions select build; no ambient switch changes callers.
mg_app_build_action() {
  # Only this producer may populate a binding; stale state must not select
  # private signing/log handling for a later, out-of-scope build.
  MG_CLOUD_AGENT_MAC_PROFILE_BINDING=""
  MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE=""
  local action="$1" standalone_arg="" plan_arg="" personal=false test_args=()
  shift
  local platform="$1" config="$2" plan="${3:-UI}" scheme="${4:-Meguri}" signing="${MEGURI_APP_SIGNING-adhoc}" dest result log rc counts sdk xctestrun policy settings=() core_args=() build_command=(xcodebuild)
  case "$action" in
    build)
      standalone_arg=" --standalone"
      if [ "$platform/$config/$plan/$scheme" != mac/Debug/UI/Meguri ]; then
        mg_failure usage "--standalone requires --platform mac --config Debug and the normal Meguri scheme"
        mg_status error
        return 1
      fi
      ;;
    personal-preview)
      if [ "$platform/$config/$plan/$scheme" != mac/Release/UI/Meguri ] || [ "$signing" != team ]; then
        mg_failure usage "--personal-preview requires Mac Release, the normal Meguri scheme and MEGURI_APP_SIGNING=team"
        mg_status error
        return 1
      fi
      action=build
      personal=true
      standalone_arg=" --personal-preview"
      ;;
    build-for-testing) test_args=(-testPlan "$plan") ;;
    *) mg_failure usage "unsupported app build action: $action"; mg_status error; return 1 ;;
  esac
  if ! mg_word_in "$signing" "adhoc team"; then
    mg_failure usage "MEGURI_APP_SIGNING takes adhoc or team"
    mg_status error
    return 1
  fi
  [ "$#" -le 4 ] || settings=("${@:5}")
  if [ "$platform" = ios-device ]; then
    if ! mg_word_in "$config" "Debug Profile"; then
      mg_failure usage "physical iOS tests require Debug or Profile with the development bundle id"
      mg_status error
      return 1
    fi
    if [ "$config" = Profile ] && { [ "$plan" != Performance ] || [ "$scheme" != Meguri-Profile ]; }; then
      mg_failure usage "physical Profile tests require the Meguri-Profile scheme and Performance plan"
      mg_status error
      return 1
    fi
    if [ -z "${MG_UI_PHYSICAL_DEVICE:-}" ]; then
      mg_failure physical-device "physical builds require the explicitly verified iOS destination"
      mg_status error
      return 1
    fi
    core_args=(--slices mac,sim,device)
    signing=team
  fi
  if [ "$platform" = mac ] && [ "$personal" != true ] && ! mg_word_in "$config" "Debug Profile"; then
    mg_failure usage "the Mac builds only Debug and Profile here: $config carries the daily-driver ids (tools/mg xcode-check builds and unregisters it)"
    mg_status error
    return 1
  fi
  if [ "$signing" = team ]; then
    settings=(${settings[0]+"${settings[@]}"} MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration)
    if [ "$action/$platform/$config/$plan/$scheme" = build-for-testing/ios-sim/Debug/CloudAgent/Meguri ]; then
      # Xcode otherwise selects simulator ad-hoc signing despite team settings.
      # Keep certificate selection inside the explicit signed agent build.
      settings=(${settings[0]+"${settings[@]}"} "CODE_SIGN_IDENTITY=Apple Development")
    fi
  fi
  if [ "$personal" = true ]; then
    # A command-line setting applies to every target before compilation/signing.
    # Do not change the Release defaults or export an override to nested jobs.
    settings=(${settings[0]+"${settings[@]}"} MEGURI_BUNDLE_ID_SUFFIX=.dev MEGURI_CLOUD_CONFIGURATION=development)
  fi
  if mg_app_mac_agent_profile_scope "$signing" "$action" "$platform" "$config" "$plan" "$scheme" &&
      [ -n "${MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE:-}" ]; then
    MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE="$(mktemp -d "${TMPDIR:-/tmp}/meguri-mac-agent-profile.XXXXXX")" || return 1
    chmod 700 "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE" || return 1
    mg_on_exit "rm -rf $(mvl_q "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE")"
    if ! MG_CLOUD_AGENT_MAC_PROFILE_BINDING="$(python3 -B -I "$MG_TOOLS/scripts/cloud-agent-mac-profile.py" \
        --root "$MG_ROOT" --file "$MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE" \
        2>"$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE/validation.log")"; then
      mg_app_mac_agent_profile_refusal
      mg_status fail
      return 1
    fi
    settings=(${settings[0]+"${settings[@]}"} MEGURI_MAC_AGENT_PROFILE_STYLE=Manual
      "MEGURI_MAC_AGENT_PROFILE_SPECIFIER=$(jq -r .uuid <<<"$MG_CLOUD_AGENT_MAC_PROFILE_BINDING")")
  fi
  mg_lint_ui_first || return 1
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json" --configuration "$config" ${core_args[0]+"${core_args[@]}"}; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_message "$MG_JOB: building the core XCFramework failed"
    mg_status fail
    return 1
  fi
  if [ "$personal" = true ] && ! mg_app_personal_settings ${settings[0]+"${settings[@]}"}; then
    mg_failure personal-preview-settings "personal preview requires Release settings, development app/widget identities, normal sandbox entitlements and no debug/test hooks; see log.txt"
    mg_status fail
    return 1
  fi
  case "$platform" in
    mac) dest="platform=macOS,arch=arm64" ;;
    ios-device) dest="platform=iOS,id=$MG_UI_PHYSICAL_DEVICE" ;;
    *) dest="generic/platform=iOS Simulator" ;;
  esac
  if [ "$platform" = mac ]; then
    mg_app_remove_embedded_widget mac "$config" || return 1
  elif [ "$signing/$action/$platform/$config/$plan/$scheme" = team/build-for-testing/ios-sim/Debug/CloudAgent/Meguri ]; then
    # Initial signing changes the embedded widget after Xcode returns.
    # Re-embed Xcode's native widget before verifying the next build's inputs.
    mg_app_remove_embedded_widget ios-sim "$config" || return 1
  fi
  if [ "$action" = build-for-testing ] && [ "$config/$plan/$scheme" = Debug/CloudAgent/Meguri ]; then
    case "$platform" in mac|ios-sim) build_command=(mg_app_cloud_agent_build) ;; esac
  fi
  result="$MG_RUN_DIR/build-$platform-$config-$plan.xcresult"
  log="$MG_RUN_DIR/build-$platform-$config-$plan.log"
  [ -z "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ] || log="$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE/native-build.log"
  rm -rf "$result"
  echo "$MG_JOB: xcodebuild $action ($platform, $config)" >&2
  "${build_command[@]}" "$action" -project app/Meguri.xcodeproj -scheme "$scheme" ${test_args[0]+"${test_args[@]}"} -configuration "$config" \
    -destination "$dest" -derivedDataPath "$MG_APP_DD" -resultBundlePath "$result" ${settings[0]+"${settings[@]}"} >"$log" 2>&1
  rc=$?
  if [ -z "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ]; then
    mg_artifact log "$log" "xcodebuild $action ($platform, $config)"
  fi
  counts="$(xcrun xcresulttool get build-results --path "$result" 2>/dev/null |
    jq -c '{errors: .errorCount, warnings: .warningCount, messages: [(.errors[]?, .warnings[]?) | .message] | .[:10]}')" || counts=null
  [ -z "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ] || counts="$(jq -c 'del(.messages)' <<<"${counts:-null}")"
  mg_detail build "$(jq -nc --arg p "$platform" --arg c "$config" --arg app "$(mg_app_products "$platform" "$config")/Meguri.app" \
    --arg action "$action" --arg plan "$plan" --arg dd "$MG_APP_DD" --argjson personal "$personal" --argjson counts "${counts:-null}" \
    '{platform: $p, config: $c, app: $app, action: $action, test_plan: $plan, standalone: ($action == "build" and ($personal | not)), personal_preview: $personal, derived_data: $dd} + ($counts // {})')"
  if [ "$rc" -ne 0 ]; then
    [ "$plan" != CloudAgent ] || plan_arg=" --test-plan $plan"
    if [ "$platform" = ios-device ] || [ -n "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ]; then
      # Signing diagnostics can include selected-device identities and paths.
      mg_failure build "physical development build failed; diagnostics remain in the private build log"
    else
      mg_failure build "xcodebuild $action failed ($platform, $config): $(grep -E 'error:' "$log" | head -n 3 | tr '\n' ' ')" \
        "tools/mg build-app --platform $platform --config $config$standalone_arg$plan_arg"
    fi
    if [ -n "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ]; then
      mg_message "$MG_JOB: scoped Mac build failed; native diagnostics remain private"
    else
      mg_message "$MG_JOB: the $platform $config build failed; see $log"
    fi
    mg_status fail
    return 1
  fi
  if [ "$action/$personal" = build/false ]; then
    # Resolve the preview with its build's destination, products and overrides.
    if ! xcodebuild -showBuildSettings -json -project app/Meguri.xcodeproj -scheme "$scheme" -configuration "$config" \
        -destination "$dest" -derivedDataPath "$MG_APP_DD" ${settings[0]+"${settings[@]}"} >"$MG_RUN_DIR/standalone-settings.json"; then
      mg_failure standalone-product "normal Debug preview settings could not be resolved; see log.txt"
      mg_status fail
      return 1
    fi

  fi
  if [ "$config" = Profile ] && [ "$plan" = Performance ]; then
    sdk=iphonesimulator
    [ "$platform" != mac ] || sdk=macosx
    [ "$platform" != ios-device ] || sdk=iphoneos
    # Select exactly as the VM consumer does, without copying or launching.
    xctestrun="$(MG_APP_TEST_SCHEME="$scheme" mg__vm_xctestrun "$plan" "$sdk" "$config")"
    policy="$(python3 "$MG_TOOLS/scripts/performance-test-policy.py" "$xctestrun" "$MG_ROOT/app/TestPlans/Performance.xctestplan")"
    rc=$?
    [ -z "$policy" ] || mg_detail performance_test_policy "$policy"
    if [ "$rc" -ne 0 ]; then
      mg_failure performance-test-policy "Profile Performance checker policy is unavailable or unsafe; require disabled source checker policies, generated UI-app checker flags false or omitted, and no MainThreadChecker/libRPAC environment injection; fix the Performance plan/Profile scheme and regenerate (do not edit the xctestrun)"
      mg_message "$MG_JOB: generated performance test policy failed; no UI execution is allowed"
      mg_status fail
      return 1
    fi
  fi
  # Scoped UI callers deliver products next and have no CloudAgent freeze.
  if mg_app_mac_agent_profile_scope "$signing" "$action" "$platform" "$config" "$plan" "$scheme" &&
      [ "$plan" = UI ] && [ -n "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ] &&
      ! mg_app_mac_agent_profile_check; then
    mg_failure agent-profile "App-only Mac profile changed before drive VM delivery"
    mg_status fail
    return 1
  fi
  return 0
}

# mg_app_input_digest: a sha256 of the sources an app build is made from,
# app/ and core/ as the working tree holds them (committed or not; ignored
# files and app/Snapshots/, whose baselines are no input, left out). Jobs that
# build the same sources print the same digest: snapshots and a11y record it
# (details.input), and the status ledger counts an audit only for the
# sources its snapshots rendered. Uses a copy of the index, so the real one
# and the staging area are untouched; prints nothing when git cannot.
mg_app_input_digest() {
  local index paths=() p tree
  for p in app core; do [ ! -e "$MG_ROOT/$p" ] || paths[${#paths[@]}]="$p"; done
  index="$(mktemp "${TMPDIR:-/tmp}/mg-input-index.XXXXXX")" || return 0
  cp "$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-path index 2>/dev/null)" "$index" 2>/dev/null || rm -f "$index"
  if [ "${#paths[@]}" -gt 0 ]; then
    GIT_INDEX_FILE="$index" git -C "$MG_ROOT" add -A -- "${paths[@]}" ':(exclude)app/Snapshots' >/dev/null 2>&1 || { rm -f "$index"; return 0; }
  fi
  tree="$(GIT_INDEX_FILE="$index" git -C "$MG_ROOT" write-tree 2>/dev/null)"
  rm -f "$index"
  [ -n "$tree" ] || return 0
  git -C "$MG_ROOT" ls-tree -r "$tree" -- app core 2>/dev/null | grep -v "$(printf '\t')app/Snapshots/" | shasum -a 256 | cut -c1-64
}

# Inspect resolved settings before any native app build. The suffix must reach
# both targets, while Release keeps its native launch/core and uses isolated development cloud.
mg_app_personal_settings() {
  local settings="$MG_RUN_DIR/personal-preview-settings.json"
  xcodebuild -showBuildSettings -json -project app/Meguri.xcodeproj -target Meguri -target MeguriWidget \
    -configuration Release -sdk macosx -arch arm64 \
    "$@" >"$settings" || return 1
  jq -e '
    type == "array" and
    ([.[] | select(.target == "Meguri" or .target == "MeguriWidget")] | length == 2) and
    all(.[] | select(.target == "Meguri" or .target == "MeguriWidget");
      .target as $target | .buildSettings |
      .CONFIGURATION == "Release" and .MEGURI_BUNDLE_ID_SUFFIX == ".dev" and
      .MEGURI_CLOUD_CONFIGURATION == "development" and
      .MEGURI_CLOUD_ENVIRONMENT == "development" and
      .MEGURI_CLOUD_CONTAINER == .MEGURI_CLOUD_CONTAINER_development and
      .MEGURI_CLOUD_CONTAINER != .MEGURI_CLOUD_CONTAINER_production and
      .PRODUCT_BUNDLE_IDENTIFIER == (if $target == "Meguri" then "com.antoinefink.meguri.dev" else "com.antoinefink.meguri.dev.widget" end) and
      .MEGURI_CORE_FEATURES == "ankiweb" and .MEGURI_SWIFT_CONDITIONS == "ANKIWEB_ENABLED" and
      ((.SWIFT_ACTIVE_COMPILATION_CONDITIONS // "" | test("(^|[ ,])DEBUG|MEGURI_TEST_HOOKS")) | not) and
      .MEGURI_CARD_INSPECTION_BUILD == "personalRelease" and .MEGURI_MAC_UI_ELEMENT == "NO" and
      # Xcode omits empty settings from its JSON; the exact entitlement path
      # below independently confirms that no hooks or ad-hoc suffix resolved.
      (.MEGURI_ENTITLEMENTS_HOOKS_SUFFIX // "") == "" and (.MEGURI_ENTITLEMENTS_SIGNING_SUFFIX // "") == "" and
      .CODE_SIGN_ENTITLEMENTS == (if $target == "Meguri" then "Meguri/Meguri-macOS.entitlements" else "MeguriWidget/MeguriWidget-macOS.entitlements" end) and
      .ENABLE_TESTABILITY == "NO" and .ENABLE_DEBUG_DYLIB == "NO" and .ENABLE_HARDENED_RUNTIME == "YES")
  ' "$settings" >/dev/null || return 1
  python3 "$MG_TOOLS/tests/cloud_build_configuration.py" --settings "$settings" \
    --configuration Release --platform mac --signing team --personal-preview
}

# Profiles are checked in memory and never published. Validate the signed
# identity/grants against an unexpired development profile, including this Mac.
mg_app_personal_profile() {
  local bundle="$1" ent="$2" hardware="$3" cert_dir rc
  [ -f "$bundle/Contents/embedded.provisionprofile" ] || return 1
  cert_dir="$(mktemp -d "$MG_RUN_DIR/personal-signer.XXXXXX")" || return 1
  if ! codesign -d --extract-certificates="$cert_dir/cert" "$bundle" >/dev/null; then
    rm -rf "$cert_dir"
    return 1
  fi
  security cms -D -i "$bundle/Contents/embedded.provisionprofile" | python3 -I -c '
import datetime, fnmatch, json, plistlib, sys
def profile_authorizes(key, grant, claim):
    # Only services/environment have authorization forms beyond exact equality.
    if key == "com.apple.developer.icloud-services":
        if claim != ["CloudKit"]:
            return False
        if grant == "*":
            return True
        choices, required = ("CloudKit", "CloudDocuments"), "CloudKit"
    elif key == "com.apple.developer.icloud-container-environment":
        if claim != "Development":
            return False
        if grant == "Development":
            return True
        choices, required = ("Development", "Production"), "Development"
    else:
        return grant == claim
    return (isinstance(grant, list) and bool(grant) and
            all(isinstance(item, str) and item in choices for item in grant) and
            len(grant) == len(set(grant)) and required in grant)

try:
    profile = plistlib.loads(sys.stdin.buffer.read())
    signed = json.loads(sys.argv[1])
    grants = profile["Entitlements"]
    team = signed["com.apple.developer.team-identifier"]
    identity = signed.get("com.apple.application-identifier", signed.get("application-identifier"))
    allowed = grants.get("com.apple.application-identifier", grants.get("application-identifier"))
    assert team in profile["TeamIdentifier"], "team membership"
    assert grants["com.apple.developer.team-identifier"] == team, "team grant"
    assert isinstance(identity, str) and fnmatch.fnmatchcase(identity, allowed), "identity grant"
    # Mac device-bound development profiles omit debugger grants. Release
    # products must not carry that entitlement even when a profile permits it.
    assert "OSX" in profile["Platform"], "Mac profile"
    assert profile["DeveloperCertificates"] and all(isinstance(cert, bytes) and cert for cert in profile["DeveloperCertificates"]), "developer certificates"
    with open(sys.argv[3], "rb") as certificate:
        assert certificate.read() in profile["DeveloperCertificates"], "authorized signer"
    assert all(grants[key] is True for key in ("get-task-allow", "com.apple.security.get-task-allow") if key in grants), "development profile"
    assert profile["ExpirationDate"] > datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None), "expiry"
    assert sys.argv[2] in profile["ProvisionedDevices"], "device coverage"
    if "com.apple.developer.icloud-container-identifiers" in signed:
        assert grants.get("com.apple.developer.icloud-container-identifiers") == signed["com.apple.developer.icloud-container-identifiers"], "isolated cloud container grant"
        for key in ("com.apple.developer.icloud-services", "com.apple.developer.icloud-container-environment", "com.apple.developer.aps-environment"):
            assert profile_authorizes(key, grants.get(key), signed[key]), "cloud/push grant"
    for key in ("keychain-access-groups", "com.apple.security.application-groups"):
        for value in signed.get(key, []):
            assert any(fnmatch.fnmatchcase(value, grant) for grant in grants.get(key, [])), "signed group grant"
except AssertionError as error:
    print("personal preview provisioning check: " + str(error), file=sys.stderr)
    sys.exit(1)
except (KeyError, TypeError, ValueError, OSError, plistlib.InvalidFileException) as error:
    print("personal preview provisioning parse: " + type(error).__name__, file=sys.stderr)
    sys.exit(1)
' "$ent" "$hardware" "$cert_dir/cert0"
  rc=$?
  rm -rf "$cert_dir"
  return "$rc"
}

mg_app_personal_check() {
  local app="$1" widget="$1/Contents/PlugIns/MeguriWidget.appex" bundle xml ent info team="" identity
  local app_ent widget_ent app_info
  local hardware stamp hash slices app_hash widget_hash app_profile_hash widget_profile_hash="" digest signed_info cert_dir widget_provision=not_embedded
  codesign --verify --deep --strict "$app" && codesign --verify --strict "$widget" || return 1
  hardware="$(system_profiler SPHardwareDataType -json | jq -er '.SPHardwareDataType[0] |
    (.provisioning_UDID // .platform_UUID) | select(type == "string" and length > 0)')" || return 1
  [ -n "$hardware" ] || return 1
  for bundle in "$app" "$widget"; do
    xml="$(codesign -d --entitlements - --xml "$bundle")" || return 1
    ent="$(plutil -convert json -o - - <<<"$xml")" || return 1
    [ -n "$team" ] || team="$(jq -er '.["com.apple.developer.team-identifier"] | select(type == "string" and test("^[A-Z0-9]{10}$"))' <<<"$ent")" || return 1
    identity=com.antoinefink.meguri.dev
    [ "$bundle" = "$app" ] || identity="$identity.widget"
    signed_info="$(codesign -d --verbose=4 "$bundle" 2>&1)" || return 1
    [ "$(sed -n 's/^Identifier=//p' <<<"$signed_info")" = "$identity" ] &&
      [ "$(sed -n 's/^TeamIdentifier=//p' <<<"$signed_info")" = "$team" ] || return 1
    jq -e --arg team "$team" --arg identity "$identity" '
      type == "object" and .["com.apple.security.app-sandbox"] == true and
      ((.["com.apple.security.get-task-allow"] // false) == false) and ((.["get-task-allow"] // false) == false) and
      .["com.apple.security.application-groups"] == [$team + ".com.antoinefink.meguri.dev"] and
      ([keys[] | select(startswith("com.apple.security.temporary-exception."))] == []) and
      ([keys[] | select(startswith("com.apple.security.cs."))] == [])
    ' <<<"$ent" >/dev/null || return 1
    if [ "$bundle" = "$app" ]; then
      jq -e --arg team "$team" --arg identity "$identity" '
        .["com.apple.developer.team-identifier"] == $team and
        (.["com.apple.application-identifier"] // .["application-identifier"]) == ($team + "." + $identity) and
        .["keychain-access-groups"] == [$team + ".com.antoinefink.meguri.dev"] and
        .["com.apple.security.network.client"] == true and .["com.apple.security.network.server"] == true and
        .["com.apple.security.files.user-selected.read-write"] == true and .["com.apple.security.files.bookmarks.app-scope"] == true
      ' <<<"$ent" >/dev/null || return 1
      mg_app_personal_profile "$bundle" "$ent" "$hardware" || return 1
      app_ent="$ent"
    else
      # Native Mac widgets need no profile/identity entitlement for their App
      # Group. Their signed metadata and certificate still bind them to the app.
      jq -e --arg team "$team" --arg identity "$identity" '
        (.["keychain-access-groups"] // []) == [] and
        (.["com.apple.developer.team-identifier"] // $team) == $team and
        (.["com.apple.application-identifier"] // .["application-identifier"] // ($team + "." + $identity)) == ($team + "." + $identity)
      ' <<<"$ent" >/dev/null || return 1
      widget_ent="$ent"
      if [ -f "$bundle/Contents/embedded.provisionprofile" ]; then
        mg_app_personal_profile "$bundle" "$ent" "$hardware" || return 1
        widget_provision=verified
      fi
    fi
    info="$(plutil -convert json -o - "$bundle/Contents/Info.plist")" || return 1
    jq -e --arg identity "$identity" '.CFBundleIdentifier == $identity' <<<"$info" >/dev/null || return 1
    if [ "$bundle" = "$app" ]; then
      app_info="$info"
      jq -e '.CFBundleExecutable == "Meguri" and .MeguriCardInspectionBuild == "personalRelease" and .LSUIElement == false' <<<"$info" >/dev/null || return 1
    else
      jq -e '.CFBundleExecutable == "MeguriWidget"' <<<"$info" >/dev/null || return 1
    fi
  done
  # Use the same cloud policy for resolved settings and signed product facts.
  python3 -B - "$MG_TOOLS/tests" "$MG_RUN_DIR/personal-preview-settings.json" "$app_info" "$app_ent" "$widget_ent" <<'PY_CLOUD' || return 1
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from cloud_build_configuration import check
settings = json.loads(Path(sys.argv[2]).read_text())
app = next(target["buildSettings"] for target in settings if target["target"] == "Meguri")
check(app, "Release", "mac", "team", *(json.loads(value) for value in sys.argv[3:]), personal_preview=True)
PY_CLOUD
  cert_dir="$(mktemp -d "$MG_RUN_DIR/personal-widget-signer.XXXXXX")" || return 1
  if ! codesign -d --extract-certificates="$cert_dir/app" "$app" >/dev/null ||
      ! codesign -d --extract-certificates="$cert_dir/widget" "$widget" >/dev/null ||
      ! cmp -s "$cert_dir/app0" "$cert_dir/widget0"; then
    rm -rf "$cert_dir"
    return 1
  fi
  rm -rf "$cert_dir"
  [ "$(lipo -archs "$app/Contents/MacOS/Meguri")" = arm64 ] &&
    [ "$(lipo -archs "$widget/Contents/MacOS/MeguriWidget")" = arm64 ] || return 1
  stamp="$(cat "$MG_ROOT/app/Packages/MeguriCore/Binaries/.stamp")" || return 1
  grep -qx 'configuration=Release' <<<"$stamp" && grep -qx 'profile=release' <<<"$stamp" &&
    grep -qx 'features=ankiweb' <<<"$stamp" || return 1
  hash="$(sed -n 's/^hash=//p' <<<"$stamp")"
  printf '%s\n' "$hash" | grep -Eq '^[0-9a-f]{32}$' || return 1
  slices="$(sed -n 's/^slices=//p' <<<"$stamp")"
  printf '%s\n' "$slices" | tr ',' '\n' | grep -qx mac || return 1
  app_hash="$(shasum -a 256 "$app/Contents/MacOS/Meguri" | cut -c1-64)"
  widget_hash="$(shasum -a 256 "$widget/Contents/MacOS/MeguriWidget" | cut -c1-64)"
  app_profile_hash="$(shasum -a 256 "$app/Contents/embedded.provisionprofile" | cut -c1-64)"
  [ "$widget_provision" != verified ] || widget_profile_hash="$(shasum -a 256 "$widget/Contents/embedded.provisionprofile" | cut -c1-64)"
  for digest in "$app_hash" "$widget_hash" "$app_profile_hash" ${widget_profile_hash:+"$widget_profile_hash"}; do
    printf '%s\n' "$digest" | grep -Eq '^[0-9a-f]{64}$' || return 1
  done
  mg_detail personal_preview_product "$(jq -nc --arg hash "$app_hash" --arg widget_hash "$widget_hash" \
    --arg app_profile "$app_profile_hash" --arg widget_profile "$widget_profile_hash" --arg widget_provision "$widget_provision" \
    --arg core_hash "$hash" --arg slices "$slices" '
    {signature_verified: true, provisioning_verified: true, architecture: {app: "arm64", widget: "arm64"},
     executable_sha256: $hash, widget_executable_sha256: $widget_hash,
     provisioning: {app: "verified", widget: $widget_provision}, widget_signer_matches_app: true,
     provisioning_sha256: {app: $app_profile, widget: (if $widget_profile == "" then null else $widget_profile end)},
     bundle_id: "com.antoinefink.meguri.dev", widget_bundle_id: "com.antoinefink.meguri.dev.widget",
     cloud_environment: "development", card_inspection_build: "personalRelease", native_visible_application: true, fixture_read_scope: false, test_hooks: false,
     core: {hash: $core_hash, configuration: "Release", profile: "release", features: "ankiweb", slices: $slices}}')"
}

# Read signed facts, never repair or re-sign a product. These are the normal
# Debug checks from xcode_project_check.sh (KTD22, fixture scope, stamp, arm64),
# without that script's ad-hoc-only key inventory: team/profile/Keychain/App
# Group keys must survive. No test runner or xctestrun is needed here.
mg_app_standalone_check() {
  local app="$1" widget="$1/Contents/PlugIns/MeguriWidget.appex" bundle xml ent info app_info app_ent="" widget_ent=""
  local stamp hash slices executable_hash widget_hash
  codesign --verify --deep --strict "$app" && codesign --verify --strict "$widget" || return 1
  for bundle in "$app" "$widget"; do
    xml="$(codesign -d --entitlements - --xml "$bundle")" || return 1
    ent="$(plutil -convert json -o - - <<<"$xml")" || return 1
    jq -e 'type == "object" and .["com.apple.security.app-sandbox"] == true
      and .["com.apple.security.get-task-allow"] == true' <<<"$ent" >/dev/null || return 1
    if [ "$bundle" = "$app" ]; then
      # Exact array equality rejects root read, extra paths and invalid shape.
      # Reject every other filesystem exception, including write variants.
      jq -e --arg fixture "$MG_ROOT/" '
        .["com.apple.security.network.client"] == true
        and .["com.apple.security.network.server"] == true
        and .["com.apple.security.files.user-selected.read-write"] == true
        and .["com.apple.security.files.bookmarks.app-scope"] == true
        and .["com.apple.security.temporary-exception.files.absolute-path.read-only"] == [$fixture]
        and ([keys[] | select(startswith("com.apple.security.temporary-exception.files."))]
          == ["com.apple.security.temporary-exception.files.absolute-path.read-only"])' <<<"$ent" >/dev/null || return 1
      app_ent="$ent"
    else
      jq -e '[keys[] | select(startswith("com.apple.security.temporary-exception.files."))] == []' \
        <<<"$ent" >/dev/null || return 1
      widget_ent="$ent"
    fi
  done
  [ "$(lipo -archs "$app/Contents/MacOS/Meguri")" = arm64 ] &&
    [ "$(lipo -archs "$widget/Contents/MacOS/MeguriWidget")" = arm64 ] || return 1
  info="$(plutil -convert json -o - "$app/Contents/Info.plist")" || return 1
  jq -e '.CFBundleIdentifier == "com.antoinefink.meguri.dev" and .CFBundleExecutable == "Meguri"
    and .MeguriCardInspectionBuild == "debug" and .LSUIElement == true' <<<"$info" >/dev/null || return 1
  app_info="$info"
  info="$(plutil -convert json -o - "$widget/Contents/Info.plist")" || return 1
  jq -e '.CFBundleIdentifier == "com.antoinefink.meguri.dev.widget" and .CFBundleExecutable == "MeguriWidget"' \
    <<<"$info" >/dev/null || return 1
  python3 -B - "$MG_TOOLS/tests" "$MG_RUN_DIR/standalone-settings.json" "$app_info" "$app_ent" "$widget_ent" <<'PY_CLOUD' || return 1
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from cloud_build_configuration import check, require
settings = json.loads(Path(sys.argv[2]).read_text())
apps = [target["buildSettings"] for target in settings if target["target"] == "Meguri"]
require(len(apps) == 1, "expected exactly one Meguri target in resolved settings")
check(apps[0], "Debug", "mac", apps[0]["MEGURI_SIGNING"], *(json.loads(value) for value in sys.argv[3:]))
PY_CLOUD
  stamp="$(cat "$MG_ROOT/app/Packages/MeguriCore/Binaries/.stamp")" || return 1
  grep -qx 'configuration=Debug' <<<"$stamp" && grep -qx 'profile=dev' <<<"$stamp" &&
    grep -qx 'features=ankiweb,test-hooks' <<<"$stamp" || return 1
  hash="$(sed -n 's/^hash=//p' <<<"$stamp")"
  printf '%s\n' "$hash" | grep -Eq '^[0-9a-f]{32}$' || return 1
  slices="$(sed -n 's/^slices=//p' <<<"$stamp")"
  printf '%s\n' "$slices" | tr ',' '\n' | grep -qx mac || return 1
  executable_hash="$(shasum -a 256 "$app/Contents/MacOS/Meguri" | cut -c1-64)"
  widget_hash="$(shasum -a 256 "$widget/Contents/MacOS/MeguriWidget" | cut -c1-64)"
  printf '%s\n' "$executable_hash" | grep -Eq '^[0-9a-f]{64}$' || return 1
  printf '%s\n' "$widget_hash" | grep -Eq '^[0-9a-f]{64}$' || return 1
  mg_detail standalone_product "$(jq -nc --arg hash "$executable_hash" --arg widget_hash "$widget_hash" \
    --arg core_hash "$hash" --arg slices "$slices" --argjson app "$app_ent" --argjson widget "$widget_ent" '
    def normal: with_entries(select(.key | startswith("com.apple.security.")))
      | del(.["com.apple.security.application-groups"]);
    {signature_verified: true, cloud_configuration_verified: true, architecture: {app: "arm64", widget: "arm64"},
     executable_sha256: $hash, widget_executable_sha256: $widget_hash,
     bundle_id: "com.antoinefink.meguri.dev", card_inspection_build: "debug",
     core: {hash: $core_hash, configuration: "Debug", profile: "dev", features: "ankiweb,test-hooks", slices: $slices},
     normal_entitlements: {app: ($app | normal), widget: ($widget | normal)}}')"
}

# mg_app_xcodebuild_test <out prefix> <mac|ios|ios-device> <destination> <config> [xcodebuild args]:
# test-without-building of the UI plan (or MG_APP_TEST_PLAN with the scheme
# MG_APP_TEST_SCHEME, as perf-ui sets them) into <prefix>.xcresult and <prefix>.log.
# Mac runs go where mg_mac_ui_begin sent them (MG_MAC_RUNNER), simulator runs
# where mg_sim_begin did (MG_SIM_RUNNER): to the test VM (mac-vm.sh) or this
# Mac. ios-device runs on the host only for ui-test's verified destination.
# Returns 0 pass, 1 test failures, 2 could not run (records a user step
# when macOS has not allowed UI automation), 4 queued: the test VM could not
# be reached (mg_vm_xcodebuild_test recorded the queued part, user step
# mac-vm).
mg_app_xcodebuild_test() {
  local prefix="$1" platform="$2" dest="$3" config="$4" rc summary sdk="" physical_args=() bundle
  shift 4
  if [ "$platform" = ios-device ]; then
    if [ -z "${MG_UI_PHYSICAL_DEVICE:-}" ] || [ "$dest" != "platform=iOS,id=$MG_UI_PHYSICAL_DEVICE" ] ||
        ! mg_word_in "$config" "Debug Profile"; then
      mg_failure physical-device "physical tests require the explicitly verified iOS destination and Debug or Profile"
      return 2
    fi
    bundle="$(plutil -extract CFBundleIdentifier raw -o - "$(mg_app_products ios-device "$config")/Meguri.app/Info.plist" 2>/dev/null)"
    if [ "$bundle" != com.antoinefink.meguri.dev ]; then
      mg_failure physical-device-bundle "refusing physical tests: built app must have CFBundleIdentifier com.antoinefink.meguri.dev"
      return 2
    fi
    physical_args=(MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration -parallel-testing-enabled NO -destination-timeout 60
      -test-timeouts-enabled YES)
  fi
  rm -rf "$prefix.xcresult"
  mkdir -p "$(dirname "$prefix")"
  if [ "$platform" = mac ] && [ "$MG_MAC_RUNNER" = vm ]; then
    sdk=macosx
  elif [ "$platform" != mac ] && [ "$platform" != ios-device ] && [ "$MG_SIM_RUNNER" = vm ]; then
    sdk=iphonesimulator
  fi
  if [ -n "$sdk" ]; then
    mg_vm_xcodebuild_test "$prefix" "${MG_APP_TEST_PLAN:-UI}" "$sdk" "$dest" "$config" "$@"
    rc=$?
    mg_artifact log "$prefix.log" "xcodebuild test ($platform, in the test VM)"
    [ -z "$MG_MAC_VM_RUN_QUEUED" ] || return 4
    [ -z "$MG_MAC_VM_RUN_ERROR" ] || return 2
  else
    # Destination artifacts reuse only the admitted installed runner/app. This
    # invocation has no project, scheme, build settings or provisioning updates.
    if [ -n "${MG_DRIVE_DESTINATION_XCTESTRUN:-}" ]; then
      if [ "$platform" != ios-device ] || [ "$config" != Debug ] ||
          [ "$MG_DRIVE_DESTINATION_XCTESTRUN" != "$MG_RUN_DIR/physical/destination.xctestrun" ]; then
        mg_failure physical-preservation "destination artifact selection refused"
        return 2
      fi
      xcodebuild test-without-building -xctestrun "$MG_DRIVE_DESTINATION_XCTESTRUN" \
        -destination "$dest" -resultBundlePath "$prefix.xcresult" -collect-test-diagnostics never \
        -parallel-testing-enabled NO -destination-timeout 60 -test-timeouts-enabled YES \
        "$@" >"$prefix.log" 2>&1
      rc=$?
    else
      # No diagnostics on failure: they take up to 10 minutes (simctl diagnose)
      # and, on the Mac, are a sysdiagnose of the user's machine.
      xcodebuild test-without-building -project app/Meguri.xcodeproj -scheme "${MG_APP_TEST_SCHEME:-Meguri}" \
        -testPlan "${MG_APP_TEST_PLAN:-UI}" -configuration "$config" \
        -destination "$dest" -derivedDataPath "$MG_APP_DD" -resultBundlePath "$prefix.xcresult" -collect-test-diagnostics never \
        ${physical_args[0]+"${physical_args[@]}"} "$@" >"$prefix.log" 2>&1
      rc=$?
    fi
    mg_artifact log "$prefix.log" "xcodebuild test ($platform)"
  fi
  summary="$(xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -r '"\(.result) passed=\(.passedTests) failed=\(.failedTests) skipped=\(.skippedTests)"')"
  echo "$MG_JOB: xcodebuild test ($platform): exit $rc ${summary:-no result}" >&2
  if [ "$platform" != ios-device ] && grep -q 'failed to initialize for UI testing' "$prefix.log"; then
    if [ "$platform" = mac ] && [ "$MG_MAC_RUNNER" = vm ]; then
      mg_user_step uitest-automation missing true "Let the Xcode UI-test runner control the test VM" \
        "In the test VM: sudo automationmodetool enable-automationmode-without-authentication (tools/vm/setup-guest.sh)."
    else
      mg_user_step uitest-automation missing true "Let the Xcode UI-test runner control the Mac" \
        "System Settings > Privacy & Security > Accessibility and Automation: allow the UI-test runner (docs/process/one-time-setup.md)."
    fi
    mg_failure blocked-uitest-automation "macOS has not allowed UI automation; the tests did not run"
    return 2
  fi
  [ "$rc" -eq 0 ] && return 0
  [ -n "$summary" ] && return 1
  mg_failure xcodebuild-test "xcodebuild test-without-building could not run (exit $rc); see $prefix.log"
  return 2
}

# mg_app_log_now: the local time where the Mac tests run (this Mac, or the
# test VM), as `log show --start` takes it.
mg_app_log_now() {
  local t
  if [ "$MG_MAC_RUNNER" = vm ]; then
    t="$(mg_mac_vm_now 2>/dev/null)"
    case "$t" in [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]\ [0-9][0-9]:[0-9][0-9]:[0-9][0-9]) printf '%s\n' "$t" && return 0 ;; esac
  fi
  date '+%Y-%m-%d %H:%M:%S'
}

# mg_app_mac_input_refused <since> [part]: whether macOS dropped input that
# XCUITest synthesized since then, on this Mac or in the test VM
# (MG_MAC_RUNNER). On macOS 27.0.1 (26A434) WindowServer refuses
# testmanagerd's virtual input service ("missing the
# 'com.apple.private.hid.client.event-dispatch' entitlement"), testmanagerd
# times out on its "Recap playback", and no click, key press or drag reaches
# the app: a Mac UI run with input then tests nothing it acts on. Records
# <part> (default: the job) as queued with the user step mac-input
# (mg_queued_part; the step once per job) and where it was seen, for doctor
# (mg_app_mac_input_record), and returns 0; the caller queues the run.
# Returns 1 when the unified log shows no such drop, or cannot be read.
mg_app_mac_input_refused() {
  local pred='process == "testmanagerd" AND eventMessage CONTAINS "Recap playback"' where repro
  if [ "$MG_MAC_RUNNER" = vm ]; then
    mg_mac_vm_log_show "$1" "$pred" 2>/dev/null | grep -q 'Timed out after waiting' || return 1
    where="in the test VM"
    repro="$(jq -r .ssh <<<"$MG_MAC_VM")$(mg__quote_args "$(mg_mac_vm_q sudo log show --start "$1" --predicate "$pred")")"
  else
    command -v log >/dev/null 2>&1 || return 1
    log show --style compact --start "$1" --predicate "$pred" 2>/dev/null | grep -q 'Timed out after waiting' || return 1
    where="on this Mac"
    repro="log show --start '$1' --predicate '$pred'"
  fi
  mg_queued_part mac-input "${2:-$MG_JOB}" "macOS dropped the input XCUITest synthesized $where (testmanagerd's Recap playback timed out)" "$repro"
  [ -z "${MG_MAC_INPUT_REFUSED:-}" ] || return 0
  MG_MAC_INPUT_REFUSED=1
  mg_user_step mac-input missing true "Use a macOS and Xcode pair whose Mac UI tests can click and type" \
    "macOS drops the input XCUITest synthesizes $where (on 27.0.1, WindowServer refuses testmanagerd's input service), so taps, typing and drags never reach the app. Update macOS or Xcode (docs/process/one-time-setup.md); tools/mg catch-up runs the queued runs again once doctor sees another pair."
  mg_detail blocked '"mac-input"'
  mg_app_mac_input_record
  return 0
}

# mg_app_mac_input_result <xcresult> <result> <failure prefix>: the result of
# a Mac XCUITest run whose input macOS dropped (mg_app_mac_input_refused said
# so), from its xcodebuild result (0 passed, 1 failed): 3 (queued) when it
# passed or when the drop explains every failing test, otherwise 1, with each
# failure the drop does not explain recorded as <prefix>/<test id>. The drop
# explains a failing test only when the test synthesized input (an activity
# "Synthesize event") and its first failure (the earliest activity associated
# with a failure) came at or after its first synthesized event, as for a
# drive's steps from its first tap on. A failure
# before it, a failing test that synthesized nothing (fixtures, dates,
# configuration), a test whose activities cannot be read and a failed run
# without a failing test tested the app regardless: they fail the run.
mg_app_mac_input_result() {
  local bundle="$1" prefix="$3" tests id n=0
  [ "$2" -eq 1 ] || return 3
  if ! tests="$(xcrun xcresulttool get test-results tests --path "$bundle" 2>/dev/null |
    jq -c '[.. | objects | select(.nodeType? == "Test Case" and .result? == "Failed") | {id: .nodeIdentifier,
      message: ([.children[]? | select(.nodeType == "Failure Message") | .name] | join("; ") | .[0:800])}]')" ||
    [ -z "$tests" ]; then
    mg_failure "$prefix/xcresult" "the run failed and its failing tests cannot be read from $bundle: the input macOS dropped does not explain the failure"
    return 1
  fi
  if [ "$tests" = '[]' ]; then
    mg_failure "$prefix/xcresult" "the run failed, but $bundle lists no failing test: the input macOS dropped does not explain it; see the log"
    return 1
  fi
  while IFS= read -r id; do
    xcrun xcresulttool get test-results activities --test-id "$id" --path "$bundle" 2>/dev/null |
      jq -e '[.testRuns[]? | [.activities[]? | .. | objects | select(has("startTime"))] as $a
          | ([$a[] | select(.title == "Synthesize event") | .startTime] | min) as $input
          | ([$a[] | select(.isAssociatedWithFailure == true) | .startTime] | min) as $failure
          | select($failure != null) | $input != null and $failure >= $input]
        | length > 0 and all' >/dev/null 2>&1 && continue
    n=$((n + 1))
    mg_failure "$prefix/$id" "$(jq -r --arg id "$id" 'first(.[] | select(.id == $id) | .message) // ""' <<<"$tests") (failed before or without the input macOS dropped)"
  done < <(jq -r '.[].id' <<<"$tests")
  [ "$n" -eq 0 ] && return 3
  return 1
}

# mg_app_mac_input_path: where the last drop is recorded, beside the leases
# every checkout shares (the main checkout's build/mg).
mg_app_mac_input_path() { printf '%s/mac-input.json\n' "$(mvl_lease_base "$MG_ROOT")"; }

# mg_app_mac_input_record: records the runner that dropped the input and its
# macOS and Xcode builds (doctor lists mac-input until one of them changes),
# also in details.mac_input. Values it cannot read are null.
mg_app_mac_input_record() {
  local out macos="" build="" xcode="" vm="" path
  local probe='echo "macos=$(sw_vers -productVersion 2>/dev/null)"; echo "build=$(sw_vers -buildVersion 2>/dev/null)"; xcodebuild -version 2>/dev/null'
  if [ "$MG_MAC_RUNNER" = vm ]; then
    vm="$(jq -r .name <<<"$MG_MAC_VM")"
    out="$(mg_mac_vm_ssh "$probe" 2>/dev/null)"
  else
    out="$(bash -c "$probe" 2>/dev/null)"
  fi
  macos="$(printf '%s\n' "$out" | sed -n 's/^macos=//p' | head -n 1)"
  build="$(printf '%s\n' "$out" | sed -n 's/^build=//p' | head -n 1)"
  xcode="$(printf '%s\n' "$out" | sed -n 's/^Xcode \([0-9.]*\).*/\1/p' | head -n 1)"
  [ -z "$xcode" ] || xcode="$xcode ($(printf '%s\n' "$out" | sed -n 's/^Build version \(.*\)/\1/p' | head -n 1))"
  out="$(jq -nc --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg job "$MG_JOB" --arg run "${MG_RUN_ID:-}" --arg wt "$MG_ROOT" \
    --arg runner "${MG_MAC_RUNNER:-host}" --arg vm "$vm" --arg m "$macos" --arg b "$build" --arg x "$xcode" '
    def v: if . == "" then null else . end;
    {at: $at, job: $job, run_id: $run, worktree: $wt, runner: $runner, vm: ($vm | v), macos: ($m | v),
     macos_build: ($b | v), xcode: ($x | v)}')"
  mg_detail mac_input "$out"
  path="$(mg_app_mac_input_path)"
  mkdir -p "$(dirname "$path")" && printf '%s\n' "$out" >"$path.tmp.$$" && mv -f "$path.tmp.$$" "$path"
}

# ---------------------------------------------------------------------------
# Simulators from the pool

# mg_sim_lease <kind> <release|keep>: leases a pool device for this job (in
# the test VM, sim-pool.sh); sets MG_SIM_UDID. With release, the device goes
# back to the pool (reset) when the job ends. On failure records it and
# returns 1; when the test VM could not be reached (sim-pool.sh exit 5) the
# simulator runs are queued (mg__mac_vm_unreachable, user step mac-vm).
mg_sim_lease() {
  local rc err="$MG_RUN_DIR/sim-pool-acquire-$1.err"
  MG_SIM_UDID="$(mg_sim_pool_run "$err" acquire "$1" --holder "$(basename "$MG_ROOT") $MG_JOB")"
  rc=$?
  case "$rc" in
    0) ;;
    4)
      mg_detail lane "$(jq -n --arg k "$1" '{kind: $k, status: "unavailable", reason: "toolchain"}')"
      mg_message "$MG_JOB: unavailable: toolchain (no $1 simulator in this Xcode)"
      mg_status warn
      return 1
      ;;
    3)
      mg_failure pool-busy "every $1 pool device stayed leased; see log.txt" "tools/mg sim status"
      mg_status error
      return 1
      ;;
    5)
      mg__mac_vm_unreachable "sim-pool.sh acquire $1: $(mg_sim_pool_queued_reason "$err")"
      return 1
      ;;
    6)
      mg_failure sim-vm "sim-pool.sh acquire $1: the test VM could not lease a simulator (exit $rc); see log.txt" "tools/mg doctor"
      mg_status error
      return 1
      ;;
    *)
      mg_failure sim-pool "sim-pool.sh acquire $1 exited with code $rc; see log.txt" "tools/mg sim status"
      mg_status error
      return 1
      ;;
  esac
  [ "$2" = keep ] || mg_on_exit "'$MG_TOOLS/scripts/sim-pool.sh' release '$MG_SIM_UDID' >&2"
  mg_detail device "$("$MG_TOOLS/scripts/sim-pool.sh" status 2>/dev/null | jq -c --arg u "$MG_SIM_UDID" 'first(.devices[] | select(.udid == $u) | {udid, name, kind}) // {udid: $u}')"
  return 0
}

# mg_sim_pool_run <stderr copy> <sim-pool.sh args>: sim-pool.sh, its stdout
# printed, its stderr on this job's stderr and in <stderr copy> (for the
# reason of an exit 5); returns its exit code.
mg_sim_pool_run() {
  local copy="$1" rc
  shift
  { "$MG_TOOLS/scripts/sim-pool.sh" "$@" 2>&1 1>&3 3>&- | tee "$copy" >&2 3>&-; rc="${PIPESTATUS[0]}"; } 3>&1
  return "$rc"
}

# mg_sim_pool_queued_reason <stderr copy>: why sim-pool.sh exited 5 (its
# last "sim-pool: queued: " line).
mg_sim_pool_queued_reason() {
  local r
  r="$(sed -n 's/^sim-pool: queued: //p' "$1" 2>/dev/null | tail -n 1)"
  printf '%s\n' "${r:-the test VM could not run the pool; see log.txt}"
}

# mg_sim_check_owned <udid>: a pool device leased from this checkout. Another
# checkout's device would get its app replaced. On failure records it and
# returns 1.
mg_sim_check_owned() {
  local rec
  rec="$("$MG_TOOLS/scripts/sim-pool.sh" status 2>/dev/null | jq -c --arg u "$1" 'first(.devices[] | select(.udid == $u)) // null')"
  if [ -z "$rec" ] || [ "$rec" = null ]; then
    mg_failure device "$1 is not a pool simulator; lease one with tools/mg sim acquire <kind>" "tools/mg sim status"
    mg_status error
    return 1
  fi
  if [ "$(jq -r '.lease.worktree // ""' <<<"$rec")" != "$MG_ROOT" ]; then
    mg_failure device "$1 ($(jq -r .name <<<"$rec")) is not leased from this checkout ($(jq -r 'if .lease == null then "not leased" else "leased to \(.lease.holder) in \(.lease.worktree)" end' <<<"$rec")); lease one with tools/mg sim acquire <kind>" \
      "tools/mg sim status"
    mg_status error
    return 1
  fi
  mg_detail device "$(jq -c '{udid, name, kind}' <<<"$rec")"
  return 0
}

# mg_sim_check_runtime <udid>: the device runs the pinned iOS runtime and
# build (tools/sim-devices.json, R1966), so nothing is installed, launched or
# captured on an unapproved OS build. Every job that uses a simulator checks
# it before touching the device. On a mismatch records it (error) and
# returns 1.
mg_sim_check_runtime() {
  local pinned rid actual
  pinned="$(jq -c .runtime tools/sim-devices.json)"
  rid="$(mg_simctl list devices --json 2>/dev/null |
    jq -r --arg u "$1" 'first(.devices | to_entries[] | select(any(.value[]; .udid == $u)) | .key) // empty')"
  actual="$(mg_simctl list runtimes --json 2>/dev/null |
    jq -c --arg r "$rid" 'first(.runtimes[] | select(.identifier == $r) | {identifier, version, build: .buildversion}) // null')"
  [ -n "$actual" ] || actual=null
  # A foldable pool device runs the foldable kind's own pinned runtime.
  if [ "$rid" = "$(jq -r .foldable.runtime.identifier tools/sim-devices.json)" ]; then
    pinned="$(jq -c .foldable.runtime tools/sim-devices.json)"
  fi
  mg_detail runtime "$(jq -nc --arg u "$1" --argjson e "$pinned" --argjson a "$actual" '{device: $u, expected: $e, actual: $a}')"
  if [ "$(jq -r '"\(.identifier) \(.build)"' <<<"$pinned")" = "$(jq -r 'if . == null then "none" else "\(.identifier) \(.build)" end' <<<"$actual")" ]; then
    return 0
  fi
  mg_failure runtime-mismatch "$1 runs $(jq -r --arg r "$rid" 'if . == null then "an unknown runtime \($r)" else "iOS \(.version) (\(.build), \(.identifier))" end' <<<"$actual"); tools/sim-devices.json pins iOS $(jq -r '"\(.version) (\(.build), \(.identifier))"' <<<"$pinned"). Nothing was installed or captured: captures are pinned to that build (R1966). Install the pinned runtime, or update the pin and re-record the baselines (R1974)." \
    "tools/mg sim-setup"
  mg_message "$MG_JOB: runtime build mismatch on $1: expected $(jq -r .build <<<"$pinned"), found $(jq -r '.build // "none"' <<<"$actual")"
  mg_status error
  return 1
}

# mg_sim_check_given <udid>: a --device the job was given is a pool simulator
# leased from this checkout, on the pinned runtime. Jobs check it before
# building, and again once they hold the host lease, before touching the
# device: while they queued, the lease may have been released and the device
# leased to another checkout. On failure records it and returns 1.
mg_sim_check_given() {
  mg_sim_check_owned "$1" && mg_sim_check_runtime "$1"
}

# mg_sim_prepare <udid>: the pinned status bar (R1966), after every boot and
# reset; pool devices stay booted, so before every use. Captures with the live
# clock and battery would not match the baselines: when the override fails,
# records the failure (error) and returns 1, and the job stops.
mg_sim_prepare() {
  local args
  args="$(jq -r '.status_bar | to_entries[] | "--\(.key)\n\(.value)"' tools/sim-devices.json)"
  local a=() line
  while IFS= read -r line; do a[${#a[@]}]="$line"; done <<<"$args"
  mg_simctl status_bar "$1" override "${a[@]}" >&2 && return 0
  mg_failure status-bar "simctl status_bar override failed on $1; nothing was captured with the live status bar (R1966)" "$MG_DEFAULT_REPRO"
  mg_message "$MG_JOB: could not pin the status bar of $1"
  mg_status error
  return 1
}

# ---------------------------------------------------------------------------
# The Mac app

# mg_app_mac_pids <app>: processes running the app's executable.
mg_app_mac_pids() {
  ps -Aww -o pid=,command= | awk -v e="$1/Contents/MacOS/Meguri" '
    { pid = $1; sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "") }
    $0 == e || index($0, e " ") == 1 { print pid }'
}

# mg_app_mac_dev_instances: "<pid> <app path>" for every running Meguri with
# the .dev bundle id, from any build.
mg_app_mac_dev_instances() {
  local pid app
  ps -Aww -o pid=,command= | awk '
    { pid = $1; sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "")
      i = index($0, "/Meguri.app/Contents/MacOS/Meguri")
      if (i > 0) print pid "\t" substr($0, 1, i + length("/Meguri.app") - 1) }' |
    while IFS="$(printf '\t')" read -r pid app; do
      [ "$(plutil -extract CFBundleIdentifier raw -o - "$app/Contents/Info.plist" 2>/dev/null)" = "$MG_APP_DEV_ID" ] &&
        printf '%s %s\n' "$pid" "$app"
    done
}

# mg_app_mac_guard: refuses (blocked: mac-app) while a .dev Meguri that this
# job did not start is running. A second instance would share its data
# container, and an XCUITest launch would terminate it. Records the result
# and returns 1.
mg_app_mac_guard() {
  local running
  running="$(mg_app_mac_dev_instances)"
  [ -n "$running" ] || return 0
  mg_failure mac-app-in-use "a $MG_APP_DEV_ID app this job did not start is running ($(printf '%s' "$running" | tr '\n' ';' | sed 's/;$//')); quit it, or wait for its job, before launching another" \
    "$MG_DEFAULT_REPRO"
  mg_detail blocked '"mac-app"'
  mg_message "$MG_JOB: blocked: another $MG_APP_DEV_ID app is running (pid $(printf '%s' "$running" | cut -d' ' -f1 | tr '\n' ' ' | sed 's/ $//'))"
  mg_status error
  return 1
}

# mg_app_mac_launchable <config>: only a Debug build stays out of sight on
# this Mac (U8d); Profile keeps Release's launch for performance runs (R1866)
# and would get a Dock tile. Otherwise records a usage error and returns 1,
# before anything is built or launched.
mg_app_mac_launchable() {
  [ "$1" != Debug ] || return 0
  mg_failure usage "$MG_JOB: the Mac app is launched on this Mac in Debug only: $1 has no background launch (U8d; Profile keeps Release's launch for performance runs, R1866) and would show in the Dock" "tools/mg --list"
  mg_message "$MG_JOB: the Mac app is launched on this Mac in Debug only, not $1 (U8d)"
  mg_status error
  return 1
}

# mg_app_open_mac <app> [launch args...]: launches the Mac app the only way
# tools/mg launches it on this Mac (root AGENTS.md): a new instance, hidden
# and in the background (open -g -j), with -MeguriBackgroundLaunch YES, which
# keeps the Debug build out of the Dock and never active (U8d,
# app/Meguri/TestHooks/BackgroundLaunch.swift; mg_app_mac_launchable first).
mg_app_open_mac() {
  local app="$1"
  shift
  open -g -j -n "$app" --args -MeguriBackgroundLaunch YES "$@" >&2
}

# mg_app_mac_presence <pid>: how LaunchServices shows the process (lsappinfo):
# its application type (Foreground: a Dock tile; UIElement; BackgroundOnly),
# followed by "front" while it is the active app. A failed query returns 1;
# empty or unknown types are not evidence of an out-of-sight launch.
mg_app_mac_presence() {
  local info type
  case "$1" in "" | 0 | *[!0-9]*) return 1 ;; esac
  info="$(lsappinfo info -only ApplicationType "$1" 2>/dev/null)" || return 1
  type="$(printf '%s\n' "$info" | sed -nE 's/.*type="([A-Za-z]+)".*/\1/p' | head -n 1)"
  case "$info" in *"(in front)"*) type="${type:+$type }front" ;; esac
  printf '%s\n' "$type"
}

# mg_app_mac_out_of_sight <pid> <when>: a hidden launch gets no Dock tile and
# never becomes the active app (U8d). Sets MG_APP_MAC_PRESENCE; otherwise
# records the failure and returns 1.
mg_app_mac_out_of_sight() {
  if MG_APP_MAC_PRESENCE="$(mg_app_mac_presence "$1")"; then
    case "$MG_APP_MAC_PRESENCE" in
      UIElement | BackgroundOnly) return 0 ;;
      Foreground | *" front" | front)
        mg_failure in-sight "the Mac app (pid $1) showed on this Mac $2 (lsappinfo: $MG_APP_MAC_PRESENCE): a hidden tools/mg launch must get no Dock tile and never be the active app (U8d, -MeguriBackgroundLaunch YES)"
        return 1
        ;;
    esac
  fi
  mg_failure no-evidence "cannot verify the Mac app stays out of sight (pid ${1:-unknown}) $2 (lsappinfo: ${MG_APP_MAC_PRESENCE:-empty or failed}): expected UIElement or BackgroundOnly for a known pid (U8d)"
  return 1
}

# mg_app_quit_mac <app>: terminates the app (SIGKILL after 10 s).
mg_app_quit_mac() {
  local pids i
  pids="$(mg_app_mac_pids "$1")"
  [ -n "$pids" ] || return 0
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null
  for i in 1 2 3 4 5 6 7 8 9 10; do
    [ -n "$(mg_app_mac_pids "$1")" ] || return 0
    sleep 1
  done
  # shellcheck disable=SC2046
  kill -9 $(mg_app_mac_pids "$1") 2>/dev/null
}

# ---------------------------------------------------------------------------
# Jobs

# Freeze only actual team-signed CloudAgent products, after all build mutations.
# Ad-hoc or initially dirty builds compile without agent_build proof.
mg_app_agent_freeze() {
  local platform="$1" source_commit="$2" sdk=iphonesimulator products xctestrun profile_settings=() settings_command=(xcodebuild)
  local settings="$MG_RUN_DIR/agent-settings.json" policy="$MG_RUN_DIR/agent-cloud-settings.json"
  local widget_settings="$MG_RUN_DIR/agent-widget-settings.json"
  local manifest="$MG_RUN_DIR/agent-build.json" reference="$MG_RUN_DIR/agent-build-reference.json"
  if [ "${MEGURI_APP_SIGNING-adhoc}" != team ] || [ -z "$source_commit" ]; then
    mg_detail agent_compile_only true
    return 0
  fi
  [ "$platform" != mac ] || sdk=macosx
  if [ -n "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ]; then
    if [ "$platform" != mac ] || ! mg_app_mac_agent_profile_check; then
      mg_failure agent-profile "App-only Mac CloudAgent profile changed before freezing"
      mg_status fail
      return 1
    fi
    profile_settings=(MEGURI_MAC_AGENT_PROFILE_STYLE=Manual
      "MEGURI_MAC_AGENT_PROFILE_SPECIFIER=$(jq -r .uuid <<<"$MG_CLOUD_AGENT_MAC_PROFILE_BINDING")")
    settings_command=(mg_app_mac_agent_private_settings xcodebuild)
  fi
  [ "$platform" != ios-sim ] || profile_settings=("CODE_SIGN_IDENTITY=Apple Development" MEGURI_CLOUD_AGENT_INJECTION=YES)
  products="$MG_ROOT/$MG_APP_DD/Build/Products"
  xctestrun="$(MG_APP_TEST_SCHEME=Meguri mg__vm_xctestrun CloudAgent "$sdk" Debug)"
  if [ -z "$xctestrun" ] || ! "${settings_command[@]}" build-for-testing -showBuildSettings -json -project app/Meguri.xcodeproj \
      -scheme Meguri -testPlan CloudAgent -configuration Debug -sdk "$sdk" -derivedDataPath "$MG_APP_DD" \
      MEGURI_SIGNING=team ${profile_settings[0]+"${profile_settings[@]}"} >"$settings"; then
    mg_failure agent-build "CloudAgent build inputs or resolved settings are missing"
    mg_status fail
    return 1
  fi
  if [ "$platform" = ios-sim ]; then
    # A scheme query can omit the extension. Bind its actual named role and the
    # same producer output roots; no scheme-shape inference from public policy.
    if ! (umask 077; xcodebuild -showBuildSettings -json -project app/Meguri.xcodeproj \
        -target MeguriWidget -configuration Debug -sdk iphonesimulator \
        "SYMROOT=$products" "OBJROOT=$MG_ROOT/$MG_APP_DD/Build/Intermediates.noindex" \
        MEGURI_SIGNING=team "CODE_SIGN_IDENTITY=Apple Development" MEGURI_CLOUD_AGENT_INJECTION=YES >"$widget_settings" \
        2>"$MG_RUN_DIR/agent-widget-settings.log"); then
      rm -f "$settings" "$widget_settings"
      mg_failure agent-build "CloudAgent widget resolved settings are missing"
      mg_status fail
      return 1
    fi
  fi
  # Retain app policy and agent host bindings; omit raw signing settings.
  if ! jq '[.[] | select(.target == "Meguri" or .target == "MeguriSnapshotTests") | {target, buildSettings:
    (.buildSettings | with_entries(.key as $k | select([
      "CONFIGURATION", "PRODUCT_BUNDLE_IDENTIFIER", "MEGURI_BUNDLE_ID_SUFFIX", "TEST_HOST", "BUNDLE_LOADER",
      "MEGURI_CORE_FEATURES", "SWIFT_ACTIVE_COMPILATION_CONDITIONS",
      "MEGURI_CLOUD_CONFIGURATION", "MEGURI_CLOUD_CONTAINER", "MEGURI_CLOUD_ENVIRONMENT",
      "MEGURI_CLOUD_CONTAINER_development", "MEGURI_CLOUD_CONTAINER_production",
      "MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT", "MEGURI_CLOUD_PUSH_ENVIRONMENT",
      "INFOPLIST_KEY_MeguriCloudContainer", "INFOPLIST_KEY_MeguriCloudEnvironment",
      "MEGURI_CLOUD_SCHEMA_CONFIGURED_development", "MEGURI_CLOUD_SCHEMA_CONFIGURED_production",
      "MEGURI_CLOUD_SCHEMA_CONFIGURED", "MEGURI_CLOUD_SCHEMA_BOOLEAN"
    ] | index($k))))}]' "$settings" >"$policy"; then
    mg_failure agent-build "could not retain CloudAgent policy settings"
    mg_status fail
    return 1
  fi
  if [ "$platform" = ios-sim ]; then
    # Initial signing and the first publication share held source/native/settings
    # descriptors. The helper publishes both exact outputs; do not freeze twice.
    if ! python3 -B -I "$MG_TOOLS/scripts/cloud-agent-initial-signing.py" \
        --root "$MG_ROOT" --run-directory "$MG_RUN_DIR" --source-commit "$source_commit" \
        --products "$products" --xctestrun "$xctestrun" --settings "$settings" \
        --widget-settings "$widget_settings"; then
      rm -f "$settings" "$widget_settings"
      mg_failure agent-build "Initial simulator development signing refused; no signed build admission"
      mg_status fail
      return 1
    fi
    rm -f "$settings" "$widget_settings"
  else
    rm -f "$settings"
    if ! python3 -B -I "$MG_TOOLS/scripts/cloud-agent-verify.py" --freeze-build \
        --root "$MG_ROOT" --run-id "$MG_RUN_ID" --source-commit "$source_commit" \
        --platform "$platform" --products "$products" --xctestrun "$xctestrun" \
        --manifest "$manifest" >"$reference"; then
      mg_failure agent-build "CloudAgent product freezing failed; no signed build admission"
      mg_status fail
      return 1
    fi
  fi
  if ! mg_app_mac_agent_profile_check; then
    mg_failure agent-profile "App-only Mac CloudAgent profile changed during first freeze"
    mg_status fail
    return 1
  fi
  mg_artifact directory "$products" "frozen CloudAgent build inputs"
  mg_artifact json "$manifest" "CloudAgent app, agent, xctestrun and product inventory"
  mg_artifact json "$policy" "CloudAgent cloud/debug policy settings"
  mg_detail agent_build "$(cat "$reference")"
}

mg_build_app() {
  local platform="" config=Debug config_given=false plan=UI standalone=false personal=false input="" app agent_source="" agent_tree=""
  # Bash dynamic scope constrains this override to this CLI build and its
  # product lookup; later/nested test jobs retain the default DerivedData.
  local MG_APP_DD="$MG_APP_DD"
  local MG_CLOUD_AGENT_MAC_PROFILE_BINDING="" MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "ios-sim mac" || { mg_drive__usage "--platform takes ios-sim or mac"; return 0; }
        platform="$2" && shift 2
        ;;
      --config)
        [ "$#" -ge 2 ] && mg_word_in "$2" "Debug Profile Release AppStore" || { mg_drive__usage "--config takes Debug, Profile, Release or AppStore"; return 0; }
        config="$2" && config_given=true && shift 2
        ;;
      --test-plan)
        [ "$#" -ge 2 ] && mg_word_in "$2" "UI CloudAgent" || { mg_drive__usage "--test-plan takes UI or CloudAgent"; return 0; }
        plan="$2" && shift 2
        ;;
      --standalone) standalone=true; shift ;;
      --personal-preview) personal=true; shift ;;
      *) mg_drive__usage "usage: tools/mg build-app --platform ios-sim|mac [--config C] [--test-plan UI|CloudAgent] [--standalone|--personal-preview]"; return 0 ;;
    esac
  done
  [ -n "$platform" ] || { mg_drive__usage "usage: tools/mg build-app --platform ios-sim|mac [--config C] [--test-plan UI|CloudAgent] [--standalone|--personal-preview]"; return 0; }
  if [ "$plan" = CloudAgent ] && { [ "$config" != Debug ] || [ "$standalone" = true ] || [ "$personal" = true ]; }; then
    mg_drive__usage "--test-plan CloudAgent requires Debug on mac or ios-sim, without --standalone or --personal-preview"
    return 0
  fi
  if [ "$plan" = CloudAgent ]; then
    # Whole-product frozen inventories must survive the peer platform build.
    case "$platform" in
      mac) MG_APP_DD=build/app/CloudAgentMacDerivedData ;;
      ios-sim) MG_APP_DD=build/app/CloudAgentSimDerivedData ;;
    esac
  fi
  if [ "$plan" = CloudAgent ] && [ "${MEGURI_APP_SIGNING-adhoc}" = team ]; then
    if ! agent_source="$(git -C "$MG_ROOT" rev-parse HEAD)" ||
        ! agent_tree="$(git -C "$MG_ROOT" status --porcelain=v1 --untracked-files=all)" ||
        [ -z "$agent_source" ] || [ -n "$agent_tree" ]; then
      # Keep the real MG dirty receipt; only a clean start can freeze proof.
      agent_source=""
    fi
  fi
  if [ "$personal" = true ]; then
    if [ "$platform" != mac ] || [ "$standalone" = true ] || { [ "$config_given" = true ] && [ "$config" != Release ]; }; then
      mg_drive__usage "--personal-preview requires --platform mac, Release (explicit or derived), and no --standalone"
      return 0
    fi
    config=Release
    MG_APP_DD=build/app/PersonalPreviewDerivedData
    input="$(mg_app_input_digest)"
    mg_app_build_action personal-preview "$platform" "$config" || return 0
    app="$(mg_app_products "$platform" "$config")/Meguri.app"
    if ! printf '%s\n' "$input" | grep -Eq '^[0-9a-f]{64}$' ||
        [ "$input" != "$(mg_app_input_digest)" ] || ! mg_app_personal_check "$app"; then
      mg_failure personal-preview-product "personal preview validation failed: require stable source digest, valid arm64 dev app/widget signatures and provisioning, normal sandbox entitlements without fixture scope, Release core without hooks and personalRelease inspection policy; see log.txt"
      mg_status fail
      return 0
    fi
    mg_detail input "$(jq -nc --arg input "$input" '$input')"
  elif [ "$standalone" = true ]; then
    [ "$platform/$config" = mac/Debug ] || { mg_drive__usage "--standalone requires --platform mac --config Debug"; return 0; }
    MG_APP_DD=build/app/StandaloneDerivedData
    input="$(mg_app_input_digest)"
    mg_app_build_action build "$platform" "$config" || return 0
    app="$(mg_app_products "$platform" "$config")/Meguri.app"
    if ! printf '%s\n' "$input" | grep -Eq '^[0-9a-f]{64}$' ||
        [ "$input" != "$(mg_app_input_digest)" ] || ! mg_app_standalone_check "$app"; then
      mg_failure standalone-product "normal Debug preview validation failed: require stable source digest, valid app/widget signatures, normal sandbox entitlements with only checkout fixture read access, arm64 dev products and the Debug/dev core stamp; see log.txt"
      mg_status fail
      return 0
    fi
    mg_detail input "$(jq -nc --arg input "$input" '$input')"
  else
    mg_app_build "$platform" "$config" "$plan" || return 0
    app="$(mg_app_products "$platform" "$config")/Meguri.app"
  fi
  mg_artifact app "$app"
  if [ "$plan" = CloudAgent ]; then
    mg_app_agent_freeze "$platform" "$agent_source" || return 0
  fi
  mg_message "build-app: $platform $config built: $app"
  mg_status pass
}

mg_run() {
  local seconds=5 window="" args=() a
  # --seconds and --window-size are run's own; the rest are drive's.
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --seconds)
        [ "$#" -ge 2 ] && case "$2" in "" | *[!0-9]*) false ;; esac || { mg_drive__usage "--seconds takes whole seconds"; return 0; }
        seconds="$2" && shift 2
        ;;
      --window-size)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_drive__usage "--window-size takes minimum, default, large or <w>x<h>"; return 0; }
        window="$2" && shift 2
        ;;
      --record) mg_drive__usage "run does not record; use tools/mg record or drive --record"; return 0 ;;
      --) args[${#args[@]}]="$1" && shift && for a in "$@"; do args[${#args[@]}]="$a"; done && break ;;
      *) args[${#args[@]}]="$1" && shift ;;
    esac
  done
  mg_drive__args ${args[0]+"${args[@]}"} || return 0
  [ "${#MG_D_POSITIONAL[@]}" -eq 0 ] || { mg_drive__usage "unexpected argument"; return 0; }
  if [ -n "$MG_D_PHYSICAL" ]; then
    [ -z "$window" ] && [ -z "$MG_D_ROUTE" ] && [ "${#MG_D_REST[@]}" -eq 0 ] || {
      mg_drive__usage "physical run requires a normal launch without window, route or raw launch overrides"; return 0;
    }
    mg_run__physical "$MG_D_PHYSICAL" "$MG_D_KIND"
    return 0
  fi
  local launch=(-ApplePersistenceIgnoreState YES)
  [ -z "$MG_D_ROUTE" ] || launch=("${launch[@]}" -MeguriRoute "$MG_D_ROUTE")
  if [ -n "$window" ]; then
    [ "$MG_D_KIND" = mac ] || { mg_drive__usage "--window-size is for the Mac (resizable iPhone windows are unavailable on simulators)"; return 0; }
    launch=("${launch[@]}" -MeguriWindowSize "$window")
  fi
  launch=("${launch[@]}" ${MG_D_REST[0]+"${MG_D_REST[@]}"})
  mg_detail launch_arguments "$(mg_json_array "${launch[@]}")"

  if [ "$MG_D_KIND" = mac ]; then
    mg_run__mac "$MG_D_CONFIG" "$seconds" "${launch[@]}"
    return 0
  fi
  local udid="$MG_D_UDID" app out pid
  mg_sim_begin || return 0
  if [ -n "$udid" ]; then
    mg_sim_check_given "$udid" || return 0
  fi
  mg_app_build ios-sim "$MG_D_CONFIG" || return 0
  mg_sim_take || return 0
  # A fixture the launch names must be in the VM, where the app reads it.
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_vm_launch_args "${launch[@]}"
    case $? in
      0) ;;
      # The VM could not be reached: queued (mg__mac_vm_unreachable).
      2) return 0 ;;
      *)
        mg_message "run: a launch fixture could not be put in the test VM; nothing was launched"
        mg_status error
        return 0
        ;;
    esac
    launch=("${MG_VM_ARGS[@]}")
  fi
  if [ -n "$udid" ]; then
    mg_sim_check_given "$udid" || return 0
  else
    mg_sim_lease "$MG_D_KIND" keep || return 0
    udid="$MG_SIM_UDID"
    if ! mg_sim_check_runtime "$udid"; then
      "$MG_TOOLS/scripts/sim-pool.sh" release "$udid" >&2
      return 0
    fi
  fi
  if ! mg_sim_prepare "$udid"; then
    [ -n "$MG_D_UDID" ] || "$MG_TOOLS/scripts/sim-pool.sh" release "$udid" >&2
    return 0
  fi
  app="$(mg_app_products ios-sim "$MG_D_CONFIG")/Meguri.app"
  if ! mg_simctl install "$udid" "$app" >&2; then
    mg_failure install "simctl install failed on $udid"
    mg_status error
    return 0
  fi
  if ! out="$(mg_simctl launch --terminate-running-process "$udid" "$MG_APP_DEV_ID" "${launch[@]}")"; then
    mg_failure launch "simctl launch failed on $udid"
    mg_status fail
    return 0
  fi
  pid="${out##*: }"
  mg_detail udid "$(jq -n --arg u "$udid" '$u')"
  mg_detail pid "$(jq -n --arg p "$pid" '$p')"
  mg_message "run: Meguri ($MG_D_CONFIG) runs on $udid (pid $pid)$([ -n "$MG_D_UDID" ] || echo "; the simulator stays leased: tools/mg sim release $udid when done")"
  mg_status pass
}

# Normal preparation is separate from preservation: a build, installation and
# development-app restart are authorized here, never in --keep-app-running.
mg_run__physical() {
  local device="$1" kind="$2" source after xctestrun MG_UI_PHYSICAL_DEVICE=""
  mg_ui_test__physical_check "$device" "$kind" || return 0
  mg_host_lease --no-session || return 0
  mg_ui_test__physical_check "$device" "$kind" || return 0
  source="$(mg_app_input_digest)"
  [ -n "$source" ] || { mg_failure physical-source "development source evidence unavailable"; mg_status fail; return 0; }
  export MG_APP_TEST_SCHEME=Meguri MG_APP_TEST_PLAN=UI
  mg_app_build ios-device Debug || return 0
  after="$(mg_app_input_digest)"
  [ "$source" = "$after" ] || { mg_failure physical-source "development sources changed during preparation"; mg_status fail; return 0; }
  xctestrun="$(MG_APP_TEST_SCHEME=Meguri mg__vm_xctestrun UI iphoneos Debug)"
  if [ -z "$xctestrun" ] || ! python3 -B -I "$MG_TOOLS/scripts/physical-drive.py" prepare --root "$MG_ROOT" \
      --output "$MG_RUN_DIR/physical" --device "$device" --source "$source" --xctestrun "$xctestrun" \
      >"$MG_RUN_DIR/physical-preparation.log" 2>&1 || [ "$source" != "$(mg_app_input_digest)" ]; then
    mg_failure physical-preparation "normal physical development preparation failed: require matching signed Debug products, targeted public installation metadata and the launched development process; see private build evidence"
    mg_message "run: physical development preparation failed; no fallback"
    mg_status fail
    return 0
  fi
  mg_artifact json "$MG_RUN_DIR/physical/preparation.json" "private selected development app preparation"
  mg_artifact json "$MG_RUN_DIR/physical/install-witness.json" "private sealed runner bootstrap witness"
  mg_detail normal_launch_verified true
  mg_detail continuity '"unknown"'
  mg_message "run: signed Debug development app launched normally on the selected physical device; preservation remains unproved"
  mg_status pass
}

# The Mac: a hidden background launch that must stay up, out of the Dock and
# never active, for <seconds>, then quits (it holds the host lease meanwhile,
# so no other job launches the app).
mg_run__mac() {
  local config="$1" seconds="$2" app pid="" i
  shift 2
  mg_app_mac_launchable "$config" || return 0
  mg_app_build mac "$config" || return 0
  mg_host_lease || return 0
  mg_app_mac_guard || return 0
  app="$(mg_app_products mac "$config")/Meguri.app"
  mg_app_open_mac "$app" "$@"
  mg_on_exit "mg_app_quit_mac '$app'"
  for i in 1 2 3 4 5 6 7 8 9 10; do
    pid="$(mg_app_mac_pids "$app" | head -n 1)"
    [ -z "$pid" ] || break
    mg_sleep 1
  done
  if [ -z "$pid" ]; then
    mg_failure launch "the Mac app did not start within 10 s of a hidden launch"
    mg_status fail
    return 0
  fi
  mg_detail pid "$(jq -n --arg p "$pid" '$p')"
  for i in $(seq 1 "$seconds"); do
    if ! kill -0 "$pid" 2>/dev/null; then
      mg_failure exited "the Mac app (pid $pid) quit $i s after a hidden launch"
      mg_message "run: the Mac app quit after $i s"
      mg_status fail
      return 0
    fi
    if ! mg_app_mac_out_of_sight "$pid" "$i s after a hidden launch"; then
      mg_message "run: could not verify the Mac app stays out of sight after $i s (lsappinfo: ${MG_APP_MAC_PRESENCE:-empty or failed})"
      mg_status fail
      return 0
    fi
    mg_sleep 1
  done
  mg_detail application_type "$(jq -n --arg t "$MG_APP_MAC_PRESENCE" '$t')"
  mg_message "run: the Mac app ($config) ran hidden for $seconds s (pid $pid, lsappinfo: ${MG_APP_MAC_PRESENCE:-unknown}) and was quit"
  mg_status pass
}
