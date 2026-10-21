#!/usr/bin/env bats
# Real job reporting with fake native Cargo artifacts and libtest binaries.
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
  *) exit 97;;
esac
for suite in $suites; do
  [ "${FX_OMIT:-}" != "$suite" ] || continue
  exe="$FX_NATIVE_BIN/$suite"
  cp "$FX_NATIVE_BIN/template" "$exe"
  chmod +x "$exe"
  [ "${FX_NOT_EXECUTABLE:-}" != "$suite" ] || chmod -x "$exe"
  jq -nc --arg suite "$suite" --arg exe "$exe" \
    '{reason:"compiler-artifact",profile:{test:true},target:{name:$suite,kind:["test"]},executable:$exe}'
done
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
  echo "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
fi
EOF
  export PATH="$BIN:$PATH"
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }
mg() {
  run --separate-stderr "$SB/tools/mg" native-durability "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }

@test "native durability builds before leasing, runs three suites once and retains timing" {
  MEGURI_BENCH_FIXTURES=/must-not-use mg
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .summary.passed)" -eq 6 ]
  [ "$(field .summary.total)" -eq 6 ]
  [ "$(field .details.environment.rustc)" = 'rustc fake' ]
  grep -Fx "rustc $SB/core -V" "$STUB_LOG"
  [ "$(field '.details.wal_durability.passed')" -eq 2 ]
  [ "$(grep -c '^cargo test .*--no-run .*--message-format json-render-diagnostics$' "$STUB_LOG")" -eq 2 ]
  [ "$(grep -c '^run .* --test-threads=1 --nocapture$' "$STUB_LOG")" -eq 3 ]
  [ "$(sed -n '4p' "$STUB_LOG")" = 'lease --no-session' ]
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

@test "native durability failing libtest is counted and never retried" {
  FX_TEST_FAIL=wal_durability mg
  [ "$status" -eq 1 ]
  [ "$(field .summary.failed)" -eq 1 ]
  [ "$(field '.details.wal_durability.exit_code')" -eq 101 ]
  [ "$(grep -c '^run wal_durability ' "$STUB_LOG")" -eq 1 ]
  [ "$(grep -c '^run ' "$STUB_LOG")" -eq 3 ]
}

@test "native durability rejects empty suites and absent summaries" {
  FX_EMPTY=suspend mg
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = empty/suspend ]
  FX_NO_SUMMARY=collection_ffi mg
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = summary/collection_ffi ]
}

@test "native durability exposes no arbitrary native Cargo or shell interface" {
  mg -- --test arbitrary
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  [ ! -s "$STUB_LOG" ]
}
