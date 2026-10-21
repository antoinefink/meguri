#!/usr/bin/env bats
# Control flow of tools/tests/xcode_project_check.sh (tools/mg xcode-check),
# run by `tools/mg selftest` with the host tools and the simulator pool
# stubbed: which simulators it leases and releases, and which test runs it
# starts. The checks themselves run on the host, through `tools/mg xcode-check`.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$SB/tools/tests" "$SB/tools/scripts" "$SB/app/Meguri/App"
  cp "$REPO_ROOT/tools/tests/xcode_project_check.sh" "$SB/tools/tests/"
  cp "$REPO_ROOT/tools/scripts/mac-vm-lib.sh" "$SB/tools/scripts/"
  # Keep regression-only application usable for behavioral RED on the old
  # runner, which has not extracted this helper yet.
  if [ -f "$REPO_ROOT/tools/scripts/mac-hidden-launch.sh" ]; then
    cp "$REPO_ROOT/tools/scripts/mac-hidden-launch.sh" "$SB/tools/scripts/"
  fi
  STUBS="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$STUBS"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  : >"$STUB_LOG"
  # The pool leases POOL-<kind>; with STUB_POOL_BUSY=<kind>, every device of
  # that kind stays leased (exit 3, as sim-pool.sh does); with
  # STUB_POOL_UNREACHABLE=<kind>, the test VM cannot be reached when it
  # leases one (exit 5, queued).
  cat >"$SB/tools/scripts/sim-pool.sh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "sim-pool.sh $*" >>"$STUB_LOG"
if [ "$1" = acquire ]; then
  [ "$2" != "${STUB_POOL_BUSY:-}" ] || { echo "sim-pool: every $2 device stayed leased" >&2 && exit 3; }
  [ "$2" != "${STUB_POOL_UNREACHABLE:-}" ] ||
    { echo "sim-pool: queued: meguri-mac did not answer over SSH in a logged-in session within 240 s (last: exec request failed on channel 0)" >&2 && exit 5; }
  echo "POOL-$2"
fi
exit 0
EOF
  chmod +x "$SB/tools/scripts/sim-pool.sh"
  unset STUB_POOL_BUSY STUB_POOL_UNREACHABLE
  # Every call is logged. Builds fail (pass with STUB_BUILD_OK=1), so no
  # product check runs; builds for testing write the plan's simulator
  # xctestrun; test runs pass. A launched app stays up (`open -W` becomes a
  # process whose command line is the app's executable), except on the route
  # stub/quits; lsappinfo reports it as STUB_APP_TYPE (UIElement).
  cat >"$STUBS/_stub" <<'EOF'
#!/usr/bin/env bash
name="$(basename "$0")"
if [ -n "${FAKE_VM_FS:-}" ]; then
  STUB_APP_INFO="${FAKE_VM_APP_INFO:-}" STUB_APP_TYPE="${FAKE_VM_APP_TYPE:-UIElement}" STUB_APP_FRONT="${FAKE_VM_APP_FRONT:-}"
  args="$*"
  printf '%s\n' "vm-$name ${args//"$FAKE_VM_FS"/}" >>"$STUB_LOG"
else
  printf '%s\n' "$name $*" >>"$STUB_LOG"
fi
case "$name $*" in
  "xcodebuild build-for-testing "*)
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -destination) dest="$2" ;; -derivedDataPath) dd="$2" ;; -scheme) scheme="$2" ;; -testPlan) plan="$2" ;; -configuration) config="$2" ;;
      esac
      shift
    done
    mkdir -p "$dd/Build/Products"
    sdk=iphonesimulator root="$config-iphonesimulator"
    case "$dest" in platform=macOS*) sdk=macosx root="$config" ;; esac
    if [ "$sdk" = macosx ] && [ "$plan" = Unit ]; then
      excluded="$dd/Build/Products/$root/Meguri.app/Contents/PlugIns/MeguriSnapshotTests.xctest"
      if [ -e "$excluded" ] || [ "${STUB_MAC_UNIT_BUILD_FAIL:-0}" = 1 ]; then
        echo "    export PRIVATE_PLACEHOLDER=error: synthetic-environment-marker"
        echo "$dd/Build/Products/$root/Meguri.app: signing refused the generated bundle"
        echo "In subcomponent: $excluded"
        echo "Command CodeSign failed with a nonzero exit code"
        exit 65
      fi
    fi
    [ "${STUB_NO_XCTESTRUN:-}" != "$sdk" ] || exit 0
    echo "__TESTROOT__/$root/" >"$dd/Build/Products/${scheme}_${plan}_${sdk}27.0-arm64.xctestrun"
    mkdir -p "$dd/Build/Products/$root/Meguri.app"
    echo "$sdk:$config:$plan" >"$dd/Build/Products/$root/Meguri.app/Info.plist"
    ;;
  "xcrun xcresulttool get build-results "*) echo '{"errorCount":0,"warningCount":0,"warnings":[]}' ;;
  "xcrun xcresulttool get "*)
    if [ "${STUB_BAD_SUMMARY:-}" = 1 ] && [[ "$*" = *summary*mac-unit.xcresult* ]]; then echo '{}'
    elif [ -n "${STUB_UNIT_SUMMARY:-}" ] && [[ "$*" = *summary*mac-unit.xcresult* ]]; then printf '%s\n' "$STUB_UNIT_SUMMARY"
    else echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}'; fi ;;

  "xcodebuild build "*)
    [ "${STUB_BUILD_OK:-}" = 1 ] || exit 65
    while [ "$#" -gt 0 ]; do case "$1" in -derivedDataPath) dd="$2" ;; esac; shift; done
    mkdir -p "$dd/Build/Products/Debug/Meguri.app/Contents/MacOS"
    echo checked-debug >"$dd/Build/Products/Debug/Meguri.app/Contents/MacOS/Meguri"
    ;;
  "open "*"-MeguriRoute stub/quits"*) ;;
  "open "*)
    while [ "$#" -gt 0 ] && [ "$1" != --args ]; do app="$1"; shift; done
    if [ -n "${FAKE_VM:-}" ]; then
      [ "$(cat "$app/Contents/MacOS/Meguri")" = checked-debug ] || exit 1
      printf '%s\n' "$$" "$PPID" >>"$FAKE_VM/tests.pids"
      if [ -n "${FAKE_VM_HIDDEN_DROP:-}" ] && [ ! -f "$FAKE_VM/hidden-dropped" ]; then
        : >"$FAKE_VM/hidden-dropped"
        remote="$(ps -o ppid= -p "$PPID" | tr -d " ")"
        kill -9 "$remote"
        trap "" TERM
      fi
    fi
    [ "${STUB_APP_INFO:-}" != no-pid ] || exec sleep 30
    exec -a "$app/Contents/MacOS/Meguri" sleep 30
    ;;
  "lsappinfo "*)
    [ "${STUB_APP_INFO:-}" != empty ] || exit 0
    [ "${STUB_APP_INFO:-}" != malformed ] || { echo "ApplicationType unavailable"; exit 0; }
    printf '[ NULL ]  ASN:0x0-0x1a01a:%s\n !cgsConnection !signalled type="%s" flavor=[ NULL ]\n' "${STUB_APP_FRONT:+ (in front)}" "${STUB_APP_TYPE:-UIElement}"
    [ "${STUB_APP_INFO:-}" != failed ] || exit 1
    ;;
esac
exit 0
EOF
  chmod +x "$STUBS/_stub"
  local c
  for c in xcrun xcodebuild codesign lipo segedit pluginkit plutil xmllint open lsappinfo; do
    ln -s _stub "$STUBS/$c"
  done
  export PATH="$STUBS:$PATH"
  unset MEGURI_ALLOW_MAC_UI STUB_APP_TYPE
}

teardown() {
  # A VM run left going by a lost SSH session.
  cat "$BATS_TEST_TMPDIR"/vms/*/tests.pids 2>/dev/null | xargs kill -9 2>/dev/null || true
}

# The test VM (--vm-ssh): the fake VM of tests/mac_vm.bats (mac_vm_fake_ssh.sh),
# which runs each command here, in its own copy of the file system
# ($FAKE_VM_DIR/meguri-mac/fs); its wrapper logs each command as
# `vm-ssh <command>`. The VM's xcodebuild test-without-building needs the
# products at the -xctestrun path and, at the repository's paths, what the
# run's tests read there, as this checkout holds it: the snapshot baselines
# (Snapshot plan), the hook fixtures (TestHooksTests) and the example drive
# scripts (DriveScriptTests); otherwise its tests fail (exit 65). It exits
# FAKE_VM_TEST_RC (0) and leaves a result bundle. Each run is a process of
# the VM that its pgrep and pkill see (tests.pids). FAKE_VM_SSH_DROP=1: the
# first run loses its SSH session and goes on there until it is stopped;
# =down: SSH stops answering as it does.
fake_vm() {
  export FAKE_VM_DIR="$BATS_TEST_TMPDIR/vms" FAKE_VM_NAME=meguri-mac FAKE_VM_BIN="$BATS_TEST_TMPDIR/vm-bin"
  FAKE_VM_MIRROR="$(cd "$BATS_TEST_TMPDIR" && pwd -P)"
  FAKE_VM_REPO="$(cd "$SB" && pwd -P)"
  export FAKE_VM_MIRROR FAKE_VM_REPO MEGURI_MAC_VM_STOP_SECONDS=2
  VM_FS="$FAKE_VM_DIR/meguri-mac/fs"
  mkdir -p "$VM_FS" "$FAKE_VM_BIN"
  echo running >"$FAKE_VM_DIR/meguri-mac/state"
  printf '#!/usr/bin/env bash\nprintf "vm-ssh %%s\\n" "$1" >>"$STUB_LOG"\nexec %q "$@"\n' \
    "$REPO_ROOT/tools/tests/mac_vm_fake_ssh.sh" >"$STUBS/vm-ssh"
  cat >"$FAKE_VM_BIN/pgrep" <<'EOF'
#!/usr/bin/env bash
rc=1
for pid in $(cat "$FAKE_VM/tests.pids" 2>/dev/null); do
  cmd="$(ps -o command= -p "$pid" 2>/dev/null)" || continue
  if printf "%s\n" "$cmd" | grep -Eq -- "${!#}"; then echo "$pid"; rc=0; fi
done
exit "$rc"
EOF
  cat >"$FAKE_VM_BIN/pkill" <<'EOF'
#!/usr/bin/env bash
echo "vm-pkill $*" >>"$STUB_LOG"
sig=TERM
[ "$1" != -9 ] || sig=KILL
pids="$(pgrep -f "${!#}")" || exit 1
kill -s "$sig" $pids 2>/dev/null
exit 0
EOF
  cat >"$FAKE_VM_BIN/xcodebuild" <<'EOF'
#!/usr/bin/env bash
unmap() { local s="$*"; printf "%s" "${s//"$FAKE_VM_FS"/}"; }
echo "vm-xcodebuild $(unmap "$@")" >>"$STUB_LOG"
other="$(pgrep -f 'xcodebuild[ ]test-without-building|UITests-Runner[.]app/[^/]*UITests-Runner|/[x]ctest( |$)|/Build/Products/Debug/Meguri[.]app/Contents/MacOS/[M]eguri')" && echo "vm-overlap with $other" >>"$STUB_LOG"
echo "$$" >>"$FAKE_VM/tests.pids"
only=""
while [ "$#" -gt 0 ]; do
  case "$1" in -xctestrun) run="$2" && shift ;; -resultBundlePath) bundle="$2" && shift ;; -only-testing:*) only="${1#-only-testing:}" ;; esac
  shift
done
[ -f "$run" ] || { echo "xcodebuild: error: no xctestrun at $(unmap "$run")"; exit 70; }
root="$(sed -n "s|__TESTROOT__/\(.*\)/|\1|p" "$run")"
product="$(dirname "$run")/$root/Meguri.app/Info.plist"
[ -f "$product" ] || { echo "xcodebuild: error: missing test product $product"; exit 70; }
echo "vm-test-products $root $(cat "$product")" >>"$STUB_LOG"
case "$(basename "$run") $only" in
  *_Unit_macosx*) need="core/Cargo.toml app/Packages/MeguriKit/Sources/DesignSystem/Resources/Colors.xcassets" ;;
  *_Snapshot_*) need=app/Snapshots ;;
  *TestHooksTests) need=app/Fixtures ;;
  *DriveScriptTests) need=tools/drive-scripts ;;
  *) need="" ;;
esac
for input in $need; do
  if ! diff -r "$FAKE_VM_REPO/$input" "$FAKE_VM_FS$FAKE_VM_REPO/$input" >/dev/null 2>&1; then
    echo "Test failed: $input in the VM is not this checkout's"
    exit 65
  fi
done
if [ -n "${FAKE_VM_SSH_DROP:-}" ] && [ ! -e "$FAKE_VM/ssh-dropped" ]; then
  : >"$FAKE_VM/ssh-dropped"
  [ "$FAKE_VM_SSH_DROP" != down ] || : >"$FAKE_VM_DIR/ssh-down"
  # The test process survives xcodebuild's SIGTERM until SIGKILL cleanup.
  case "$run" in
    *_macosx*) runner=/Xcode/usr/bin/xctest pattern='/[x]ctest( |$)' ;;
    *) runner=/Simulator/MeguriUITests-Runner.app/MeguriUITests-Runner pattern='UITests-Runner[.]app/[^/]*UITests-Runner' ;;
  esac
  bash -c 'trap "" TERM; exec -a "$1" sleep 600' _ "$runner" \
    </dev/null >/dev/null 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&- &
  echo "$!" >>"$FAKE_VM/tests.pids"
  until pgrep -f "$pattern" >/dev/null; do sleep 0.01; done
  kill -9 "$PPID"
  exec </dev/null >/dev/null 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
  for _ in $(seq 1 6000); do sleep 0.1; done
  exit 0
fi
mkdir -p "$bundle"
exit "${FAKE_VM_TEST_RC:-0}"
EOF
  cp "$STUBS/_stub" "$FAKE_VM_BIN/open"
  cp "$STUBS/_stub" "$FAKE_VM_BIN/lsappinfo"
  chmod +x "$STUBS/vm-ssh" "$FAKE_VM_BIN"/*
}

vm_tests_running() { FAKE_VM="$FAKE_VM_DIR/meguri-mac" "$FAKE_VM_BIN/pgrep" -f 'xcodebuild[ ]test-without-building' >/dev/null; }

# The sources VM runs read at the repository's paths.
add_test_inputs() {
  mkdir -p "$SB/app/Snapshots/iPhone18,1-24A434" "$SB/app/Fixtures/TestHooks" "$SB/tools/drive-scripts/examples"
  echo baseline-1 >"$SB/app/Snapshots/iPhone18,1-24A434/home.png"
  echo fixture-1 >"$SB/app/Fixtures/TestHooks/single-file.fixture"
  echo '{"name": "x", "steps": []}' >"$SB/tools/drive-scripts/examples/x.json"
  mkdir -p "$SB/core" "$SB/app/Packages/MeguriKit/Sources/DesignSystem/Resources/Colors.xcassets/test.colorset"
  printf '[workspace.package]\nversion = "1.2.3"\n' >"$SB/core/Cargo.toml"
  echo '{"colors": []}' >"$SB/app/Packages/MeguriKit/Sources/DesignSystem/Resources/Colors.xcassets/test.colorset/Contents.json"
}

check_project() {
  local arg explicit=""
  for arg in "$@"; do case "$arg" in --vm-ssh | --no-sims) explicit=1 ;; esac; done
  if [ -z "$explicit" ]; then
    [ -x "$STUBS/vm-ssh" ] || { fake_vm; add_test_inputs; }
    set -- --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac "$@"
  fi
  FAKE_VM_APP_INFO="${STUB_APP_INFO:-}" FAKE_VM_APP_TYPE="${STUB_APP_TYPE:-UIElement}" FAKE_VM_APP_FRONT="${STUB_APP_FRONT:-}" \
    run --separate-stderr /bin/bash "$SB/tools/tests/xcode_project_check.sh" "$BATS_TEST_TMPDIR/work" "$@"
}

called() { grep -qE "$1" "$STUB_LOG" || { echo "no call matches: $1" && cat "$STUB_LOG" && return 1; }; }
not_called() { ! grep -E "$1" "$STUB_LOG"; }
tap_line() { grep -qxE "$1" <<<"$output" || { echo "no TAP line matches: $1" && echo "$output" && return 1; }; }
# no_vm_failure: no lease or test run failed (the stubbed builds do).
no_vm_failure() { ! grep -E '^not ok [0-9]+ (lease|.*-app-hosted|mac-unit|.*TestHooksTests|.*DriveScriptTests)' <<<"$output" || { echo "$output" && return 1; }; }

@test "xcode-check leases its simulators from the pool and releases them at exit" {
  check_project
  called '^sim-pool\.sh acquire iphone --holder xcode-check$'
  called '^sim-pool\.sh acquire ipad --holder xcode-check$'
  called '^xcodebuild build .* -destination generic/platform=iOS Simulator '
  called '^xcodebuild build .* -destination generic/platform=iOS Simulator '
  called "^vm-xcodebuild .* -destination id=POOL-iphone "
  called "^vm-xcodebuild .* -destination id=POOL-ipad "
  [ "$(grep -c '^sim-pool\.sh acquire ' "$STUB_LOG")" -eq 2 ]
  [ "$(grep -c '^sim-pool\.sh release ' "$STUB_LOG")" -eq 2 ]
  # Both are leased after the Mac configuration builds, and each is released
  # once, after the last run that uses it.
  local first_lease mac_appstore kind last released
  first_lease="$(grep -n '^sim-pool\.sh acquire ' "$STUB_LOG" | head -n 1 | cut -d: -f1)"
  mac_appstore="$(grep -n '^xcodebuild build .* -configuration AppStore -destination platform=macOS' "$STUB_LOG" | cut -d: -f1)"
  [ -n "$mac_appstore" ]
  [ "$first_lease" -gt "$mac_appstore" ]
  for kind in iphone ipad; do
    last="$(grep -n "id=POOL-$kind" "$STUB_LOG" | tail -n 1 | cut -d: -f1)"
    released="$(grep -nx "sim-pool.sh release POOL-$kind" "$STUB_LOG" | cut -d: -f1)"
    [ -n "$released" ]
    [ "$released" -gt "$last" ]
  done
  not_called '^xcrun simctl (create|boot|shutdown|erase|delete) '
}

@test "xcode-check reports a pool that stays busy and builds without that simulator" {
  STUB_POOL_BUSY=iphone check_project --no-ui
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ lease an iPhone simulator from the pool'
  tap_line '# sim-pool: every iphone device stayed leased'
  not_called 'id=POOL-iphone'
  called '^xcodebuild build .* -destination generic/platform=iOS Simulator '
  called '^sim-pool\.sh release POOL-ipad$'
  [ "$(grep -c '^sim-pool\.sh release ' "$STUB_LOG")" -eq 1 ]
}

@test "xcode-check never leases, creates or deletes a simulator it is given" {
  check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  not_called '^sim-pool\.sh '
  not_called '^xcrun simctl (create|boot|shutdown|erase|delete) '
  called '^xcodebuild build .* -destination generic/platform=iOS Simulator '
  called '^xcodebuild build .* -destination generic/platform=iOS Simulator '
}

@test "xcode-check queues the Mac UI tests for tools/mg mac-ui" {
  check_project --iphone PHONE-1 --ipad PAD-1
  tap_line 'ok [0-9]+ mac-Debug-TestHooksTests # SKIP queued for tools/mg mac-ui'
  not_called '^xcodebuild test .* -destination platform=macOS,arch=arm64 .*MeguriUITests'
  # The in-process Mac runs and the simulator UI runs still happen.
  called '^vm-xcodebuild .*Meguri_Unit_macosx27.0-arm64.xctestrun -destination platform=macOS,arch=arm64 '
  called '^vm-xcodebuild .*Meguri_Snapshot_macosx27.0-arm64.xctestrun -destination platform=macOS,arch=arm64 '
  called '^vm-xcodebuild .* -destination id=PHONE-1 .* -only-testing:MeguriUITests/TestHooksTests$'
}

@test "with MEGURI_ALLOW_MAC_UI=1 xcode-check runs the Mac UI tests" {
  MEGURI_ALLOW_MAC_UI=1 check_project --iphone PHONE-1 --ipad PAD-1
  tap_line 'ok [0-9]+ mac-Debug-TestHooksTests: Passed .*'
  called '^vm-xcodebuild .*Meguri_UI_macosx27.0-arm64.xctestrun -destination platform=macOS,arch=arm64 .* -only-testing:MeguriUITests/TestHooksTests$'
}

@test "xcode-check reads the test-hook routes from their registrations and refuses a computed path" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  cat >"$SB/app/Meguri/TestHooks/AreaRoutes.swift" <<'SWIFT'
    static func register(in registry: inout RouteRegistry) {
        // registry.register(path) in a comment is not a registration
        registry.register("area/one") { _ in .screen { Text("1") } }
        registry.register("area/two-b/state", handler)
    }
SWIFT
  check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  tap_line 'ok [0-9]+ test-hook routes are registered with literal paths: area/one area/two-b/state'
  echo '        registry.register("area/\(state)") { _ in .screen { Text("") } }' >>"$SB/app/Meguri/TestHooks/AreaRoutes.swift"
  check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  tap_line 'not ok [0-9]+ test-hook routes are registered with literal paths'
  grep -qF 'registry.register("area/\(state)")' <<<"$output"
}

@test "xcode-check launches the Mac Debug app hidden on Home and on every route, and fails when it quits" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  printf '%s\n' 'registry.register("stub/stays") { _ in .screen { Text("") } }' \
    'registry.register("stub/quits") { _ in .alert(title: "", message: "") {} }' >"$SB/app/Meguri/TestHooks/StubRoutes.swift"
  STUB_BUILD_OK=1 check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  local app
  app="$(cd "$BATS_TEST_TMPDIR/work" && pwd -P)/DerivedData/Build/Products/Debug/Meguri.app"
  called "^vm-open -g -j -n -W $app --args -MeguriBackgroundLaunch YES -ApplePersistenceIgnoreState YES\$"
  called "^vm-open -g -j -n -W $app --args -MeguriBackgroundLaunch YES -ApplePersistenceIgnoreState YES -MeguriRoute stub/stays\$"
  called "^vm-open -g -j -n -W $app --args -MeguriBackgroundLaunch YES -ApplePersistenceIgnoreState YES -MeguriRoute stub/quits\$"
  [ "$(grep -c '^vm-open ' "$STUB_LOG")" -eq 3 ]
  not_called '^open '
  not_called '^lsappinfo '
  [ "$(cat "$VM_FS$app/Contents/MacOS/Meguri")" = checked-debug ]
  [ "$(grep -c "^vm-ssh rm -rf '$app' && mkdir -p " "$STUB_LOG")" = 1 ]
  tap_line 'ok [0-9]+ mac-Debug launched hidden stays up: Home'
  tap_line 'ok [0-9]+ mac-Debug launched hidden stays up: route stub/stays'
  tap_line 'not ok [0-9]+ mac-Debug launched hidden stays up: route stub/quits'
  tap_line '# the app quit within 3 s of a hidden launch \(open -W exit 0\)'
  [ "$(grep -c '^vm-lsappinfo info -only ApplicationType [0-9]' "$STUB_LOG")" -eq 2 ]
}

@test "a hidden Mac launch that shows in the Dock fails xcode-check (U8d)" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  STUB_BUILD_OK=1 STUB_APP_TYPE=Foreground check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  tap_line 'not ok [0-9]+ mac-Debug launched hidden stays up: Home'
  tap_line '# the app showed on this Mac \(pid [0-9]+; a Dock tile or the active app\): lsappinfo:.*type="Foreground".*'
}

assert_project_presence_missing() {
  mkdir -p "$SB/app/Meguri/TestHooks"
  STUB_BUILD_OK=1 check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  tap_line 'not ok [0-9]+ mac-Debug launched hidden stays up: Home'
  tap_line '# cannot verify the Mac app stays out of sight.*'
}

@test "failed lsappinfo fails xcode-check even when it prints a safe type" {
  STUB_APP_INFO=failed assert_project_presence_missing
}

@test "empty lsappinfo fails xcode-check" {
  STUB_APP_INFO=empty assert_project_presence_missing
}

@test "unrecognized lsappinfo type fails xcode-check" {
  STUB_APP_TYPE=Unknown assert_project_presence_missing
}

@test "malformed lsappinfo output fails xcode-check" {
  STUB_APP_INFO=malformed assert_project_presence_missing
}

@test "no known Mac app pid fails xcode-check" {
  STUB_APP_INFO=no-pid assert_project_presence_missing
  not_called '^vm-lsappinfo '
}

@test "BackgroundOnly lsappinfo passes xcode-check" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  STUB_BUILD_OK=1 STUB_APP_TYPE=BackgroundOnly check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  tap_line 'ok [0-9]+ mac-Debug launched hidden stays up: Home'
}

@test "a UIElement in front fails xcode-check" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  STUB_BUILD_OK=1 STUB_APP_FRONT=1 check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  tap_line 'not ok [0-9]+ mac-Debug launched hidden stays up: Home'
  tap_line '# the app showed on this Mac .*'
}

@test "the builds phase builds and checks, but launches, leases and tests nothing, and prints no plan" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  printf '%s\n' 'registry.register("stub/stays") { _ in .screen { Text("") } }' >"$SB/app/Meguri/TestHooks/StubRoutes.swift"
  STUB_BUILD_OK=1 check_project --phase builds
  called '^xcodebuild build .* -configuration AppStore -destination platform=macOS'
  called '^xcodebuild build .* -configuration Debug -destination platform=macOS.*mac-Debug-probe\.xcresult'
  tap_line 'ok [0-9]+ test-hook routes are registered with literal paths: stub/stays'
  not_called '^open '
  not_called '^vm-open '
  not_called '^sim-pool\.sh '
  not_called '^xcodebuild (test|build .* -destination id=)'
  ! grep -qE '^1\.\.' <<<"$output"
}

@test "the devices phase launches the Debug app the builds left, then leases, builds and tests, numbering on" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  printf '%s\n' 'registry.register("stub/stays") { _ in .screen { Text("") } }' >"$SB/app/Meguri/TestHooks/StubRoutes.swift"
  STUB_BUILD_OK=1 check_project --phase builds
  local n app
  n="$(grep -cE '^(not )?ok ' <<<"$output")"
  : >"$STUB_LOG"
  check_project --phase devices --continue-from "$n"
  app="$(cd "$BATS_TEST_TMPDIR/work" && pwd -P)/DerivedData/Build/Products/Debug/Meguri.app"
  [ "$(head -n 1 <<<"$output")" = "ok $((n + 1)) mac-Debug launched hidden stays up: Home" ]
  called "^vm-open -g -j -n -W $app --args -MeguriBackgroundLaunch YES -ApplePersistenceIgnoreState YES -MeguriRoute stub/stays\$"
  called '^sim-pool\.sh acquire iphone --holder xcode-check$'
  called "^vm-xcodebuild .* -destination id=POOL-iphone "
  not_called '^xcodebuild (-list|build .* -destination platform=macOS)'
  ! grep -q 'test-hook routes are registered' <<<"$output"
  [ "$(tail -n 1 <<<"$output")" = "1..$((n + $(grep -cE '^(not )?ok ' <<<"$output")))" ]
}

@test "without a Debug build the devices phase launches nothing on the Mac" {
  check_project --phase builds --no-ui
  check_project --phase devices --continue-from 3 --no-ui
  not_called '^open '
  not_called '^vm-open '
  [ "$(head -n 1 <<<"$output" | sed -E 's/^(not )?ok ([0-9]+) .*/\2/')" = 4 ]
}

@test "with the test VM, simulator builds use any simulator and each simulator run is built here and tested in the VM" {
  fake_vm
  add_test_inputs
  check_project --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  called '^sim-pool\.sh acquire iphone --holder xcode-check$'
  called '^xcodebuild build .* -configuration Debug -destination generic/platform=iOS Simulator '
  not_called '^xcodebuild (build|test) .* -destination id='
  called '^xcodebuild build-for-testing .* -scheme Meguri-Profile -testPlan Performance -configuration Profile -destination generic/platform=iOS Simulator '
  called "^vm-ssh \\{ mkdir -p .* xcodebuild test-without-building -xctestrun '[^']*/DerivedData-tests/Build/Products/Meguri_UI_iphonesimulator27.0-arm64.xctestrun' "
  called "^vm-ssh .*-destination 'id=POOL-ipad' .*'-only-testing:MeguriUITests/TestHooksTests'"
  # The products go to the VM before each run, the bundle comes back.
  called "^vm-ssh rm -rf '[^']*/DerivedData-tests/Build/Products' && mkdir -p "
  tap_line 'ok [0-9]+ iphone-Release-TestHooksTests: Passed .*'
  tap_line 'ok [0-9]+ ipad-Debug-TestHooksTests: Passed .*'
  called '^sim-pool\.sh release POOL-iphone$'
  # Mac Unit and app-hosted plans also execute in the VM.
  called '^vm-xcodebuild .*Meguri_Unit_macosx27.0-arm64.xctestrun -destination platform=macOS,arch=arm64 '
  called '^vm-test-products Debug macosx:Debug:Unit$'
  called '^vm-test-products Debug macosx:Debug:Snapshot$'
  [ -d "$BATS_TEST_TMPDIR/work/mac-unit.xcresult" ]
  [ -d "$BATS_TEST_TMPDIR/work/mac-app-hosted.xcresult" ]
  not_called '^xcodebuild test '

  not_called '^vm-overlap'

  # A run that fails in the VM is a failing test.
  FAKE_VM_TEST_RC=65 check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ iphone-Debug-TestHooksTests: Passed .*'
}

@test "xcode-check removes only the excluded Snapshot bundle before its first Mac Unit build" {
  local work="$BATS_TEST_TMPDIR/work" dd="$BATS_TEST_TMPDIR/work/DerivedData-tests"
  local stale="$dd/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriSnapshotTests.xctest"
  mkdir -p "$stale" "$dd/Build/Products/Debug/Unrelated.framework" \
    "$dd/Build/Products/Debug/Meguri.app/Contents/PlugIns/OtherTests.xctest" \
    "$dd/Build/Intermediates.noindex" "$work/earlier.xcresult"
  echo malformed >"$stale/Info.plist"
  echo unrelated-product >"$dd/Build/Products/Debug/Unrelated.framework/keep"
  echo unrelated-bundle >"$dd/Build/Products/Debug/Meguri.app/Contents/PlugIns/OtherTests.xctest/keep"
  echo cached-intermediate >"$dd/Build/Intermediates.noindex/keep"
  echo earlier-result >"$work/earlier.xcresult/keep"
  check_project --phase devices
  tap_line 'ok [0-9]+ mac-unit: Passed .*'
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  [ ! -e "$stale" ]
  [ "$(cat "$dd/Build/Products/Debug/Unrelated.framework/keep")" = unrelated-product ]
  [ "$(cat "$dd/Build/Products/Debug/Meguri.app/Contents/PlugIns/OtherTests.xctest/keep")" = unrelated-bundle ]
  [ "$(cat "$dd/Build/Intermediates.noindex/keep")" = cached-intermediate ]
  [ "$(cat "$work/earlier.xcresult/keep")" = earlier-result ]
  [ "$(grep -c '^xcodebuild build-for-testing .* -testPlan Unit .* -destination platform=macOS,arch=arm64 ' "$STUB_LOG")" -eq 1 ]
  called '^vm-xcodebuild .*Meguri_Unit_macosx27.0-arm64.xctestrun '
  not_called '^xcodebuild test '
}

@test "xcode-check keeps a failed Mac Unit build red and prints only bounded diagnostic lines" {
  STUB_MAC_UNIT_BUILD_FAIL=1 check_project --phase devices
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ mac-unit: .*'
  tap_line '# xcodebuild exit 70; log: .*mac-unit.log'
  tap_line '# .*Meguri.app: signing refused the generated bundle'
  tap_line '# In subcomponent: .*MeguriSnapshotTests.xctest'
  tap_line '# Command CodeSign failed with a nonzero exit code'
  ! grep -q 'synthetic-environment-marker' <<<"$output"
  ! grep -Eq '^ok [0-9]+ mac-unit' <<<"$output"
  [ "$(grep -c '^xcodebuild build-for-testing .* -testPlan Unit .* -destination platform=macOS,arch=arm64 ' "$STUB_LOG")" -eq 1 ]
  not_called '^vm-xcodebuild .*Meguri_Unit_macosx27.0-arm64.xctestrun '
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
}

@test "VM runs read current baselines, fixtures, scripts, workspace manifest and color catalog, not earlier copies" {
  fake_vm
  add_test_inputs
  check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  tap_line 'ok [0-9]+ iphone-app-hosted: Passed .*'
  tap_line 'ok [0-9]+ iphone-Debug-TestHooksTests: Passed .*'
  tap_line 'ok [0-9]+ ipad-Debug-TestHooksTests: Passed .*'
  tap_line 'ok [0-9]+ iphone-Debug-DriveScriptTests: Passed .*'
  # Copied once per run, not before every test.
  [ "$(grep -c "^vm-ssh rm -rf '[^']*/app/Snapshots' && mkdir -p " "$STUB_LOG")" = 1 ]

  # Changed here since: the next run's copy in the VM is the new one.
  echo baseline-2 >"$SB/app/Snapshots/iPhone18,1-24A434/home.png"
  rm "$SB/app/Fixtures/TestHooks/single-file.fixture"
  echo fixture-2 >"$SB/app/Fixtures/TestHooks/directory.fixture"
  printf '[workspace.package]\nversion = "2.3.4"\n' >"$SB/core/Cargo.toml"
  echo '{"colors": [{"idiom":"universal"}]}' >"$SB/app/Packages/MeguriKit/Sources/DesignSystem/Resources/Colors.xcassets/test.colorset/Contents.json"
  check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  tap_line 'ok [0-9]+ iphone-app-hosted: Passed .*'
  tap_line 'ok [0-9]+ iphone-Debug-TestHooksTests: Passed .*'
  [ "$(cat "$VM_FS$FAKE_VM_REPO/app/Snapshots/iPhone18,1-24A434/home.png")" = baseline-2 ]
  [ ! -e "$VM_FS$FAKE_VM_REPO/app/Fixtures/TestHooks/single-file.fixture" ]
  tap_line 'ok [0-9]+ mac-unit: Passed .*'
  cmp "$SB/core/Cargo.toml" "$VM_FS$FAKE_VM_REPO/core/Cargo.toml"
  cmp "$SB/app/Packages/MeguriKit/Sources/DesignSystem/Resources/Colors.xcassets/test.colorset/Contents.json" \
    "$VM_FS$FAKE_VM_REPO/app/Packages/MeguriKit/Sources/DesignSystem/Resources/Colors.xcassets/test.colorset/Contents.json"
}

@test "a VM run that loses its SSH session is stopped there before the next run starts; one that cannot be stopped ends the VM runs" {
  fake_vm
  add_test_inputs
  FAKE_VM_SSH_DROP=1 check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ mac-unit: .*'
  tap_line '# xcode_project_check: the SSH session to meguri-mac ended before the remote command did'
  tap_line '# xcode_project_check: stopped that run in meguri-mac'
  tap_line 'ok [0-9]+ iphone-Debug-TestHooksTests: Passed .*'
  local stop second
  stop="$(grep -n '^vm-pkill -f ' "$STUB_LOG" | head -n 1 | cut -d: -f1)"
  second="$(grep -n '^vm-xcodebuild ' "$STUB_LOG" | sed -n 2p | cut -d: -f1)"
  [ -n "$stop" ] && [ -n "$second" ] && [ "$stop" -lt "$second" ] || { cat "$STUB_LOG"; return 1; }
  not_called '^vm-overlap'
  ! vm_tests_running
  ! FAKE_VM="$FAKE_VM_DIR/meguri-mac" "$FAKE_VM_BIN/pgrep" -f '/[x]ctest( |$)'
  ! FAKE_VM="$FAKE_VM_DIR/meguri-mac" "$FAKE_VM_BIN/pgrep" -f 'UITests-Runner[.]app/[^/]*UITests-Runner'
  called '^vm-pkill -9 -f '

  # SSH stops answering as well: the run cannot be stopped, and no other run
  # starts in the VM. The VM cannot be reached: those runs are queued (U19c).
  rm -f "$FAKE_VM_DIR/meguri-mac/ssh-dropped"
  : >"$STUB_LOG"
  FAKE_VM_SSH_DROP=down check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  no_vm_failure
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" = 1 ]
  tap_line 'ok [0-9]+ mac-unit # SKIP queued: the SSH session to meguri-mac ended before the remote command did, and meguri-mac did not answer after it \(the VM did not answer .*\) \(user step mac-vm\)'
  tap_line 'ok [0-9]+ iphone-Debug-TestHooksTests # SKIP queued: not run: the run mac-unit lost its SSH session and may still be running in meguri-mac \(the VM did not answer .*'
  tap_line 'ok [0-9]+ ipad-Debug-TestHooksTests # SKIP queued: not run: .*'
  [ "$(cut -f1 "$BATS_TEST_TMPDIR/work/vm-queued.tsv" | head -n 3 | tr '\n' ' ')" = "mac-unit mac-app-hosted iphone-app-hosted " ]
  not_called '^xcodebuild build-for-testing .* -scheme Meguri-Profile '
  vm_tests_running
}

# bats test_tags=gate
@test "a VM that cannot be reached during the devices phase queues the leases and runs it keeps from happening, never a failure (U19c)" {
  fake_vm
  add_test_inputs
  # The iPad's lease: the pool's VM did not answer.
  STUB_POOL_UNREACHABLE=ipad check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  no_vm_failure
  tap_line 'ok [0-9]+ lease an iPad simulator from the pool # SKIP queued: meguri-mac did not answer over SSH in a logged-in session within 240 s \(last: exec request failed on channel 0\) \(user step mac-vm\)'
  tap_line 'ok [0-9]+ iphone-Debug-TestHooksTests: Passed .*'
  [ "$(cat "$BATS_TEST_TMPDIR/work/vm-queued.tsv")" = "lease an iPad simulator from the pool$(printf '\t')meguri-mac did not answer over SSH in a logged-in session within 240 s (last: exec request failed on channel 0)" ]

  # The VM refuses one run's session: that run is queued, the others run.
  FAKE_VM_SSH_REFUSE='xcodebuild test-without-building .*Meguri-Profile_Performance' check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  no_vm_failure
  tap_line 'ok [0-9]+ iphone-Profile-TestHooksTests # SKIP queued: meguri-mac refused the run.s SSH session \(ssh exit 255: exec request failed on channel 0\) \(user step mac-vm\)'
  tap_line 'ok [0-9]+ iphone-Debug-DriveScriptTests: Passed .*'
  [ "$(cut -f1 "$BATS_TEST_TMPDIR/work/vm-queued.tsv")" = iphone-Profile-TestHooksTests ]

  # Its shell cannot fork the copy of the products: queued too.
  : >"$STUB_LOG"
  FAKE_VM_FORK_FAIL="DerivedData-tests/Build/Products' && mkdir -p" check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
  no_vm_failure
  tap_line 'ok [0-9]+ iphone-app-hosted # SKIP queued: copying .*/DerivedData-tests/Build/Products into the VM meguri-mac failed \(tar and ssh exit codes: [0-9]+ 1: zsh:2: fork failed: resource temporarily unavailable\); see log.txt \(user step mac-vm\)'
  not_called '^vm-xcodebuild '
}

@test "without the test VM, xcode-check builds for the simulator but leases and tests none" {
  check_project --no-sims
  not_called '^sim-pool\.sh '
  called '^xcodebuild build .* -destination generic/platform=iOS Simulator '
  not_called 'id=POOL'
  not_called '^xcodebuild build-for-testing'
  not_called '^open '
  not_called '^xcodebuild test '
  tap_line 'ok [0-9]+ mac-unit # SKIP queued: no test VM \(user step mac-vm\)'
  tap_line 'ok [0-9]+ mac-app-hosted # SKIP queued: no test VM \(user step mac-vm\)'
  tap_line 'ok [0-9]+ lease the iPhone and iPad simulators # SKIP queued: no test VM \(user step mac-vm\)'
  tap_line 'ok [0-9]+ simulator test runs # SKIP queued: no test VM \(user step mac-vm\)'
}

@test "without the test VM, supplied iPhone and iPad IDs do not start simulator test runs" {
  check_project --phase devices --no-sims --iphone GIVEN-IPHONE --ipad GIVEN-IPAD
  not_called '^sim-pool\.sh '
  not_called '^xcodebuild test .* -destination id='
  not_called '^xcodebuild build-for-testing'
  tap_line 'ok [0-9]+ simulator test runs # SKIP queued: no test VM \(user step mac-vm\)'
}

@test "Xcode Mac plans reject a missing macOS xctestrun rather than using an iOS run or the host" {
  STUB_NO_XCTESTRUN=macosx check_project --phase devices
  tap_line 'not ok [0-9]+ mac-unit: .*'
  tap_line '# xcode_project_check: no Meguri Unit xctestrun for Debug .*'
  tap_line 'not ok [0-9]+ mac-app-hosted: .*'
  tap_line 'ok [0-9]+ iphone-app-hosted: Passed .*'
  not_called '^xcodebuild test '
  not_called '^vm-xcodebuild .*macosx'
}

@test "Xcode Mac test product copy failure fails its checks; refused Mac sessions queue without host execution" {
  fake_vm
  add_test_inputs
  # Malformed output is an actual copy fault, not a reason to queue.
  FAKE_VM_SSH_EMPTY_TAR='tar -C .* -cf - .*mac-unit.xcresult' check_project --phase devices --vm-ssh "$STUBS/vm-ssh"
  tap_line 'not ok [0-9]+ mac-unit: .*'
  not_called '^xcodebuild test '
  : >"$STUB_LOG"
  FAKE_VM_SSH_REFUSE='xcodebuild test-without-building .*Meguri_Unit_macosx' check_project --phase devices --vm-ssh "$STUBS/vm-ssh"
  tap_line 'ok [0-9]+ mac-unit # SKIP queued: .*refused.*'
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  not_called '^xcodebuild test '
}

@test "a refused hidden Mac launch queues while real hidden product copy faults fail" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  STUB_BUILD_OK=1 FAKE_VM_SSH_REFUSE='^bash .*mac-hidden-launch.sh' check_project --no-ui
  tap_line 'ok [0-9]+ mac-Debug launched hidden stays up: Home # SKIP queued: .*refused.*'
  not_called '^open '
  not_called '^vm-open '
  : >"$STUB_LOG"
  # A failing tar is reachable and remains a check failure.
  cat >"$STUBS/tar" <<'EOF'
#!/bin/bash
case "$*" in *Debug*'-cf - Meguri.app'*) exit 1 ;; esac
exec /usr/bin/tar "$@"
EOF
  chmod +x "$STUBS/tar"
  STUB_BUILD_OK=1 check_project --no-ui
  tap_line 'not ok [0-9]+ mac-Debug launched hidden stays up: Home'
  tap_line '# copying .*Meguri.app into the VM .* failed .*'
  not_called '^open '
  not_called '^vm-open '
}

@test "a hidden launch that loses SSH is stopped before the next route or Mac plan starts" {
  mkdir -p "$SB/app/Meguri/TestHooks"
  echo 'registry.register("stub/stays", handler)' >"$SB/app/Meguri/TestHooks/StubRoutes.swift"
  STUB_BUILD_OK=1 FAKE_VM_HIDDEN_DROP=1 check_project
  tap_line 'not ok [0-9]+ mac-Debug launched hidden stays up: Home'
  tap_line '# xcode_project_check: the SSH session to meguri-mac ended before the remote command did'
  tap_line 'ok [0-9]+ mac-Debug launched hidden stays up: route stub/stays'
  tap_line 'ok [0-9]+ mac-unit: Passed .*'
  local stop next
  stop="$(grep -n '^vm-pkill ' "$STUB_LOG" | head -n 1 | cut -d: -f1)"
  next="$(grep -n '^vm-open ' "$STUB_LOG" | sed -n 2p | cut -d: -f1)"
  [ -n "$stop" ] && [ "$stop" -lt "$next" ]
  not_called '^open '
  ! FAKE_VM="$FAKE_VM_DIR/meguri-mac" "$FAKE_VM_BIN/pgrep" -f '/Build/Products/Debug/Meguri[.]app/Contents/MacOS/[M]eguri'
}

@test "a missing Mac Unit source input fails instead of reusing the previous VM copy" {
  check_project --phase devices
  tap_line 'ok [0-9]+ mac-unit: Passed .*'
  rm "$SB/core/Cargo.toml"
  : >"$STUB_LOG"
  check_project --phase devices
  tap_line 'not ok [0-9]+ mac-unit: .*'
  tap_line '# xcode_project_check: missing runtime input .*/core/Cargo.toml'
  not_called '^vm-xcodebuild '
  not_called '^xcodebuild test '
}

@test "a malformed returned Mac Unit result cannot become a passing TAP outcome" {
  STUB_BAD_SUMMARY=1 check_project --phase devices
  tap_line 'not ok [0-9]+ mac-unit: no result'
  tap_line '# the VM run returned no readable test summary; log: .*'
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  not_called '^xcodebuild test '
}

@test "result precedence: a Failed Mac Unit summary with exit zero fails TAP" {
  FAKE_VM_TEST_RC=0 STUB_UNIT_SUMMARY='{"result":"Failed","passedTests":0,"failedTests":1,"skippedTests":0}' check_project --phase devices
  tap_line 'not ok [0-9]+ mac-unit: Failed passed=0 failed=1 skipped=0'
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  ! grep -Eq '^ok [0-9]+ mac-unit' <<<"$output"
  not_called '^xcodebuild test( |-)'
}

@test "result precedence: a Passed Mac Unit summary with positive failedTests fails TAP" {
  FAKE_VM_TEST_RC=0 STUB_UNIT_SUMMARY='{"result":"Passed","passedTests":1,"failedTests":1,"skippedTests":0}' check_project --phase devices
  tap_line 'not ok [0-9]+ mac-unit: Passed passed=1 failed=1 skipped=0'
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  ! grep -Eq '^ok [0-9]+ mac-unit' <<<"$output"
  not_called '^xcodebuild test( |-)'
}

@test "result precedence: a Passed Mac Unit summary with zero failedTests and exit zero passes TAP" {
  FAKE_VM_TEST_RC=0 STUB_UNIT_SUMMARY='{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' check_project --phase devices
  tap_line 'ok [0-9]+ mac-unit: Passed passed=1 failed=0 skipped=0'
  ! grep -Eq '^not ok [0-9]+ mac-unit' <<<"$output"
  not_called '^xcodebuild test( |-)'
}
