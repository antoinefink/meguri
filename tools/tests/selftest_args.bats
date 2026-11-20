#!/usr/bin/env bats
# mg_selftest's arguments (selftest.sh): test files named by path replace the
# full list instead of running twice; a missing named file is a usage error.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  WORK="$BATS_TEST_TMPDIR/work"
  mkdir -p "$WORK/tools/tests" "$BATS_TEST_TMPDIR/bin"
  touch "$WORK/tools/tests/a.bats" "$WORK/tools/tests/b.bats"
  cat >"$BATS_TEST_TMPDIR/bin/bats" <<EOF
#!/bin/bash
printf '%s\n' "\$@" >"$BATS_TEST_TMPDIR/args"
echo "1..1"
echo "ok 1 fake"
EOF
  chmod +x "$BATS_TEST_TMPDIR/bin/bats"
}

run_selftest() {
  run --separate-stderr bash -c '
    cd "$1" && shift
    PATH="$BIN:$PATH" MG_RUN_DIR="$PWD" MG_STATE="$PWD" MG_JOB=selftest MG_DEFAULT_REPRO=x
    mg_job() { :; }
    mg_artifact() { :; }; mg_count() { :; }; mg_message() { echo "message $*" >>status; }
    mg_failure() { echo "failure $1" >>status; }; mg_status() { echo "status $1" >>status; }
    source "'"$REPO_ROOT"'/tools/mg.d/selftest.sh"
    mg_selftest "$@"
  ' _ "$WORK" "$@"
}

@test "without files every test file runs once" {
  BIN="$BATS_TEST_TMPDIR/bin" run_selftest --filter x
  [ "$status" -eq 0 ]
  [ "$(grep -c '\.bats$' "$BATS_TEST_TMPDIR/args")" -eq 2 ]
  grep -qx "status pass" "$WORK/status"
}

@test "named files replace the full list and keep the other arguments" {
  BIN="$BATS_TEST_TMPDIR/bin" run_selftest --filter x tools/tests/b.bats
  [ "$status" -eq 0 ]
  [ "$(grep '\.bats$' "$BATS_TEST_TMPDIR/args")" = tools/tests/b.bats ]
  grep -qx -- "--filter" "$BATS_TEST_TMPDIR/args"
}

@test "a named file that does not exist is a usage error before bats runs" {
  BIN="$BATS_TEST_TMPDIR/bin" run_selftest b.bats
  [ "$status" -eq 0 ]
  [ ! -f "$BATS_TEST_TMPDIR/args" ]
  grep -qx "failure usage" "$WORK/status"
  grep -qx "status error" "$WORK/status"
}
