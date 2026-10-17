# App builds and launches (U8, R1961, R1962). Sourced by tools/mg; host jobs.
#
# Builds go to build/app/DerivedData (build-for-testing of the UI plan, so
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
  "Build the app and its UI tests: --platform ios-sim|mac [--config Debug|Profile|Release|AppStore] (lint-ui first, core XCFramework, xcodebuild build-for-testing into build/app/DerivedData; Mac: Debug and Profile only)"
mg_job run host mg_run \
  "Install and launch with test-hook options: --platform iphone|iphone-small|ipad|ipad-small|foldable|mac [--device <leased udid>] [--config Debug|Profile] [--route r] [--window-size s] [--seconds N] [-- launch args]; a simulator keeps running the app (and its lease), the Mac app (Debug only) runs hidden for N s (5), out of the Dock and never active (checked with lsappinfo every second), and quits"

MG_APP_DD=build/app/DerivedData
MG_APP_DEV_ID=com.antoinefink.meguri.dev

# mg_app_products <mac|ios-sim> <config>
mg_app_products() {
  case "$1" in
    mac) printf '%s/%s/Build/Products/%s\n' "$MG_ROOT" "$MG_APP_DD" "$2" ;;
    *) printf '%s/%s/Build/Products/%s-iphonesimulator\n' "$MG_ROOT" "$MG_APP_DD" "$2" ;;
  esac
}

# mg_app_build <mac|ios-sim> <config> [plan] [scheme]: UI lint first (AE90),
# core XCFramework and build-for-testing of the plan (UI by default;
# snapshots build Snapshot; perf-ui builds Performance with the
# Meguri-Profile scheme). On failure records it and returns 1.
mg_app_build() {
  local platform="$1" config="$2" plan="${3:-UI}" scheme="${4:-Meguri}" dest result log rc counts
  if [ "$platform" = mac ] && ! mg_word_in "$config" "Debug Profile"; then
    mg_failure usage "the Mac builds only Debug and Profile here: $config carries the daily-driver ids (tools/mg xcode-check builds and unregisters it)"
    mg_status error
    return 1
  fi
  mg_lint_ui_first || return 1
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_message "$MG_JOB: building the core XCFramework failed"
    mg_status fail
    return 1
  fi
  case "$platform" in
    mac) dest="platform=macOS,arch=arm64" ;;
    *) dest="generic/platform=iOS Simulator" ;;
  esac
  result="$MG_RUN_DIR/build-$platform-$config-$plan.xcresult"
  log="$MG_RUN_DIR/build-$platform-$config-$plan.log"
  rm -rf "$result"
  echo "$MG_JOB: xcodebuild build-for-testing ($platform, $config, $plan plan)" >&2
  xcodebuild build-for-testing -project app/Meguri.xcodeproj -scheme "$scheme" -testPlan "$plan" -configuration "$config" \
    -destination "$dest" -derivedDataPath "$MG_APP_DD" -resultBundlePath "$result" >"$log" 2>&1
  rc=$?
  mg_artifact log "$log" "xcodebuild build-for-testing ($platform, $config, $plan plan)"
  counts="$(xcrun xcresulttool get build-results --path "$result" 2>/dev/null |
    jq -c '{errors: .errorCount, warnings: .warningCount, messages: [(.errors[]?, .warnings[]?) | .message] | .[:10]}')" || counts=null
  mg_detail build "$(jq -nc --arg p "$platform" --arg c "$config" --arg app "$(mg_app_products "$platform" "$config")/Meguri.app" \
    --argjson counts "${counts:-null}" '{platform: $p, config: $c, app: $app} + ($counts // {})')"
  if [ "$rc" -ne 0 ]; then
    mg_failure build "xcodebuild build-for-testing failed ($platform, $config): $(grep -E 'error:' "$log" | head -n 3 | tr '\n' ' ')" \
      "tools/mg build-app --platform $platform --config $config"
    mg_message "$MG_JOB: the $platform $config build failed; see $log"
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

# mg_app_xcodebuild_test <out prefix> <mac|ios> <destination> <config> [xcodebuild args]:
# test-without-building of the UI plan (or MG_APP_TEST_PLAN with the scheme
# MG_APP_TEST_SCHEME, as perf-ui sets them) into <prefix>.xcresult and <prefix>.log.
# Mac runs go where mg_mac_ui_begin sent them (MG_MAC_RUNNER), simulator runs
# where mg_sim_begin did (MG_SIM_RUNNER): to the test VM (mac-vm.sh) or this
# Mac. Returns 0 pass, 1 test failures, 2 could not run (records a user step
# when macOS has not allowed UI automation).
mg_app_xcodebuild_test() {
  local prefix="$1" platform="$2" dest="$3" config="$4" rc summary sdk=""
  shift 4
  rm -rf "$prefix.xcresult"
  mkdir -p "$(dirname "$prefix")"
  if [ "$platform" = mac ] && [ "$MG_MAC_RUNNER" = vm ]; then
    sdk=macosx
  elif [ "$platform" != mac ] && [ "$MG_SIM_RUNNER" = vm ]; then
    sdk=iphonesimulator
  fi
  if [ -n "$sdk" ]; then
    mg_vm_xcodebuild_test "$prefix" "${MG_APP_TEST_PLAN:-UI}" "$sdk" "$dest" "$config" "$@"
    rc=$?
    mg_artifact log "$prefix.log" "xcodebuild test ($platform, in the test VM)"
    [ -z "$MG_MAC_VM_RUN_ERROR" ] || return 2
  else
    # No diagnostics on failure: they take up to 10 minutes (simctl diagnose)
    # and, on the Mac, are a sysdiagnose of the user's machine.
    xcodebuild test-without-building -project app/Meguri.xcodeproj -scheme "${MG_APP_TEST_SCHEME:-Meguri}" \
      -testPlan "${MG_APP_TEST_PLAN:-UI}" -configuration "$config" \
      -destination "$dest" -derivedDataPath "$MG_APP_DD" -resultBundlePath "$prefix.xcresult" -collect-test-diagnostics never \
      "$@" >"$prefix.log" 2>&1
    rc=$?
    mg_artifact log "$prefix.log" "xcodebuild test ($platform)"
  fi
  summary="$(xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -r '"\(.result) passed=\(.passedTests) failed=\(.failedTests) skipped=\(.skippedTests)"')"
  echo "$MG_JOB: xcodebuild test ($platform): exit $rc ${summary:-no result}" >&2
  if grep -q 'failed to initialize for UI testing' "$prefix.log"; then
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
# returns 1.
mg_sim_lease() {
  local rc
  MG_SIM_UDID="$("$MG_TOOLS/scripts/sim-pool.sh" acquire "$1" --holder "$(basename "$MG_ROOT") $MG_JOB")"
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
    5 | 6)
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

mg_build_app() {
  local platform="" config=Debug
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "ios-sim mac" || { mg_drive__usage "--platform takes ios-sim or mac"; return 0; }
        platform="$2" && shift 2
        ;;
      --config)
        [ "$#" -ge 2 ] && mg_word_in "$2" "Debug Profile Release AppStore" || { mg_drive__usage "--config takes Debug, Profile, Release or AppStore"; return 0; }
        config="$2" && shift 2
        ;;
      *) mg_drive__usage "usage: tools/mg build-app --platform ios-sim|mac [--config C]"; return 0 ;;
    esac
  done
  [ -n "$platform" ] || { mg_drive__usage "usage: tools/mg build-app --platform ios-sim|mac [--config C]"; return 0; }
  mg_app_build "$platform" "$config" || return 0
  mg_artifact app "$(mg_app_products "$platform" "$config")/Meguri.app"
  mg_message "build-app: $platform $config built: $(mg_app_products "$platform" "$config")/Meguri.app"
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
  [ "${#MG_D_POSITIONAL[@]}" -eq 0 ] || { mg_drive__usage "unexpected argument '${MG_D_POSITIONAL[0]}'"; return 0; }
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
    if ! mg_vm_launch_args "${launch[@]}"; then
      mg_message "run: a launch fixture could not be put in the test VM; nothing was launched"
      mg_status error
      return 0
    fi
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
