#!/usr/bin/env bats
# Control flow of tools/tests/xcode_project_check.sh (tools/mg xcode-check),
# run by `tools/mg selftest` with the host tools and the simulator pool
# stubbed: which simulators it leases and releases, and which test runs it
# starts. The checks themselves run on the host, through `tools/mg xcode-check`.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "Keychain Sharing uses only the team app's private app-ID group" {
  python3 - "$REPO_ROOT" <<'PY'
from pathlib import Path
import plistlib
import re
import sys

app = Path(sys.argv[1]) / "app"
team_files = {"Meguri-macOS.entitlements", "Meguri-macOS-Dev.entitlements"}
for directory in ("Meguri", "MeguriWidget"):
    for path in (app / directory).glob("*.entitlements"):
        with path.open("rb") as source:
            entitlements = plistlib.load(source)
        if directory == "Meguri" and path.name in team_files:
            assert entitlements.get("keychain-access-groups") == [
                "$(AppIdentifierPrefix)$(PRODUCT_BUNDLE_IDENTIFIER)"
            ], path
        else:
            assert "keychain-access-groups" not in entitlements, path
        assert "com.apple.application-identifier" not in entitlements, path
        assert "com.apple.developer.team-identifier" not in entitlements, path
for name in team_files:
    assert (app / "Meguri" / name).is_file(), name

project = (app / "Meguri.xcodeproj/project.pbxproj").read_text()
attributes = re.search(r"\t{4}TargetAttributes = \{\n(.*?)\n\t{4}\};", project, re.S)[1]
targets = dict(re.findall(r"\t{5}([A-F0-9]+) = \{\n(.*?)\n\t{5}\};", attributes, re.S))
app_target = "4D4547555249000000000100"
assert re.search(
    r"SystemCapabilities = \{\s*com\.apple\.Keychain = \{\s*enabled = 1;\s*\};\s*\};",
    targets[app_target],
)
assert project.count("com.apple.Keychain") == 1
for target, settings in targets.items():
    if target != app_target:
        assert "com.apple.Keychain" not in settings, target
PY
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$SB/tools/tests" "$SB/tools/scripts" "$SB/app/Meguri/App" "$SB/app/TestPlans"
  cp "$REPO_ROOT/tools/tests/xcode_project_check.sh" "$SB/tools/tests/"
  cp "$REPO_ROOT/tools/tests/cloud_build_configuration.py" "$SB/tools/tests/"
  cp "$REPO_ROOT/tools/scripts/mac-vm-lib.sh" "$SB/tools/scripts/"
  cp "$REPO_ROOT/tools/scripts/performance-test-policy.py" "$SB/tools/scripts/"
  cp "$REPO_ROOT/app/TestPlans/Performance.xctestplan" "$SB/app/TestPlans/"
  # Keep regression-only application usable for behavioral RED on the old
  # runner, which has not extracted this helper yet.
  if [ -f "$REPO_ROOT/tools/scripts/mac-hidden-launch.sh" ]; then
    cp "$REPO_ROOT/tools/scripts/mac-hidden-launch.sh" "$SB/tools/scripts/"
  fi
  # Wiring double only: producer and guest verification behavior is exercised
  # by xcode_unit_inputs.bats with the actual Python helper.
  export MG_RUN_DIR="$SB/build/mg/xcode-check/wiring-run"
  cat >"$SB/tools/scripts/xcode-unit-inputs.py" <<'PYINPUT'
import os, shlex, sys
assert sys.argv[2] == os.environ['MG_RUN_DIR']
action = sys.argv[1]
with open(os.environ['STUB_LOG'], 'a') as f: f.write('unit-input-' + action + '\n')
if os.environ.get('STUB_UNIT_INPUT_FAIL') == action: raise SystemExit(70)
if os.environ.get('STUB_UNIT_INPUT_UNAVAILABLE') == action: raise SystemExit(72)
if action == 'command':
    print(shlex.join(['env',
        'TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE=' + sys.argv[2] + '/vm-unit-inputs/initial-adoption-fixture.json',
        'TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI=' + sys.argv[2] + '/vm-unit-inputs/meguri-cli', 'xcodebuild']))
else:
    assert action in ('start', 'verify-inputs', 'deliver')
    assert len(sys.argv) == (3 if action == 'start' else 4)
PYINPUT
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
    if { [ "$config" = Profile ] || [ "$plan" = Performance ]; } && [ "${STUB_PERFORMANCE_NOOP:-0}" = 1 ]; then
      echo "xcodebuild: Profile build completed without product changes"
      exit 0
    fi
    mkdir -p "$dd/Build/Products"
    if [ "$config" != Profile ] && [ "$plan" != Performance ] && [ "${STUB_PRUNE_PROFILE_PRODUCTS:-0}" = 1 ]; then
      echo "prune-profile-products $dd $config $plan" >>"$STUB_LOG"
      rm -rf "$dd/Build/Products/Profile-iphonesimulator/Stale.bundle" \
        "$dd/Build/Products/Profile-iphonesimulator/Stale.o" \
        "$dd/Build/Products/Profile-iphonesimulator/Stale.swiftmodule"
    fi
    sdk=iphonesimulator root="$config-iphonesimulator"
    case "$dest" in platform=macOS*) sdk=macosx root="$config" ;; esac
    if [ "$sdk" = macosx ] && [ "$config" = Debug ] && [ "${STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY:-0}" = 1 ]; then
      widget="$dd/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex"
      [ ! -e "$widget" ] && [ ! -L "$widget" ] || { echo "xcode_project_check: stale embedded widget reached build"; exit 65; }
      echo "mac-debug-widget-absent-before-build $plan" >>"$STUB_LOG"
    fi
    if [ "$sdk" = macosx ] && [ "$plan" = Snapshot ]; then
      [ "${STUB_MAC_SNAPSHOT_BUILD_FAIL:-0}" != 1 ] || exit 65
    fi
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
    run="$dd/Build/Products/${scheme}_${plan}_${sdk}27.0-arm64.xctestrun"
    mkdir -p "$dd/Build/Products/$root/Meguri.app"
    echo "$sdk:$config:$plan" >"$dd/Build/Products/$root/Meguri.app/Info.plist"
    if [ "$config" = Profile ] || [ "$plan" = Performance ]; then
      mkdir -p "$dd/Build/Products/$root/Stale.bundle"
      echo bundle >"$dd/Build/Products/$root/Stale.bundle/keep"
      echo object >"$dd/Build/Products/$root/Stale.o"
      echo module >"$dd/Build/Products/$root/Stale.swiftmodule"
      # Preserve an older passing run while leaving new partial products.
      # Neither a failed build nor a successful build missing its new run
      # may let that older run certify these products.
      if [ "${STUB_PERFORMANCE_BUILD_FAIL:-0}" = 1 ]; then
        echo partial-build >"$dd/Build/Products/$root/partial-product"
        exit 65
      fi
      [ "${STUB_PERFORMANCE_MISSING_RUN:-0}" != 1 ] || exit 0
      python3 - "$run" "$root" <<'PY'
import os
import plistlib
import sys

path, root = sys.argv[1:]
target = {"IsUITestBundle": True, "UITargetAppPath": f"__TESTROOT__/{root}/Meguri.app"}
flags = ("UITargetAppMainThreadCheckerEnabled", "UITargetAppPerformanceAntipatternCheckerEnabled")
mode = os.environ.get("STUB_PERFORMANCE_FLAGS", "false")
if mode != "omitted":
    target.update({flag: flag == mode for flag in flags})
injection = os.environ.get("STUB_PERFORMANCE_INJECTION")
if injection:
    target["TestingEnvironmentVariables"] = {"DYLD_INSERT_LIBRARIES": f"/Xcode/{injection}.dylib"}
with open(path, "wb") as output:
    plistlib.dump({"TestConfigurations": [{"TestTargets": [target]}],
                  "__xctestrun_metadata__": {"FormatVersion": 2}}, output)
if os.environ.get("STUB_PERFORMANCE_MALFORMED") == "1":
    # Retain the selection marker, but make the generated plist unreadable.
    with open(path, "ab") as output:
        output.write(b"<broken")
PY
    else
      echo "__TESTROOT__/$root/" >"$run"
    fi
    if [ "$sdk" = macosx ] && [ "$config" = Debug ] && [ "${STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY:-0}" = 1 ]; then
      mkdir -p "$widget"
      echo "$plan" >"$widget/recreated"
      echo "mac-debug-widget-recreated $plan" >>"$STUB_LOG"
    fi
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
  unset MEGURI_ALLOW_MAC_UI STUB_APP_TYPE MEGURI_APP_SIGNING XCODE_CHECK_WORK
  unset STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY STUB_MAC_SNAPSHOT_BUILD_FAIL
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
case "$*" in
  *Meguri_Unit_macosx*)
    [ -n "${TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE:-}" ] && [ -n "${TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI:-}" ] || exit 97
    echo "unit-env $(unmap "$TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE") $(unmap "$TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI")" >>"$STUB_LOG" ;;
  *) [ -z "${TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE:-}" ] && [ -z "${TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI:-}" ] || exit 97 ;;
esac
other="$(pgrep -f 'xcodebuild[ ]test-without-building|UITests-Runner[.]app/[^/]*UITests-Runner|/[x]ctest( |$)|/Build/Products/Debug/Meguri[.]app/Contents/MacOS/[M]eguri')" && echo "vm-overlap with $other" >>"$STUB_LOG"
echo "$$" >>"$FAKE_VM/tests.pids"
only=""
while [ "$#" -gt 0 ]; do
  case "$1" in -xctestrun) run="$2" && shift ;; -resultBundlePath) bundle="$2" && shift ;; -only-testing:*) only="${1#-only-testing:}" ;; esac
  shift
done
[ -f "$run" ] || { echo "xcodebuild: error: no xctestrun at $(unmap "$run")"; exit 70; }
root="$(sed -n 's|.*__TESTROOT__/\([^/]*\)/.*|\1|p' "$run" | head -n 1)"
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
    run --separate-stderr /bin/bash "$SB/tools/tests/xcode_project_check.sh" "${XCODE_CHECK_WORK:-$BATS_TEST_TMPDIR/work}" "$@"
}

called() { grep -qE "$1" "$STUB_LOG" || { echo "no call matches: $1" && cat "$STUB_LOG" && return 1; }; }
not_called() { ! grep -E "$1" "$STUB_LOG"; }
tap_line() { grep -qxE "$1" <<<"$output" || { echo "no TAP line matches: $1" && echo "$output" && return 1; }; }
# no_vm_failure: no lease or test run failed (the stubbed builds do).
no_vm_failure() { ! grep -E '^not ok [0-9]+ (lease|.*-app-hosted|mac-unit|.*TestHooksTests|.*DriveScriptTests)' <<<"$output" || { echo "$output" && return 1; }; }

# Earlier Debug copies are allowed; after the Profile build starts, neither
# that build's products nor any retained products may cross into the VM on
# rejection. Check the transport boundary as well as actual test execution.
assert_profile_phase_stopped() {
  local products ordinary_products profile_line
  products="$(cd "$BATS_TEST_TMPDIR/work" && pwd -P)/DerivedData-tests-profile/Build/Products"
  ordinary_products="$(cd "$BATS_TEST_TMPDIR/work" && pwd -P)/DerivedData-tests/Build/Products"
  [ "$status" -ne 0 ]
  tap_line 'ok [0-9]+ iphone-Debug-TestHooksTests: Passed .*'
  tap_line 'not ok [0-9]+ iphone-Profile-TestHooksTests: .*'
  tap_line 'not ok [0-9]+ iphone-Debug-DriveScriptTests: .*'
  tap_line 'not ok [0-9]+ ipad-Debug-TestHooksTests: .*'
  tap_line '# xcode_project_check: not run: unchecked Profile/Performance products from iphone-Profile-TestHooksTests; see .*'
  called '^vm-xcodebuild .*Meguri_UI_.* -destination id=POOL-iphone .*TestHooksTests$'
  called "^vm-ssh rm -rf '$ordinary_products' && mkdir -p "
  [ -f "$products/Profile-iphonesimulator/Meguri.app/Info.plist" ]
  [ ! -e "$VM_FS$products/Profile-iphonesimulator" ]
  [ ! -e "$VM_FS$products" ]
  [ -z "$(find "$VM_FS$ordinary_products" -name '*Performance*' -print)" ]
  profile_line="$(grep -n '^xcodebuild build-for-testing .* -configuration Profile ' "$STUB_LOG" | cut -d: -f1)"
  [ -n "$profile_line" ]
  # No later Products transport from either root,
  # test execution or test build, regardless of its plan or destination.
  ! tail -n "+$((profile_line + 1))" "$STUB_LOG" |
    grep -E '^vm-xcodebuild |^xcodebuild build-for-testing |^vm-ssh .*DerivedData-tests(-profile)?/Build/Products'
  not_called '^vm-test-products Profile-'
  not_called '^vm-xcodebuild .*DriveScriptTests'
  not_called '^vm-xcodebuild .* -destination id=POOL-ipad '
  ! grep -E '^ok [0-9]+ (iphone-Profile-TestHooksTests|iphone-Debug-DriveScriptTests|ipad-Debug-TestHooksTests)' <<<"$output"
}

assert_profile_policy_rejected() {
  local report="$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy"
  assert_profile_phase_stopped
  [ "$(cat "$report.status")" = 1 ]
  jq -e '.status == "fail" and .source_policy_disabled == true' "$report.json"
  tap_line '# xcode_project_check: Performance test policy exit 1; report: .*'
}

assert_profile_policy_passes() {
  local report="$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy"
  check_project --phase devices
  no_vm_failure
  [ "$(cat "$report.status")" = 0 ]
  jq -e '.status == "pass" and .source_policy_disabled == true and .ui_targets == 1' "$report.json"
  tap_line 'ok [0-9]+ iphone-Profile-TestHooksTests: Passed .*'
  tap_line 'ok [0-9]+ iphone-Debug-DriveScriptTests: Passed .*'
  tap_line 'ok [0-9]+ ipad-Debug-TestHooksTests: Passed .*'
  called '^vm-test-products Profile-iphonesimulator iphonesimulator:Profile:Performance$'
  called '^vm-xcodebuild .*Meguri-Profile_Performance_.* -destination id=POOL-iphone '
  called '^vm-xcodebuild .* -destination id=POOL-iphone .*DriveScriptTests$'
  called '^vm-xcodebuild .* -destination id=POOL-ipad '
  local profile drive ipad
  profile="$(grep -n '^vm-test-products Profile-' "$STUB_LOG" | cut -d: -f1)"
  drive="$(grep -n '^vm-xcodebuild .*DriveScriptTests$' "$STUB_LOG" | cut -d: -f1)"
  ipad="$(grep -n '^vm-xcodebuild .* -destination id=POOL-ipad ' "$STUB_LOG" | cut -d: -f1)"
  [ "$profile" -lt "$drive" ] && [ "$drive" -lt "$ipad" ]
}

@test "generated false checker flags allow Profile execution and later Debug and iPad runs" {
  assert_profile_policy_passes
  jq -e 'all(.flags[]; .false == 1 and .true == 0 and .missing == 0)' \
    "$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy.json"
}

@test "omitted generated checker flags allow Profile execution and later Debug and iPad runs" {
  STUB_PERFORMANCE_FLAGS=omitted assert_profile_policy_passes
  jq -e 'all(.flags[]; .missing == 1 and .true == 0 and .false == 0)' \
    "$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy.json"
}

@test "enabled generated main thread checker blocks all remaining Products transports and runs" {
  STUB_PERFORMANCE_FLAGS=UITargetAppMainThreadCheckerEnabled check_project --phase devices
  assert_profile_policy_rejected
  jq -e '.reason == "policy" and .flags.UITargetAppMainThreadCheckerEnabled.true == 1' \
    "$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy.json"
}

@test "enabled generated performance checker blocks all remaining Products transports and runs" {
  STUB_PERFORMANCE_FLAGS=UITargetAppPerformanceAntipatternCheckerEnabled check_project --phase devices
  assert_profile_policy_rejected
  jq -e '.reason == "policy" and .flags.UITargetAppPerformanceAntipatternCheckerEnabled.true == 1' \
    "$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy.json"
}

@test "generated MainThreadChecker injection blocks all remaining Products transports and runs" {
  STUB_PERFORMANCE_INJECTION=libMainThreadChecker check_project --phase devices
  assert_profile_policy_rejected
  jq -e '.reason == "policy" and .main_thread_checker_injections == 1' \
    "$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy.json"
}

@test "generated libRPAC injection blocks all remaining Products transports and runs" {
  STUB_PERFORMANCE_INJECTION=libRPAC check_project --phase devices
  assert_profile_policy_rejected
  jq -e '.reason == "policy" and .performance_antipattern_checker_injections == 1' \
    "$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy.json"
}

@test "malformed generated Profile plist blocks all remaining Products transports and runs" {
  STUB_PERFORMANCE_MALFORMED=1 check_project --phase devices
  assert_profile_policy_rejected
  jq -e '.reason == "malformed"' "$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests-performance-policy.json"
}

@test "missing selected Profile xctestrun blocks all remaining Products transports and runs" {
  STUB_PERFORMANCE_MISSING_RUN=1 check_project --phase devices
  assert_profile_phase_stopped
  tap_line '# xcode_project_check: no Meguri-Profile Performance xctestrun for Profile in .*'
}

@test "failed Profile build cannot leak unchecked partial products through later Debug or iPad runs" {
  STUB_PERFORMANCE_BUILD_FAIL=1 check_project --phase devices
  assert_profile_phase_stopped
  tap_line '# xcode_project_check: the build for testing failed'
}

# Cross-invocation assertions include the earliest Mac Unit build/transport,
# not just the later Profile run. The same fake VM and local cache survive.
assert_retained_cache_stopped() {
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ mac-unit: .*'
  tap_line 'not ok [0-9]+ mac-app-hosted: .*'
  tap_line 'not ok [0-9]+ iphone-Profile-TestHooksTests: .*'
  tap_line '# xcode_project_check: Profile/Performance cache blocked: .*'
  not_called '^xcodebuild build-for-testing '
  not_called '^vm-ssh .*DerivedData-tests(-profile)?/Build/Products'
  not_called '^vm-xcodebuild '
  [ -f "${1:-$BATS_TEST_TMPDIR/work/DerivedData-tests-profile}/.profile-performance-pending.json" ]
}

@test "rejected Profile cache blocks the next invocation before its first Mac or Debug test build and copy" {
  STUB_PERFORMANCE_FLAGS=UITargetAppMainThreadCheckerEnabled check_project --phase devices
  assert_profile_policy_rejected
  local products="$BATS_TEST_TMPDIR/work/DerivedData-tests-profile/Build/Products"
  cp "$products/Profile-iphonesimulator/Meguri.app/Info.plist" "$BATS_TEST_TMPDIR/rejected-product"
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  tap_line '# xcode_project_check: Profile/Performance cache blocked: persistent unchecked .*'
  cmp "$products/Profile-iphonesimulator/Meguri.app/Info.plist" "$BATS_TEST_TMPDIR/rejected-product"
  # The earlier Debug cache is not overwritten either.
  [ -f "$BATS_TEST_TMPDIR/work/DerivedData-tests/Build/Products/Meguri_Unit_macosx27.0-arm64.xctestrun" ]
}

@test "Profile build failure preserves a blocking receipt despite earlier valid test metadata" {
  assert_profile_policy_passes
  local dd="$BATS_TEST_TMPDIR/work/DerivedData-tests-profile" run
  run="$dd/Build/Products/Meguri-Profile_Performance_iphonesimulator27.0-arm64.xctestrun"
  cp "$run" "$BATS_TEST_TMPDIR/old-passing-run"
  cp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/old-proof"
  : >"$STUB_LOG"
  STUB_PERFORMANCE_BUILD_FAIL=1 check_project --phase devices
  [ "$status" -ne 0 ]
  tap_line '# xcode_project_check: the build for testing failed'
  called '^xcodebuild build-for-testing .* -configuration Profile '
  cmp "$run" "$BATS_TEST_TMPDIR/old-passing-run"
  cmp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/old-proof"
  [ "$(cat "$dd/Build/Products/Profile-iphonesimulator/partial-product")" = partial-build ]
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  cmp "$run" "$BATS_TEST_TMPDIR/old-passing-run"
  [ -f "$dd/Build/Products/Profile-iphonesimulator/partial-product" ]
}

@test "successful no-op Profile build revalidates untouched products and continues through Drive and iPad" {
  assert_profile_policy_passes
  local dd="$BATS_TEST_TMPDIR/work/DerivedData-tests-profile" log line
  cp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/before-noop-proof"
  : >"$STUB_LOG"
  STUB_PERFORMANCE_NOOP=1 assert_profile_policy_passes
  cmp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/before-noop-proof"
  [ ! -e "$dd/.profile-performance-pending.json" ]
  called '^xcodebuild build-for-testing .* -configuration Profile '
  log="$BATS_TEST_TMPDIR/work/logs/iphone-Profile-TestHooksTests.log"
  line="$(grep -n '^xcodebuild: Profile build completed without product changes$' "$log" | cut -d: -f1)"
  [ -n "$line" ]
  tail -n "+$((line + 1))" "$log" | grep -q '^xcode_project_check: retained Performance test policy exit 0;'
  jq -e '.status == "pass" and .source_policy_disabled == true and .ui_targets == 1' \
    "$BATS_TEST_TMPDIR/work/logs/retained-performance-policy.json"
}

@test "warm ordinary builds prune only their own root and preserve isolated Profile products and proof" {
  assert_profile_policy_passes
  local work="$(cd "$BATS_TEST_TMPDIR/work" && pwd -P)" config
  local dd="$work/DerivedData-tests-profile"
  cp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/warm-proof"
  : >"$STUB_LOG"
  STUB_PRUNE_PROFILE_PRODUCTS=1 STUB_PERFORMANCE_NOOP=1 assert_profile_policy_passes
  cmp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/warm-proof"
  [ ! -e "$dd/.profile-performance-pending.json" ]
  [ "$(cat "$dd/Build/Products/Profile-iphonesimulator/Stale.bundle/keep")" = bundle ]
  [ "$(cat "$dd/Build/Products/Profile-iphonesimulator/Stale.o")" = object ]
  [ "$(cat "$dd/Build/Products/Profile-iphonesimulator/Stale.swiftmodule")" = module ]
  for config in Debug Release AppStore; do
    called "^prune-profile-products $work/DerivedData-tests $config "
    called "^xcodebuild build-for-testing .* -configuration $config .* -derivedDataPath $work/DerivedData-tests$"
  done
  not_called '^prune-profile-products .*DerivedData-tests-profile '
  called "^xcodebuild build-for-testing .* -configuration Profile .* -derivedDataPath $dd$"
  called "^vm-ssh rm -rf '$dd/Build/Products' && mkdir -p "
  called "^vm-xcodebuild test-without-building -xctestrun $dd/Build/Products/Meguri-Profile_Performance_"
  [ ! -e "$work/DerivedData-tests/.profile-performance-checked.json" ]
  [ ! -e "$work/DerivedData-tests/.profile-performance-pending.json" ]
}

@test "successful Profile build missing a new generated run cannot certify products with the old passing run" {
  assert_profile_policy_passes
  : >"$STUB_LOG"
  STUB_PERFORMANCE_MISSING_RUN=1 check_project --phase devices
  [ "$status" -ne 0 ]
  tap_line '# xcode_project_check: Profile/Performance cache blocked: selected generated run was not regenerated by this build'
  local line
  line="$(grep -n '^xcodebuild build-for-testing .* -configuration Profile ' "$STUB_LOG" | cut -d: -f1)"
  ! tail -n "+$((line + 1))" "$STUB_LOG" | grep -E '^vm-xcodebuild |^vm-ssh .*DerivedData-tests(-profile)?/Build/Products'
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
}

@test "Profile products lacking a receipt stop execution even when their test metadata passes policy" {
  assert_profile_policy_passes
  local dd="$BATS_TEST_TMPDIR/work/DerivedData-tests-profile"
  rm "$dd/.profile-performance-checked.json"
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  tap_line '# xcode_project_check: Profile/Performance cache blocked: unmarked retained .*'
  [ -f "$dd/Build/Products/Meguri-Profile_Performance_iphonesimulator27.0-arm64.xctestrun" ]
  [ -f "$dd/Build/Products/Profile-iphonesimulator/Meguri.app/Info.plist" ]
}

@test "legacy Profile product directory without any generated run blocks the earliest Products copy" {
  local dd="$BATS_TEST_TMPDIR/work/DerivedData-tests"
  mkdir -p "$dd/Build/Products/Profile-iphonesimulator/Meguri.app"
  echo legacy-partial >"$dd/Build/Products/Profile-iphonesimulator/Meguri.app/keep"
  check_project --phase devices
  assert_retained_cache_stopped "$dd"
  [ "$(cat "$dd/Build/Products/Profile-iphonesimulator/Meguri.app/keep")" = legacy-partial ]
}

@test "unchanged checked Profile cache is revalidated and reused before next Mac and Debug runs" {
  assert_profile_policy_passes
  local dd="$BATS_TEST_TMPDIR/work/DerivedData-tests-profile"
  [ -f "$dd/.profile-performance-checked.json" ]
  [ ! -e "$dd/.profile-performance-pending.json" ]
  # A failing Mac Unit build after preflight preserves its revalidation log
  # and lets us prove preflight used the current real validator and plan.
  : >"$STUB_LOG"
  STUB_MAC_UNIT_BUILD_FAIL=1 check_project --phase devices
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  tap_line 'ok [0-9]+ iphone-Profile-TestHooksTests: Passed .*'
  called '^vm-xcodebuild .*Meguri_Snapshot_macosx'
  grep -q '^xcode_project_check: retained Performance test policy exit 0;' "$BATS_TEST_TMPDIR/work/logs/mac-unit.log"
  [ ! -e "$dd/.profile-performance-pending.json" ]
  : >"$STUB_LOG"
  check_project --phase devices
  no_vm_failure
  tap_line 'ok [0-9]+ mac-unit: Passed .*'
  called '^vm-xcodebuild .*Meguri_Unit_macosx'
}

@test "changed current source policy rejects checked retained cache before earliest Products copy and stays blocked" {
  assert_profile_policy_passes
  cp "$SB/app/TestPlans/Performance.xctestplan" "$BATS_TEST_TMPDIR/original-plan"
  python3 - "$SB/app/TestPlans/Performance.xctestplan" <<'PY'
import json
from pathlib import Path
import sys
path = Path(sys.argv[1])
plan = json.loads(path.read_text())
plan["defaultOptions"]["mainThreadCheckerDetectionPolicy"] = {"enabled": True}
path.write_text(json.dumps(plan))
PY
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  [ "$(cat "$BATS_TEST_TMPDIR/work/logs/retained-performance-policy.status")" = 1 ]
  jq -e '.status == "fail" and .source_policy_disabled == false' "$BATS_TEST_TMPDIR/work/logs/retained-performance-policy.json"
  cp "$BATS_TEST_TMPDIR/original-plan" "$SB/app/TestPlans/Performance.xctestplan"
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  tap_line '# xcode_project_check: Profile/Performance cache blocked: persistent unchecked .*'
}

@test "current real validator changes are applied to retained cache before the earliest Products copy" {
  assert_profile_policy_passes
  # Increase the minimum UI target count in this test's validator copy. The
  # retained plist and source plan remain unchanged, with their old proof.
  python3 - "$SB/tools/scripts/performance-test-policy.py" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
old = 'report["ui_targets"] > 0'
assert source.count(old) == 1
path.write_text(source.replace(old, 'report["ui_targets"] > 1'))
PY
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  [ "$(cat "$BATS_TEST_TMPDIR/work/logs/retained-performance-policy.status")" = 1 ]
  jq -e '.status == "fail" and .source_policy_disabled == true and .ui_targets == 1' "$BATS_TEST_TMPDIR/work/logs/retained-performance-policy.json"
}

@test "failure to write persistent pending state prevents the next Profile build from starting" {
  assert_profile_policy_passes
  local dd="$BATS_TEST_TMPDIR/work/DerivedData-tests-profile"
  cp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/before-proof"
  # Opening the atomic-write staging path must fail, even as root.
  mkdir "$dd/.profile-performance-pending.json.tmp"
  : >"$STUB_LOG"
  check_project --phase devices
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ iphone-Profile-TestHooksTests: .*'
  tap_line '# xcode_project_check: Profile/Performance cache blocked: .*'
  tap_line '# xcode_project_check: cannot persist blocked cache state: .*'
  not_called '^xcodebuild build-for-testing .* -configuration Profile '
  not_called '^vm-test-products Profile-'
  not_called '^vm-xcodebuild .*DriveScriptTests'
  cmp "$dd/.profile-performance-checked.json" "$BATS_TEST_TMPDIR/before-proof"
  [ ! -e "$dd/Build/Products/Profile-iphonesimulator/partial-product" ]
}

@test "changed generated run invalidates checked proof even if its policy still passes" {
  assert_profile_policy_passes
  python3 - "$BATS_TEST_TMPDIR/work/DerivedData-tests-profile/Build/Products/Meguri-Profile_Performance_iphonesimulator27.0-arm64.xctestrun" <<'PY'
from pathlib import Path
import plistlib
import sys
path = Path(sys.argv[1])
run = plistlib.loads(path.read_bytes())
run["UnrelatedMetadata"] = "changed"
path.write_bytes(plistlib.dumps(run))
PY
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  tap_line '# xcode_project_check: Profile/Performance cache blocked: changed or unproven retained .*'
}

@test "changed isolated Profile product blocks ordinary builds and both Products copies" {
  assert_profile_policy_passes
  echo changed-object >"$BATS_TEST_TMPDIR/work/DerivedData-tests-profile/Build/Products/Profile-iphonesimulator/Stale.o"
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  tap_line '# xcode_project_check: Profile/Performance cache blocked: changed or unproven retained .*'
}

@test "new unproven Profile product directory invalidates checked proof before Mac copy" {
  assert_profile_policy_passes
  mkdir -p "$BATS_TEST_TMPDIR/work/DerivedData-tests-profile/Build/Products/Profile-macosx/Partial.app"
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  tap_line '# xcode_project_check: Profile/Performance cache blocked: changed or unproven retained .*'
}

@test "new retained Performance generated run invalidates checked proof before Mac copy" {
  assert_profile_policy_passes
  local products="$BATS_TEST_TMPDIR/work/DerivedData-tests-profile/Build/Products"
  cp "$products/Meguri-Profile_Performance_iphonesimulator27.0-arm64.xctestrun" "$products/Other_Performance_macosx27.0-arm64.xctestrun"
  : >"$STUB_LOG"
  check_project --phase devices
  assert_retained_cache_stopped
  [ -f "$products/Other_Performance_macosx27.0-arm64.xctestrun" ]
}

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

# Cases with an embedded widget use the production checkout/build path.
mac_debug_work() {
  export XCODE_CHECK_WORK="$(cd "$SB" && pwd -P)/build/mac-debug-signing"
  mkdir -p "$XCODE_CHECK_WORK"
}

seed_mac_debug_widget_products() {
  local products="$XCODE_CHECK_WORK/DerivedData-tests/Build/Products"
  local path
  for path in Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex \
    Debug/MeguriWidget.appex Debug/Meguri.app/Contents/PlugIns/Other.appex \
    Debug/Other.app Debug/Unrelated.framework; do
    mkdir -p "$products/$path"
    printf '%s\n' "$path" >"$products/$path/keep"
  done
  mkdir -p "$XCODE_CHECK_WORK/DerivedData-tests/Build/Intermediates.noindex"
  echo intermediate >"$XCODE_CHECK_WORK/DerivedData-tests/Build/Intermediates.noindex/keep"
}

assert_mac_debug_sentinels() {
  local products="$XCODE_CHECK_WORK/DerivedData-tests/Build/Products" path
  for path in Debug/MeguriWidget.appex Debug/Meguri.app/Contents/PlugIns/Other.appex \
    Debug/Other.app Debug/Unrelated.framework; do
    [ "$(cat "$products/$path/keep")" = "$path" ]
  done
  [ "$(cat "$XCODE_CHECK_WORK/DerivedData-tests/Build/Intermediates.noindex/keep")" = intermediate ]
}

assert_mac_debug_widget_builds() {
  python3 - "$STUB_LOG" "$@" <<'PY'
from pathlib import Path
import sys

lines = Path(sys.argv[1]).read_text().splitlines()
builds = [(i, line) for i, line in enumerate(lines)
          if line.startswith("xcodebuild build-for-testing ")
          and " -destination platform=macOS" in line and " -configuration Debug " in line]
plans = [line.split(" -testPlan ")[1].split()[0] for _, line in builds]
assert plans == sys.argv[2:], plans
for (i, line), plan in zip(builds, plans):
    assert lines[i + 1] == f"mac-debug-widget-absent-before-build {plan}", line
    assert lines[i + 2] == f"mac-debug-widget-recreated {plan}", line
assert sum(line.startswith("mac-debug-widget-absent-before-build ") for line in lines) == len(plans)
assert sum(line.startswith("mac-debug-widget-recreated ") for line in lines) == len(plans)
PY
}

assert_adhoc_mac_debug_widget() {
  mac_debug_work
  seed_mac_debug_widget_products
  MEGURI_ALLOW_MAC_UI=1 STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY=1 check_project --phase devices
  no_vm_failure
  called '^xcodebuild build-for-testing .* -testPlan Snapshot .* -destination platform=macOS,arch=arm64 .*DerivedData-tests$'
  not_called '^xcodebuild .* (MEGURI_SIGNING=|CODE_SIGN|DEVELOPMENT_TEAM=|-allowProvisioning)'
  assert_mac_debug_widget_builds Unit Snapshot UI
  [ "$(cat "$XCODE_CHECK_WORK/DerivedData-tests/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex/recreated")" = UI ]
  assert_mac_debug_sentinels
}

@test "default signing refreshes only the embedded widget before each Mac Debug test build" {
  assert_adhoc_mac_debug_widget
}

@test "explicit adhoc signing refreshes only the embedded widget before each Mac Debug test build" {
  MEGURI_APP_SIGNING=adhoc assert_adhoc_mac_debug_widget
}

@test "explicit team keeps provisioning arguments scoped to Mac Snapshot and refreshes every Mac Debug embedded widget" {
  mac_debug_work
  seed_mac_debug_widget_products
  MEGURI_APP_SIGNING=team MEGURI_ALLOW_MAC_UI=1 STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY=1 check_project
  no_vm_failure
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  assert_mac_debug_widget_builds Unit Snapshot UI
  called '^vm-xcodebuild .*Meguri_Unit_macosx'
  called '^vm-xcodebuild .*Meguri_UI_macosx'
  called '^vm-xcodebuild .*Meguri_Snapshot_iphonesimulator'
  [ "$(cat "$XCODE_CHECK_WORK/DerivedData-tests/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex/recreated")" = UI ]
  assert_mac_debug_sentinels
  python3 - "$STUB_LOG" "$(cd "$SB" && pwd -P)" "$XCODE_CHECK_WORK" "$REPO_ROOT/app/Config/MeguriUITests.xcconfig" <<'PY'
from pathlib import Path
import sys

log, repo, work, runner = sys.argv[1:]
builds = [line for line in Path(log).read_text().splitlines() if line.startswith("xcodebuild ")]
snapshot = [line for line in builds if " -testPlan Snapshot " in line and " -destination platform=macOS" in line]
expected = (f"xcodebuild build-for-testing -project {repo}/app/Meguri.xcodeproj"
            " -scheme Meguri -testPlan Snapshot -configuration Debug -destination platform=macOS,arch=arm64"
            f" -derivedDataPath {work}/DerivedData-tests"
            " MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration")
assert snapshot == [expected], snapshot
for line in builds:
    assert not any(key in line for key in ("CODE_SIGN", "DEVELOPMENT_TEAM", "PROVISIONING_PROFILE")), line
    if line != expected:
        assert not any(arg in line for arg in ("MEGURI_SIGNING=", "-allowProvisioning")), line
config = Path(runner).read_text()
for setting in ("CODE_SIGN_STYLE[sdk=macosx*] = Manual", "CODE_SIGN_IDENTITY[sdk=macosx*] = -",
                "DEVELOPMENT_TEAM[sdk=macosx*] =\n"):
    assert setting in config, setting
PY
}

@test "Team Snapshot to adhoc Unit to Team Snapshot refreshes the embedded widget in both directions" {
  mac_debug_work
  seed_mac_debug_widget_products
  MEGURI_APP_SIGNING=team STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY=1 check_project --phase devices
  no_vm_failure
  assert_mac_debug_widget_builds Unit Snapshot
  [ "$(cat "$XCODE_CHECK_WORK/DerivedData-tests/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex/recreated")" = Snapshot ]
  assert_mac_debug_sentinels

  MEGURI_APP_SIGNING=adhoc MEGURI_ALLOW_MAC_UI=1 STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY=1 check_project --phase devices
  no_vm_failure
  assert_mac_debug_widget_builds Unit Snapshot Unit Snapshot UI
  assert_mac_debug_sentinels

  MEGURI_APP_SIGNING=team MEGURI_ALLOW_MAC_UI=1 STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY=1 check_project --phase devices
  no_vm_failure
  assert_mac_debug_widget_builds Unit Snapshot Unit Snapshot UI Unit Snapshot UI
  assert_mac_debug_sentinels
  python3 - "$STUB_LOG" <<'PY'
from pathlib import Path
import sys

builds = [line for line in Path(sys.argv[1]).read_text().splitlines()
          if line.startswith("xcodebuild ")]
mac = [line for line in builds if " -destination platform=macOS" in line]
team = [mac[1], mac[6]]
suffix = " MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration"
for line in builds:
    assert not any(arg in line for arg in ("CODE_SIGN", "DEVELOPMENT_TEAM", "PROVISIONING_PROFILE")), line
    if line in team:
        assert " -testPlan Snapshot -configuration Debug " in line and line.endswith(suffix), line
    else:
        assert not any(arg in line for arg in ("MEGURI_SIGNING=", "-allowProvisioning")), line
assert len([line for line in builds if line.endswith(suffix)]) == 2
PY
}

@test "invalid and explicitly empty app signing modes stop VM test builds and Products copies with a fixed diagnostic" {
  local mode
  for mode in invalid-signing-marker ''; do
    : >"$STUB_LOG"
    MEGURI_APP_SIGNING="$mode" check_project --phase devices
    [ "$status" -ne 0 ]
    tap_line '# xcode_project_check: MEGURI_APP_SIGNING takes adhoc or team'
    ! grep -q 'invalid-signing-marker' <<<"$output"
    not_called '^xcodebuild build-for-testing '
    not_called '^vm-ssh .*DerivedData-tests(-profile)?/Build/Products'
    not_called '^vm-xcodebuild '
  done
}

@test "failed Team Mac Snapshot build starts no Snapshot VM test or fallback build" {
  mac_debug_work
  seed_mac_debug_widget_products
  MEGURI_APP_SIGNING=team STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY=1 STUB_MAC_SNAPSHOT_BUILD_FAIL=1 check_project --phase devices
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ mac-app-hosted: .*'
  tap_line '# xcode_project_check: the build for testing failed'
  [ "$(grep -c '^xcodebuild build-for-testing .* -testPlan Snapshot .* -destination platform=macOS' "$STUB_LOG")" -eq 1 ]
  called '^xcodebuild build-for-testing .* -testPlan Snapshot .* -destination platform=macOS.* MEGURI_SIGNING=team -allowProvisioningUpdates -allowProvisioningDeviceRegistration$'
  called '^mac-debug-widget-absent-before-build Snapshot$'
  not_called '^mac-debug-widget-recreated Snapshot$'
  not_called '^vm-xcodebuild .*Meguri_Snapshot_macosx'
  not_called '^xcodebuild test '
  [ ! -e "$XCODE_CHECK_WORK/DerivedData-tests/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex" ]
  assert_mac_debug_sentinels
}

@test "Mac Debug refuses widget symlinks, dangling symlinks, files and plugin ancestor symlinks in both signing modes" {
  mac_debug_work
  local plugins="$XCODE_CHECK_WORK/DerivedData-tests/Build/Products/Debug/Meguri.app/Contents/PlugIns"
  local outside="$BATS_TEST_TMPDIR/outside-widget" kind mode
  mkdir -p "$plugins" "$outside/MeguriWidget.appex"
  echo outside >"$outside/keep"
  echo outside-widget >"$outside/MeguriWidget.appex/keep"
  for mode in team adhoc; do
    for kind in widget dangling file ancestor; do
      : >"$STUB_LOG"
      case "$kind" in
        widget) ln -s "$outside" "$plugins/MeguriWidget.appex" ;;
        dangling) ln -s "$outside/missing" "$plugins/MeguriWidget.appex" ;;
        file) echo leaf-file >"$plugins/MeguriWidget.appex" ;;
        ancestor) rmdir "$plugins"; ln -s "$outside" "$plugins" ;;
      esac
      MEGURI_APP_SIGNING="$mode" check_project --phase devices
      [ "$status" -ne 0 ]
      tap_line '# xcode_project_check: unsafe Mac Debug embedded widget path'
      not_called '^xcodebuild build-for-testing .* -destination platform=macOS'
      not_called '^vm-xcodebuild .*_macosx'
      [ "$(cat "$outside/keep")" = outside ]
      [ "$(cat "$outside/MeguriWidget.appex/keep")" = outside-widget ]
      [ "$(ls -A "$outside")" = "$(printf 'MeguriWidget.appex\nkeep')" ]
      [ "$(ls -A "$outside/MeguriWidget.appex")" = keep ]
      [ ! -e "$outside/missing" ]
      case "$kind" in
        ancestor) [ -L "$plugins" ]; rm "$plugins"; mkdir "$plugins" ;;
        file) [ "$(cat "$plugins/MeguriWidget.appex")" = leaf-file ]; rm "$plugins/MeguriWidget.appex" ;;
        *) [ -L "$plugins/MeguriWidget.appex" ]; rm "$plugins/MeguriWidget.appex" ;;
      esac
    done
  done
}

@test "Mac Debug refuses an existing widget outside the canonical checkout build directory in both signing modes" {
  local widget="$BATS_TEST_TMPDIR/work/DerivedData-tests/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex" mode
  mkdir -p "$widget"
  echo keep >"$widget/keep"
  for mode in team adhoc; do
    : >"$STUB_LOG"
    MEGURI_APP_SIGNING="$mode" check_project --phase devices
    [ "$status" -ne 0 ]
    tap_line '# xcode_project_check: unsafe Mac Debug embedded widget path'
    not_called '^xcodebuild build-for-testing .* -destination platform=macOS'
    not_called '^vm-xcodebuild .*_macosx'
    [ "$(cat "$widget/keep")" = keep ]
  done
}

@test "cold Mac Debug runs with no embedded widget need no canonical build path in either signing mode" {
  local mode
  for mode in team adhoc; do
    : >"$STUB_LOG"
    MEGURI_APP_SIGNING="$mode" MEGURI_ALLOW_MAC_UI=1 check_project --phase devices
    no_vm_failure
    called '^vm-xcodebuild .*Meguri_Unit_macosx'
    called '^vm-xcodebuild .*Meguri_Snapshot_macosx'
    called '^vm-xcodebuild .*Meguri_UI_macosx'
    ! grep -q 'unsafe Mac Debug embedded widget path' <<<"$output"
  done
}

@test "retained Profile proof blocks both signing modes before widget removal, build or Products copy" {
  mac_debug_work
  seed_mac_debug_widget_products
  mkdir -p "$XCODE_CHECK_WORK/DerivedData-tests/Build/Products/Profile-iphonesimulator/Partial.app"
  local mode
  for mode in team adhoc; do
    : >"$STUB_LOG"
    MEGURI_APP_SIGNING="$mode" check_project --phase devices
    [ "$status" -ne 0 ]
    tap_line '# xcode_project_check: Profile/Performance cache blocked: .*'
    not_called '^xcodebuild build-for-testing '
    not_called '^vm-ssh .*DerivedData-tests(-profile)?/Build/Products'
    not_called '^vm-xcodebuild '
    [ -f "$XCODE_CHECK_WORK/DerivedData-tests/Build/Products/Debug/Meguri.app/Contents/PlugIns/MeguriWidget.appex/keep" ]
    assert_mac_debug_sentinels
  done
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
  mac_debug_work
  seed_mac_debug_widget_products
  STUB_EXPECT_MAC_DEBUG_WIDGET_RECOPY=1 STUB_MAC_UNIT_BUILD_FAIL=1 check_project --phase devices
  [ "$status" -ne 0 ]
  tap_line 'not ok [0-9]+ mac-unit: .*'
  tap_line '# xcodebuild exit 70; log: .*mac-unit.log'
  tap_line '# .*Meguri.app: signing refused the generated bundle'
  tap_line '# In subcomponent: .*MeguriSnapshotTests.xctest'
  tap_line '# Command CodeSign failed with a nonzero exit code'
  ! grep -q 'synthetic-environment-marker' <<<"$output"
  ! grep -Eq '^ok [0-9]+ mac-unit' <<<"$output"
  [ "$(grep -c '^xcodebuild build-for-testing .* -testPlan Unit .* -destination platform=macOS,arch=arm64 ' "$STUB_LOG")" -eq 1 ]
  called '^mac-debug-widget-absent-before-build Unit$'
  not_called '^mac-debug-widget-recreated Unit$'
  not_called '^vm-xcodebuild .*Meguri_Unit_macosx27.0-arm64.xctestrun '
  tap_line 'ok [0-9]+ mac-app-hosted: Passed .*'
  assert_mac_debug_sentinels
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
  FAKE_VM_FORK_FAIL="DerivedData-tests(-profile)?/Build/Products' && mkdir -p" check_project --phase devices --vm-ssh "$STUBS/vm-ssh" --vm-name meguri-mac
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

@test "only Mac Unit receives delivered cloud inputs with unchanged test selection" {
  MEGURI_ALLOW_MAC_UI=1 check_project --phase devices
  no_vm_failure
  [ "$(grep -c '^unit-input-deliver$' "$STUB_LOG")" -eq 1 ]
  [ "$(grep -c '^unit-input-command$' "$STUB_LOG")" -eq 1 ]
  called '^unit-env .*/build/mg/xcode-check/wiring-run/vm-unit-inputs/initial-adoption-fixture.json .*/build/mg/xcode-check/wiring-run/vm-unit-inputs/meguri-cli$'
  called '^vm-xcodebuild test-without-building .*Meguri_Unit_macosx.* -destination platform=macOS,arch=arm64 .* -collect-test-diagnostics never$'
  called '^vm-xcodebuild .*Meguri_UI_macosx.* -only-testing:MeguriUITests/TestHooksTests$'
  called '^vm-xcodebuild .*Meguri_UI_iphonesimulator.* -only-testing:MeguriUITests/DriveScriptTests$'
}

@test "Mac Unit missing or failed input delivery is a prelaunch failure while other lanes remain independent" {
  for step in missing deliver command; do
    : >"$STUB_LOG"
    if [ "$step" = missing ]; then unset MG_RUN_DIR; fi
    STUB_UNIT_INPUT_FAIL="$step" check_project --phase devices
    [ "$status" -ne 0 ]
    tap_line 'not ok [0-9]+ mac-unit: .*'
    tap_line '# xcode_project_check: Mac Unit input delivery failed before launch'
    not_called '^vm-xcodebuild .*Meguri_Unit_macosx'
    called '^vm-xcodebuild .*Meguri_Snapshot_macosx'
    export MG_RUN_DIR="$SB/build/mg/xcode-check/wiring-run"
  done
}

@test "xcode-check preparation failure retains producer evidence and still runs independent device lanes" {
  fake_vm
  add_test_inputs
  # Keep the real devices phase below the MG wrapper. Only host builds and
  # preparation are doubles; no native or VM execution evidence is claimed.
  mv "$SB/tools/tests/xcode_project_check.sh" "$SB/tools/tests/xcode_project_check.real.sh"
  cat >"$SB/tools/tests/xcode_project_check.sh" <<'SH'
#!/bin/bash
case " $* " in
  *" --phase builds "*) echo 'ok 1 builds'; exit 0 ;;
esac
[ "${MEGURI_INITIAL_ADOPTION_FIXTURE+present}" != present ] || exit 97
[ "${MEGURI_CLOUD_SERVICE_CLI+present}" != present ] || exit 97
exec /bin/bash "$(dirname "$0")/xcode_project_check.real.sh" "$@"
SH
  mv "$FAKE_VM_BIN/xcodebuild" "$FAKE_VM_BIN/xcodebuild-real"
  cat >"$FAKE_VM_BIN/xcodebuild" <<'SH'
#!/bin/bash
[ "${TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE+present}" != present ] || exit 97
[ "${TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI+present}" != present ] || exit 97
exec "$(dirname "$0")/xcodebuild-real" "$@"
SH
  chmod +x "$FAKE_VM_BIN/xcodebuild"
  check_wrapper() (
    cd "$SB" || exit 1
    export MG_ROOT="$SB" MG_JOB=xcode-check MG_STATE="$MG_RUN_DIR/state"
    mkdir -p "$MG_STATE"
    MG_DEFAULT_REPRO='tools/mg xcode-check'
    MG_QUEUED_PARTS='[]'
    MG_MAC_VM="$(jq -nc --arg ssh "$STUBS/vm-ssh" '{ssh: $ssh, name: "meguri-mac"}')"
    mg_job() { :; }
    mg_status() { printf '%s\n' "$1" >"$MG_STATE/status"; }
    mg_failure() { printf '%s\n' "$1" >>"$MG_STATE/failures.jsonl"; }
    mg_artifact() { :; }
    mg_count() { :; }
    mg_message() { :; }
    mg_lint_ui_first() { :; }
    mg__xcframework() { :; }
    mg_sim_begin() { MG_SIM_RUNNER=vm; }
    mg_sim_take() { :; }
    mg__mac_vm_stop_orphans() { :; }
    mg_queued_settle() { :; }
    source "$REPO_ROOT/tools/mg.d/selftest.sh"
    source "$REPO_ROOT/tools/mg.d/xcode.sh"
    mg__xcode_unit_inputs() {
      printf 'failed producer evidence\n' >"$MG_RUN_DIR/producer.log"
      export MEGURI_INITIAL_ADOPTION_FIXTURE=partial-fixture MEGURI_CLOUD_SERVICE_CLI=partial-helper
      mg_failure producer-failed
      mg_status fail
      return 1
    }
    mg_xcode_check
  )
  MEGURI_ALLOW_MAC_UI=1 STUB_UNIT_INPUT_FAIL=deliver \
    TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE=ambient-fixture TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI=ambient-helper \
    run check_wrapper
  [ "$status" -eq 0 ]
  [ "$(cat "$MG_RUN_DIR/state/status")" = fail ]
  grep -qx producer-failed "$MG_RUN_DIR/state/failures.jsonl"
  grep -qx xcode-unit-inputs "$MG_RUN_DIR/state/failures.jsonl"
  [ "$(cat "$MG_RUN_DIR/producer.log")" = 'failed producer evidence' ]
  tap_line 'not ok [0-9]+ mac-unit: .*'
  not_called '^vm-xcodebuild .*Meguri_Unit_macosx'
  called '^unit-input-deliver$'
  called '^vm-xcodebuild .*Meguri_Snapshot_macosx'
  called '^vm-xcodebuild .*Meguri_UI_macosx.*TestHooksTests'
  called '^vm-xcodebuild .*Meguri_Snapshot_iphonesimulator'
  called '^vm-xcodebuild .*Meguri_UI_iphonesimulator.*DriveScriptTests'
}

@test "no UI or no VM retains the absence of cloud input delivery" {
  check_project --phase devices --no-ui
  not_called '^unit-input-'
  check_project --phase devices --no-sims
  not_called '^unit-input-'
}

@test "unreachable VM during Unit input delivery queues that lane before launch" {
  STUB_UNIT_INPUT_UNAVAILABLE=deliver check_project --phase devices
  tap_line 'ok [0-9]+ mac-unit # SKIP queued: meguri-mac unavailable during Mac Unit input delivery \(user step mac-vm\)'
  not_called '^vm-xcodebuild .*Meguri_Unit_macosx'
  called '^vm-xcodebuild .*Meguri_Snapshot_macosx'
}

# Generated tooling execution only: keep the real MG routing, input admission,
# devices script and VM transport; producers and native tools are doubles.
focused_wrapper() (
  cd "$SB" || exit 1
  export MG_ROOT="$SB" MG_STATE="$MG_RUN_DIR/state"
  mkdir -p "$MG_STATE"
  MG_JOB=xcode-check MG_DEFAULT_REPRO='tools/mg xcode-check --mac-unit-only' MG_QUEUED_PARTS='[]'
  MG_MAC_VM="$(jq -nc --arg ssh "$STUBS/vm-ssh" '{ssh:$ssh,name:"meguri-mac"}')"
  mg_job() { :; }
  mg_status() { echo "$1" >"$MG_STATE/status"; }
  mg_failure() { echo "$1" >>"$MG_STATE/failures.jsonl"; }
  mg_artifact() { :; }
  mg_count() { :; }
  mg_message() { echo "$*"; }
  mg_detail() { echo "$1=$2" >>"$MG_STATE/details"; }
  mg_lint_ui_first() { echo lint >>"$STUB_LOG"; }
  mg__xcframework() { echo core-build >>"$STUB_LOG"; }
  mg_sim_begin() { echo vm-begin >>"$STUB_LOG"; MG_SIM_RUNNER=vm; MG_MAC_VM_REASON='generated VM unavailable'; [ "${STUB_NO_VM:-0}" = 0 ] || { MG_SIM_RUNNER=''; return 2; }; }
  mg_sim_take() { echo vm-take >>"$STUB_LOG"; }
  mg__mac_vm_stop_orphans() { echo stop-orphans >>"$STUB_LOG"; }
  mg__mac_vm_unreachable() { MG_QUEUED_PARTS='["mac-unit"]'; }
  mg_queued_parts_text() { echo mac-unit; }
  mg_queued_settle() { [ "$MG_QUEUED_PARTS" = '[]' ] || mg_status warn; }
  source "$REPO_ROOT/tools/mg.d/selftest.sh"
  source "$REPO_ROOT/tools/mg.d/xcode.sh"
  mg__swift_initial_adoption_fixture() { echo fixture-producer >>"$STUB_LOG"; MEGURI_INITIAL_ADOPTION_RESULT=producer-result; [ "${STUB_PRODUCER_FAIL:-0}" = 0 ]; }
  mg__swift_cloud_service_helper() { echo cli-producer >>"$STUB_LOG"; }
  mg_xcode_check "$@"
)

@test "focused MG runs only the whole Mac Unit plan through named producers and VM, with focused scope" {
  fake_vm
  add_test_inputs
  run focused_wrapper --mac-unit-only
  [ "$status" -eq 0 ]
  [ "$(cat "$MG_RUN_DIR/state/status")" = pass ]
  grep -qx 'scope="focusedMacUnit"' "$MG_RUN_DIR/state/details"
  [[ "$output" == *'full Xcode matrix, T0/gate and broader-platform coverage not evaluated'* ]]
  called '^fixture-producer$'
  called '^cli-producer$'
  called '^unit-input-verify-inputs$'
  called '^unit-input-deliver$'
  called '^vm-xcodebuild test-without-building .*Meguri_Unit_macosx.* -destination platform=macOS,arch=arm64 .* -collect-test-diagnostics never$'
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" -eq 1 ]
  [ "$(grep -c '^xcodebuild build-for-testing ' "$STUB_LOG")" -eq 1 ]
  not_called 'only-testing|skip-testing|^sim-pool|^open |^vm-open |^xcodebuild (build |test|-[a-z])'
}

@test "focused public and internal admission refuses incompatible and malformed routing before side effects" {
  for option in --no-ui --cloud-signing-matrix '--iphone ID' '--ipad ID' '--phase devices' '--vm-ssh command' --no-sims '--continue-from 1' --unknown --mac-unit-only=bad --mac-unit-only; do
    run focused_wrapper --mac-unit-only $option
    [ "$(cat "$MG_RUN_DIR/state/status")" = error ]
    [ ! -s "$STUB_LOG" ]
    run /bin/bash "$SB/tools/tests/xcode_project_check.sh" "$SB/rejected" --phase devices --continue-from 0 --no-sims --mac-unit-only $option
    [ "$status" -eq 2 ]
    [ ! -d "$SB/rejected" ]
    [ ! -s "$STUB_LOG" ]
  done
}

@test "focused absent VM queues, producer failure prevents launch, and actual results stay red" {
  fake_vm
  add_test_inputs
  STUB_NO_VM=1 run focused_wrapper --mac-unit-only
  [ "$(cat "$MG_RUN_DIR/state/status")" = warn ]
  tap_line 'ok 1 mac-unit # SKIP queued: no test VM \(user step mac-vm\)'
  not_called '^unit-input-|^fixture-producer|^cli-producer|^xcodebuild |^vm-xcodebuild '
  : >"$STUB_LOG"
  STUB_PRODUCER_FAIL=1 run focused_wrapper --mac-unit-only
  [ "$(cat "$MG_RUN_DIR/state/status")" = fail ]
  not_called '^xcodebuild |^vm-xcodebuild |^unit-input-deliver$'
  for failure in build summary exit; do
    : >"$STUB_LOG"
    case "$failure" in
      build) STUB_MAC_UNIT_BUILD_FAIL=1 run focused_wrapper --mac-unit-only ;;
      summary) STUB_UNIT_SUMMARY='{"result":"Failed","passedTests":0,"failedTests":1,"skippedTests":0}' run focused_wrapper --mac-unit-only ;;
      exit) FAKE_VM_TEST_RC=65 run focused_wrapper --mac-unit-only ;;
    esac
    [ "$(cat "$MG_RUN_DIR/state/status")" = fail ]
    tap_line 'not ok 1 mac-unit: .*'
  done
}
