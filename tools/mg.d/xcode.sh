# Xcode project checks (U7). Sourced by tools/mg; they run on the host
# (xcodebuild, codesign), with the simulator pool in the macOS test VM (U8c).

mg_job xcode-check host mg_xcode_check \
  "Xcode project checks after lint-ui: build settings, zero-warning builds of every configuration (Mac, iPhone and iPad simulators), core stamp, entitlements, arm64, hook code, hidden Mac launches of every route, synced folders, test plans, DriveScriptTests, TestHooksTests (simulators in the macOS test VM, queued without it; on the Mac only with MEGURI_ALLOW_MAC_UI=1, from tools/mg mac-ui); the launches, simulators and test runs wait for the VM's and the host lease; args go to tools/tests/xcode_project_check.sh (--iphone UDID, --ipad UDID, else leased from the simulator pool; --no-ui)"

# Builds and logs stay in build/xcode-check/ between runs, so builds are
# incremental. The script runs in two phases: the builds and their checks
# first, then, holding the test VM's lease (the simulators, mac-vm.sh) and the
# host lease (U8, tools/mg.d/host.sh), and with no other .dev Mac app running,
# the hidden Mac launches, the simulators and the test runs. Other worktrees'
# app jobs are not held up by the builds. The VM's lease is taken before the
# host's, as every job that takes both does. Without the VM the simulator
# runs are queued (--no-sims): the run is warn at best.
mg_xcode_check() {
  [ "${1:-}" = "--" ] && shift
  local tap="$MG_RUN_DIR/xcode-check.tap" rc devices_rc sims=() queued="" begin
  mg_lint_ui_first || return 0
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_status fail
    return 0
  fi
  mg_sim_begin
  begin=$?
  case "$begin" in
    0) [ "$MG_SIM_RUNNER" != vm ] || sims=(--vm-ssh "$(jq -r .ssh <<<"$MG_MAC_VM")" --vm-name "$(jq -r .name <<<"$MG_MAC_VM")") ;;
    2) queued=1 sims=(--no-sims) ;;
    *) return 0 ;;
  esac
  /bin/bash tools/tests/xcode_project_check.sh build/xcode-check --phase builds ${1+"$@"} | tee "$tap"
  rc=${PIPESTATUS[0]}
  mg_artifact tap "$tap"
  mg_artifact logs build/xcode-check/logs "xcodebuild logs"
  if { [ "$MG_SIM_RUNNER" = vm ] && ! mg_sim_take --no-copy; } || ! mg_host_lease || ! mg_app_mac_guard; then
    # The builds' results stand; the launches and test runs did not run.
    mg_tap_report "$tap" "$rc" ""
    mg_message "xcode-check: the builds were checked; the launches, simulators and test runs did not run (see the failures)"
    mg_status error
    return 0
  fi
  /bin/bash tools/tests/xcode_project_check.sh build/xcode-check --phase devices \
    --continue-from "$(grep -cE '^(not )?ok ' "$tap")" ${sims[0]+"${sims[@]}"} ${1+"$@"} | tee -a "$tap"
  devices_rc=${PIPESTATUS[0]}
  [ "$devices_rc" -eq 0 ] || rc="$devices_rc"
  mg_tap_report "$tap" "$rc" ""
  if [ -n "$queued" ] && [ "$(cat "$MG_STATE/status")" = pass ]; then
    mg_message "xcode-check: passed without the simulator runs, which are queued: $MG_MAC_VM_REASON (user step mac-vm)"
    mg_status warn
  fi
}
