#!/bin/bash
# Checks of the Xcode project (U7), run by `tools/mg xcode-check` on the host;
# prints TAP.
#
# usage: xcode_project_check.sh <work dir> [--iphone UDID] [--ipad UDID] [--no-ui]
#
# Builds every configuration for the Mac and the iPhone and iPad simulators
# into <work dir>/DerivedData and checks build settings, zero warnings, the
# core stamp, entitlements, architectures, the hook code (present in Debug
# and Profile, absent in Release and AppStore), folder-synchronized groups
# and the test plans, then runs the Unit plan, the app-hosted tests and
# TestHooksTests (into <work dir>/DerivedData-tests).
# Without --iphone/--ipad it creates throwaway simulators and deletes them at
# exit. Release and AppStore builds are launched on simulators only: on the
# Mac they would replace the daily-driver app. --no-ui skips the test runs.

set -u

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
WORK="${1:?usage: xcode_project_check.sh <work dir> [--iphone UDID] [--ipad UDID] [--no-ui]}"
shift
IPHONE=""
IPAD=""
UI=true
while [ "$#" -gt 0 ]; do
  case "$1" in
    --iphone) IPHONE="${2:?}" && shift ;;
    --ipad) IPAD="${2:?}" && shift ;;
    --no-ui) UI=false ;;
    *) echo "xcode_project_check: unknown argument '$1'" >&2 && exit 2 ;;
  esac
  shift
done
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd -P)"
DD="$WORK/DerivedData"
# Xcode signs a Mac app it builds for testing with extra entitlements (read
# access to /): test runs build elsewhere than the checked products.
DD_TESTS="$WORK/DerivedData-tests"
LOGS="$WORK/logs"
mkdir -p "$LOGS"
# Build settings are read once per run, and every product checked below is
# signed by this run (a test build into the same DerivedData would have
# re-signed them with test entitlements).
rm -rf "$WORK"/settings-*.txt "$DD/Build/Products"
PROJECT="$REPO/app/Meguri.xcodeproj"
STAMP="$REPO/app/Packages/MeguriCore/Binaries/.stamp"
CONFIGS="Debug Profile Release AppStore"
RUNTIME=com.apple.CoreSimulator.SimRuntime.iOS-27-0

N=0
FAILED=0
ok() { N=$((N + 1)) && echo "ok $N $1"; }
not_ok() {
  N=$((N + 1)) && FAILED=$((FAILED + 1))
  echo "not ok $N $1"
  shift
  local line
  for line in "$@"; do printf '# %s\n' "$line"; done
}
skip() { N=$((N + 1)) && echo "ok $N $1 # SKIP $2"; }
check() { # check <name> <command...>: ok when the command succeeds; its output is the diagnostic
  local name="$1" out
  shift
  if out="$("$@" 2>&1)"; then ok "$name"; else not_ok "$name" "${out:-exit $?}"; fi
}

CREATED=()
cleanup() {
  local udid
  for udid in ${CREATED[@]+"${CREATED[@]}"}; do
    xcrun simctl shutdown "$udid" >/dev/null 2>&1
    xcrun simctl delete "$udid" >/dev/null 2>&1
  done
  rm -f "$REPO/app/Meguri/App/SyncedFolderProbe"*.swift
}
trap cleanup EXIT
trap 'exit 130' INT TERM

simulator() { # simulator <name> <device type>: a throwaway device's UDID
  local udid
  udid="$(xcrun simctl create "Meguri-check-$$-$1" "$2" "$RUNTIME")" || return 1
  CREATED+=("$udid")
  echo "$udid"
}
[ -n "$IPHONE" ] || IPHONE="$(simulator iPhone17Pro com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro)" ||
  not_ok "create an iPhone 17 Pro simulator" "simctl create failed"
[ -n "$IPAD" ] || IPAD="$(simulator iPadPro13 com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-12GB)" ||
  not_ok "create an iPad Pro 13-inch (M5) simulator" "simctl create failed"

# setting <config> <sdk> <name>: one resolved build setting of the Meguri target.
settings_cache() { echo "$WORK/settings-$1-$2.txt"; }
setting() {
  local file
  file="$(settings_cache "$1" "$2")"
  [ -s "$file" ] || xcodebuild -showBuildSettings -project "$PROJECT" -scheme Meguri -configuration "$1" -sdk "$2" \
    -derivedDataPath "$DD" 2>/dev/null | awk '/Build settings for action build and target Meguri:/{f=1;next} /Build settings for/{f=0} f' >"$file"
  sed -n "s/^ *$3 = //p" "$file" | head -n 1
}

expect_setting() { # expect_setting <config> <sdk> <name> <expected>
  local actual
  actual="$(setting "$1" "$2" "$3")"
  [ "$actual" = "$4" ] || { echo "$3 is '$actual', expected '$4'" && return 1; }
}

# hook_markers <file...>: which hook markers the binaries contain, as
# "<swift names found> <rust exports found>": the Swift type names
# (NUL-terminated in the type metadata, which even a stripped binary keeps)
# and the Rust test-hook exports. An optimized build drops the metadata of
# types used only statically, but keeps the `TestHooks` namespace whenever
# any hook type has metadata.
hook_markers() {
  perl -e '
    my @swift = ("TestHooks\0", "LaunchOptions\0", "FixtureInstaller\0", "RouteRegistry\0",
      "AppearanceOverrides\0", "TestHookError\0");
    my $all = "";
    for my $file (@ARGV) {
      open(my $h, "<:raw", $file) or die "$file: $!";
      local $/;
      $all .= <$h>;
    }
    my @found = grep { index($all, $_) >= 0 } @swift;
    print join(",", map { s/\0//r } @found) || "-", " ",
      (index($all, "_uniffi_meguri_ffi_fn_func_test_") >= 0 ? "rust" : "-"), "\n";' "$@"
}

# executables <executable dir> <name>: the app executable and the dylibs beside it.
executables() {
  local f
  for f in "$1/$2" "$1"/*.dylib; do [ -f "$f" ] && echo "$f"; done
}

entitlement() { # entitlement <bundle> <key>: the value as JSON
  codesign -d --entitlements - --xml "$1" 2>/dev/null | plutil -convert json -o - - | jq -c --arg k "$2" '.[$k]'
}

expect_entitlement() { # expect_entitlement <bundle> <key> <JSON value>
  local actual
  actual="$(entitlement "$1" "$2")"
  [ "$actual" = "$3" ] || { echo "$2 is $actual, expected $3" && return 1; }
}

# Simulator builds carry their entitlements in the executable's
# __TEXT,__entitlements section, not in the signature.
expect_sim_entitlement() { # expect_sim_entitlement <executable> <key> <JSON value>
  local plist="$WORK/sim-entitlements.plist" actual
  rm -f "$plist"
  segedit "$1" -extract __TEXT __entitlements "$plist" || return 1
  actual="$(plutil -convert json -o - "$plist" | jq -c --arg k "$2" '.[$k]')"
  [ "$actual" = "$3" ] || { echo "$2 is $actual, expected $3" && return 1; }
}

hardened() { codesign -d -v "$1" 2>&1 | grep -q '^CodeDirectory.*flags=.*runtime' || { codesign -d -v "$1" 2>&1 | grep flags; return 1; }; }

no_build_settings_in_pbxproj() {
  local inline
  inline="$(awk '/buildSettings = \{/ {f = 1; next} f && /^[[:space:]]*\};/ {f = 0; next} f' "$PROJECT/project.pbxproj")"
  [ -z "$inline" ] || { echo "$inline" && return 1; }
}

# --- Project structure ------------------------------------------------------
list="$(xcodebuild -list -json -project "$PROJECT" 2>/dev/null)"
check "the project has the four targets, configurations and shared schemes" jq -e '
  .project
  | (.targets == ["Meguri", "MeguriWidget", "MeguriUITests", "MeguriSnapshotTests"])
    and (.configurations == ["Debug", "Profile", "Release", "AppStore"])
    and ((.schemes | index("Meguri")) != null) and ((.schemes | index("Meguri-Profile")) != null)
    and ((.schemes | index("Meguri-AppStore")) != null)' <<<"$list"

check "project.pbxproj holds no build settings (they live in app/Config)" no_build_settings_in_pbxproj

# --- Build settings per configuration (R2031, R1967, KTD18, KTD21, KTD22) ----
conditions_ok() { # conditions_ok <config> <sdk> <hooks yes|no> <ankiweb yes|no>
  local c
  c=" $(setting "$1" "$2" SWIFT_ACTIVE_COMPILATION_CONDITIONS) "
  case "$c" in *" MEGURI_TEST_HOOKS "*) [ "$3" = yes ] || { echo "MEGURI_TEST_HOOKS in '$c'" && return 1; } ;;
    *) [ "$3" = no ] || { echo "no MEGURI_TEST_HOOKS in '$c'" && return 1; } ;; esac
  case "$c" in *" ANKIWEB_ENABLED "*) [ "$4" = yes ] || { echo "ANKIWEB_ENABLED in '$c'" && return 1; } ;;
    *) [ "$4" = no ] || { echo "no ANKIWEB_ENABLED in '$c'" && return 1; } ;; esac
}
for config in $CONFIGS; do
  case "$config" in
    Debug | Profile) hooks=yes ankiweb=yes features=ankiweb,test-hooks id=com.antoinefink.meguri.dev ;;
    Release) hooks=no ankiweb=yes features=ankiweb id=com.antoinefink.meguri ;;
    AppStore) hooks=no ankiweb=no features="" id=com.antoinefink.meguri ;;
  esac
  for sdk in macosx iphonesimulator; do
    check "$config $sdk: compilation conditions (hooks $hooks, AnkiWeb $ankiweb)" conditions_ok "$config" "$sdk" "$hooks" "$ankiweb"
    check "$config $sdk: core features '$features'" expect_setting "$config" "$sdk" MEGURI_CORE_FEATURES "$features"
    check "$config $sdk: bundle id $id" expect_setting "$config" "$sdk" PRODUCT_BUNDLE_IDENTIFIER "$id"
    check "$config $sdk: arm64 only" expect_setting "$config" "$sdk" ARCHS arm64
  done
  check "$config: iOS App Group group.$id" expect_setting "$config" iphonesimulator MEGURI_APP_GROUP "group.$id"
  check "$config: Mac App Group DNQ34W43XX.$id" expect_setting "$config" macosx MEGURI_APP_GROUP "DNQ34W43XX.$id"
  case "$config" in
    Debug) check "Debug: debugger entitlement, no hardened runtime" bash -c '[ "$1" = YES ] && [ "$2" = NO ]' _ \
      "$(setting Debug macosx CODE_SIGN_INJECT_BASE_ENTITLEMENTS)" "$(setting Debug macosx ENABLE_HARDENED_RUNTIME)" ;;
    *) check "$config: no debugger entitlement, hardened runtime on the Mac" bash -c '[ "$1" = NO ] && [ "$2" = YES ]' _ \
      "$(setting "$config" macosx CODE_SIGN_INJECT_BASE_ENTITLEMENTS)" "$(setting "$config" macosx ENABLE_HARDENED_RUNTIME)" ;;
  esac
done

# --- Test plans (R1866, R1953) ----------------------------------------------
for plan in Unit UI Snapshot Performance; do
  check "test plan $plan has no automatic retries" jq -e '
    [.defaultOptions, (.configurations[].options)] | all(
      ((.testRepetitionMode // "none") == "none") and (has("maximumTestRepetitions") | not))' \
    "$REPO/app/TestPlans/$plan.xctestplan"
done
check "test plan Performance: no coverage, no sanitizers, no screenshots, not parallel" jq -e '
  .defaultOptions as $o
  | ($o.codeCoverage == false) and ($o.addressSanitizer.enabled == false) and ($o.threadSanitizerEnabled == false)
    and ($o.undefinedBehaviorSanitizerEnabled == false) and ($o.uiTestingScreenshotsLifetime == "keepNever")
    and (.testTargets | all(.parallelizable == false))' "$REPO/app/TestPlans/Performance.xctestplan"
check "scheme Meguri-Profile tests in Profile without a debugger" bash -c '
  f="$1/app/Meguri.xcodeproj/xcshareddata/xcschemes/Meguri-Profile.xcscheme"
  [ "$(xmllint --xpath "string(//TestAction/@buildConfiguration)" "$f")" = Profile ] &&
  [ -z "$(xmllint --xpath "string(//TestAction/@selectedDebuggerIdentifier)" "$f")" ] &&
  [ "$(xmllint --xpath "string(//BuildActionEntry/@buildForArchiving)" "$f")" = NO ]' _ "$REPO"

# --- Builds -----------------------------------------------------------------
# build <name> <config> <destination>: xcodebuild build with a result bundle;
# sets BUILT to the products directory.
build() {
  local name="$1" config="$2" dest="$3" result="$WORK/$1.xcresult" counts
  rm -rf "$result"
  xcodebuild build -project "$PROJECT" -scheme Meguri -configuration "$config" -destination "$dest" \
    -derivedDataPath "$DD" -resultBundlePath "$result" >"$LOGS/$name.log" 2>&1
  local rc=$?
  counts="$(xcrun xcresulttool get build-results --path "$result" 2>/dev/null |
    jq -r '"\(.errorCount) \(.warningCount) \([.warnings[]?.message] | join(" | "))"')"
  local errors="${counts%% *}" rest="${counts#* }"
  local warnings="${rest%% *}" messages="${rest#* }"
  if [ "$rc" -eq 0 ] && [ "$errors" = 0 ] && [ "$warnings" = 0 ]; then
    ok "$name builds with zero warnings"
    return 0
  fi
  not_ok "$name builds with zero warnings" "xcodebuild exit $rc, ${errors:-?} error(s), ${warnings:-?} warning(s): $messages" \
    "log: $LOGS/$name.log" "$(grep -E 'error:' "$LOGS/$name.log" | head -n 5)"
  return 1
}

stamp_is() { # stamp_is <config> <features> <slice>
  local s
  s="$(cat "$STAMP" 2>/dev/null)"
  grep -qx "configuration=$1" <<<"$s" && grep -qx "features=$2" <<<"$s" &&
    grep -q "^slices=.*$3" <<<"$s" || { echo "stamp: $s" && return 1; }
}

entitlements_are() { # entitlements_are <bundle> <expected keys...>
  local bundle="$1" actual expected
  shift
  actual="$(codesign -d --entitlements - --xml "$bundle" 2>/dev/null | plutil -convert json -o - - | jq -r 'keys[]' | sort)"
  expected="$(printf '%s\n' "$@" | sort)"
  [ "$actual" = "$expected" ] || { printf 'entitlements: %s\nexpected: %s\n' "$(echo $actual)" "$(echo $expected)" && return 1; }
}

KTD22="com.apple.security.app-sandbox com.apple.security.network.client com.apple.security.network.server com.apple.security.files.user-selected.read-write com.apple.security.files.bookmarks.app-scope"
FIXTURES=com.apple.security.temporary-exception.files.absolute-path.read-only
for config in $CONFIGS; do
  case "$config" in
    Debug | Profile) features=ankiweb,test-hooks ;;
    Release) features=ankiweb ;;
    AppStore) features="" ;;
  esac
  build "mac-$config" "$config" "platform=macOS,arch=arm64" || continue
  app="$DD/Build/Products/$config/Meguri.app"
  check "mac-$config links the $config core (features '$features')" stamp_is "$config" "$features" mac
  case "$config" in
    Debug)
      # shellcheck disable=SC2086
      check "mac-Debug entitlements: KTD22 set, fixture read access, debugger" entitlements_are "$app" $KTD22 $FIXTURES com.apple.security.get-task-allow
      check "mac-Debug fixture read access is the checkout, read-only" expect_entitlement "$app" "$FIXTURES" "[\"$REPO/\"]" ;;
    Profile)
      # shellcheck disable=SC2086
      check "mac-Profile entitlements: KTD22 set and fixture read access" entitlements_are "$app" $KTD22 $FIXTURES ;;
    *)
      # shellcheck disable=SC2086
      check "mac-$config entitlements are exactly the KTD22 set (ad hoc: no App Group)" entitlements_are "$app" $KTD22
      check "mac-$config has the hardened runtime" hardened "$app" ;;
  esac
  widget=com.apple.security.app-sandbox
  [ "$config" != Debug ] || widget="$widget com.apple.security.get-task-allow"
  # shellcheck disable=SC2086
  check "mac-$config widget entitlements: sandbox only (ad hoc)" entitlements_are "$app/Contents/PlugIns/MeguriWidget.appex" $widget
  check "mac-$config app and widget are arm64 only" bash -c '
    [ "$(lipo -archs "$1/Contents/MacOS/Meguri")" = arm64 ] &&
    [ "$(lipo -archs "$1/Contents/PlugIns/MeguriWidget.appex/Contents/MacOS/MeguriWidget")" = arm64 ] ||
    { lipo -archs "$1/Contents/MacOS/Meguri"; exit 1; }' _ "$app"
  # shellcheck disable=SC2046
  markers="$(hook_markers $(executables "$app/Contents/MacOS" Meguri))"
  case "$config" in
    Debug | Profile) check "mac-$config contains the test hooks (found: $markers)" \
      bash -c 'case "$1" in TestHooks*" rust") ;; *) exit 1 ;; esac' _ "$markers" ;;
    *) check "mac-$config contains no test hook (found: $markers)" test "$markers" = "- -" ;;
  esac
done

if [ -n "$IPHONE" ] && build "iphone-Debug" Debug "id=$IPHONE"; then
  check "iphone-Debug links the Debug core with the simulator slice" stamp_is Debug ankiweb,test-hooks sim
  check "iphone-Debug entitlements: the .dev App Group" expect_sim_entitlement "$DD/Build/Products/Debug-iphonesimulator/Meguri.app/Meguri" \
    com.apple.security.application-groups '["group.com.antoinefink.meguri.dev"]'

fi
[ -z "$IPAD" ] || build "ipad-Debug" Debug "id=$IPAD"

# --- Folder-synchronized groups ---------------------------------------------
probe="SyncedFolderProbe$$"
if printf 'enum %s {\n    static let marker = "%s"\n}\n' "$probe" "$probe" >"$REPO/app/Meguri/App/$probe.swift" &&
  build "mac-Debug-probe" Debug "platform=macOS,arch=arm64"; then
  # shellcheck disable=SC2046
  check "a new Swift file under app/Meguri is compiled" \
    perl -e 'my $p = shift; local $/; for (@ARGV) { open(my $h, "<:raw", $_) or die; exit 0 if index(<$h>, $p) >= 0 } exit 1' \
    "$probe" $(executables "$DD/Build/Products/Debug/Meguri.app/Contents/MacOS" Meguri)
  check "adding it needed no project change" git -C "$REPO" diff --quiet -- app/Meguri.xcodeproj
fi
rm -f "$REPO/app/Meguri/App/$probe.swift"

# --- Tests ------------------------------------------------------------------
# run_tests <name> <scheme> <plan> <config> <destination> [-only-testing:...]
run_tests() {
  local name="$1" scheme="$2" plan="$3" config="$4" dest="$5" result="$WORK/$1.xcresult"
  shift 5
  rm -rf "$result"
  xcodebuild test -project "$PROJECT" -scheme "$scheme" -testPlan "$plan" -configuration "$config" -destination "$dest" \
    -derivedDataPath "$DD_TESTS" -resultBundlePath "$result" "$@" >"$LOGS/$name.log" 2>&1
  local rc=$? summary
  summary="$(xcrun xcresulttool get test-results summary --path "$result" 2>/dev/null |
    jq -r '"\(.result) passed=\(.passedTests) failed=\(.failedTests) skipped=\(.skippedTests)"')"
  if [ "$rc" -eq 0 ]; then
    ok "$name: $summary"
  elif grep -q 'failed to initialize for UI testing' "$LOGS/$name.log"; then
    skip "$name" "user step uitest-automation: macOS has not allowed UI automation (docs/process/one-time-setup.md)"
  else
    not_ok "$name: ${summary:-no result}" "xcodebuild exit $rc; log: $LOGS/$name.log" \
      "$(grep -E 'error:|failed \(' "$LOGS/$name.log" | head -n 8)"
  fi
}

if [ "$UI" = true ]; then
  run_tests mac-unit Meguri Unit Debug "platform=macOS,arch=arm64"
  run_tests mac-app-hosted Meguri Snapshot Debug "platform=macOS,arch=arm64"
  run_tests mac-Debug-TestHooksTests Meguri UI Debug "platform=macOS,arch=arm64" -only-testing:MeguriUITests/TestHooksTests
  if [ -n "$IPHONE" ]; then
    run_tests iphone-app-hosted Meguri Snapshot Debug "id=$IPHONE"
    for config in Debug Release AppStore; do
      run_tests "iphone-$config-TestHooksTests" Meguri UI "$config" "id=$IPHONE" -only-testing:MeguriUITests/TestHooksTests
    done
    # Profile through its own scheme and the Performance plan (no debugger).
    run_tests iphone-Profile-TestHooksTests Meguri-Profile Performance Profile "id=$IPHONE" -only-testing:MeguriUITests/TestHooksTests
  fi
  [ -z "$IPAD" ] || run_tests ipad-Debug-TestHooksTests Meguri UI Debug "id=$IPAD" -only-testing:MeguriUITests/TestHooksTests
fi

echo "1..$N"
[ "$FAILED" -eq 0 ]
