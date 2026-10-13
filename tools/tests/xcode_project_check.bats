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
  # Every call is logged. Builds fail (pass with STUB_BUILD_OK=1), so no
  # product check runs; test runs pass. A launched app stays up, except on the
  # route stub/quits.
  cat >"$STUBS/_stub" <<'EOF'
#!/usr/bin/env bash
name="$(basename "$0")"
printf '%s\n' "$name $*" >>"$STUB_LOG"
case "$name $*" in
  "xcrun simctl create "*) echo "UDID-$3" ;;
  "xcrun xcresulttool get build-results "*) echo '{"errorCount":0,"warningCount":0,"warnings":[]}' ;;
  "xcrun xcresulttool get "*) echo '{"result":"Passed","passedTests":1,"failedTests":0,"skippedTests":0}' ;;
  "xcodebuild build "*) [ "${STUB_BUILD_OK:-}" = 1 ] || exit 65 ;;
  "open "*"-MeguriRoute stub/quits"*) ;;
  "open "*) exec sleep 30 ;;
esac
exit 0
EOF
  chmod +x "$STUBS/_stub"
  local c
  for c in xcrun xcodebuild codesign lipo segedit pluginkit plutil xmllint open; do
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
  called "^open -g -j -n -W $app --args -ApplePersistenceIgnoreState YES\$"
  called "^open -g -j -n -W $app --args -ApplePersistenceIgnoreState YES -MeguriRoute stub/stays\$"
  called "^open -g -j -n -W $app --args -ApplePersistenceIgnoreState YES -MeguriRoute stub/quits\$"
  [ "$(grep -c '^open ' "$STUB_LOG")" -eq 3 ]
  tap_line 'ok [0-9]+ mac-Debug launched hidden stays up: Home'
  tap_line 'ok [0-9]+ mac-Debug launched hidden stays up: route stub/stays'
  tap_line 'not ok [0-9]+ mac-Debug launched hidden stays up: route stub/quits'
  tap_line '# the app quit within 3 s of a hidden launch \(open -W exit 0\)'
}
