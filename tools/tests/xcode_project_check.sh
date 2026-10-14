#!/bin/bash
# Checks of the Xcode project (U7), run by `tools/mg xcode-check` on the host;
# prints TAP.
#
# usage: xcode_project_check.sh <work dir> [--phase builds|devices [--continue-from N]]
#          [--iphone UDID] [--ipad UDID] [--no-ui]
#
# Builds every configuration for the Mac and the iPhone and iPad simulators
# into <work dir>/DerivedData and checks build settings, zero warnings, the
# core stamp, entitlements, architectures, the hook code (present in Debug
# and Profile, absent in Release and AppStore), folder-synchronized groups
# and the test plans, launches the Mac Debug app hidden on Home and on every
# test-hook route and checks it stays up, then runs the Unit plan, the
# app-hosted tests, TestHooksTests and the agent driver's DriveScriptTests
# (into <work dir>/DerivedData-tests).
#
# Phases: `builds` is everything up to the Mac checks (no launch, no
# simulator, no test run, no TAP plan line); `devices` is the rest: the
# hidden Mac launches of the Debug app `builds` left, the simulator builds
# and every test run, numbered from N + 1. `tools/mg xcode-check` takes the
# host lease between the two (tools/mg.d/host.sh). Without --phase, both.
# Without --iphone/--ipad it leases the simulators from the shared pool
# (tools/scripts/sim-pool.sh) after the Mac configuration builds, and releases
# them at exit; it never creates, shuts down or deletes a simulator (root
# AGENTS.md). Release and AppStore builds are launched on simulators only: on
# the Mac they would replace the daily-driver app; their Mac builds are
# unregistered from LaunchServices and deleted once checked. TestHooksTests run
# on the Mac only with MEGURI_ALLOW_MAC_UI=1 (`tools/mg mac-ui`) and are
# reported as queued otherwise. --no-ui skips the test runs.

set -u

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
WORK="${1:?usage: xcode_project_check.sh <work dir> [--iphone UDID] [--ipad UDID] [--no-ui]}"
shift
IPHONE=""
IPAD=""
UI=true
PHASE=all
FIRST=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --iphone) IPHONE="${2:?}" && shift ;;
    --ipad) IPAD="${2:?}" && shift ;;
    --no-ui) UI=false ;;
    --phase)
      case "${2:-}" in builds | devices) PHASE="$2" ;; *) echo "xcode_project_check: --phase takes builds or devices" >&2 && exit 2 ;; esac
      shift
      ;;
    --continue-from)
      case "${2:-}" in "" | *[!0-9]*) echo "xcode_project_check: --continue-from takes a test count" >&2 && exit 2 ;; esac
      FIRST="$2" && shift
      ;;
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
# re-signed them with test entitlements). The devices phase uses the Debug
# app the builds phase left.
[ "$PHASE" = devices ] || rm -rf "$WORK"/settings-*.txt "$DD/Build/Products" "$WORK/mac-debug-built"
PROJECT="$REPO/app/Meguri.xcodeproj"
STAMP="$REPO/app/Packages/MeguriCore/Binaries/.stamp"
CONFIGS="Debug Profile Release AppStore"
POOL="$REPO/tools/scripts/sim-pool.sh"

N="$FIRST"
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
builds_phase() { [ "$PHASE" != devices ]; }
devices_phase() { [ "$PHASE" != builds ]; }
# Written by the builds phase when mac-Debug built: the devices phase
# launches that app.
MAC_DEBUG_BUILT="$WORK/mac-debug-built"

# Xcode registers every Mac app it builds with LaunchServices, and the widget
# inside with PlugInKit. Release and AppStore carry the daily-driver ids: left
# registered, a build copy (ad hoc, no App Group) could take the installed
# app's widget, URL scheme or documents. They are unregistered and deleted
# once checked, and at exit.
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
forget_mac_app() { # forget_mac_app <config>: unregisters and deletes a Release or AppStore Mac app
  case "$1" in Release | AppStore) ;; *) return 0 ;; esac
  local app="$DD/Build/Products/$1/Meguri.app"
  pluginkit -r "$app/Contents/PlugIns/MeguriWidget.appex" >/dev/null 2>&1
  "$LSREGISTER" -u "$app" >/dev/null 2>&1
  rm -rf "$app"
}

# running <executable>: the PIDs of the processes running it.
running() {
  ps -Aww -o pid=,command= | awk -v e="$1" '
    { pid = $1; sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "") }
    $0 == e || index($0, e " ") == 1 { print pid }'
}

# quit_running <executable>: terminates the processes running it (SIGKILL after 10 s).
quit_running() {
  local pids
  pids="$(running "$1")"
  [ -n "$pids" ] || return 0
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    [ -n "$(running "$1")" ] || return 0
    sleep 1
  done
  # shellcheck disable=SC2046
  kill -9 $(running "$1") 2>/dev/null
}

LEASED=()
cleanup() {
  local udid
  for udid in ${LEASED[@]+"${LEASED[@]}"}; do
    "$POOL" release "$udid" >&2
  done
  quit_running "$DD/Build/Products/Debug/Meguri.app/Contents/MacOS/Meguri"
  forget_mac_app Release
  forget_mac_app AppStore
  rm -f "$REPO/app/Meguri/App/SyncedFolderProbe"*.swift
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# lease <kind> <name>: leases a pool device of <kind> (sim-pool.sh) and sets
# LEASE to its UDID, or reports the failure and sets LEASE to "". Called
# directly, not in a command substitution: LEASED must outlive it.
lease() {
  local log="$LOGS/sim-pool-$1.log" rc
  LEASE="$("$POOL" acquire "$1" --holder xcode-check 2>"$log")"
  rc=$?
  if [ "$rc" -eq 0 ] && [ -n "$LEASE" ]; then
    LEASED+=("$LEASE")
    return 0
  fi
  not_ok "lease an $2 simulator from the pool" "$(tail -n 1 "$log")" "sim-pool.sh acquire $1 exit $rc; log: $log"
  LEASE=""
  return 1
}

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
if builds_phase; then
  list="$(xcodebuild -list -json -project "$PROJECT" 2>/dev/null)"
  check "the project has the four targets, configurations and shared schemes" jq -e '
    .project
    | (.targets == ["Meguri", "MeguriWidget", "MeguriUITests", "MeguriSnapshotTests"])
      and (.configurations == ["Debug", "Profile", "Release", "AppStore"])
      and ((.schemes | index("Meguri")) != null) and ((.schemes | index("Meguri-Profile")) != null)
      and ((.schemes | index("Meguri-AppStore")) != null)' <<<"$list"

  check "project.pbxproj holds no build settings (they live in app/Config)" no_build_settings_in_pbxproj
fi

# routes_from_source: the test-hook routes, read from their registrations
# (`register("area/screen")`), for the hidden-launch check below. Fails when a
# route is registered with a computed path, which this cannot read.
routes_from_source() {
  local dir="$REPO/app/Meguri/TestHooks" literal='register\("[a-z0-9][a-z0-9/-]*"[,)]' other routes
  [ -d "$dir" ] || { echo "no $dir" && return 1; }
  other="$(grep -rn --include='*.swift' 'register(' "$dir" |
    grep -vE -e '^[^:]+:[0-9]+:[[:space:]]*//' -e 'func register\(' -e 'register\(in:' -e "$literal")"
  [ -z "$other" ] || { printf 'registered with a computed path:\n%s\n' "$other" && return 1; }
  routes="$(grep -rhoE --include='*.swift' "$literal" "$dir" | sed -E 's/^register\("//; s/"[,)]$//' | sort -u)"
  [ -n "$routes" ] || { echo "no route registration in $dir" && return 1; }
  echo "$routes"
}
if ! builds_phase; then
  ROUTES="$(routes_from_source 2>/dev/null)" || ROUTES=""
elif ROUTES="$(routes_from_source 2>&1)"; then
  ok "test-hook routes are registered with literal paths: ${ROUTES//$'\n'/ }"
else
  not_ok "test-hook routes are registered with literal paths" "$ROUTES"
  ROUTES=""
fi

# --- Build settings per configuration (R2031, R1967, KTD18, KTD21, KTD22) ----
conditions_ok() { # conditions_ok <config> <sdk> <hooks yes|no> <ankiweb yes|no>
  local c
  c=" $(setting "$1" "$2" SWIFT_ACTIVE_COMPILATION_CONDITIONS) "
  case "$c" in *" MEGURI_TEST_HOOKS "*) [ "$3" = yes ] || { echo "MEGURI_TEST_HOOKS in '$c'" && return 1; } ;;
    *) [ "$3" = no ] || { echo "no MEGURI_TEST_HOOKS in '$c'" && return 1; } ;; esac
  case "$c" in *" ANKIWEB_ENABLED "*) [ "$4" = yes ] || { echo "ANKIWEB_ENABLED in '$c'" && return 1; } ;;
    *) [ "$4" = no ] || { echo "no ANKIWEB_ENABLED in '$c'" && return 1; } ;; esac
}
if builds_phase; then
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

  # --- Test plans (R1866, R1953) --------------------------------------------
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
  # XCTest's own failure attachments include a recording of the whole screen:
  # on the Mac, the user's desktop. UI tests attach what they need themselves.
  check "test plan UI keeps no system attachments" jq -e '.defaultOptions.uiTestingScreenshotsLifetime == "keepNever"' \
    "$REPO/app/TestPlans/UI.xctestplan"
  check "scheme Meguri-Profile tests in Profile without a debugger" bash -c '
    f="$1/app/Meguri.xcodeproj/xcshareddata/xcschemes/Meguri-Profile.xcscheme"
    [ "$(xmllint --xpath "string(//TestAction/@buildConfiguration)" "$f")" = Profile ] &&
    [ -z "$(xmllint --xpath "string(//TestAction/@selectedDebuggerIdentifier)" "$f")" ] &&
    [ "$(xmllint --xpath "string(//BuildActionEntry/@buildForArchiving)" "$f")" = NO ]' _ "$REPO"
fi

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

# stays_up <app> [launch args...]: launches the app hidden and in the
# background, as agents must (`open -g -j`, root AGENTS.md), checks that it
# still runs 3 s later, then quits it. On the Mac an alert presented while the
# app is hidden closes the window, and the app quits (app/AGENTS.md).
stays_up() {
  local app="$1" opener rc=0
  shift
  open -g -j -n -W "$app" --args -ApplePersistenceIgnoreState YES "$@" >/dev/null 2>&1 &
  opener=$!
  sleep 3
  if ! kill -0 "$opener" 2>/dev/null; then
    wait "$opener"
    echo "the app quit within 3 s of a hidden launch (open -W exit $?)"
    rc=1
  fi
  quit_running "$app/Contents/MacOS/Meguri"
  kill "$opener" 2>/dev/null
  wait "$opener" 2>/dev/null
  return "$rc"
}

KTD22="com.apple.security.app-sandbox com.apple.security.network.client com.apple.security.network.server com.apple.security.files.user-selected.read-write com.apple.security.files.bookmarks.app-scope"
FIXTURES=com.apple.security.temporary-exception.files.absolute-path.read-only
if builds_phase; then
  for config in $CONFIGS; do
    case "$config" in
      Debug | Profile) features=ankiweb,test-hooks ;;
      Release) features=ankiweb ;;
      AppStore) features="" ;;
    esac
    build "mac-$config" "$config" "platform=macOS,arch=arm64" || { forget_mac_app "$config"; continue; }
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
    [ "$config" != Debug ] || : >"$MAC_DEBUG_BUILT"
    forget_mac_app "$config"
  done

  # not_registered <app...>: the apps are gone, and LaunchServices and
  # PlugInKit hold no record of them or of anything inside them.
  not_registered() {
    local records app found left=""
    records="$("$LSREGISTER" -dump 2>/dev/null | sed -n 's/^path: *\(.*\) (0x[0-9a-fA-F]*)$/\1/p'
      pluginkit -mADvvv -i com.antoinefink.meguri.widget 2>/dev/null | sed -n 's/^[[:space:]]*Path = //p')"
    for app in "$@"; do
      [ ! -e "$app" ] || left="$left$app is still on disk"$'\n'
      found="$(awk -v p="$app" '$0 == p || index($0, p "/") == 1 { print $0 " is registered" }' <<<"$records")"
      [ -z "$found" ] || left="$left$found"$'\n'
    done
    [ -z "$left" ] || { printf '%s' "$left" && return 1; }
  }
  check "mac Release and AppStore apps are deleted and unregistered (LaunchServices, PlugInKit)" \
    not_registered "$DD/Build/Products/Release/Meguri.app" "$DD/Build/Products/AppStore/Meguri.app"

  # --- Folder-synchronized groups -------------------------------------------
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
fi

# --- Launches and simulators (the devices phase) ----------------------------
# Hidden launches of the Mac Debug app the builds left, on Home and on every
# test-hook route.
if devices_phase && [ -f "$MAC_DEBUG_BUILT" ]; then
  app="$DD/Build/Products/Debug/Meguri.app"
  check "mac-Debug launched hidden stays up: Home" stays_up "$app"
  for route in $ROUTES; do
    check "mac-Debug launched hidden stays up: route $route" stays_up "$app" -MeguriRoute "$route"
  done
fi

if devices_phase; then
  # Pool devices are leased only now, after the Mac configuration builds, so
  # that other agents can use them meanwhile.
  [ -n "$IPHONE" ] || { lease iphone iPhone && IPHONE="$LEASE"; }
  [ -n "$IPAD" ] || { lease ipad iPad && IPAD="$LEASE"; }
  if [ -n "$IPHONE" ] && build "iphone-Debug" Debug "id=$IPHONE"; then
    check "iphone-Debug links the Debug core with the simulator slice" stamp_is Debug ankiweb,test-hooks sim
    check "iphone-Debug entitlements: the .dev App Group" expect_sim_entitlement "$DD/Build/Products/Debug-iphonesimulator/Meguri.app/Meguri" \
      com.apple.security.application-groups '["group.com.antoinefink.meguri.dev"]'

  fi
  [ -z "$IPAD" ] || build "ipad-Debug" Debug "id=$IPAD"
fi

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

if devices_phase && [ "$UI" = true ]; then
  run_tests mac-unit Meguri Unit Debug "platform=macOS,arch=arm64"
  run_tests mac-app-hosted Meguri Snapshot Debug "platform=macOS,arch=arm64"
  # Mac XCUITest takes over the cursor and keyboard: it runs only through
  # `tools/mg mac-ui`, which sets MEGURI_ALLOW_MAC_UI (root AGENTS.md).
  if [ "${MEGURI_ALLOW_MAC_UI:-}" = 1 ]; then
    run_tests mac-Debug-TestHooksTests Meguri UI Debug "platform=macOS,arch=arm64" -only-testing:MeguriUITests/TestHooksTests
  else
    skip mac-Debug-TestHooksTests "queued for tools/mg mac-ui"
  fi
  if [ -n "$IPHONE" ]; then
    run_tests iphone-app-hosted Meguri Snapshot Debug "id=$IPHONE"
    for config in Debug Release AppStore; do
      run_tests "iphone-$config-TestHooksTests" Meguri UI "$config" "id=$IPHONE" -only-testing:MeguriUITests/TestHooksTests
    done
    # Profile through its own scheme and the Performance plan (no debugger).
    run_tests iphone-Profile-TestHooksTests Meguri-Profile Performance Profile "id=$IPHONE" -only-testing:MeguriUITests/TestHooksTests
    # The agent driver's script parser (U8); it launches no app.
    run_tests iphone-Debug-DriveScriptTests Meguri UI Debug "id=$IPHONE" -only-testing:MeguriUITests/DriveScriptTests
  fi
  [ -z "$IPAD" ] || run_tests ipad-Debug-TestHooksTests Meguri UI Debug "id=$IPAD" -only-testing:MeguriUITests/TestHooksTests
fi

[ "$PHASE" = builds ] || echo "1..$N"
[ "$FAILED" -eq 0 ]
