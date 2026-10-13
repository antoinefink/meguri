# App builds and launches (U8, R1961, R1962). Sourced by tools/mg; host jobs.
#
# Builds go to build/app/DerivedData (build-for-testing of the UI plan, so
# drive, ui-tree and screenshot reuse them). Launches take the host lease
# (host.sh). Simulators come from the pool (tools/scripts/sim-pool.sh): a
# --device must be a pool device this checkout leased. The Mac app is
# launched hidden (open -g -j) and never in Release or AppStore, whose ids
# are the daily driver's (app/AGENTS.md).

mg_job build-app host mg_build_app \
  "Build the app and its UI tests: --platform ios-sim|mac [--config Debug|Profile|Release|AppStore] (core XCFramework, UI lint once U9 adds it, xcodebuild build-for-testing into build/app/DerivedData; Mac: Debug and Profile only)"
mg_job run host mg_run \
  "Install and launch with test-hook options: --platform iphone|iphone-small|ipad|ipad-small|foldable|mac [--device <leased udid>] [--config Debug|Profile] [--route r] [--window-size s] [--seconds N] [-- launch args]; a simulator keeps running the app (and its lease), the Mac app runs hidden for N s (5) and quits"

MG_APP_DD=build/app/DerivedData
MG_APP_DEV_ID=com.antoinefink.meguri.dev

# mg_app_products <mac|ios-sim> <config>
mg_app_products() {
  case "$1" in
    mac) printf '%s/%s/Build/Products/%s\n' "$MG_ROOT" "$MG_APP_DD" "$2" ;;
    *) printf '%s/%s/Build/Products/%s-iphonesimulator\n' "$MG_ROOT" "$MG_APP_DD" "$2" ;;
  esac
}

# mg_app__lint_ui: the UI lint gate (U9) when the lint-ui job exists.
mg_app__lint_ui() {
  if ! mg__job_index lint-ui >/dev/null 2>&1; then
    mg_detail lint_ui '"not available until U9 adds tools/mg lint-ui"'
    return 0
  fi
  local result
  result="$("$MG_TOOLS/mg" lint-ui | tail -n 1)"
  mg_detail lint_ui "$(jq -c '{status, message, result: input_filename}' "$result" 2>/dev/null || echo '"no result"')"
  if [ "$(jq -r .status "$result" 2>/dev/null)" != pass ] && [ "$(jq -r .status "$result" 2>/dev/null)" != warn ]; then
    mg_failure lint-ui "the UI lint gate did not pass; see $result" "tools/mg lint-ui"
    mg_message "$MG_JOB: the UI lint gate did not pass"
    mg_status fail
    return 1
  fi
}

# mg_app_build <mac|ios-sim> <config>: core XCFramework, UI lint and
# build-for-testing of the UI plan. On failure records it (fail) and
# returns 1.
mg_app_build() {
  local platform="$1" config="$2" dest result log rc counts
  if [ "$platform" = mac ] && ! mg_word_in "$config" "Debug Profile"; then
    mg_failure usage "the Mac builds only Debug and Profile here: $config carries the daily-driver ids (tools/mg xcode-check builds and unregisters it)"
    mg_status error
    return 1
  fi
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_message "$MG_JOB: building the core XCFramework failed"
    mg_status fail
    return 1
  fi
  mg_app__lint_ui || return 1
  case "$platform" in
    mac) dest="platform=macOS,arch=arm64" ;;
    *) dest="generic/platform=iOS Simulator" ;;
  esac
  result="$MG_RUN_DIR/build-$platform-$config.xcresult"
  log="$MG_RUN_DIR/build-$platform-$config.log"
  rm -rf "$result"
  echo "$MG_JOB: xcodebuild build-for-testing ($platform, $config)" >&2
  xcodebuild build-for-testing -project app/Meguri.xcodeproj -scheme Meguri -testPlan UI -configuration "$config" \
    -destination "$dest" -derivedDataPath "$MG_APP_DD" -resultBundlePath "$result" >"$log" 2>&1
  rc=$?
  mg_artifact log "$log" "xcodebuild build-for-testing ($platform, $config)"
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

# mg_app_xcodebuild_test <out prefix> <mac|ios> <destination> <config> [xcodebuild args]:
# test-without-building of the UI plan into <prefix>.xcresult and <prefix>.log.
# Returns 0 pass, 1 test failures, 2 could not run (records a user step when
# macOS has not allowed UI automation).
mg_app_xcodebuild_test() {
  local prefix="$1" platform="$2" dest="$3" config="$4" rc summary
  shift 4
  rm -rf "$prefix.xcresult"
  mkdir -p "$(dirname "$prefix")"
  xcodebuild test-without-building -project app/Meguri.xcodeproj -scheme Meguri -testPlan UI -configuration "$config" \
    -destination "$dest" -derivedDataPath "$MG_APP_DD" -resultBundlePath "$prefix.xcresult" "$@" >"$prefix.log" 2>&1
  rc=$?
  mg_artifact log "$prefix.log" "xcodebuild test ($platform)"
  summary="$(xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -r '"\(.result) passed=\(.passedTests) failed=\(.failedTests) skipped=\(.skippedTests)"')"
  echo "$MG_JOB: xcodebuild test ($platform): exit $rc ${summary:-no result}" >&2
  if grep -q 'failed to initialize for UI testing' "$prefix.log"; then
    mg_user_step uitest-automation missing true "Let the Xcode UI-test runner control the Mac" \
      "System Settings > Privacy & Security > Accessibility and Automation: allow the UI-test runner (docs/process/one-time-setup.md)."
    mg_failure blocked-uitest-automation "macOS has not allowed UI automation; the tests did not run"
    return 2
  fi
  [ "$rc" -eq 0 ] && return 0
  [ -n "$summary" ] && return 1
  mg_failure xcodebuild-test "xcodebuild test-without-building could not run (exit $rc); see $prefix.log"
  return 2
}

# ---------------------------------------------------------------------------
# Simulators from the pool

# mg_sim_lease <kind> <release|keep>: leases a pool device for this job; sets
# MG_SIM_UDID. With release, the device goes back to the pool (reset) when
# the job ends. On failure records it and returns 1.
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

# mg_sim_prepare <udid>: the pinned status bar (R1966), after every boot and
# reset; pool devices stay booted, so before every use.
mg_sim_prepare() {
  local args
  args="$(jq -r '.status_bar | to_entries[] | "--\(.key)\n\(.value)"' tools/sim-devices.json)"
  local a=() line
  while IFS= read -r line; do a[${#a[@]}]="$line"; done <<<"$args"
  xcrun simctl status_bar "$1" override "${a[@]}" >&2 || echo "$MG_JOB: could not override the status bar of $1" >&2
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
  [ -z "$udid" ] || mg_sim_check_owned "$udid" || return 0
  mg_app_build ios-sim "$MG_D_CONFIG" || return 0
  mg_host_lease || return 0
  if [ -z "$udid" ]; then
    mg_sim_lease "$MG_D_KIND" keep || return 0
    udid="$MG_SIM_UDID"
  fi
  mg_sim_prepare "$udid"
  app="$(mg_app_products ios-sim "$MG_D_CONFIG")/Meguri.app"
  if ! xcrun simctl install "$udid" "$app" >&2; then
    mg_failure install "simctl install failed on $udid"
    mg_status error
    return 0
  fi
  if ! out="$(xcrun simctl launch --terminate-running-process "$udid" "$MG_APP_DEV_ID" "${launch[@]}")"; then
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

# The Mac: a hidden background launch that must stay up for <seconds>, then
# quits (it holds the host lease meanwhile, so no other job launches the app).
mg_run__mac() {
  local config="$1" seconds="$2" app pid="" i
  shift 2
  mg_app_build mac "$config" || return 0
  mg_host_lease || return 0
  mg_app_mac_guard || return 0
  app="$(mg_app_products mac "$config")/Meguri.app"
  open -g -j -n "$app" --args "$@" >&2
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
    mg_sleep 1
  done
  mg_message "run: the Mac app ($config) ran hidden for $seconds s (pid $pid) and was quit"
  mg_status pass
}
