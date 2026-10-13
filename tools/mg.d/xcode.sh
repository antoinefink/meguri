# Xcode project checks (U7). Sourced by tools/mg; they run on the host
# (xcodebuild, simctl, codesign).

mg_job xcode-check host mg_xcode_check \
  "Xcode project checks: build settings, zero-warning builds of every configuration (Mac, iPhone and iPad simulators), core stamp, entitlements, arm64, hook code, synced folders, test plans, TestHooksTests; args go to tools/tests/xcode_project_check.sh (--iphone UDID, --ipad UDID, --no-ui)"

# Builds and logs stay in build/xcode-check/ between runs, so builds are
# incremental.
mg_xcode_check() {
  [ "${1:-}" = "--" ] && shift
  local tap="$MG_RUN_DIR/xcode-check.tap" rc
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_status fail
    return 0
  fi
  /bin/bash tools/tests/xcode_project_check.sh build/xcode-check ${1+"$@"} | tee "$tap"
  rc=${PIPESTATUS[0]}
  mg_artifact tap "$tap"
  mg_artifact logs build/xcode-check/logs "xcodebuild logs"
  mg_tap_report "$tap" "$rc" ""
}
