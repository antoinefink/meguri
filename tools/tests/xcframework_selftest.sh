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
# A test that was stopped may have left a read-only directory behind.
[ ! -d "$1" ] || chmod -R u+w "$1"
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
  LOG_START="$(wc -c <"$SB/build.log" 2>/dev/null || echo 0)"
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

# The output of the last build only.
run_log() { tail -c +"$((LOG_START + 1))" "$SB/build.log"; }

# test_hooks_in <library>: 0 when the library exports a test hook, 1 when it
# exports uniffi symbols and no test hook, 2 when nm fails or finds no uniffi
# symbol (a scan that proves nothing). Same nm flags as the build script.
test_hooks_in() {
  local syms="$SB/.nm.out"
  nm --no-llvm-bc -gUj "$1" >"$syms" 2>"$SB/.nm.err" || {
    echo "nm failed on $1: $(head -n 3 "$SB/.nm.err")"
    return 2
  }
  grep -q '^_uniffi_' "$syms" || {
    echo "no uniffi symbol in $1"
    return 2
  }
  grep -q '^_uniffi_meguri_ffi_fn_func_test_' "$syms" && return 0
  return 1
}

# Path and mtime (nanoseconds) of every output file, sorted.
snapshot() {
  find "$PKG/Binaries" "$PKG/Sources/MeguriCoreBindings" -type f ! -name .lock -print0 2>/dev/null |
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
  local before after times="" best _
  before="$(snapshot)"
  build
  after="$(snapshot)"
  expect "exit code" "$RC" 0 &&
    expect "cargo invocations" "$(cargo_calls)" 0 &&
    expect "output mtimes" "$after" "$before" || return 1
  # The budget is the script's own cost: the fastest of three runs, so one
  # spike from other processes on the Mac does not fail it.
  best=$MS
  times=" $MS"
  for _ in 1 2; do
    build
    times="$times $MS"
    [ "$MS" -ge "$best" ] || best=$MS
  done
  [ "$best" -lt 100 ] || { echo "no-change runs took$times ms (budget 100 ms)"; false; }
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

t_swift_reader_repair_is_hashed_without_changing_output_bytes() {
  local helper="$SB/tools/scripts/repair-swift-integer-reader.py" before failed=0
  before="$(snapshot | grep -vF "$STAMP ")"
  cp "$helper" "$SB/.reader-repair.orig"
  printf '\n# input hash probe\n' >>"$helper"
  build
  expect "exit code after editing the repair helper" "$RC" 0 &&
    { [ "$(cargo_calls)" -gt 0 ] || { echo "editing the repair helper was a cache hit"; false; }; } &&
    expect "output mtimes after a comment-only repair edit" "$(snapshot | grep -vF "$STAMP ")" "$before" || failed=1
  cp "$SB/.reader-repair.orig" "$helper"
  rm -f "$SB/.reader-repair.orig"
  return "$failed"
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
  if ! (cd "$PKG" && xcodebuild build-for-testing -scheme MeguriCore-Package -destination 'generic/platform=iOS' \
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

t_cfg_gated_signature_or_record_field_fails_before_the_xcframework_is_replaced() {
  # Same symbol names on both slices, different signature or record layout:
  # only the bindings generated from each slice tell them apart.
  local before stamp_before probe rc after failed=0
  before="$(snapshot)"
  stamp_before="$(cat "$STAMP")"
  cp "$FFI_LIB" "$SB/.lib.rs.orig"
  for probe in signature record; do
    cp "$SB/.lib.rs.orig" "$FFI_LIB"
    if [ "$probe" = signature ]; then
      cat >>"$FFI_LIB" <<'EOF'

#[cfg(target_os = "macos")]
#[uniffi::export]
pub fn platform_probe(value: u32) -> Result<u32, CoreError> {
    callback::enter("platform_probe");
    Ok(value)
}

#[cfg(not(target_os = "macos"))]
#[uniffi::export]
pub fn platform_probe(value: String) -> Result<u32, CoreError> {
    callback::enter("platform_probe");
    Ok(value.len() as u32)
}
EOF
    else
      cat >>"$FFI_LIB" <<'EOF'

#[derive(uniffi::Record)]
pub struct PlatformProbe {
    #[cfg(target_os = "macos")]
    pub mac_only_field: u32,
    pub shared_field: String,
}
EOF
    fi
    build --slices mac,sim
    rc=$RC
    after="$(snapshot)"
    { [ "$rc" -ne 0 ] || { echo "the script passed with a cfg-gated $probe"; false; }; } &&
      { run_log | grep -q 'platformProbe\|macOnlyField' || { echo "the $probe failure does not show the difference"; false; }; } &&
      expect "outputs after the failed $probe run" "$after" "$before" &&
      expect "stamp after the failed $probe run" "$(cat "$STAMP")" "$stamp_before" || {
      failed=1
      break
    }
  done
  # cp, not mv: the restored file needs a new mtime.
  cp "$SB/.lib.rs.orig" "$FFI_LIB" && rm -f "$SB/.lib.rs.orig"
  return $failed
}

t_missing_header_or_modulemap_is_rebuilt() {
  # Every rebuild rewrites the stamp; every other output keeps its mtime.
  local f before after
  for f in macos-arm64/Headers/MeguriCoreFFI/module.modulemap ios-arm64-simulator/Headers/MeguriCoreFFI/MeguriCoreFFI.h; do
    before="$(snapshot | grep -vF -e "$XCF/$f " -e "$STAMP ")"
    rm -f "$XCF/$f"
    build
    after="$(snapshot | grep -vF -e "$XCF/$f " -e "$STAMP ")"
    expect "exit code after deleting $f" "$RC" 0 &&
      { [ "$(cargo_calls)" -gt 0 ] || { echo "the run after deleting $f was a cache hit"; false; }; } &&
      { [ -f "$XCF/$f" ] || { echo "$f was not restored"; false; }; } &&
      expect "the other outputs after restoring $f" "$after" "$before" || return 1
  done
}

t_contents_of_files_with_quoted_names_are_hashed() {
  # git ls-files quotes these names (non-ASCII, tab); the content of every
  # input must still reach the hash.
  local dir="$SB/core/crates/meguri-ffi" tab f failed=0
  tab="$(printf 'tab\tname.txt')"
  printf 'one\n' >"$dir/é.txt"
  printf 'one\n' >"$dir/$tab"
  printf 'one\n' >"$dir/plain-probe.txt"
  build
  expect "exit code with quoted names" "$RC" 0 &&
    { run_log | { ! grep 'fatal:'; } || { echo "git failed while hashing"; false; }; } || failed=1
  if [ "$failed" = 0 ]; then
    build
    expect "no-change cargo invocations" "$(cargo_calls)" 0 || failed=1
  fi
  for f in "é.txt" "$tab" plain-probe.txt; do
    [ "$failed" = 0 ] || break
    printf 'two\n' >"$dir/$f"
    build
    expect "exit code after editing '$f'" "$RC" 0 &&
      { [ "$(cargo_calls)" -gt 0 ] || { echo "editing the contents of '$f' was a cache hit"; false; }; } || failed=1
  done
  rm -f "$dir/é.txt" "$dir/$tab" "$dir/plain-probe.txt"
  return $failed
}

t_unreadable_input_fails_the_run_before_cargo() {
  local probe="$SB/core/crates/meguri-ffi/unreadable-probe.txt" before rc after
  before="$(snapshot)"
  printf 'secret\n' >"$probe"
  chmod 000 "$probe"
  build
  rc=$RC
  after="$(snapshot)"
  rm -f "$probe"
  { [ "$rc" -ne 0 ] || { echo "the run passed although it could not hash an input"; false; }; } &&
    expect "cargo invocations" "$(cargo_calls)" 0 &&
    expect "outputs after the failed run" "$after" "$before"
}

t_failed_output_copy_fails_and_forces_the_next_build() {
  local slice="$XCF/macos-arm64" rc
  perl -0pi -e 's/(fn target_name\(\) -> String \{\n)/$1    std::hint::black_box(0x0c0f_fa11_u64);\n/' "$FFI_LIB"
  grep -q 0x0c0f_fa11 "$FFI_LIB" || {
    echo "could not edit target_name() in the sandbox"
    return 1
  }
  # The new library cannot be written next to the old one.
  chmod a-w "$slice"
  build
  rc=$RC
  chmod u+w "$slice"
  { [ "$rc" -ne 0 ] || { echo "the run passed although it could not replace macos-arm64/libmeguri_ffi.a"; false; }; } || return 1
  build
  expect "exit code of the next run" "$RC" 0 &&
    { [ "$(cargo_calls)" -gt 0 ] || { echo "the next run was a cache hit over the stale library"; false; }; } &&
    { cmp -s "$slice/libmeguri_ffi.a" "$CARGO_TARGET_DIR/aarch64-apple-darwin/debug/libmeguri_ffi.a" ||
      { echo "the packaged Mac library is not the one cargo built"; false; }; }
}

t_appstore_configuration_has_no_test_exports() {
  local hooks
  build --configuration AppStore --slices mac
  expect "exit code" "$RC" 0 &&
    expect "slices" "$(slice_dirs)" "macos-arm64" &&
    { ! grep -q 'func testPanic' "$BINDINGS" || { echo "AppStore bindings contain test exports"; false; }; } &&
    { ! grep -q 'test-hooks\|ankiweb' "$STAMP" || { echo "AppStore features in the stamp: $(grep features "$STAMP")"; false; }; } || return 1
  test_hooks_in "$XCF/macos-arm64/libmeguri_ffi.a"
  hooks=$?
  expect "test-hook scan of the AppStore library (0 hooks, 1 none, 2 no scan)" "$hooks" 1 || return 1
  # The scan proves nothing when nm cannot read its input.
  test_hooks_in "$BINDINGS" >/dev/null
  expect "test-hook scan of a file nm cannot read" "$?" 2 || return 1
  build --configuration Debug
  expect "Debug rebuild exit code" "$RC" 0 &&
    grep -q 'func testPanic' "$BINDINGS" || return 1
  test_hooks_in "$XCF/macos-arm64/libmeguri_ffi.a"
  expect "test-hook scan of the Debug library" "$?" 0
}

# outputs_match_stamp: the packaged library and bindings have test hooks
# exactly when the stamp's configuration has them.
outputs_match_stamp() {
  local configuration want hooks
  configuration="$(sed -n 's/^configuration=//p' "$STAMP")"
  case "$configuration" in
    Debug | Profile) want=0 ;;
    *) want=1 ;;
  esac
  test_hooks_in "$XCF/macos-arm64/libmeguri_ffi.a"
  hooks=$?
  expect "test-hook scan of the $configuration library (0 hooks, 1 none, 2 no scan)" "$hooks" "$want" || return 1
  if [ "$want" = 0 ]; then
    grep -q 'func testPanic' "$BINDINGS" || { echo "$configuration bindings lack test exports"; return 1; }
  else
    ! grep -q 'func testPanic' "$BINDINGS" || { echo "$configuration bindings contain test exports"; return 1; }
  fi
}

t_concurrent_appstore_and_profile_runs_do_not_mix_artifacts() {
  # Profile and AppStore share cargo's release archive. The AppStore run's
  # cargo shim holds it right after its meguri-ffi build until the Profile run
  # waits for the lock or has finished: without serialization, the Profile
  # run rebuilds that archive with test hooks in the window, and the AppStore
  # run packages it.
  local dir="$SB/.concurrent" gate="$SB/.shim-gate" i
  rm -rf "$dir" "$gate"
  mkdir -p "$dir" "$gate"
  cat >"$gate/cargo" <<EOF
#!/bin/bash
"$REAL_CARGO" "\$@" || exit
case " \$* " in *" -p meguri-ffi "*) ;; *) exit 0 ;; esac
: >"$dir/appstore-built"
for _ in \$(seq 1 1200); do
  grep -q 'waiting for another run' "$dir/profile.log" 2>/dev/null && exit 0
  [ -f "$dir/profile.rc" ] && exit 0
  sleep 0.5
done
EOF
  chmod +x "$gate/cargo"
  (
    PATH="$gate:$PATH" /bin/bash "$SCRIPT" --configuration AppStore --slices mac >"$dir/appstore.log" 2>&1
    echo $? >"$dir/appstore.rc"
  ) &
  for i in $(seq 1 1200); do
    [ -f "$dir/appstore-built" ] || [ -f "$dir/appstore.rc" ] && break
    sleep 0.5
  done
  [ -f "$dir/appstore-built" ] || {
    wait
    echo "the AppStore run never built meguri-ffi: $(tail -n 5 "$dir/appstore.log")"
    return 1
  }
  (
    PATH="$SHIM:$PATH" /bin/bash "$SCRIPT" --configuration Profile --slices mac >"$dir/profile.log" 2>&1
    echo $? >"$dir/profile.rc"
  ) &
  wait
  cat "$dir/appstore.log" "$dir/profile.log" >>"$SB/build.log"
  expect "AppStore exit code" "$(cat "$dir/appstore.rc")" 0 &&
    expect "Profile exit code" "$(cat "$dir/profile.rc")" 0 &&
    { grep -q 'waiting for another run' "$dir/profile.log" || { echo "the Profile run did not wait for the AppStore run"; false; }; } &&
    outputs_match_stamp
}

# Order matters: each test starts from the outputs the previous one left. The
# private change runs with all three slices, whose Info.plist xcodebuild
# would otherwise write in a random library order.
TESTS="t_builds_mac_and_sim_by_default
t_no_change_run_skips_cargo_and_keeps_mtimes
t_xcode_device_preaction_adds_the_device_slice_and_links
t_private_change_replaces_only_the_library
t_swift_reader_repair_is_hashed_without_changing_output_bytes
t_missing_header_or_modulemap_is_rebuilt
t_contents_of_files_with_quoted_names_are_hashed
t_unreadable_input_fails_the_run_before_cargo
t_one_slice_only_export_fails_before_the_xcframework_is_replaced
t_cfg_gated_signature_or_record_field_fails_before_the_xcframework_is_replaced
t_failed_output_copy_fails_and_forces_the_next_build
t_appstore_configuration_has_no_test_exports
t_concurrent_appstore_and_profile_runs_do_not_mix_artifacts"

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
