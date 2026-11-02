#!/usr/bin/env bats
# U32/U37/U113 job reporting with fake native Cargo artifacts and libtest binaries.
bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN" "$SB/core" "$SB/build/fake-native"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log" FX_NATIVE_BIN="$SB/build/fake-native"
  : >"$STUB_LOG"
  cat >"$SB/tools/mg.d/zz-native-fakes.sh" <<'EOF'
mg_host_lease() { echo "lease $*" >>"$STUB_LOG"; [ "$*" = --no-session ]; }
EOF
  stub uname 'case "$*" in -s) echo Darwin;; -m) echo arm64;; -sm) echo "Darwin arm64";; *) exit 97;; esac'
  stub rustc 'echo "rustc $PWD $*" >>"$STUB_LOG"; echo "rustc fake"'
  cat >"$BIN/cargo" <<'EOF'
#!/bin/bash
echo "cargo $*" >>"$STUB_LOG"
if [ "${FX_CACHE_COLD:-}" = 1 ] && [[ " $* " == *" --offline "* ]]; then
  echo "fake missing cached native test dependency" >&2
  exit 42
fi
package=""; previous=""
for arg in "$@"; do
  if [ "$previous" = -p ]; then package="$arg"; fi
  previous="$arg"
done
if [ "${FX_BUILD_FAIL:-}" = "$package" ]; then
  echo "fake native build failure for $package" >&2
  exit 42
fi
case "$package" in
  meguri-engine) suites="suspend wal_durability";;
  meguri-ffi) suites="collection_ffi";;
  meguri-sync) suites="cloud_records cloud_recovery cloud_unit";;
  meguri-storage) suites="persistent_identity";;
  *) exit 97;;
esac
for suite in $suites; do
  [ "${FX_OMIT:-}" != "$suite" ] || continue
  exe="$FX_NATIVE_BIN/$suite"
  cp "$FX_NATIVE_BIN/template" "$exe"
  chmod +x "$exe"
  [ "${FX_NOT_EXECUTABLE:-}" != "$suite" ] || chmod -x "$exe"
  target_name="$suite"; kind=test
  [ "$suite" != cloud_unit ] || { target_name=meguri_sync; kind=lib; }
  [ "$suite" != persistent_identity ] || { target_name=meguri_storage; kind=lib; }
  [ "${FX_WRONG_KIND:-}" != "$suite" ] || { if [ "$kind" = lib ]; then kind=test; else kind=lib; fi; }
  artifact="$(jq -nc --arg name "$target_name" --arg exe "$exe" --arg kind "$kind" \
    --argjson test "$([ "${FX_WRONG_PROFILE:-}" != "$suite" ] && echo true || echo false)" \
    --argjson missing "$([ "${FX_NO_EXECUTABLE:-}" = "$suite" ] && echo true || echo false)" \
    '{reason:"compiler-artifact",profile:{test:$test},target:{name:$name,kind:[$kind]},executable:(if $missing then null else $exe end)}')"
  # Cargo also reports the ordinary library without a test executable.
  if [ "$suite" = cloud_unit ]; then
    printf '%s\n' "$artifact" | jq -c '.profile.test = false | .executable = null'
  fi
  printf '%s\n' "$artifact"
  [ "${FX_DUPLICATE:-}" != "$suite" ] || printf '%s\n' "$artifact"
done
if [ "${FX_STREAM_PACKAGE:-}" = "$package" ]; then
  case "${FX_STREAM:-}" in
    incomplete) exit 0;;
    failed) echo '{"reason":"build-finished","success":false}'; exit 0;;
    malformed) echo '{'; exit 0;;
  esac
fi
echo '{"reason":"build-finished","success":true}'
EOF
  chmod +x "$BIN/cargo"
  cat >"$FX_NATIVE_BIN/template" <<'EOF'
#!/bin/bash
suite="${0##*/}"
echo "run $suite $*" >>"$STUB_LOG"
echo "fixtures $MEGURI_BENCH_FIXTURES"
if [ "$suite" = wal_durability ]; then
  echo "U32 full WAL sync: p50=123us, p95=456us, VFS calls=20"
fi
[ "${FX_NO_SUMMARY:-}" != "$suite" ] || exit 0
if [ "${FX_EMPTY:-}" = "$suite" ]; then
  echo "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
elif [ "${FX_TEST_FAIL:-}" = "$suite" ]; then
  echo "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
  exit 101
else
  passed=2
  case "$suite" in cloud_records|cloud_recovery) passed=5;; cloud_unit|persistent_identity) passed=1;; esac
  echo "test result: ok. $passed passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
fi
[ "${FX_DOUBLE_SUMMARY:-}" != "$suite" ] || echo "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
[ "${FX_EXIT_FAIL:-}" != "$suite" ] || exit 101
exit 0
EOF
  export PATH="$BIN:$PATH"
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }
mg() {
  run --separate-stderr "$SB/tools/mg" native-durability "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }

@test "native durability builds before leasing, runs seven suites once and retains timing" {
  FX_CACHE_COLD=1 MEGURI_BENCH_FIXTURES=/must-not-use mg
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .summary.passed)" -eq 18 ]
  [ "$(field .summary.total)" -eq 18 ]
  [ "$(field .details.environment.rustc)" = 'rustc fake' ]
  grep -Fx "rustc $SB/core -V" "$STUB_LOG"
  [ "$(field '.details.wal_durability.passed')" -eq 2 ]
  [ "$(field '.details.cloud_records.passed')" -eq 5 ]
  [ "$(field '.details.cloud_recovery.passed')" -eq 5 ]
  [ "$(field '.details.cloud_unit.passed')" -eq 1 ]
  local target="$SB/build/native-durability/target"
  grep -Fx "cargo test --locked -p meguri-engine --test suspend --test wal_durability --target aarch64-apple-darwin --target-dir $target --no-run --message-format json-render-diagnostics" "$STUB_LOG"
  grep -Fx "cargo test --locked -p meguri-ffi --features test-hooks --test collection_ffi --target aarch64-apple-darwin --target-dir $target --no-run --message-format json-render-diagnostics" "$STUB_LOG"
  grep -Fx "cargo test --locked -p meguri-sync --features test-hooks --lib --test cloud_records --test cloud_recovery --target aarch64-apple-darwin --target-dir $target --no-run --message-format json-render-diagnostics" "$STUB_LOG"
  grep -Fx "cargo test --locked -p meguri-storage --lib --target aarch64-apple-darwin --target-dir $target --no-run --message-format json-render-diagnostics" "$STUB_LOG"
  grep -Fx 'run persistent_identity persistent_identity::tests::darwin_generated_regular_fd_reads_full_volume_uuid_and_retains_binding --exact --test-threads=1 --nocapture' "$STUB_LOG"
  [ "$(field '.details.persistent_identity.passed')" -eq 1 ]
  [ "$(grep -c '^cargo test .*--no-run .*--message-format json-render-diagnostics$' "$STUB_LOG")" -eq 4 ]
  [ "$(grep -c '^run .* --test-threads=1 --nocapture$' "$STUB_LOG")" -eq 7 ]
  grep -Fx 'run cloud_unit cloud:: --test-threads=1 --nocapture' "$STUB_LOG"
  [ "$(sed -n '6p' "$STUB_LOG")" = 'lease --no-session' ]
  grep -q '^U32 full WAL sync: p50=123us, p95=456us' "$(dirname "$result")/wal_durability.log"
  grep -Fx "fixtures $SB/build/fixtures" "$(dirname "$result")/suspend.log"
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "native durability missing executable errors before any suite or lease" {
  FX_NOT_EXECUTABLE=collection_ffi mg
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = executable/collection_ffi ]
  ! grep -Eq '^(run|lease) ' "$STUB_LOG"
}

@test "native durability omitted artifact cannot pass an incomplete build" {
  FX_OMIT=suspend mg
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = executable/suspend ]
  ! grep -Eq '^(run|lease) ' "$STUB_LOG"
}

@test "native durability cloud artifacts require one executable of the correct kind and profile" {
  local suite fault
  for suite in cloud_records cloud_recovery cloud_unit persistent_identity; do
    for fault in FX_OMIT FX_DUPLICATE FX_NO_EXECUTABLE FX_NOT_EXECUTABLE FX_WRONG_KIND FX_WRONG_PROFILE; do
      export "$fault=$suite"
      mg
      unset "$fault"
      [ "$status" -eq 2 ]
      [ "$(field '.failures[0].id')" = "executable/$suite" ]
      ! grep -Eq '^(run|lease) ' "$STUB_LOG"
    done
  done
}

@test "native durability requires a complete successful Cargo JSON stream" {
  local stream
  for stream in incomplete failed malformed; do
    FX_STREAM_PACKAGE=meguri-sync FX_STREAM="$stream" mg
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = executable/cloud_records ]
    ! grep -Eq '^(run|lease) ' "$STUB_LOG"
  done
}

@test "native durability build failure is final and preserves artifacts" {
  FX_BUILD_FAIL=meguri-engine mg
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = build/meguri-engine ]
  [ "$(grep -c '^cargo ' "$STUB_LOG")" -eq 1 ]
  local compiler_json build_log
  compiler_json="$(field '.artifacts[] | select(.kind == "build-jsonl" and (.path | endswith("/meguri-engine-build.jsonl"))) | .path')"
  build_log="$(field '.artifacts[] | select(.kind == "build-log" and (.path | endswith("/meguri-engine-build.log"))) | .path')"
  [ -f "$compiler_json" ]
  [ -f "$build_log" ]
  grep -Fx 'fake native build failure for meguri-engine' "$build_log"
  ! grep -Eq '^(run|lease) ' "$STUB_LOG"
}

@test "native durability sync build failure is final before the lease" {
  FX_BUILD_FAIL=meguri-sync mg
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = build/meguri-sync ]
  [ "$(grep -c '^cargo ' "$STUB_LOG")" -eq 3 ]
  [ -f "$(dirname "$result")/meguri-sync-build.jsonl" ]
  grep -Fx 'fake native build failure for meguri-sync' "$(dirname "$result")/meguri-sync-build.log"
  ! grep -Eq '^(run|lease) ' "$STUB_LOG"
}

@test "native durability failing libtest is counted and never retried" {
  FX_TEST_FAIL=wal_durability mg
  [ "$status" -eq 1 ]
  [ "$(field .summary.failed)" -eq 1 ]
  [ "$(field '.details.wal_durability.exit_code')" -eq 101 ]
  [ "$(grep -c '^run wal_durability ' "$STUB_LOG")" -eq 1 ]
  [ "$(grep -c '^run ' "$STUB_LOG")" -eq 7 ]
}

@test "native durability rejects empty suites and absent summaries" {
  FX_EMPTY=suspend mg
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = empty/suspend ]
  FX_NO_SUMMARY=collection_ffi mg
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = summary/collection_ffi ]
}

@test "native durability cloud suites reject zero tests and missing or duplicate summaries" {
  local suite
  for suite in cloud_records cloud_recovery cloud_unit persistent_identity; do
    FX_EMPTY="$suite" mg
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = "empty/$suite" ]
    [ "$(field ".details.$suite.passed")" -eq 0 ]
    FX_NO_SUMMARY="$suite" mg
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = "summary/$suite" ]
    FX_DOUBLE_SUMMARY="$suite" mg
    [ "$status" -eq 1 ]
    [ "$(field '.failures[0].id')" = "summary/$suite" ]
  done
}

@test "native durability cloud failures and nonzero exits cannot pass or retry" {
  local suite fault
  for suite in cloud_records cloud_recovery cloud_unit persistent_identity; do
    for fault in FX_TEST_FAIL FX_EXIT_FAIL; do
      : >"$STUB_LOG"
      export "$fault=$suite"
      mg
      unset "$fault"
      [ "$status" -eq 1 ]
      [ "$(field '.failures[0].id')" = "test/$suite" ]
      [ "$(field ".details.$suite.exit_code")" -eq 101 ]
      if [ "$fault" = FX_TEST_FAIL ]; then
        [ "$(field .summary.failed)" -eq 1 ]
      fi
      [ "$(grep -c "^run $suite " "$STUB_LOG")" -eq 1 ]
      [ "$(grep -c '^run ' "$STUB_LOG")" -eq 7 ]
    done
  done
}

@test "native durability exposes no arbitrary native Cargo or shell interface" {
  mg -- --test arbitrary
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  [ ! -s "$STUB_LOG" ]
}
