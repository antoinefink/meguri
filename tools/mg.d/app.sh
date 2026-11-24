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
  "Build the app and its tests: --platform ios-sim|mac [--config Debug|Profile|Release|AppStore] [--test-plan UI|CloudAgent] [--standalone|--personal-preview|--real-copy --owner-requested] (lint-ui first, core XCFramework; default build-for-testing with UI plan; CloudAgent: Debug only, no previews; --standalone: Mac Debug automation preview; --personal-preview: Mac Release with development identities and team signing in PersonalPreviewDerivedData; --real-copy --owner-requested: only on Antoine's request, the daily-driver Mac Release (development push) with team signing and installed profiles only (never provisioning updates) in RealCopyDerivedData, verified, packed as build/app/real-copy/Meguri.zip, then the built app unregistered and deleted, and with --install put in /Applications while Meguri is not running; nothing launches)"
mg_job run host mg_run \
  "Install and launch: --platform iphone|iphone-small|ipad|ipad-small|foldable|mac [--device <leased udid> | --physical-device <explicit identifier>] [--config Debug|Profile]; physical requires Debug, or --personal-preview for manual team Release without test hooks, or --real-copy --owner-requested (only on Antoine's request) to install, never launch, the daily-driver Release from installed profiles; physical launches have no overrides; simulator/Mac also accept --route, --window-size, --seconds and -- launch args"

# shellcheck source=/dev/null
. "$(dirname "${BASH_SOURCE[0]}")/../scripts/host-build.sh"

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
    mac/build/app/DerivedData|mac/build/app/StandaloneDerivedData|mac/build/app/PersonalPreviewDerivedData|mac/build/app/CloudAgentMacDerivedData|mac/build/app/RealCopyDerivedData)
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
  local action="$1" standalone_arg="" plan_arg="" personal=false real=false test_args=()
  shift
  local platform="$1" config="$2" plan="${3:-UI}" scheme="${4:-Meguri}" signing="${MEGURI_APP_SIGNING-adhoc}" dest result log rc counts sdk xctestrun policy settings=() core_args=() build_command=(mg_host_build xcodebuild)
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
      if { [ "$platform/$config/$plan/$scheme" != mac/Release/UI/Meguri ] &&
          { [ "$platform/$config/$plan/$scheme" != ios-device/Release/UI/Meguri ] ||
            [ "${MG_APP_PERSONAL_DEVICE_ALLOWED:-false}" != true ] || [ "${MG_JOB:-}" != run ] ||
            [ "$MG_APP_DD" != build/app/PersonalDevicePreviewDerivedData ] || [ -z "${MG_UI_PHYSICAL_DEVICE:-}" ]; }; } ||
          [ "$signing" != team ]; then
        mg_failure usage "--personal-preview requires Mac Release or the verified physical run route, the normal Meguri scheme and team signing"
        mg_status error
        return 1
      fi
      action=build
      personal=true
      standalone_arg=" --personal-preview"
      ;;
    real-copy)
      # The daily-driver app (Antoine's real copy), only from the owner-requested
      # routes (build-app on the Mac, run on a phone): checks are mg_app_real_copy_*.
      if [ "$signing" != team ] || [ "${MG_APP_REAL_COPY_ALLOWED:-false}" != true ] || ! {
          { [ "$platform/$config/$plan/$scheme" = mac/Release/UI/Meguri ] && [ "$MG_APP_DD" = build/app/RealCopyDerivedData ]; } ||
          { [ "$platform/$config/$plan/$scheme" = ios-device/Release/UI/Meguri ] && [ "$MG_APP_DD" = build/app/RealCopyDeviceDerivedData ] &&
            [ "${MG_JOB:-}" = run ] && [ -n "${MG_UI_PHYSICAL_DEVICE:-}" ]; }; }; then
        mg_failure usage "--real-copy requires an owner-requested route: Mac or physical-device Release, the normal Meguri scheme and team signing"
        mg_status error
        return 1
      fi
      action=build
      real=true
      standalone_arg=" --real-copy --owner-requested"
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
    if [ "$personal" != true ] && [ "$real" != true ] && ! mg_word_in "$config" "Debug Profile"; then
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
    [ "$personal" != true ] || core_args=(--slices device)
    [ "$real" != true ] || core_args=(--slices device)
    signing=team
  fi
  if [ "$platform" = mac ] && [ "$personal" != true ] && [ "$real" != true ] && ! mg_word_in "$config" "Debug Profile"; then
    mg_failure usage "the Mac builds only Debug and Profile here: $config carries the daily-driver ids (tools/mg xcode-check builds and unregisters it)"
    mg_status error
    return 1
  fi
  if [ "$signing/$real" = team/true ]; then
    # Installed profiles only: never register identifiers, capabilities or
    # devices on Antoine's account (one-time step daily-driver-account).
    # Development push, so an ordinary development profile authorizes it;
    # CloudKit stays production (standing decision 2026-11-21, KTD22). Manual
    # signing with the profiles the preflight selected: automatic signing
    # would need Xcode-managed profiles and provisioning updates.
    settings=(${settings[0]+"${settings[@]}"} MEGURI_SIGNING=team MEGURI_CLOUD_PUSH_ENVIRONMENT=development)
    if [ "$platform" = mac ]; then
      settings=(${settings[0]+"${settings[@]}"} MEGURI_MAC_AGENT_PROFILE_STYLE=Manual
        "MEGURI_MAC_AGENT_PROFILE_SPECIFIER=${MG_APP_REAL_COPY_APP_PROFILE:-}")
    else
      settings=(${settings[0]+"${settings[@]}"} MEGURI_DEVICE_PROFILE_STYLE=Manual
        "MEGURI_DEVICE_PROFILE_SPECIFIER=${MG_APP_REAL_COPY_APP_PROFILE:-}"
        MEGURI_WIDGET_DEVICE_PROFILE_STYLE=Manual "MEGURI_WIDGET_DEVICE_PROFILE_SPECIFIER=${MG_APP_REAL_COPY_WIDGET_PROFILE:-}")
    fi
  elif [ "$signing" = team ]; then
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
  if [ "$real" = true ] && ! mg_app_real_copy_settings ${settings[0]+"${settings[@]}"}; then
    mg_failure real-copy-settings "the real copy requires Release settings with the daily-driver app/widget identities, production cloud settings, team signing, normal sandbox entitlements, no debug or test hooks, and the name Meguri with AppIcon; see log.txt"
    mg_status fail
    return 1
  fi
  if [ "$personal" = true ] || [ "$real" = true ]; then
    local identity_settings="$MG_RUN_DIR/personal-preview-settings.json"
    [ "$real" != true ] || identity_settings="$MG_RUN_DIR/real-copy-settings.json"
    mg_app_dd_identity "$identity_settings" "$(mg_app_products "$platform" "$config")/Meguri.app" || return 1
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
  if [ "$platform/$personal" = ios-device/true ] || [ "$platform/$real" = ios-device/true ] ||
      [ -n "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ]; then
    counts="$(jq -c 'del(.messages)' <<<"${counts:-null}")"
  fi
  mg_detail build "$(jq -nc --arg p "$platform" --arg c "$config" --arg app "$(mg_app_products "$platform" "$config")/Meguri.app" \
    --arg action "$action" --arg plan "$plan" --arg dd "$MG_APP_DD" --argjson personal "$personal" --argjson real "$real" --argjson counts "${counts:-null}" \
    '{platform: $p, config: $c, app: $app, action: $action, test_plan: $plan, standalone: ($action == "build" and ($personal | not) and ($real | not)), personal_preview: $personal, real_copy: $real, derived_data: $dd} + ($counts // {})')"
  if [ "$rc" -ne 0 ]; then
    [ "$plan" != CloudAgent ] || plan_arg=" --test-plan $plan"
    # The destination's state, when Xcode stopped at it (fixed messages; the
    # log names the device). Developer Mode first: Xcode then also times out.
    if [ "$platform" = ios-device ] && grep -Eqi 'error: ?.*(Developer Mode (is )?disabled|enable Developer Mode)' "$log"; then
      mg_app_physical_device_failure developer-mode "Xcode stopped at the destination; nothing was installed"
    elif [ "$platform" = ios-device ] && grep -Eqi 'error: ?.*(device is locked|is passcode protected|Unlock .* to continue)' "$log"; then
      mg_app_physical_device_failure locked "Xcode stopped at the destination; nothing was installed"
    elif [ "$platform" = ios-device ] && grep -Eqi 'Timed out waiting for all destinations' "$log"; then
      mg_app_physical_device_failure unreachable "Xcode timed out waiting for the destination; nothing was installed"
    elif [ "$real" = true ] && grep -Eqi 'provisioning profile|No profiles for|requires a development team|No signing certificate' "$log"; then
      # A fixed message: device signing diagnostics stay in the private log.
      mg_failure real-copy-profile "$MG_APP_REAL_COPY_PROFILE_HELP" "tools/mg doctor"
    elif [ "$platform" = ios-device ] || [ -n "${MG_CLOUD_AGENT_MAC_PROFILE_BINDING:-}" ]; then
      # Signing diagnostics can include selected-device identities and paths.
      mg_failure build "physical development build failed; diagnostics remain in the private build log"
    elif [ "$real" = true ]; then
      # No rerun hint: the route runs only at Antoine's request.
      mg_failure build "xcodebuild build failed for the real copy: $(grep -E 'error:' "$log" | head -n 3 | tr '\n' ' ')"
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
  if [ "$action/$personal/$real" = build/false/false ]; then
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

# --- Derived data of the personal previews and real copies -----------------
# Xcode's incremental builds keep files the current settings no longer make:
# a folder last built as the other identity kept its AppIcon.icns in the
# bundle (2026-11-22), and removing the product alone did not help. Each of
# these folders records the identity settings it was built with
# (meguri-build-identity.json); a folder without that record, or with other
# settings, is removed before the build, once.
MG_APP_DD_RECORD=meguri-build-identity.json

# mg_app_dd_identity <resolved settings JSON> <app>: compare, clean, record.
mg_app_dd_identity() {
  local settings="$1" app="$2" dd="$MG_ROOT/$MG_APP_DD" identity recorded
  identity="$(jq -cS '{version: 1, targets: ([.[] | select(.target == "Meguri" or .target == "MeguriWidget") |
    {target} + (.buildSettings | {CONFIGURATION, PLATFORM_NAME, PRODUCT_BUNDLE_IDENTIFIER, MEGURI_BUNDLE_ID_SUFFIX,
      MEGURI_APP_NAME, MEGURI_APP_ICON, ASSETCATALOG_COMPILER_APPICON_NAME, INFOPLIST_KEY_CFBundleDisplayName,
      EXCLUDED_SOURCE_FILE_NAMES, MEGURI_CLOUD_CONFIGURATION, MEGURI_SIGNING, CODE_SIGN_ENTITLEMENTS})] | sort_by(.target))}' \
    "$settings" 2>/dev/null)" && [ -n "$identity" ] || {
    mg_failure derived-data "the identity settings of $MG_APP_DD could not be read; nothing was built"
    mg_status error
    return 1
  }
  # Never read, write or remove through a link (out of the checkout, say).
  local path="$dd"
  while [ "$path" != "$MG_ROOT" ] && [ -n "$path" ] && [ "$path" != / ]; do
    if [ -L "$path" ]; then
      mg_failure derived-data "refusing to use $MG_APP_DD through a symbolic link ($path); nothing was built"
      mg_status error
      return 1
    fi
    path="${path%/*}"
  done
  if [ -d "$dd" ]; then
    recorded="$(jq -cS . "$dd/$MG_APP_DD_RECORD" 2>/dev/null)" || recorded=""
    if [ -z "$recorded" ]; then
      mg_app_dd_clean no-identity-record "$app" || return 1
    elif [ "$recorded" != "$identity" ]; then
      mg_app_dd_clean identity-settings-changed "$app" || return 1
    fi
  fi
  # Recorded before the build: a failed build leaves partial products of
  # these same settings, which the next incremental build may reuse.
  if ! mkdir -p "$dd" || ! printf '%s\n' "$identity" >"$dd/$MG_APP_DD_RECORD"; then
    mg_failure derived-data "could not record the identity settings of $MG_APP_DD; nothing was built"
    mg_status error
    return 1
  fi
}

# mg_app_dd_clean <reason> <app>: remove this route's derived data, after
# forgetting a daily-driver product in it. Only these four folders.
mg_app_dd_clean() {
  local reason="$1" app="$2"
  case "$MG_APP_DD" in
    build/app/PersonalPreviewDerivedData | build/app/PersonalDevicePreviewDerivedData | \
      build/app/RealCopyDerivedData | build/app/RealCopyDeviceDerivedData) ;;
    *)
      mg_failure derived-data "refusing to clean $MG_APP_DD: only the personal preview and real copy folders are cleaned"
      mg_status error
      return 1
      ;;
  esac
  if [ "${MG_APP_REAL_COPY_ALLOWED:-false}" = true ] && ! mg_app_real_copy_forget "$app"; then
    mg_failure real-copy-forget "the earlier daily-driver build in $MG_APP_DD is still on disk or registered, or LaunchServices could not be read: $app; nothing was built"
    mg_status fail
    return 1
  fi
  if ! rm -rf "${MG_ROOT:?}/$MG_APP_DD"; then
    mg_failure derived-data "could not remove $MG_APP_DD ($reason); nothing was built"
    mg_status error
    return 1
  fi
  MG_APP_DD_CLEANED="$reason"
  mg_detail derived_data_cleaned "$(jq -nc --arg p "$MG_APP_DD" --arg r "$reason" '{path: $p, reason: $r}')"
  echo "$MG_JOB: removed $MG_APP_DD ($reason); building from scratch" >&2
}

# mg_app_dd_note: the clause a route's message adds when it cleaned.
mg_app_dd_note() {
  [ -z "${MG_APP_DD_CLEANED:-}" ] || printf '; %s was removed first (%s)' "$MG_APP_DD" "$MG_APP_DD_CLEANED"
}

# Inspect resolved settings before any native app build. The suffix must reach
# both targets, while Release keeps its native launch/core and uses isolated development cloud.
mg_app_personal_settings() {
  local settings="$MG_RUN_DIR/personal-preview-settings.json" sdk=macosx cloud_platform=mac
  [ "$platform" != ios-device ] || { sdk=iphoneos; cloud_platform=ios; }
  xcodebuild -showBuildSettings -json -project app/Meguri.xcodeproj -target Meguri -target MeguriWidget \
    -configuration Release -sdk "$sdk" -arch arm64 \
    "$@" >"$settings" || return 1
  jq -e --arg sdk "$sdk" '
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
      .CODE_SIGN_ENTITLEMENTS == ((if $target == "Meguri" then "Meguri/Meguri-" else "MeguriWidget/MeguriWidget-" end) + (if $sdk == "macosx" then "macOS" else "iOS" end) + ".entitlements") and
      .ENABLE_TESTABILITY == "NO" and .ENABLE_DEBUG_DYLIB == "NO" and
      (if $sdk == "macosx" then .ENABLE_HARDENED_RUNTIME == "YES" else
        .PLATFORM_NAME == "iphoneos" and .MEGURI_SIGNING == "team" and
        .DEVELOPMENT_TEAM == .MEGURI_TEAM_ID and (.DEVELOPMENT_TEAM | test("^[A-Z0-9]{10}$")) and
        .CODE_SIGN_IDENTITY == "Apple Development" end))
  ' "$settings" >/dev/null || return 1
  python3 "$MG_TOOLS/tests/cloud_build_configuration.py" --settings "$settings" \
    --configuration Release --platform "$cloud_platform" --signing team --personal-preview
}

# Profiles are checked in memory and never published. Validate the signed
# identity/grants against an unexpired development profile, including the selected device (this Mac by default).
mg_app_personal_profile() {
  local bundle="$1" ent="$2" hardware="$3" platform="${4:-mac}" environment="${5:-Development}" cert_dir rc profile="$1/Contents/embedded.provisionprofile"
  [ "$platform" != ios ] || profile="$bundle/embedded.mobileprovision"
  [ -f "$profile" ] || return 1
  cert_dir="$(mktemp -d "$MG_RUN_DIR/personal-signer.XXXXXX")" || return 1
  if ! codesign -d --extract-certificates="$cert_dir/cert" "$bundle" >/dev/null; then
    rm -rf "$cert_dir"
    return 1
  fi
  security cms -D -i "$profile" | python3 -I -c '
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
        # Development for previews; Production for the real copy (argv[5]).
        if claim != sys.argv[5]:
            return False
        if grant == sys.argv[5]:
            return True
        choices, required = ("Development", "Production"), sys.argv[5]
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
    # Mac previews forbid debugger grants; normal iOS development signing
    # may add get-task-allow, independently of Swift/Rust test hooks.
    assert ("OSX" if sys.argv[4] == "mac" else "iOS") in profile["Platform"], "profile platform"
    if sys.argv[4] == "ios":
        assert grants.get("get-task-allow") is True, "iOS development profile"
        assert profile.get("ProvisionsAllDevices", False) is False, "device-bound profile"
        assert type(signed.get("get-task-allow", False)) is bool, "debugger grant"
    assert profile["DeveloperCertificates"] and all(isinstance(cert, bytes) and cert for cert in profile["DeveloperCertificates"]), "developer certificates"
    with open(sys.argv[3], "rb") as certificate:
        assert certificate.read() in profile["DeveloperCertificates"], "authorized signer"
    assert all(grants[key] is True for key in ("get-task-allow", "com.apple.security.get-task-allow") if key in grants), "development profile"
    assert profile["ExpirationDate"] > datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None), "expiry"
    assert sys.argv[2] in profile["ProvisionedDevices"], "device coverage"
    if "com.apple.developer.icloud-container-identifiers" in signed:
        assert grants.get("com.apple.developer.icloud-container-identifiers") == signed["com.apple.developer.icloud-container-identifiers"], "isolated cloud container grant"
        for key in ("com.apple.developer.icloud-services", "com.apple.developer.icloud-container-environment",
                    "com.apple.developer.aps-environment" if sys.argv[4] == "mac" else "aps-environment"):
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
' "$ent" "$hardware" "$cert_dir/cert0" "$platform" "$environment"
  rc=$?
  rm -rf "$cert_dir"
  return "$rc"
}

mg_app_personal_check() {
  local app="$1" platform="${2:-mac}" hardware="${3:-}" prefix=Contents/ slice=mac cloud_platform=mac
  [ "$platform" != ios-device ] || { prefix=""; slice=device; cloud_platform=ios; }
  local widget="$1/${prefix}PlugIns/MeguriWidget.appex" bundle xml ent info team="" identity
  local app_ent widget_ent app_info
  local stamp hash slices app_hash widget_hash app_profile_hash widget_profile_hash="" digest signed_info cert_dir widget_provision=not_embedded
  codesign --verify --deep --strict "$app" && codesign --verify --strict "$widget" || return 1
  [ "$platform" != mac ] || hardware="$(system_profiler SPHardwareDataType -json | jq -er '.SPHardwareDataType[0] |
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
    jq -e --arg team "$team" --arg identity "$identity" --arg platform "$platform" '
      type == "object" and
      (if $platform == "mac" then .["com.apple.security.app-sandbox"] == true and
        ((.["com.apple.security.get-task-allow"] // false) == false) and ((.["get-task-allow"] // false) == false)
       else .["com.apple.developer.team-identifier"] == $team and
        .["application-identifier"] == ($team + "." + $identity) end) and
      .["com.apple.security.application-groups"] == [(if $platform == "mac" then $team else "group" end) + ".com.antoinefink.meguri.dev"] and
      ([keys[] | select(startswith("com.apple.security.temporary-exception."))] == []) and
      ([keys[] | select(startswith("com.apple.security.cs."))] == [])
    ' <<<"$ent" >/dev/null || return 1
    if [ "$platform" = ios-device ]; then
      jq -e --arg team "$team" --arg identity "$identity" '
        (.["keychain-access-groups"] // []) as $groups |
        ($groups == [] or $groups == [$team + "." + $identity]) and
        (.["com.apple.security.get-task-allow"] // false) == false
      ' <<<"$ent" >/dev/null || return 1
      mg_app_personal_profile "$bundle" "$ent" "$hardware" ios || return 1
      if [ "$bundle" = "$app" ]; then app_ent="$ent"; else widget_ent="$ent"; widget_provision=verified; fi
    elif [ "$bundle" = "$app" ]; then
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
    info="$(plutil -convert json -o - "$bundle/${prefix}Info.plist")" || return 1
    jq -e --arg identity "$identity" '.CFBundleIdentifier == $identity' <<<"$info" >/dev/null || return 1
    if [ "$platform" = ios-device ]; then
      jq -e '.MeguriAppGroupIdentifier == "group.com.antoinefink.meguri.dev"' <<<"$info" >/dev/null || return 1
    fi
    if [ "$bundle" = "$app" ]; then
      app_info="$info"
      jq -e --arg platform "$platform" '.CFBundleExecutable == "Meguri" and .MeguriCardInspectionBuild == "personalRelease" and
        (if $platform == "mac" then .LSUIElement == false else .CFBundleSupportedPlatforms == ["iPhoneOS"] end)' <<<"$info" >/dev/null || return 1
    else
      jq -e '.CFBundleExecutable == "MeguriWidget"' <<<"$info" >/dev/null || return 1
    fi
  done
  # Use the same cloud policy for resolved settings and signed product facts.
  python3 -B - "$MG_TOOLS/tests" "$MG_RUN_DIR/personal-preview-settings.json" "$app_info" "$app_ent" "$widget_ent" "$cloud_platform" <<'PY_CLOUD' || return 1
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from cloud_build_configuration import check
settings = json.loads(Path(sys.argv[2]).read_text())
app = next(target["buildSettings"] for target in settings if target["target"] == "Meguri")
if sys.argv[6] == "ios":
    assert all(target["buildSettings"]["DEVELOPMENT_TEAM"] == json.loads(sys.argv[4])["com.apple.developer.team-identifier"] for target in settings)
check(app, "Release", sys.argv[6], "team", *(json.loads(value) for value in sys.argv[3:6]), personal_preview=True)
PY_CLOUD
  cert_dir="$(mktemp -d "$MG_RUN_DIR/personal-widget-signer.XXXXXX")" || return 1
  if ! codesign -d --extract-certificates="$cert_dir/app" "$app" >/dev/null ||
      ! codesign -d --extract-certificates="$cert_dir/widget" "$widget" >/dev/null ||
      ! cmp -s "$cert_dir/app0" "$cert_dir/widget0"; then
    rm -rf "$cert_dir"
    return 1
  fi
  rm -rf "$cert_dir"
  local executable_prefix="${prefix}"
  [ "$platform" != mac ] || executable_prefix=Contents/MacOS/
  [ "$(lipo -archs "$app/${executable_prefix}Meguri")" = arm64 ] &&
    [ "$(lipo -archs "$widget/${executable_prefix}MeguriWidget")" = arm64 ] || return 1
  stamp="$(cat "$MG_ROOT/app/Packages/MeguriCore/Binaries/.stamp")" || return 1
  grep -qx 'configuration=Release' <<<"$stamp" && grep -qx 'profile=release' <<<"$stamp" &&
    grep -qx 'features=ankiweb' <<<"$stamp" || return 1
  hash="$(sed -n 's/^hash=//p' <<<"$stamp")"
  printf '%s\n' "$hash" | grep -Eq '^[0-9a-f]{32}$' || return 1
  slices="$(sed -n 's/^slices=//p' <<<"$stamp")"
  printf '%s\n' "$slices" | tr ',' '\n' | grep -qx "$slice" || return 1
  app_hash="$(shasum -a 256 "$app/${executable_prefix}Meguri" | cut -c1-64)"
  widget_hash="$(shasum -a 256 "$widget/${executable_prefix}MeguriWidget" | cut -c1-64)"
  local profile_path=Contents/embedded.provisionprofile
  [ "$platform" != ios-device ] || profile_path=embedded.mobileprovision
  app_profile_hash="$(shasum -a 256 "$app/$profile_path" | cut -c1-64)"
  [ "$widget_provision" != verified ] || widget_profile_hash="$(shasum -a 256 "$widget/$profile_path" | cut -c1-64)"
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

# --- The real copy (build-app --real-copy --owner-requested) ---------------
# Antoine's daily-driver Mac app, built only at his request. Unlike every other
# team build it never passes -allowProvisioningUpdates or
# -allowProvisioningDeviceRegistration: a missing or incomplete profile fails
# the run instead of registering anything on his account. The checked product
# is packed as a zip, then unregistered and deleted, so the only registered
# daily-driver app stays the one he installs (app/AGENTS.md).
MG_APP_REAL_COPY_ZIP=build/app/real-copy/Meguri.zip
# Not taken from the environment: unregistering must never be replaceable.
MG_LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
MG_APP_REAL_COPY_PROFILE_HELP="no installed development profile authorizes the real copy on this device (com.antoinefink.meguri: App Group, Keychain group, CloudKit container iCloud.com.antoinefink.meguri in Production, development push; on iOS also the widget com.antoinefink.meguri.widget); this route never requests provisioning updates. Antoine's one-time step daily-driver-account (docs/process/one-time-setup.md); tools/mg doctor shows what is missing"

# Resolved settings of both targets before any app build: daily ids,
# production cloud, team signing, no hooks, the daily name and icon.
mg_app_real_copy_settings() {
  local settings="$MG_RUN_DIR/real-copy-settings.json" sdk=macosx cloud_platform=mac
  [ "$platform" != ios-device ] || { sdk=iphoneos; cloud_platform=ios; }
  xcodebuild -showBuildSettings -json -project app/Meguri.xcodeproj -target Meguri -target MeguriWidget \
    -configuration Release -sdk "$sdk" -arch arm64 "$@" >"$settings" || return 1
  jq -e --arg sdk "$sdk" --arg app_profile "${MG_APP_REAL_COPY_APP_PROFILE:-}" --arg widget_profile "${MG_APP_REAL_COPY_WIDGET_PROFILE:-}" '
    type == "array" and
    ([.[] | select(.target == "Meguri" or .target == "MeguriWidget")] | length == 2) and
    all(.[] | select(.target == "Meguri" or .target == "MeguriWidget");
      .target as $target | .buildSettings |
      .CONFIGURATION == "Release" and (.MEGURI_BUNDLE_ID_SUFFIX // "") == "" and
      .MEGURI_CLOUD_CONFIGURATION == "production" and .MEGURI_CLOUD_ENVIRONMENT == "production" and
      .MEGURI_CLOUD_PUSH_ENVIRONMENT == "development" and
      .MEGURI_CLOUD_CONTAINER == .MEGURI_CLOUD_CONTAINER_production and
      .PRODUCT_BUNDLE_IDENTIFIER == (if $target == "Meguri" then "com.antoinefink.meguri" else "com.antoinefink.meguri.widget" end) and
      .MEGURI_APP_NAME == "Meguri" and .MEGURI_SIGNING == "team" and
      .MEGURI_CORE_FEATURES == "ankiweb" and .MEGURI_SWIFT_CONDITIONS == "ANKIWEB_ENABLED" and
      ((.SWIFT_ACTIVE_COMPILATION_CONDITIONS // "" | test("(^|[ ,])DEBUG|MEGURI_TEST_HOOKS")) | not) and
      .MEGURI_CARD_INSPECTION_BUILD == "personalRelease" and .MEGURI_MAC_UI_ELEMENT == "NO" and
      (.MEGURI_ENTITLEMENTS_HOOKS_SUFFIX // "") == "" and (.MEGURI_ENTITLEMENTS_SIGNING_SUFFIX // "") == "" and
      .CODE_SIGN_ENTITLEMENTS == ((if $target == "Meguri" then "Meguri/Meguri-" else "MeguriWidget/MeguriWidget-" end) + (if $sdk == "macosx" then "macOS" else "iOS" end) + ".entitlements") and
      .ENABLE_TESTABILITY == "NO" and .ENABLE_DEBUG_DYLIB == "NO" and
      (if $sdk == "macosx" then .ENABLE_HARDENED_RUNTIME == "YES" else
        .PLATFORM_NAME == "iphoneos" and .DEVELOPMENT_TEAM == .MEGURI_TEAM_ID and
        (.DEVELOPMENT_TEAM | test("^[A-Z0-9]{10}$")) and .CODE_SIGN_IDENTITY == "Apple Development" end) and
      (if $target == "Meguri" then .ASSETCATALOG_COMPILER_APPICON_NAME == "AppIcon" else true end) and
      # The profiles the preflight selected, signed manually.
      (if $target == "Meguri" then $app_profile != "" and .CODE_SIGN_STYLE == "Manual" and .PROVISIONING_PROFILE_SPECIFIER == $app_profile
       elif $sdk == "iphoneos" then $widget_profile != "" and .CODE_SIGN_STYLE == "Manual" and .PROVISIONING_PROFILE_SPECIFIER == $widget_profile
       else true end))
  ' "$settings" >/dev/null || return 1
  python3 "$MG_TOOLS/tests/cloud_build_configuration.py" --settings "$settings" \
    --configuration Release --platform "$cloud_platform" --signing team --real-copy
}

# The signed daily-driver product: identities, team, profile, entitlements,
# Info, core, architecture, then the name and icon it shows.
mg_app_real_copy_check() {
  local app="$1" widget="$1/Contents/PlugIns/MeguriWidget.appex" bundle xml ent info team="" identity hardware signed_info
  local app_ent="" widget_ent="" app_info="" stamp hash slices cert_dir widget_provision=not_embedded
  codesign --verify --deep --strict "$app" && codesign --verify --strict "$widget" || return 1
  hardware="$(system_profiler SPHardwareDataType -json | jq -er '.SPHardwareDataType[0] |
    (.provisioning_UDID // .platform_UUID) | select(type == "string" and length > 0)')" || return 1
  for bundle in "$app" "$widget"; do
    xml="$(codesign -d --entitlements - --xml "$bundle")" || return 1
    ent="$(plutil -convert json -o - - <<<"$xml")" || return 1
    [ -n "$team" ] || team="$(jq -er '.["com.apple.developer.team-identifier"] | select(type == "string" and test("^[A-Z0-9]{10}$"))' <<<"$ent")" || return 1
    identity=com.antoinefink.meguri
    [ "$bundle" = "$app" ] || identity="$identity.widget"
    signed_info="$(codesign -d --verbose=4 "$bundle" 2>&1)" || return 1
    [ "$(sed -n 's/^Identifier=//p' <<<"$signed_info")" = "$identity" ] &&
      [ "$(sed -n 's/^TeamIdentifier=//p' <<<"$signed_info")" = "$team" ] || return 1
    jq -e --arg team "$team" '
      type == "object" and .["com.apple.security.app-sandbox"] == true and
      ((.["com.apple.security.get-task-allow"] // false) == false) and ((.["get-task-allow"] // false) == false) and
      .["com.apple.security.application-groups"] == [$team + ".com.antoinefink.meguri"] and
      ([keys[] | select(startswith("com.apple.security.temporary-exception."))] == []) and
      ([keys[] | select(startswith("com.apple.security.cs."))] == [])
    ' <<<"$ent" >/dev/null || return 1
    if [ "$bundle" = "$app" ]; then
      jq -e --arg team "$team" '
        .["com.apple.developer.team-identifier"] == $team and
        (.["com.apple.application-identifier"] // .["application-identifier"]) == ($team + ".com.antoinefink.meguri") and
        .["keychain-access-groups"] == [$team + ".com.antoinefink.meguri"] and
        .["com.apple.security.network.client"] == true and .["com.apple.security.network.server"] == true and
        .["com.apple.security.files.user-selected.read-write"] == true and .["com.apple.security.files.bookmarks.app-scope"] == true
      ' <<<"$ent" >/dev/null || return 1
      mg_app_personal_profile "$bundle" "$ent" "$hardware" mac Production || return 1
      mg_app_real_copy_profile_is "$bundle/Contents/embedded.provisionprofile" "${MG_APP_REAL_COPY_APP_PROFILE:-}" || return 1
      app_ent="$ent"
    else
      jq -e --arg team "$team" '
        (.["keychain-access-groups"] // []) == [] and
        (.["com.apple.developer.team-identifier"] // $team) == $team and
        (.["com.apple.application-identifier"] // .["application-identifier"] // ($team + ".com.antoinefink.meguri.widget")) == ($team + ".com.antoinefink.meguri.widget")
      ' <<<"$ent" >/dev/null || return 1
      widget_ent="$ent"
      if [ -f "$bundle/Contents/embedded.provisionprofile" ]; then
        mg_app_personal_profile "$bundle" "$ent" "$hardware" mac Production || return 1
        widget_provision=verified
      fi
    fi
    info="$(plutil -convert json -o - "$bundle/Contents/Info.plist")" || return 1
    jq -e --arg identity "$identity" '.CFBundleIdentifier == $identity' <<<"$info" >/dev/null || return 1
    if [ "$bundle" = "$app" ]; then
      app_info="$info"
      jq -e '.CFBundleExecutable == "Meguri" and .MeguriCardInspectionBuild == "personalRelease" and .LSUIElement == false' \
        <<<"$info" >/dev/null || return 1
    else
      jq -e '.CFBundleExecutable == "MeguriWidget"' <<<"$info" >/dev/null || return 1
    fi
  done
  python3 -B - "$MG_TOOLS/tests" "$MG_RUN_DIR/real-copy-settings.json" "$app_info" "$app_ent" "$widget_ent" <<'PY_CLOUD' || return 1
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from cloud_build_configuration import check
settings = json.loads(Path(sys.argv[2]).read_text())
app = next(target["buildSettings"] for target in settings if target["target"] == "Meguri")
check(app, "Release", "mac", "team", *(json.loads(value) for value in sys.argv[3:6]), real_copy=True)
PY_CLOUD
  cert_dir="$(mktemp -d "$MG_RUN_DIR/real-copy-signer.XXXXXX")" || return 1
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
  mg_app_real_copy_identity "$app" || return 1
  mg_detail real_copy_product "$(jq -nc --arg hash "$(shasum -a 256 "$app/Contents/MacOS/Meguri" | cut -c1-64)" \
    --arg widget_provision "$widget_provision" --arg core_hash "$hash" --arg slices "$slices" '
    {signature_verified: true, provisioning_verified: true, architecture: {app: "arm64", widget: "arm64"},
     executable_sha256: $hash, provisioning: {app: "verified", widget: $widget_provision}, widget_signer_matches_app: true,
     bundle_id: "com.antoinefink.meguri", widget_bundle_id: "com.antoinefink.meguri.widget",
     cloud_environment: "production", push_environment: "development", card_inspection_build: "personalRelease", test_hooks: false,
     name: "Meguri", icon: "AppIcon",
     core: {hash: $core_hash, configuration: "Release", profile: "release", features: "ankiweb", slices: $slices}}')"
}

# The signed iPhone/iPad real copy: identities, team, the device development
# profiles of app and widget covering <device>, entitlements (production
# CloudKit, development push, the daily App Group), Info, core, architecture,
# name and icon. The personal preview's device check, with the daily ids.
mg_app_real_copy_device_check() {
  local app="$1" device="$2" widget="$1/PlugIns/MeguriWidget.appex" bundle xml ent info team="" identity signed_info
  local app_ent="" widget_ent="" app_info="" stamp hash slices cert_dir
  codesign --verify --deep --strict "$app" && codesign --verify --strict "$widget" || return 1
  for bundle in "$app" "$widget"; do
    xml="$(codesign -d --entitlements - --xml "$bundle")" || return 1
    ent="$(plutil -convert json -o - - <<<"$xml")" || return 1
    [ -n "$team" ] || team="$(jq -er '.["com.apple.developer.team-identifier"] | select(type == "string" and test("^[A-Z0-9]{10}$"))' <<<"$ent")" || return 1
    identity=com.antoinefink.meguri
    [ "$bundle" = "$app" ] || identity="$identity.widget"
    signed_info="$(codesign -d --verbose=4 "$bundle" 2>&1)" || return 1
    [ "$(sed -n 's/^Identifier=//p' <<<"$signed_info")" = "$identity" ] &&
      [ "$(sed -n 's/^TeamIdentifier=//p' <<<"$signed_info")" = "$team" ] || return 1
    jq -e --arg team "$team" --arg identity "$identity" '
      type == "object" and .["com.apple.developer.team-identifier"] == $team and
      .["application-identifier"] == ($team + "." + $identity) and
      .["com.apple.security.application-groups"] == ["group.com.antoinefink.meguri"] and
      ((.["keychain-access-groups"] // []) as $groups | $groups == [] or $groups == [$team + "." + $identity]) and
      ((.["get-task-allow"] // false) | type) == "boolean" and
      (.["com.apple.security.get-task-allow"] // false) == false and
      ([keys[] | select(startswith("com.apple.security.temporary-exception."))] == []) and
      ([keys[] | select(startswith("com.apple.security.cs."))] == [])
    ' <<<"$ent" >/dev/null || return 1
    mg_app_personal_profile "$bundle" "$ent" "$device" ios Production || return 1
    if [ "$bundle" = "$app" ]; then
      mg_app_real_copy_profile_is "$bundle/embedded.mobileprovision" "${MG_APP_REAL_COPY_APP_PROFILE:-}" || return 1
      app_ent="$ent"
    else
      mg_app_real_copy_profile_is "$bundle/embedded.mobileprovision" "${MG_APP_REAL_COPY_WIDGET_PROFILE:-}" || return 1
      widget_ent="$ent"
    fi
    info="$(plutil -convert json -o - "$bundle/Info.plist")" || return 1
    jq -e --arg identity "$identity" '.CFBundleIdentifier == $identity and .CFBundleSupportedPlatforms == ["iPhoneOS"]' \
      <<<"$info" >/dev/null || return 1
    jq -e '.MeguriAppGroupIdentifier == "group.com.antoinefink.meguri"' <<<"$info" >/dev/null || return 1
    if [ "$bundle" = "$app" ]; then
      app_info="$info"
      jq -e '.CFBundleExecutable == "Meguri" and .MeguriCardInspectionBuild == "personalRelease"' <<<"$info" >/dev/null || return 1
    else
      jq -e '.CFBundleExecutable == "MeguriWidget"' <<<"$info" >/dev/null || return 1
    fi
  done
  python3 -B - "$MG_TOOLS/tests" "$MG_RUN_DIR/real-copy-settings.json" "$app_info" "$app_ent" "$widget_ent" <<'PY_CLOUD' || return 1
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from cloud_build_configuration import check
settings = json.loads(Path(sys.argv[2]).read_text())
app = next(target["buildSettings"] for target in settings if target["target"] == "Meguri")
assert all(target["buildSettings"]["DEVELOPMENT_TEAM"] == json.loads(sys.argv[4])["com.apple.developer.team-identifier"]
           for target in settings)
check(app, "Release", "ios", "team", *(json.loads(value) for value in sys.argv[3:6]), real_copy=True)
PY_CLOUD
  cert_dir="$(mktemp -d "$MG_RUN_DIR/real-copy-signer.XXXXXX")" || return 1
  if ! codesign -d --extract-certificates="$cert_dir/app" "$app" >/dev/null ||
      ! codesign -d --extract-certificates="$cert_dir/widget" "$widget" >/dev/null ||
      ! cmp -s "$cert_dir/app0" "$cert_dir/widget0"; then
    rm -rf "$cert_dir"
    return 1
  fi
  rm -rf "$cert_dir"
  [ "$(lipo -archs "$app/Meguri")" = arm64 ] && [ "$(lipo -archs "$widget/MeguriWidget")" = arm64 ] || return 1
  stamp="$(cat "$MG_ROOT/app/Packages/MeguriCore/Binaries/.stamp")" || return 1
  grep -qx 'configuration=Release' <<<"$stamp" && grep -qx 'profile=release' <<<"$stamp" &&
    grep -qx 'features=ankiweb' <<<"$stamp" || return 1
  hash="$(sed -n 's/^hash=//p' <<<"$stamp")"
  printf '%s\n' "$hash" | grep -Eq '^[0-9a-f]{32}$' || return 1
  slices="$(sed -n 's/^slices=//p' <<<"$stamp")"
  printf '%s\n' "$slices" | tr ',' '\n' | grep -qx device || return 1
  mg_app_real_copy_identity "$app" || return 1
  mg_detail real_copy_product "$(jq -nc --arg core_hash "$hash" --arg slices "$slices" '
    {signature_verified: true, provisioning_verified: true, platform: "ios", architecture: {app: "arm64", widget: "arm64"},
     provisioning: {app: "verified", widget: "verified"}, widget_signer_matches_app: true,
     bundle_id: "com.antoinefink.meguri", widget_bundle_id: "com.antoinefink.meguri.widget",
     cloud_environment: "production", push_environment: "development", card_inspection_build: "personalRelease",
     test_hooks: false, name: "Meguri", icon: "AppIcon",
     core: {hash: $core_hash, configuration: "Release", profile: "release", features: "ankiweb", slices: $slices}}')"
}

# mg_app_real_copy_profile_is <profile file> <uuid>: the embedded profile is
# the one the preflight selected and the build was pinned to.
mg_app_real_copy_profile_is() {
  [ -n "$2" ] && [ "$(security cms -D -i "$1" 2>/dev/null | python3 -c '
import plistlib, sys
print(plistlib.loads(sys.stdin.buffer.read()).get("UUID", ""))' 2>/dev/null)" = "$2" ]
}

# The personal preview shows the dev identity: "Meguri Dev" and the blue icon.
mg_app_personal_identity() {
  python3 "$MG_TOOLS/tests/app_identity_check.py" product "$1" --identity dev >&2
}

# The name and icon the system shows: "Meguri" and the red AppIcon.
mg_app_real_copy_identity() {
  python3 "$MG_TOOLS/tests/app_identity_check.py" product "$1" --identity daily >&2
}

# Unregister and delete the built app (and the widget inside), then check
# LaunchServices and PlugInKit no longer know it. Runs on every exit path.
mg_app_real_copy_forget() {
  local app="$1" products records dump plugins path
  [ -n "$app" ] || return 0
  products="${app%/Meguri.app}"
  # Never follow a link out of the checkout (to /Applications, say): refuse.
  path="$app"
  while [ "$path" != "$MG_ROOT" ] && [ -n "$path" ] && [ "$path" != / ]; do
    [ ! -L "$path" ] || return 1
    path="${path%/*}"
  done
  if [ -e "$app" ]; then
    # The widget sits under Contents/PlugIns on the Mac, PlugIns on iOS.
    pluginkit -r "$app/Contents/PlugIns/MeguriWidget.appex" >/dev/null 2>&1
    pluginkit -r "$app/PlugIns/MeguriWidget.appex" >/dev/null 2>&1
    "$MG_LSREGISTER" -u "$app" >/dev/null 2>&1
    rm -rf "$app" || return 1
  fi
  # Xcode also leaves the widget product itself beside the app.
  if [ -e "$products/MeguriWidget.appex" ]; then
    pluginkit -r "$products/MeguriWidget.appex" >/dev/null 2>&1
    rm -rf "$products/MeguriWidget.appex" || return 1
  fi
  # An unreadable LaunchServices database proves nothing: fail.
  dump="$("$MG_LSREGISTER" -dump 2>/dev/null)" && grep -q '^path: ' <<<"$dump" || return 1
  plugins="$(pluginkit -mADvvv -i com.antoinefink.meguri.widget 2>/dev/null)" || return 1
  records="$(sed -n 's/^path: *\(.*\) (0x[0-9a-fA-F]*)$/\1/p' <<<"$dump"
    sed -n 's/^[[:space:]]*Path = //p' <<<"$plugins")"
  # Nothing registered from this build's products directory.
  ! awk -v p="$products" '$0 == p || index($0, p "/") == 1 { found = 1 } END { exit !found }' <<<"$records"
}

# build-app --real-copy --owner-requested --install: the coordinator installs
# the real copy at Antoine's request (standing decision 2026-11-21). Refuses
# while the installed Meguri runs, and when /Applications/Meguri.app is not
# the daily driver (rename a test copy to "Meguri Dev.app" first); an earlier
# real copy goes to the Trash (its data stays in its container). Never launches.
MG_APP_REAL_COPY_APPLICATIONS=/Applications
MG_APP_REAL_COPY_TRASH="$HOME/.Trash"
# mg_app_real_copy_install_ready: whether --install may proceed (also checked
# before building). Meguri must not run (pgrep: 1 = none; anything else
# refuses), and an existing /Applications/Meguri.app must be the daily driver.
mg_app_real_copy_install_ready() {
  local target="$MG_APP_REAL_COPY_APPLICATIONS/Meguri.app" existing rc
  pgrep -f "^$target/Contents/MacOS/Meguri( |$)" >/dev/null 2>&1
  rc=$?
  if [ "$rc" -ne 1 ]; then
    mg_failure real-copy-install "Meguri is running from $target, or that could not be checked: quit it, then install again; nothing was changed"
    return 1
  fi
  if [ -e "$target" ] || [ -L "$target" ]; then
    existing="$(plutil -convert json -o - "$target/Contents/Info.plist" 2>/dev/null | jq -r '.CFBundleIdentifier // empty' 2>/dev/null)"
    if [ -L "$target" ] || [ "$existing" != com.antoinefink.meguri ]; then
      mg_failure real-copy-install "$target is not the daily-driver app (${existing:-unreadable}); rename a test copy to \"Meguri Dev.app\" first; nothing was changed"
      return 1
    fi
  fi
}

# mg_app_real_copy_same_volume <a> <b>: both paths on one file system, so a
# mv between them is a rename (atomic, and its roll-back is one too).
mg_app_real_copy_same_volume() {
  python3 -c 'import os, sys; sys.exit(os.stat(sys.argv[1]).st_dev != os.stat(sys.argv[2]).st_dev)' "$1" "$2" 2>/dev/null
}

# mg_app_real_copy_drop_stage <stage>: the staging folder leaves LaunchServices,
# PlugInKit and the disk, whether its app is still in it or was moved away.
mg_app_real_copy_drop_stage() {
  [ -n "$1" ] || return 1
  pluginkit -r "$1/Meguri.app/Contents/PlugIns/MeguriWidget.appex" >/dev/null 2>&1
  "$MG_LSREGISTER" -u "$1/Meguri.app" >/dev/null 2>&1
  rm -rf "${1:?}"
}

mg_app_real_copy_install_mac() {
  local zip="$1" target="$MG_APP_REAL_COPY_APPLICATIONS/Meguri.app" stage stage_root old=""
  mg_app_real_copy_install_ready || { mg_status fail; return 1; }
  # Unpack and verify outside Applications, where LaunchServices registers new
  # apps on sight (it kept such a staging path, 2026-11-22), but on its
  # volume, so the swap stays a rename. Otherwise stage beside the target.
  stage_root="$MG_ROOT/$(dirname "$MG_APP_REAL_COPY_ZIP")"
  mkdir -p "$stage_root" 2>/dev/null
  if ! mg_app_real_copy_same_volume "$stage_root" "$MG_APP_REAL_COPY_APPLICATIONS"; then
    stage_root="$MG_APP_REAL_COPY_APPLICATIONS"
  fi
  # A killed earlier install left its staging folder (a daily-id app): drop it.
  local leftover
  for leftover in "$stage_root"/.meguri-install.*; do
    [ ! -d "$leftover" ] || [ -L "$leftover" ] || mg_app_real_copy_drop_stage "$leftover"
  done
  stage="$(mktemp -d "$stage_root/.meguri-install.XXXXXX")" && [ -n "$stage" ] || {
    mg_failure real-copy-install "could not create a staging folder in $stage_root; nothing was changed"
    mg_status fail; return 1
  }
  mg_on_exit "[ ! -d $(mvl_q "$stage") ] || mg_app_real_copy_drop_stage $(mvl_q "$stage")"
  if ! ditto -x -k "$zip" "$stage/" || ! codesign --verify --deep --strict "$stage/Meguri.app" ||
      [ "$(plutil -convert json -o - "$stage/Meguri.app/Contents/Info.plist" 2>/dev/null | jq -r '.CFBundleIdentifier // empty')" != com.antoinefink.meguri ]; then
    mg_app_real_copy_drop_stage "$stage"
    mg_failure real-copy-install "the real copy did not unpack and verify cleanly; nothing was changed"
    mg_status fail; return 1
  fi
  if [ -e "$target" ]; then
    old="$MG_APP_REAL_COPY_TRASH/Meguri $(date +%Y%m%d-%H%M%S)-$$.app"
    # The earlier copy leaves LaunchServices and PlugInKit before the Trash.
    pluginkit -r "$target/Contents/PlugIns/MeguriWidget.appex" >/dev/null 2>&1
    "$MG_LSREGISTER" -u "$target" >/dev/null 2>&1
    if [ -e "$old" ] || ! mkdir -p "$MG_APP_REAL_COPY_TRASH" || ! mv "$target" "$old"; then
      # It stays in place: register it again.
      "$MG_LSREGISTER" -f "$target" >/dev/null 2>&1
      mg_app_real_copy_drop_stage "$stage"
      mg_failure real-copy-install "could not move the earlier real copy to the Trash (Full Disk Access?); it stays at $target"
      mg_status fail; return 1
    fi
    mg_detail real_copy_previous "$(jq -nc --arg p "$old" '{moved_to: $p}')"
  fi
  if ! mv "$stage/Meguri.app" "$target"; then
    local restored="nothing was installed and there was no earlier copy"
    if [ -n "$old" ]; then
      if mv "$old" "$target"; then
        "$MG_LSREGISTER" -f "$target" >/dev/null 2>&1
        restored="the earlier one is back at $target"
      else
        restored="the earlier one could not be put back and is still at $old"
      fi
    fi
    mg_app_real_copy_drop_stage "$stage"
    mg_failure real-copy-install "the new real copy could not be moved into place; $restored"
    mg_status fail; return 1
  fi
  # The staging path leaves LaunchServices and PlugInKit; the installed path
  # is registered in its place (never launched).
  mg_app_real_copy_drop_stage "$stage"
  "$MG_LSREGISTER" -f "$target" >/dev/null 2>&1
  mg_detail real_copy_installed "$(jq -nc --arg p "$target" '{path: $p, launched: false}')"
  echo "build-app: installed $target (not launched)." >&2
  if ! mg_app_real_copy_registration_check "$target" "$stage"; then
    mg_message "build-app: the real copy is installed at $target, but its LaunchServices registration needs the repair in failures[0]"
    mg_status fail; return 1
  fi
}

# mg_app_real_copy_registration_check <target> <stage>: LaunchServices lists
# the installed app and no staging path (this run's or a leftover
# .meguri-install.* of an earlier one, which it unregisters first); PlugInKit
# lists no staging widget. Read-only but for those unregistrations.
mg_app_real_copy_registration_check() {
  local target="$1" stage="$2" dump records plugins stale p pass
  [ -n "$stage" ] && [ -n "$target" ] || return 1
  for pass in 1 2; do
    dump="$("$MG_LSREGISTER" -dump 2>/dev/null)" && grep -q '^path: ' <<<"$dump" || {
      mg_failure real-copy-registration "LaunchServices could not be read after the install, so its registration of $target is unverified (the app is installed): run \"$MG_LSREGISTER -dump | grep -E '^path: .*Meguri.*\\.app'\""
      return 1
    }
    plugins="$(pluginkit -mADvvv -i com.antoinefink.meguri.widget 2>/dev/null)" || {
      mg_failure real-copy-registration "PlugInKit could not be read after the install, so the widget registration of $target is unverified (the app is installed): run \"pluginkit -mAvvv -i com.antoinefink.meguri.widget\""
      return 1
    }
    records="$(sed -n 's/^path: *\(.*\) (0x[0-9a-fA-F]*)$/\1/p' <<<"$dump"
      sed -n 's/^[[:space:]]*Path = //p' <<<"$plugins")"
    stale="$(awk -v s="$stage" -v a="$MG_APP_REAL_COPY_APPLICATIONS/.meguri-install." -v b="$MG_ROOT/$(dirname "$MG_APP_REAL_COPY_ZIP")/.meguri-install." '
      $0 == s || index($0, s "/") == 1 || index($0, a) == 1 || index($0, b) == 1' <<<"$records" | sort -u)"
    [ -n "$stale" ] && [ "$pass" = 1 ] || break
    while IFS= read -r p; do
      case "$p" in
        *.appex) pluginkit -r "$p" >/dev/null 2>&1 ;;
        *) "$MG_LSREGISTER" -u "$p" >/dev/null 2>&1 ;;
      esac
    done <<<"$stale"
    # A stale record may have stood for the moved bundle: register it again.
    "$MG_LSREGISTER" -f "$target" >/dev/null 2>&1
  done
  if [ -n "$stale" ]; then
    mg_failure real-copy-registration "LaunchServices or PlugInKit still lists a staging path after the install: $(tr '\n' ' ' <<<"$stale")(the app is installed at $target); remove each with \"$MG_LSREGISTER -u <path>\" (\"pluginkit -r <path>\" for a widget), then run \"$MG_LSREGISTER -f $target\""
    return 1
  fi
  if ! grep -Fxq "$target" <<<"$records"; then
    mg_failure real-copy-registration "LaunchServices does not list $target after the install (the app is installed there): run \"$MG_LSREGISTER -f $target\", then check \"$MG_LSREGISTER -dump | grep -E '^path: .*Meguri.*\\.app'\""
    return 1
  fi
  mg_detail real_copy_registration "$(jq -nc --arg p "$target" '{installed_path_registered: true, staging_paths_registered: false, path: $p}')"
}

# What Antoine runs, read-only, once he has installed the zip.
mg_app_real_copy_cleanup_checks() {
  jq -nc --arg zip "$MG_APP_REAL_COPY_ZIP" --arg dd "$MG_APP_DD" --arg ls "$MG_LSREGISTER" '[
    {step: "install (the coordinator, at Antoine'"'"'s request, Meguri quit): build-app --real-copy --owner-requested --install, or by hand after moving any /Applications/Meguri.app aside (ditto merges into an existing bundle)", command: ("cd <the checkout> && ditto -x -k " + $zip + " /Applications/")},
    {step: "only the installed copies are registered", command: ($ls + " -dump | grep -E \"^path: .*Meguri.*\\.app\""),
     expect: "only /Applications/Meguri.app (daily driver) and /Applications/Meguri Dev.app, besides dev build products under build/"},
    {step: "only the installed widget is registered", command: "pluginkit -mAvvv -i com.antoinefink.meguri.widget",
     expect: "one path, inside /Applications/Meguri.app"},
    {step: "no daily-driver product left in derived data", command: ("ls " + $dd + "/Build/Products/Release"),
     expect: "no Meguri.app and no MeguriWidget.appex"}]'
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
  local pinned rid actual config="${2:-tools/sim-devices.json}"
  pinned="$(jq -c .runtime "$config")"
  rid="$(mg_simctl list devices --json 2>/dev/null |
    jq -r --arg u "$1" 'first(.devices | to_entries[] | select(any(.value[]; .udid == $u)) | .key) // empty')"
  actual="$(mg_simctl list runtimes --json 2>/dev/null |
    jq -c --arg r "$rid" 'first(.runtimes[] | select(.identifier == $r) | {identifier, version, build: .buildversion}) // null')"
  [ -n "$actual" ] || actual=null
  # A foldable pool device runs the foldable kind's own pinned runtime.
  if [ "$rid" = "$(jq -r .foldable.runtime.identifier "$config")" ]; then
    pinned="$(jq -c .foldable.runtime "$config")"
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
  local platform="" config=Debug config_given=false plan=UI standalone=false personal=false real=false owner=false install=false input="" app agent_source="" agent_tree=""
  # Set by mg_app_dd_clean: why this run removed the route's derived data.
  local MG_APP_DD_CLEANED=""
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
      --real-copy) real=true; shift ;;
      --owner-requested) owner=true; shift ;;
      --install) install=true; shift ;;
      *) mg_drive__usage "usage: tools/mg build-app --platform ios-sim|mac [--config C] [--test-plan UI|CloudAgent] [--standalone|--personal-preview|--real-copy --owner-requested [--install]]"; return 0 ;;
    esac
  done
  [ -n "$platform" ] || { mg_drive__usage "usage: tools/mg build-app --platform ios-sim|mac [--config C] [--test-plan UI|CloudAgent] [--standalone|--personal-preview|--real-copy --owner-requested [--install]]"; return 0; }
  if { [ "$owner" = true ] || [ "$install" = true ]; } && [ "$real" != true ]; then
    mg_drive__usage "--owner-requested and --install go only with --real-copy"
    return 0
  fi
  if [ "$real" = true ]; then
    # Antoine's real copy: refuse before any work unless he asked for it.
    if [ "$platform" != mac ] || [ "$standalone" = true ] || [ "$personal" = true ] || [ "$plan" != UI ] ||
        { [ "$config_given" = true ] && [ "$config" != Release ]; }; then
      mg_drive__usage "--real-copy requires --platform mac, Release (explicit or derived), and no --standalone, --personal-preview or --test-plan"
      return 0
    fi
    if [ "$owner" != true ]; then
      mg_drive__usage "--real-copy builds Antoine's daily-driver app: run it only at his request, with --owner-requested"
      return 0
    fi
    if [ "${MEGURI_APP_SIGNING-adhoc}" != team ]; then
      mg_drive__usage "--real-copy requires MEGURI_APP_SIGNING=team: ad-hoc signing drops the Keychain group, so the Meguri Sync sign-in could not be stored"
      return 0
    fi
    mg_build_app__real_copy "$install"
    return 0
  fi
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
    app="$(mg_app_products "$platform" "$config")/Meguri.app"
    while :; do
      mg_app_build_action personal-preview "$platform" "$config" || return 0
      if ! printf '%s\n' "$input" | grep -Eq '^[0-9a-f]{64}$' ||
          [ "$input" != "$(mg_app_input_digest)" ] || ! mg_app_personal_check "$app"; then
        mg_failure personal-preview-product "personal preview validation failed: require stable source digest, valid arm64 dev app/widget signatures and provisioning, normal sandbox entitlements without fixture scope, Release core without hooks and personalRelease inspection policy; see log.txt"
        mg_status fail
        return 0
      fi
      # The test copy must never look like the daily driver: "Meguri Dev", blue icon.
      mg_app_personal_identity "$app" && break
      # A stale incremental product can carry the other identity's files:
      # build once more from an empty folder, unless this build was one.
      if [ -n "$MG_APP_DD_CLEANED" ]; then
        mg_failure personal-preview-identity "personal preview does not show the dev identity (Meguri Dev, blue icon), also when built from an empty $MG_APP_DD; see log.txt"
        mg_status fail
        return 0
      fi
      mg_app_dd_clean stale-product "$app" || return 0
    done
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
  mg_message "build-app: $platform $config built: $app$(mg_app_dd_note)"
  mg_status pass
}

# build-app --real-copy --owner-requested (checked by mg_build_app): profile
# preflight, the build, its checks, the zip, then the built app forgotten.
mg_build_app__real_copy() {
  local install="${1:-false}" MG_APP_DD=build/app/RealCopyDerivedData MG_APP_REAL_COPY_ALLOWED=true app input profiles hardware zip
  app="$(mg_app_products mac Release)/Meguri.app"
  # Refuse --install before any work when it could not complete.
  if [ "$install" = true ] && ! mg_app_real_copy_install_ready; then
    mg_status fail; return 0
  fi
  mg_detail real_copy_cleanup_checks "$(mg_app_real_copy_cleanup_checks)"
  # A killed earlier run may have left a registered build: forget it first.
  if ! mg_app_real_copy_forget "$app"; then
    mg_failure real-copy-forget "an earlier daily-driver build is still on disk or registered, or LaunchServices could not be read: $app"
    mg_status fail
    return 0
  fi
  # Fail before building when no installed profile authorizes the app: the
  # route never asks Xcode to create, download or register one.
  hardware=""
  if hardware="$(system_profiler SPHardwareDataType -json 2>/dev/null)"; then
    hardware="$(jq -r '.SPHardwareDataType[0] | (.provisioning_UDID // .platform_UUID) // empty' <<<"$hardware" 2>/dev/null)"
  else
    hardware=""
  fi
  if [ -z "$hardware" ]; then
    mg_failure real-copy-profile "this Mac's provisioning UDID could not be read, so profile coverage cannot be checked"
    mg_status fail
    return 0
  fi
  if ! profiles="$(python3 "$MG_TOOLS/scripts/daily-driver-profiles.py" --require mac --device "$hardware")"; then
    local missing="the profile list could not be read"
    if [ -n "$profiles" ]; then
      mg_detail daily_driver_profiles "$profiles"
      missing="$(jq -r '.mac.missing | join(", ")' <<<"$profiles")"
    fi
    mg_failure real-copy-profile "$MG_APP_REAL_COPY_PROFILE_HELP; missing: $missing" "tools/mg doctor"
    mg_message "build-app: the real copy was not built: no authorizing profile is installed (nothing was registered)"
    mg_status fail
    return 0
  fi
  mg_detail daily_driver_profiles "$profiles"
  local MG_APP_REAL_COPY_APP_PROFILE
  MG_APP_REAL_COPY_APP_PROFILE="$(jq -r '.mac.uuid // empty' <<<"$profiles")"
  # Whatever happens next, no built daily-driver app stays on disk or registered.
  mg_on_exit "mg_app_real_copy_forget $(mvl_q "$app") || { mg_failure real-copy-forget $(mvl_q "the built daily-driver app is still on disk or registered: $app"); mg_status fail; }"
  input="$(mg_app_input_digest)"
  while :; do
    mg_app_build_action real-copy mac Release || return 0
    if printf '%s\n' "$input" | grep -Eq '^[0-9a-f]{64}$' &&
        [ "$input" = "$(mg_app_input_digest)" ] && mg_app_real_copy_check "$app"; then
      break
    fi
    # A stale incremental product can carry the other identity's files: when
    # the name and icon are wrong, build once more from an empty folder,
    # unless this build was one.
    if [ -z "$MG_APP_DD_CLEANED" ] && [ "$input" = "$(mg_app_input_digest)" ] && ! mg_app_real_copy_identity "$app"; then
      mg_app_dd_clean stale-product "$app" || return 0
      continue
    fi
    mg_failure real-copy-product "real copy validation failed: require stable source digest, valid arm64 daily-driver app/widget signatures and installed provisioning, normal sandbox entitlements with production CloudKit and development push, Release core without hooks, and Meguri with the red icon; see log.txt"
    mg_status fail
    return 0
  done
  zip="$MG_ROOT/$MG_APP_REAL_COPY_ZIP"
  mkdir -p "$(dirname "$zip")" && rm -f "$zip" || { mg_failure real-copy-zip "could not prepare $MG_APP_REAL_COPY_ZIP"; mg_status error; return 0; }
  if ! ditto -c -k --keepParent "$app" "$zip"; then
    mg_failure real-copy-zip "ditto could not pack the real copy"
    mg_status fail
    return 0
  fi
  if ! mg_app_real_copy_forget "$app"; then
    mg_failure real-copy-forget "the built daily-driver app is still on disk or registered with LaunchServices or PlugInKit: $app"
    mg_status fail
    return 0
  fi
  mg_detail input "$(jq -nc --arg input "$input" '$input')"
  mg_detail real_copy "$(jq -nc --arg zip "$MG_APP_REAL_COPY_ZIP" --arg sha "$(shasum -a 256 "$zip" | cut -c1-64)" \
    '{zip: $zip, zip_sha256: $sha, built_app_forgotten: true}')"
  mg_artifact zip "$zip" "the real copy$([ "$install" = true ] || echo ", for the coordinator to install at Antoine's request")"
  if [ "$install" = true ]; then
    mg_app_real_copy_install_mac "$zip" || return 0
  fi
  {
    echo "build-app: the real copy is $MG_APP_REAL_COPY_ZIP; the built app was unregistered and deleted."
    echo "build-app: after Antoine installs it, these read-only checks confirm only the installed copies are registered:"
    mg_app_real_copy_cleanup_checks | jq -r '.[] | "  \(.step): \(.command)" + (if .expect then "\n      expect: \(.expect)" else "" end)'
  } >&2
  if [ "$install" = true ]; then
    mg_message "build-app: real copy installed in $MG_APP_REAL_COPY_APPLICATIONS (not launched; zip $MG_APP_REAL_COPY_ZIP; built app forgotten; LaunchServices lists only the installed path; clean-up checks in details.real_copy_cleanup_checks)$(mg_app_dd_note)"
  else
    mg_message "build-app: real copy packed: $MG_APP_REAL_COPY_ZIP (built app forgotten; see the clean-up checks in details.real_copy_cleanup_checks)$(mg_app_dd_note)"
  fi
  mg_status pass
}

mg_run() {
  local seconds=5 window="" args=() a personal=false real=false owner=false seconds_given=false
  # --seconds and --window-size are run's own; the rest are drive's.
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --personal-preview) personal=true; shift ;;
      --real-copy) real=true; shift ;;
      --owner-requested) owner=true; shift ;;
      --seconds)
        [ "$#" -ge 2 ] && case "$2" in "" | *[!0-9]*) false ;; esac || { mg_drive__usage "--seconds takes whole seconds"; return 0; }
        seconds="$2" && seconds_given=true && shift 2
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
  if [ "$owner" = true ] && [ "$real" != true ]; then
    mg_drive__usage "--owner-requested acknowledges only --real-copy"; return 0
  fi
  if [ "$real" = true ]; then
    # Antoine's real copy on his phone: refuse before any work unless he asked.
    [ "$owner" = true ] || { mg_drive__usage "--real-copy installs Antoine's daily-driver app: run it only at his request, with --owner-requested"; return 0; }
    [ "$personal" = false ] && [ -z "$window" ] && [ "$seconds_given" = false ] || {
      mg_drive__usage "--real-copy installs only: no --personal-preview, --window-size or --seconds"; return 0;
    }
    mg_run__personal real-copy ${args[0]+"${args[@]}"}
    return 0
  fi
  if [ "$personal" = true ]; then
    [ -z "$window" ] && [ "$seconds_given" = false ] || {
      mg_drive__usage "--personal-preview requires a normal physical launch without run overrides"; return 0;
    }
    mg_run__personal personal-preview ${args[0]+"${args[@]}"}
    return 0
  fi
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

# Consume Release before the Debug-only shared parser. Only this manual run
# route admits the device preview; drive/continuity retain their existing policy.
mg_run__personal() {
  # mg_run passes the mode first: personal-preview or real-copy.
  local args=() platform="" mode="$1" name="--$1"
  shift
  mg_word_in "$mode" "personal-preview real-copy" || { mg_drive__usage "unknown physical preview mode"; return 0; }
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --config)
        [ "$#" -ge 2 ] && [ "$2" = Release ] || {
          mg_drive__usage "$name implies Release; explicit --config must be Release"; return 0;
        }
        shift 2 ;;
      --platform|--physical-device)
        [ "$#" -ge 2 ] || { mg_drive__usage "$name requires --platform iphone|ipad and --physical-device <explicit existing identifier>"; return 0; }
        [ "$1" != --platform ] || platform="$2"
        args[${#args[@]}]="$1"; args[${#args[@]}]="$2"; shift 2 ;;
      *) mg_drive__usage "$name accepts only --platform iphone|ipad, --physical-device and optional --config Release; no launch overrides"; return 0 ;;
    esac
  done
  mg_word_in "$platform" "iphone ipad" || { mg_drive__usage "$name requires --platform iphone or ipad"; return 0; }
  mg_drive__args ${args[0]+"${args[@]}"} || return 0
  [ -n "$MG_D_PHYSICAL" ] || { mg_drive__usage "$name requires --physical-device <explicit existing identifier>"; return 0; }
  mg_run__physical "$MG_D_PHYSICAL" "$MG_D_KIND" "$mode"
}

# Normal preparation is separate from preservation: a build, installation and
# development-app restart are authorized here, never in --keep-app-running.
mg_run__physical() {
  local device="$1" kind="$2" mode="${3:-debug}" source after xctestrun MG_UI_PHYSICAL_DEVICE=""
  local MG_APP_PERSONAL_DEVICE_ALLOWED=false
  mg_ui_test__physical_check "$device" "$kind" || return 0
  mg_host_lease --no-session || return 0
  mg_ui_test__physical_check "$device" "$kind" || return 0
  if [ "$mode" = personal-preview ]; then
    MG_APP_PERSONAL_DEVICE_ALLOWED=true
    mg_run__physical_personal "$device"
    return 0
  fi
  if [ "$mode" = real-copy ]; then
    # Also on SIGTERM: the job shell forgets the signed phone products.
    mg_on_exit "mg_app_real_copy_forget $(mvl_q "$MG_ROOT/build/app/RealCopyDeviceDerivedData/Build/Products/Release-iphoneos/Meguri.app")"
    mg_run__physical_real_copy "$device"
    return 0
  fi
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

# The lease and exact inventory are owned by mg_run__physical. Seal local
# inputs before policy verification, then match those same bytes at installation.
mg_run__physical_personal() (
  umask 077
  local device="$1" source app
  local MG_APP_DD=build/app/PersonalDevicePreviewDerivedData MEGURI_APP_SIGNING=team
  local MG_APP_DD_CLEANED=""
  source="$(mg_app_input_digest)"
  if ! printf '%s\n' "$source" | grep -Eq '^[0-9a-f]{64}$'; then
    mg_failure physical-source "personal preview source evidence unavailable"; mg_status fail; return 0
  fi
  mg_app_build_action personal-preview ios-device Release >"$MG_RUN_DIR/personal-preview-build.log" 2>&1 || return 0
  app="$(mg_app_products ios-device Release)/Meguri.app"
  if [ "$source" != "$(mg_app_input_digest)" ] ||
      ! python3 -B -I "$MG_TOOLS/scripts/physical-drive.py" preview-seal --root "$MG_ROOT" \
        --output "$MG_RUN_DIR/personal-preview" --device "$device" --source "$source" \
        >"$MG_RUN_DIR/personal-preview-verification.log" 2>&1 ||
      ! mg_app_personal_check "$app" ios-device "$device" >>"$MG_RUN_DIR/personal-preview-verification.log" 2>&1 ||
      [ "$source" != "$(mg_app_input_digest)" ]; then
    mg_failure personal-preview-product "device preview requires stable sources, signed arm64 dev app/widget, authorized device development profiles and Release core without test hooks; see private verification evidence"
    mg_status fail; return 0
  fi
  # Registered before launch: a locked device can retain the install witness
  # even when normal launch fails. These artifacts never authorize Debug drive.
  mg_artifact json "$MG_RUN_DIR/personal-preview/preview-install-witness.json" "private personal preview install witness (when installed)"
  if ! python3 -B -I "$MG_TOOLS/scripts/physical-drive.py" preview --root "$MG_ROOT" \
      --output "$MG_RUN_DIR/personal-preview" --device "$device" --source "$source" \
      >"$MG_RUN_DIR/personal-preview-install.log" 2>&1; then
    mg_failure personal-preview-launch "personal device preview install or normal launch failed; any verified install witness remains private; no fallback"
    mg_status fail; return 0
  fi
  mg_artifact json "$MG_RUN_DIR/personal-preview/preview-launch.json" "private personal preview normal launch"
  mg_detail input "$(jq -nc --arg input "$source" '$input')"
  mg_detail normal_launch_verified true
  mg_message "run: signed Release personal preview installed and launched normally on the selected physical device$(mg_app_dd_note)"
  mg_status pass
)

# The owner-requested real copy on a phone: the lease and exact inventory are
# mg_run__physical's. Profiles of app and widget covering this device first,
# then the daily-driver Release with development push signed from them, its
# checks, a sealed install with its witness. Never launched (root AGENTS.md).
mg_run__physical_real_copy() (
  umask 077
  local device="$1" profiles missing app
  local MG_APP_DD=build/app/RealCopyDeviceDerivedData MEGURI_APP_SIGNING=team MG_APP_REAL_COPY_ALLOWED=true
  local MG_APP_REAL_COPY_APP_PROFILE="" MG_APP_REAL_COPY_WIDGET_PROFILE=""
  local MG_APP_DD_CLEANED=""
  if ! profiles="$(python3 "$MG_TOOLS/scripts/daily-driver-profiles.py" --require ios --ios-device "$device")"; then
    missing="the profile list could not be read"
    if [ -n "$profiles" ]; then
      mg_detail daily_driver_profiles "$profiles"
      missing="app: $(jq -r '.ios.missing | join(", ")' <<<"$profiles"); widget: $(jq -r '.ios_widget.missing | join(", ")' <<<"$profiles")"
      if jq -e '[.ios.missing[], .ios_widget.missing[]] | index("this-device")' <<<"$profiles" >/dev/null; then
        missing="$missing; the installed iOS profiles do not list the selected device $device: add it to both development profiles (app and widget) under Devices, then Xcode › Settings › Accounts › Download Manual Profiles"
      fi
    fi
    mg_failure real-copy-profile "$MG_APP_REAL_COPY_PROFILE_HELP; missing: $missing" "tools/mg doctor"
    mg_message "run: the real copy was not built: no authorizing iOS profiles for this device are installed (nothing was registered or installed)"
    mg_status fail; return 0
  fi
  mg_detail daily_driver_profiles "$profiles"
  MG_APP_REAL_COPY_APP_PROFILE="$(jq -r '.ios.uuid // empty' <<<"$profiles")"
  MG_APP_REAL_COPY_WIDGET_PROFILE="$(jq -r '.ios_widget.uuid // empty' <<<"$profiles")"
  # Before the long build: Developer Mode, pairing and lock, read-only.
  mg_run__physical_device_ready "$device" || return 0
  # Apple silicon can register iOS products: the signed daily-id app never
  # stays in derived data, before or after this run.
  app="$(mg_app_products ios-device Release)/Meguri.app"
  if ! mg_app_real_copy_forget "$app"; then
    mg_failure real-copy-forget "an earlier daily-driver phone build is still on disk or registered, or LaunchServices could not be read: $app"
    mg_status fail; return 0
  fi
  mg_run__physical_real_copy_steps "$device" "$app"
  if ! mg_app_real_copy_forget "$app"; then
    mg_failure real-copy-forget "the built daily-driver phone app is still on disk or registered: $app"
    mg_status fail
  fi
)

# mg_app_physical_device_failure <developer-mode|unpaired|locked|unreachable> <what happened>:
# a fixed failure naming the step on the selected device. Device names and
# devicectl or Xcode diagnostics never enter it.
mg_app_physical_device_failure() {
  case "$1" in
    developer-mode)
      mg_failure physical-developer-mode "Developer Mode is off on the selected device: on it, open Settings › Privacy & Security › Developer Mode, turn it on, let it restart and confirm Turn On; then run the same command again ($2)"
      mg_user_step device-developer-mode missing true "Turn on Developer Mode on the selected iOS device" \
        "On the device: Settings › Privacy & Security › Developer Mode, turn it on, let it restart and confirm Turn On; then run the same command again."
      ;;
    unpaired)
      mg_failure physical-device-unpaired "the selected device is not paired with this Mac: attach it to this Mac over USB, unlock it, tap Trust on it, and pair it in Xcode; then run the same command again ($2)" ;;
    locked)
      mg_failure physical-device-locked "the selected device is locked: unlock it and keep it unlocked until the install finishes; then run the same command again ($2)" ;;
    *)
      mg_failure physical-device-unreachable "this Mac could not reach the selected device: attach it to this Mac over USB, unlock it, and check that Xcode lists it as available; then run the same command again ($2)" ;;
  esac
}

# mg_run__physical_device_ready <device>: read-only devicectl queries (device
# details, then lock state) that tell Developer Mode off, an unpaired device
# and a locked one apart before the long build. Xcode otherwise reports only
# a destination time-out. Raw responses stay private: kept on failure,
# removed on success; the receipt gets the three states only.
mg_run__physical_device_ready() {
  local device="$1" dir="$MG_RUN_DIR/device-preflight" state lock=unknown kind=""
  mkdir -p "$dir" && chmod 700 "$dir" || { mg_failure physical-device-unreachable "could not prepare the private device check folder"; mg_status error; return 1; }
  if ! xcrun devicectl device info details --device "$device" --json-output "$dir/details.json" \
        --log-output "$dir/details.log" >/dev/null 2>&1 ||
      ! state="$(jq -ce 'if .info.outcome == "success" and (.result | type) == "object" then .result |
          {developer_mode: (.deviceProperties.developerModeStatus as $v | if $v == null then "unknown"
             elif ($v | type) == "string" and ($v == "enabled" or $v == "disabled") then $v else "other" end),
           pairing: (.connectionProperties.pairingState as $v | if $v == null then "unknown"
             elif ($v | type) == "string" and ($v == "paired" or $v == "unpaired") then $v else "other" end)}
          else error("refused") end' "$dir/details.json" 2>/dev/null)"; then
    kind=unreachable
    ! grep -Eqi 'Developer Mode (is )?disabled|enable Developer Mode' "$dir/details.json" "$dir/details.log" 2>/dev/null || kind=developer-mode
  elif [ "$(jq -r .developer_mode <<<"$state")" = disabled ]; then
    kind=developer-mode
  elif ! mg_word_in "$(jq -r .pairing <<<"$state")" "paired unknown"; then
    kind=unpaired
  else
    # A failed lock query leaves the build to report it.
    if xcrun devicectl device info lockState --device "$device" --json-output "$dir/lock.json" \
          --log-output "$dir/lock.log" >/dev/null 2>&1; then
      lock="$(jq -r 'if .info.outcome != "success" then "unknown"
        elif .result.passcodeRequired == true or .result.unlockedSinceBoot == false then "locked"
        elif .result.passcodeRequired == false then "unlocked" else "unknown" end' "$dir/lock.json" 2>/dev/null)" || lock=unknown
    fi
    [ "$lock" != locked ] || kind=locked
  fi
  mg_detail device_preflight "$(jq -nc --argjson s "${state:-null}" --arg lock "$lock" --arg kind "${kind:-ready}" \
    '{developer_mode: ($s.developer_mode // "unknown"), pairing: ($s.pairing // "unknown"), lock: $lock, result: $kind}')"
  if [ -n "$kind" ]; then
    mg_app_physical_device_failure "$kind" "nothing was built or installed"
    mg_message "$MG_JOB: the selected device is not ready ($kind); nothing was built or installed"
    mg_status fail
    return 1
  fi
  rm -rf "$dir"
}

# The build, checks, seal and install of mg_run__physical_real_copy. Never launches.
mg_run__physical_real_copy_steps() {
  local device="$1" app="$2" source
  source="$(mg_app_input_digest)"
  if ! printf '%s\n' "$source" | grep -Eq '^[0-9a-f]{64}$'; then
    mg_failure physical-source "real copy source evidence unavailable"; mg_status fail; return 0
  fi
  mg_app_build_action real-copy ios-device Release >"$MG_RUN_DIR/real-copy-build.log" 2>&1 || return 0
  if [ "$source" != "$(mg_app_input_digest)" ] || ! mg_run__real_copy_drive real-seal "$device" "$source" ||
      ! mg_app_real_copy_device_check "$app" "$device" >>"$MG_RUN_DIR/real-copy-verification.log" 2>&1 ||
      [ "$source" != "$(mg_app_input_digest)" ]; then
    mg_failure real-copy-product "the phone's real copy requires stable sources, signed arm64 daily-driver app/widget from the selected device profiles, production CloudKit with development push, Release core without test hooks, and Meguri with the red icon; see private verification evidence"
    mg_status fail; return 0
  fi
  mg_artifact json "$MG_RUN_DIR/real-copy/real-install-witness.json" "private real copy install witness (when installed)"
  if ! mg_run__real_copy_drive real-install "$device" "$source"; then
    mg_failure real-copy-install "the real copy could not be installed on the selected device (it is never launched); any verified install witness remains private; no fallback"
    mg_status fail; return 0
  fi
  mg_detail input "$(jq -nc --arg input "$source" '$input')"
  mg_detail launched false
  mg_detail real_copy_profiles "$(jq -nc --arg app "$MG_APP_REAL_COPY_APP_PROFILE" --arg widget "$MG_APP_REAL_COPY_WIDGET_PROFILE" '{app: $app, widget: $widget}')"
  mg_message "run: the real copy (Release, daily-driver ids, development push) is installed on the selected device; it was not launched$(mg_app_dd_note)"
  mg_status pass
}

# mg_run__real_copy_drive <real-seal|real-install> <device> <source>: physical-drive.py
# seals or installs the checked phone product (devicectl; never a launch).
mg_run__real_copy_drive() {
  local log=verification
  [ "$1" != real-install ] || log=install
  python3 -B -I "$MG_TOOLS/scripts/physical-drive.py" "$1" --root "$MG_ROOT" \
    --output "$MG_RUN_DIR/real-copy" --device "$2" --source "$3" >>"$MG_RUN_DIR/real-copy-$log.log" 2>&1
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
