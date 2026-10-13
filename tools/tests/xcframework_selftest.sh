#!/bin/bash
# Self-tests for tools/scripts/build-core-xcframework.sh, run by
# `tools/mg selftest-xcframework` on the host; prints TAP.
#
# usage: xcframework_selftest.sh <sandbox dir> <cargo target dir>
# The sandbox is recreated from the checkout's core/, tools/scripts/ and
# app/Packages/MeguriCore/ (tracked and untracked, not ignored) in its own git
# repo, so the tests can edit Rust sources freely. The cargo target dir is
# kept between runs so dependencies build once.

set -u

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
rm -rf "$1"
mkdir -p "$1" "$2"
SB="$(cd "$1" && pwd -P)"
CARGO_TARGET_DIR="$(cd "$2" && pwd -P)"
export CARGO_TARGET_DIR
SCRIPT="$SB/tools/scripts/build-core-xcframework.sh"
PKG="$SB/app/Packages/MeguriCore"
XCF="$PKG/Binaries/MeguriCoreFFI.xcframework"
STAMP="$PKG/Binaries/.stamp"
BINDINGS="$PKG/Sources/MeguriCoreBindings/MeguriCoreBindings.swift"
FFI_LIB="$SB/core/crates/meguri-ffi/src/lib.rs"
REAL_CARGO="$(command -v cargo || echo "$HOME/.cargo/bin/cargo")"
SHIM="$SB/.shim"
CARGO_LOG="$SB/.cargo-calls.log"

setup_sandbox() {
  (cd "$REPO" && git ls-files -co --exclude-standard -- core tools/scripts app/Packages/MeguriCore .gitignore) >"$SB/.files" &&
    # -m: files get the current mtime. cargo's freshness check compares
    # mtimes, and the kept target dir may hold newer builds of older edits.
    (cd "$REPO" && tar -cf - -T "$SB/.files") | tar -xmf - -C "$SB" &&
    rm -f "$SB/.files" &&
    git -C "$SB" init -q &&
    git -C "$SB" add -A &&
    git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm sandbox &&
    mkdir -p "$SHIM" &&
    printf '#!/bin/bash\necho "cargo $*" >>"%s"\nexec "%s" "$@"\n' "$CARGO_LOG" "$REAL_CARGO" >"$SHIM/cargo" &&
    chmod +x "$SHIM/cargo"
}

# build <args>: run the script through the cargo shim. Sets RC and MS (wall
# time of the script alone), and appends its output to $SB/build.log.
build() {
  : >"$CARGO_LOG"
  MS="$(PATH="$SHIM:$PATH" perl -MTime::HiRes=time -e '
    my $log = shift;
    open(my $out, ">&", \*STDOUT) or die;
    open(STDOUT, ">>", $log) or die;
    open(STDERR, ">>", $log) or die;
    my $t = time; system(@ARGV); my $rc = $? >> 8;
    printf $out "%d %d\n", (time - $t) * 1000, $rc;' -- "$SB/build.log" /bin/bash "$SCRIPT" "$@")"
  RC="${MS#* }"
  MS="${MS% *}"
}

cargo_calls() { grep -c . "$CARGO_LOG" 2>/dev/null || true; }

# Path and mtime (nanoseconds) of every output file, sorted.
snapshot() {
  find "$PKG/Binaries" "$PKG/Sources/MeguriCoreBindings" -type f -print0 2>/dev/null |
    xargs -0 stat -f '%N %Fm' | sort
}

mtime() { stat -f '%Fm' "$1"; }

slice_dirs() { find "$XCF" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort | tr '\n' ' ' | sed 's/ $//'; }

expect() { # expect <description> <actual> <expected>
  if [ "$2" != "$3" ]; then
    echo "$1: got '$2', want '$3'"
    return 1
  fi
}

t_builds_mac_and_sim_by_default() {
  build
  expect "exit code" "$RC" 0 &&
    expect "slices" "$(slice_dirs)" "ios-arm64-simulator macos-arm64" &&
    [ -f "$BINDINGS" ] &&
    [ -f "$XCF/macos-arm64/Headers/MeguriCoreFFI/MeguriCoreFFI.h" ] &&
    [ -f "$XCF/macos-arm64/Headers/MeguriCoreFFI/module.modulemap" ] &&
    grep -q '^module MeguriCoreFFI ' "$XCF/macos-arm64/Headers/MeguriCoreFFI/module.modulemap" &&
    grep -q 'import MeguriCoreFFI' "$BINDINGS" &&
    [ -f "$STAMP" ]
}

t_no_change_run_skips_cargo_and_keeps_mtimes() {
  local before after
  before="$(snapshot)"
  build
  after="$(snapshot)"
  expect "exit code" "$RC" 0 &&
    expect "cargo invocations" "$(cargo_calls)" 0 &&
    expect "output mtimes" "$after" "$before" &&
    { [ "$MS" -lt 100 ] || { echo "no-change run took $MS ms (budget 100 ms)"; false; }; }
}

t_private_change_replaces_only_the_library() {
  local lib="$XCF/macos-arm64/libmeguri_ffi.a" header="$XCF/macos-arm64/Headers/MeguriCoreFFI/MeguriCoreFFI.h"
  local lib_before swift_before header_before plist_before old
  old="$(mktemp)"
  cp "$lib" "$old"
  lib_before="$(mtime "$lib")"
  swift_before="$(mtime "$BINDINGS")"
  header_before="$(mtime "$header")"
  plist_before="$(mtime "$XCF/Info.plist")"
  # A private function body changes; the exported surface does not.
  perl -0pi -e 's/(fn target_name\(\) -> String \{\n)/$1    std::hint::black_box(0x5e1f_7e57_u64);\n/' "$FFI_LIB"
  grep -q 0x5e1f_7e57 "$FFI_LIB" || {
    echo "could not edit target_name() in the sandbox"
    return 1
  }
  build
  expect "exit code" "$RC" 0 &&
    expect "slices" "$(slice_dirs)" "ios-arm64 ios-arm64-simulator macos-arm64" &&
    { [ "$(cargo_calls)" -gt 0 ] || { echo "cargo did not run"; false; }; } &&
    { ! cmp -s "$old" "$lib" || { echo "libmeguri_ffi.a bytes did not change"; false; }; } &&
    { [ "$(mtime "$lib")" != "$lib_before" ] || { echo ".a mtime unchanged"; false; }; } &&
    expect "bindings mtime" "$(mtime "$BINDINGS")" "$swift_before" &&
    expect "header mtime" "$(mtime "$header")" "$header_before" &&
    expect "Info.plist mtime" "$(mtime "$XCF/Info.plist")" "$plist_before"
  local rc=$?
  rm -f "$old"
  return $rc
}

t_xcode_device_preaction_adds_the_device_slice_and_links() {
  # What the scheme pre-action sees when building for an iOS device: Xcode's
  # build settings in the environment, including an iOS SDKROOT.
  PLATFORM_NAME=iphoneos CONFIGURATION=Debug SDKROOT="$(xcrun --sdk iphoneos --show-sdk-path)" \
    IPHONEOS_DEPLOYMENT_TARGET=27.0 build --xcode
  expect "exit code" "$RC" 0 &&
    expect "slices" "$(slice_dirs)" "ios-arm64 ios-arm64-simulator macos-arm64" || return 1
  PLATFORM_NAME=iphoneos CONFIGURATION=Debug build --xcode
  expect "second pre-action exit code" "$RC" 0 &&
    expect "second pre-action cargo invocations" "$(cargo_calls)" 0 || return 1
  # The device link: the package's test bundle links the device slice and the
  # bindings generated from the Mac slice.
  local log="$SB/device-link.log"
  if ! (cd "$PKG" && xcodebuild build-for-testing -scheme MeguriCore -destination 'generic/platform=iOS' \
    -derivedDataPath "$SB/DerivedData" CODE_SIGNING_ALLOWED=NO) >"$log" 2>&1; then
    echo "iOS device build-for-testing failed:"
    grep -E 'error:|Undefined symbol|ld:' "$log" | head -n 20
    return 1
  fi
  local bundle
  bundle="$(find "$SB/DerivedData/Build/Products/Debug-iphoneos" -name 'CoreBridgeTests.xctest' -type d | head -n 1)"
  [ -n "$bundle" ] || {
    echo "no CoreBridgeTests.xctest for iphoneos"
    return 1
  }
  expect "linked platform" "$(xcrun vtool -show-build "$bundle/CoreBridgeTests" 2>/dev/null | awk '/platform/ { print $2; exit }')" IOS
}

t_one_slice_only_export_fails_before_the_xcframework_is_replaced() {
  local before stamp_before
  before="$(snapshot)"
  stamp_before="$(cat "$STAMP")"
  cp "$FFI_LIB" "$SB/.lib.rs.orig"
  cat >>"$FFI_LIB" <<'EOF'

#[cfg(target_os = "macos")]
#[uniffi::export]
pub fn mac_only_probe() -> Result<(), CoreError> {
    callback::enter("mac_only_probe");
    Ok(())
}
EOF
  build --slices mac,sim
  local rc=$RC after
  after="$(snapshot)"
  # cp, not mv: the restored file needs a new mtime, or cargo keeps the build
  # that has the probe.
  cp "$SB/.lib.rs.orig" "$FFI_LIB" && rm -f "$SB/.lib.rs.orig"
  { [ "$rc" -ne 0 ] || { echo "the script passed with a Mac-only export"; false; }; } &&
    { grep -q 'mac_only_probe' "$SB/build.log" || { echo "the failure does not name the mismatched symbol"; false; }; } &&
    expect "outputs after the failed run" "$after" "$before" &&
    expect "stamp after the failed run" "$(cat "$STAMP")" "$stamp_before"
}

t_appstore_configuration_has_no_test_exports() {
  build --configuration AppStore --slices mac
  expect "exit code" "$RC" 0 &&
    expect "slices" "$(slice_dirs)" "macos-arm64" &&
    { ! grep -q 'func testPanic' "$BINDINGS" || { echo "AppStore bindings contain test exports"; false; }; } &&
    { ! nm -gUj "$XCF/macos-arm64/libmeguri_ffi.a" 2>/dev/null | grep -q '_uniffi_meguri_ffi_fn_func_test_' ||
      { echo "AppStore library exports test hooks"; false; }; } &&
    { ! grep -q 'test-hooks\|ankiweb' "$STAMP" || { echo "AppStore features in the stamp: $(grep features "$STAMP")"; false; }; } || return 1
  build --configuration Debug
  expect "Debug rebuild exit code" "$RC" 0 &&
    grep -q 'func testPanic' "$BINDINGS"
}

# Order matters: each test starts from the outputs the previous one left. The
# private change runs with all three slices, whose Info.plist xcodebuild
# would otherwise write in a random library order.
TESTS="t_builds_mac_and_sim_by_default
t_no_change_run_skips_cargo_and_keeps_mtimes
t_xcode_device_preaction_adds_the_device_slice_and_links
t_private_change_replaces_only_the_library
t_one_slice_only_export_fails_before_the_xcframework_is_replaced
t_appstore_configuration_has_no_test_exports"

n="$(printf '%s\n' "$TESTS" | grep -c .)"
echo "1..$n"
if ! setup_sandbox >"$SB/setup.log" 2>&1; then
  i=0
  for t in $TESTS; do
    i=$((i + 1))
    echo "not ok $i $t"
    echo "# sandbox setup failed: $(tail -n 3 "$SB/setup.log" | tr '\n' ' ')"
  done
  exit 1
fi
i=0
failed=0
for t in $TESTS; do
  i=$((i + 1))
  out="$("$t" 2>&1)"
  if [ $? -eq 0 ]; then
    echo "ok $i $t"
  else
    failed=$((failed + 1))
    echo "not ok $i $t"
    printf '%s\n' "$out" | sed 's/^/# /'
    echo "# build log tail:"
    tail -n 15 "$SB/build.log" | sed 's/^/#   /'
  fi
done
[ "$failed" -eq 0 ]
