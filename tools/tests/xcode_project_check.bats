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
  STUBS="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$STUBS"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  : >"$STUB_LOG"
  # The pool leases POOL-<kind>; with STUB_POOL_BUSY=<kind>, every device of
  # that kind stays leased (exit 3, as sim-pool.sh does).
  cat >"$SB/tools/scripts/sim-pool.sh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "sim-pool.sh $*" >>"$STUB_LOG"
if [ "$1" = acquire ]; then
  [ "$2" != "${STUB_POOL_BUSY:-}" ] || { echo "sim-pool: every $2 device stayed leased" >&2 && exit 3; }
  echo "POOL-$2"
fi
exit 0
EOF
  chmod +x "$SB/tools/scripts/sim-pool.sh"
  unset STUB_POOL_BUSY
  # Every call is logged. Builds fail (pass with STUB_BUILD_OK=1), so no
  # product check runs; test runs pass. A launched app stays up, except on the
  # route stub/quits.
  cat >"$STUBS/_stub" <<'EOF'
#!/usr/bin/env bash
name="$(basename "$0")"
printf '%s\n' "$name $*" >>"$STUB_LOG"
case "$name $*" in
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

@test "xcode-check leases its simulators from the pool and releases them at exit" {
  check_project
  called '^sim-pool\.sh acquire iphone --holder xcode-check$'
  called '^sim-pool\.sh acquire ipad --holder xcode-check$'
  called '^xcodebuild build .* -destination id=POOL-iphone '
  called '^xcodebuild build .* -destination id=POOL-ipad '
  called '^xcodebuild test .* -destination id=POOL-iphone .*'
  called '^xcodebuild test .* -destination id=POOL-ipad .*'
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
  called '^xcodebuild build .* -destination id=POOL-ipad '
  called '^sim-pool\.sh release POOL-ipad$'
  [ "$(grep -c '^sim-pool\.sh release ' "$STUB_LOG")" -eq 1 ]
}

@test "xcode-check never leases, creates or deletes a simulator it is given" {
  check_project --no-ui --iphone PHONE-1 --ipad PAD-1
  not_called '^sim-pool\.sh '
  not_called '^xcrun simctl (create|boot|shutdown|erase|delete) '
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
