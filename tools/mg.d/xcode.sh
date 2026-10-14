# Xcode project checks (U7). Sourced by tools/mg; they run on the host
# (xcodebuild, codesign, the simulator pool).

mg_job xcode-check host mg_xcode_check \
  "Xcode project checks after lint-ui: build settings, zero-warning builds of every configuration (Mac, iPhone and iPad simulators), core stamp, entitlements, arm64, hook code, hidden Mac launches of every route, synced folders, test plans, DriveScriptTests, TestHooksTests (on the Mac only with MEGURI_ALLOW_MAC_UI=1, from tools/mg mac-ui); the launches, simulators and test runs wait for the host lease; args go to tools/tests/xcode_project_check.sh (--iphone UDID, --ipad UDID, else leased from the simulator pool; --no-ui)"

# Builds and logs stay in build/xcode-check/ between runs, so builds are
# incremental. The script runs in two phases: the builds and their checks
# first, then, holding the host lease (U8, tools/mg.d/host.sh) and with no
# other .dev Mac app running, the hidden Mac launches, the simulators and the
# test runs. Other worktrees' app jobs are not held up by the builds.
mg_xcode_check() {
  [ "${1:-}" = "--" ] && shift
  local tap="$MG_RUN_DIR/xcode-check.tap" rc devices_rc
  mg_lint_ui_first || return 0
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_status fail
    return 0
  fi
  /bin/bash tools/tests/xcode_project_check.sh build/xcode-check --phase builds ${1+"$@"} | tee "$tap"
  rc=${PIPESTATUS[0]}
  mg_artifact tap "$tap"
  mg_artifact logs build/xcode-check/logs "xcodebuild logs"
  if ! mg_host_lease || ! mg_app_mac_guard; then
    # The builds' results stand; the launches and test runs did not run.
    mg_tap_report "$tap" "$rc" ""
    mg_message "xcode-check: the builds were checked; the launches, simulators and test runs did not run (see the failures)"
    mg_status error
    return 0
  fi
  /bin/bash tools/tests/xcode_project_check.sh build/xcode-check --phase devices \
    --continue-from "$(grep -cE '^(not )?ok ' "$tap")" ${1+"$@"} | tee -a "$tap"
  devices_rc=${PIPESTATUS[0]}
  [ "$devices_rc" -eq 0 ] || rc="$devices_rc"
  mg_tap_report "$tap" "$rc" ""
}
