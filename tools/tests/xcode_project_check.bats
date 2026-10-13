#!/usr/bin/env bats
# Control flow of tools/tests/xcode_project_check.sh (tools/mg xcode-check),
# run by `tools/mg selftest` with the host tools stubbed: which simulators it
# creates and deletes, and which test runs it starts. The checks themselves
# run on the host, through `tools/mg xcode-check`.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$SB/tools/tests" "$SB/app/Meguri/App"
  cp "$REPO_ROOT/tools/tests/xcode_project_check.sh" "$SB/tools/tests/"
  STUBS="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$STUBS"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  : >"$STUB_LOG"
  # Every call is logged. Builds fail, so no product check runs; test runs pass.
  cat >"$STUBS/_stub" <<'EOF'
#!/usr/bin/env bash
name="$(basename "$0")"
printf '%s\n' "$name $*" >>"$STUB_LOG"
case "$name $1 $2" in
  "xcrun simctl create") echo "UDID-$3" ;;
  "xcrun xcresulttool get") echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' ;;
  "xcodebuild build "*) exit 65 ;;
esac
exit 0
EOF
  chmod +x "$STUBS/_stub"
  local c
  for c in xcrun xcodebuild codesign lipo segedit pluginkit plutil xmllint; do
    ln -s _stub "$STUBS/$c"
  done
  export PATH="$STUBS:$PATH"
  unset MEGURI_ALLOW_MAC_UI
}

check_project() {
  run --separate-stderr /bin/bash "$SB/tools/tests/xcode_project_check.sh" "$BATS_TEST_TMPDIR/work" "$@"
}

called() { grep -qE "$1" "$STUB_LOG" || { echo "no call matches: $1" && cat "$STUB_LOG" && return 1; }; }
not_called() { ! grep -E "$1" "$STUB_LOG"; }
tap_line() { grep -qxE "$1" <<<"$output" || { echo "no TAP line matches: $1" && echo "$output" && return 1; }; }

@test "xcode-check deletes the simulators it created, at exit" {
  check_project --no-ui
  called '^xcrun simctl create Meguri-check-[0-9]+-iPhone17Pro com\.apple\.CoreSimulator\.SimDeviceType\.iPhone-17-Pro '
  called '^xcrun simctl create Meguri-check-[0-9]+-iPadPro13 '
  local name
  for name in $(sed -n 's/^xcrun simctl create \([^ ]*\) .*/\1/p' "$STUB_LOG"); do
    called "^xcrun simctl delete UDID-$name\$"
  done
  [ "$(grep -c '^xcrun simctl delete ' "$STUB_LOG")" -eq 2 ]
}

@test "xcode-check never creates or deletes the simulators it is given" {
  check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  not_called '^xcrun simctl (create|delete|shutdown) '
  called '^xcodebuild build .* -destination id=PHONE-1 '
  called '^xcodebuild build .* -destination id=PAD-1 '
}

@test "xcode-check queues the Mac UI tests for tools/mg mac-ui" {
  check_project --iphone PHONE-1 --ipad PAD-1
  tap_line 'ok [0-9]+ mac-Debug-TestHooksTests # SKIP queued for tools/mg mac-ui'
  not_called '^xcodebuild test .* -destination platform=macOS,arch=arm64 .*MeguriUITests'
  # The in-process Mac runs and the simulator UI runs still happen.
  called '^xcodebuild test .* -testPlan Unit .* -destination platform=macOS,arch=arm64 '
  called '^xcodebuild test .* -testPlan Snapshot .* -destination platform=macOS,arch=arm64 '
  called '^xcodebuild test .* -destination id=PHONE-1 .* -only-testing:MeguriUITests/TestHooksTests$'
}

@test "with MEGURI_ALLOW_MAC_UI=1 xcode-check runs the Mac UI tests" {
  MEGURI_ALLOW_MAC_UI=1 check_project --iphone PHONE-1 --ipad PAD-1
  tap_line 'ok [0-9]+ mac-Debug-TestHooksTests: Passed .*'
  called '^xcodebuild test -project .* -scheme Meguri -testPlan UI -configuration Debug -destination platform=macOS,arch=arm64 .* -only-testing:MeguriUITests/TestHooksTests$'
}
