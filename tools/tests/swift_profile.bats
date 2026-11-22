#!/usr/bin/env bats
# Profile routing and stale-product guards; all Apple tools are fakes.
bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/app/Packages/MeguriCore"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/.gitignore" "$t/"
  echo '// synthetic package manifest' >"$t/app/Packages/MeguriCore/Package.swift"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo with spaces"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log" STUB_CALLS="$BATS_TEST_TMPDIR/calls.jsonl"
  : >"$STUB_LOG"
  : >"$STUB_CALLS"
  cat >"$BIN/sysctl" <<'EOF'
#!/bin/bash
[ "$*" = '-n machdep.cpu.brand_string' ] || exit 97
echo 'Apple M3 Max'
EOF
  cat >"$BIN/sw_vers" <<'EOF'
#!/bin/bash
case "$*" in
  -productVersion) echo '27.0' ;;
  -buildVersion) echo '26A123' ;;
  *) exit 97 ;;
esac
EOF
  cat >"$BIN/uname" <<'EOF'
#!/bin/bash
[ "$*" = -m ] || exit 97
echo arm64
EOF
  chmod +x "$BIN/sysctl" "$BIN/sw_vers" "$BIN/uname"
  cat >"$SB/tools/mg.d/zz-swift-profile-fakes.sh" <<'EOF'
mg_lint_ui_first() { echo lint >>"$STUB_LOG"; }
mg_host_lease() {
  echo "lease $*" >>"$STUB_LOG"
  [ "$*" = --no-session ] || return 1
  if [ "${FX_LEASE_FAIL:-0}" = 1 ]; then
    mg_failure host-busy 'synthetic occupied lease'
    mg_status error
    return 1
  fi
}
EOF
  cat >"$SB/tools/scripts/build-core-xcframework.sh" <<'EOF'
echo "core $*" >>"$STUB_LOG"
[ "${FX_CORE_FAIL:-0}" = 0 ] || exit 1
config=""; report=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --configuration) config="$2"; shift ;;
    --report) report="$2"; shift ;;
    *) exit 97 ;;
  esac
  shift
done
jq -nc --arg config "$config" '{configuration: $config}' >"$report"
EOF
  cat >"$BIN/swift" <<'EOF'
#!/bin/bash
echo "swift $1" >>"$STUB_LOG"
printf '%s\0' "$@" | jq -Rsc 'split("\u0000")[:-1]' >>"$STUB_CALLS"
action="$1"; shift
if [ "$action" = build ]; then exit "${FX_SWIFT_BUILD_RC:-0}"; fi
[ "$action" = test ] || exit 97
xunit=""
while [ "$#" -gt 0 ]; do
  case "$1" in --xunit-output) xunit="$2"; shift ;; esac
  shift
done
tests="${xunit%.xml}-swift-testing.xml"
if [ "${FX_SWIFT_TEST_RC:-0}" = 0 ]; then
  echo '<testsuite><testcase name="profileProbe"/></testsuite>' >"$tests"
else
  echo '<testsuite><testcase name="profileProbe"><failure/></testcase></testsuite>' >"$tests"
fi
exit "${FX_SWIFT_TEST_RC:-0}"
EOF
  chmod +x "$BIN/swift"
  # xmllint ships on the Mac where swift-test runs, not in docker:dev.
  # Read XML test reports with a local parser.
  cat >"$BIN/xmllint" <<'EOF'
#!/usr/bin/env python3
import sys
import xml.etree.ElementTree as ET
from xml.sax.saxutils import quoteattr

if len(sys.argv) != 4 or sys.argv[1] != "--xpath":
    sys.exit(97)
expression, path = sys.argv[2:]
cases = list(ET.parse(path).iter("testcase"))
failed = [case for case in cases if case.find("failure") is not None or case.find("error") is not None]
if expression == "count(//testcase)":
    print(len(cases))
elif expression == "count(//testcase[failure or error])":
    print(len(failed))
elif expression == "count(//testcase[skipped])":
    print(sum(case.find("skipped") is not None for case in cases))
elif expression == "//testcase[failure or error]/@name":
    print(" ".join("name=" + quoteattr(case.attrib["name"]) for case in failed))
else:
    sys.exit(97)
EOF
  chmod +x "$BIN/xmllint"
  export PATH="$BIN:$PATH"
}

teardown() {
  if [ "${BATS_TEST_COMPLETED:-0}" != 1 ]; then
    printf 'nested tools/mg status: %s\n' "${status:-unset}"
    [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, summary, failures, details}' "$result"
    [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 25
    [ ! -s "$STUB_LOG" ] || cat "$STUB_LOG"
  fi
}

mg() {
  run --separate-stderr "$SB/tools/mg" swift-test "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }

@test "swift profile routes matched core and optimized SPM products before taking the measurement lease" {
  mg --config Profile -- --filter 'DeckServiceTests/twoThousandDecks'
  [ "$status" -eq 0 ]
  [ "$(field .summary.passed)" -eq 1 ]
  [ "$(field .details.configuration.core)" = Profile ]
  [ "$(field .details.configuration.swift)" = release ]
  [ "$(field '.details.environment | tojson')" = '{"cpu":"Apple M3 Max","os":"macOS 27.0","os_build":"26A123","arch":"arm64"}' ]
  grep -Fx "core --configuration Profile --report $(dirname "$result")/xcframework.json" "$STUB_LOG"
  [ "$(sed -n '3,5p' "$STUB_LOG")" = "swift build
lease --no-session
swift test" ]
  jq -se --arg scratch "$SB/build/swift-test/MeguriCore/Profile" '
    .[0][:-2] == ["build", "--package-path", "app/Packages/MeguriCore", "--configuration", "release", "--scratch-path", $scratch, "-Xswiftc", "-enable-testing", "--build-tests"]
    and .[0][-2] == "-j"
    and .[1][0:9] == ["test", "--package-path", "app/Packages/MeguriCore", "--configuration", "release", "--scratch-path", $scratch, "-Xswiftc", "-enable-testing"]
    and .[1][9:11] == ["--skip-build", "--xunit-output"]
    and .[1][-2:] == ["--filter", "DeckServiceTests/twoThousandDecks"]' "$STUB_CALLS"
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "swift profile keeps default Debug products on their existing cache without a timing lease" {
  mg -- --filter profileProbe
  [ "$status" -eq 0 ]
  [ "$(field .details.configuration.core)" = Debug ]
  [ "$(field .details.configuration.swift)" = debug ]
  [ "$(field '.details | has("environment")')" = false ]
  grep -Fx "core --configuration Debug --report $(dirname "$result")/xcframework.json" "$STUB_LOG"
  ! grep -q '^lease ' "$STUB_LOG"
  jq -se 'all(.[]; (index("--scratch-path") == null) and (index("-Xswiftc") == null) and (index("--configuration") as $i | .[$i + 1] == "debug"))' "$STUB_CALLS"
}

@test "swift profile rejects configuration cache and target overrides before building" {
  local option
  for option in --configuration -c --scratch-path --build-path --package-path --triple --sdk --swift-sdk --arch --destination --sanitize -Xswiftc -Xlinker; do
    mg --config Profile -- "$option" ignored
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
    [ ! -s "$STUB_LOG" ]
  done
  for option in --configuration=debug -cdebug --scratch-path=/tmp/stale --package-path=/tmp/stale --triple=wrong --enable-code-coverage --parallel --list-tests; do
    mg --config Profile -- "$option"
    [ "$status" -eq 2 ]
    [ ! -s "$STUB_LOG" ]
  done
}

@test "swift profile passes only complete filter and skip selections to the prebuilt runner" {
  mg --config Profile -- --filter=DeckServiceTests --skip slow --filter exact
  [ "$status" -eq 0 ]
  jq -se '.[1][-5:] == ["--filter=DeckServiceTests", "--skip", "slow", "--filter", "exact"]' "$STUB_CALLS"
  local option
  for option in --filter --skip --filter= --skip=; do
    : >"$STUB_LOG"
    mg --config Profile -- "$option"
    [ "$status" -eq 2 ]
    [ ! -s "$STUB_LOG" ]
  done
  mg --config Profile -- --filter --configuration debug
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

@test "swift profile rejects unsupported and missing modes without invoking tools" {
  mg --config Release
  [ "$status" -eq 2 ]
  mg --config
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

@test "swift profile build failures never run or lease preexisting products" {
  FX_CORE_FAIL=1 mg --config Profile
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = xcframework ]
  ! grep -Eq '^(swift|lease) ' "$STUB_LOG"
  : >"$STUB_LOG"
  FX_SWIFT_BUILD_RC=42 mg --config Profile
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = swift-build ]
  grep -Fx 'swift build' "$STUB_LOG"
  ! grep -Eq '^(swift test|lease)' "$STUB_LOG"
}

@test "swift profile test failure remains failed and its repro preserves Profile" {
  FX_SWIFT_TEST_RC=1 mg --config Profile
  [ "$status" -eq 1 ]
  [ "$(field .summary.failed)" -eq 1 ]
  [ "$(field '.failures[0].id')" = test/profileProbe ]
  [ "$(field '.failures[0].repro')" = "tools/mg swift-test --package MeguriCore --config Profile -- --filter 'profileProbe'" ]
  [ "$(grep -c '^swift test$' "$STUB_LOG")" -eq 1 ]
}

@test "swift profile lease failure stops before executing optimized tests" {
  FX_LEASE_FAIL=1 mg --config Profile
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = host-busy ]
  grep -Fx 'swift build' "$STUB_LOG"
  ! grep -q '^swift test$' "$STUB_LOG"
}
