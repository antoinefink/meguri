#!/usr/bin/env bats
# mg_selftest's arguments (selftest.sh): test files named by path replace the
# full list instead of running twice; a missing named file is a usage error;
# --shard M/N runs one share of the full list, and the shares of any N hold
# every test file once.

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
    mg_detail() { printf "%s\n" "$2" >"detail-$1"; }
    mg_json_array() { local a; for a in "$@"; do jq -n --arg a "$a" "\$a"; done | jq -sc .; }
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

@test "an option value that ends in .bats is a filter, not a test file" {
  BIN="$BATS_TEST_TMPDIR/bin" run_selftest --filter 'runs once|\.bats' --filter-tags x.bats
  [ "$status" -eq 0 ]
  [ "$(grep -c '\.bats$' "$BATS_TEST_TMPDIR/args")" -eq 4 ]
  grep -qx 'tools/tests/a.bats' "$BATS_TEST_TMPDIR/args"
  grep -qx 'tools/tests/b.bats' "$BATS_TEST_TMPDIR/args"
  grep -qx "status pass" "$WORK/status"
}

# --shard M/N (the hosted shards, .github/workflows/meguri-verification.yml).

# shard_files <M/N>: the test files the fake bats got for that share.
shard_files() {
  rm -f "$BATS_TEST_TMPDIR/args"
  BIN="$BATS_TEST_TMPDIR/bin" run_selftest --shard "$1"
  [ "$status" -eq 0 ] || return 1
  grep '\.bats$' "$BATS_TEST_TMPDIR/args"
}

@test "for any share count every test file is in exactly one share, listed in the seconds file or not" {
  local name n m
  for name in c d e f g h i; do touch "$WORK/tools/tests/$name.bats"; done
  printf '# seconds\tfile\n300\tc.bats\n40.5\ta.bats\n40.5\tg.bats\n7\te.bats\nnot-a-number\tb.bats\n9\tgone.bats\n' \
    >"$WORK/tools/tests/selftest-seconds.tsv"
  for n in 1 2 3 4 5 6 7 8 9; do
    : >"$BATS_TEST_TMPDIR/seen"
    for m in $(seq 1 "$n"); do
      shard_files "$m/$n" >"$BATS_TEST_TMPDIR/share"
      # No share is empty, so no shard passes without running a test.
      [ -s "$BATS_TEST_TMPDIR/share" ]
      cat "$BATS_TEST_TMPDIR/share" >>"$BATS_TEST_TMPDIR/seen"
    done
    [ "$(LC_ALL=C sort "$BATS_TEST_TMPDIR/seen")" = "$(cd "$WORK" && printf '%s\n' tools/tests/*.bats | LC_ALL=C sort)" ]
  done
}

@test "shares balance the listed seconds, an unlisted file counts as the mean, and the longest file starts first" {
  touch "$WORK/tools/tests/c.bats" "$WORK/tools/tests/d.bats"
  printf '100\ta.bats\n60\tb.bats\n50\tc.bats\n' >"$WORK/tools/tests/selftest-seconds.tsv"
  # d counts as (100 + 60 + 50) / 3 = 70: a to share 1, d to 2, b to 2, c to 1.
  [ "$(shard_files 1/2)" = "$(printf '%s\n' tools/tests/a.bats tools/tests/c.bats)" ]
  [ "$(shard_files 2/2)" = "$(printf '%s\n' tools/tests/d.bats tools/tests/b.bats)" ]
  # The share is ours: bats never sees it, and keeps its other arguments.
  BIN="$BATS_TEST_TMPDIR/bin" run_selftest --filter x --shard=2/2
  [ "$status" -eq 0 ]
  ! grep -q -- 'shard' "$BATS_TEST_TMPDIR/args"
  grep -qx -- "--filter" "$BATS_TEST_TMPDIR/args"
  [ "$(grep -c '\.bats$' "$BATS_TEST_TMPDIR/args")" -eq 2 ]
  jq -e '. == {index: 2, count: 2, files: ["tools/tests/d.bats", "tools/tests/b.bats"], all_files: 4}' "$WORK/detail-shard"
}

@test "a share that is not M of N, has more shares than files or comes with named files is a usage error before bats runs" {
  local spec
  for spec in 0/2 3/2 2 a/b 1/0 1/3 ''; do
    rm -f "$WORK/status"
    BIN="$BATS_TEST_TMPDIR/bin" run_selftest --shard "$spec"
    [ "$status" -eq 0 ]
    [ ! -f "$BATS_TEST_TMPDIR/args" ]
    grep -qx "failure usage" "$WORK/status"
    grep -qx "status error" "$WORK/status"
  done
  for spec in "--shard 1/2 tools/tests/a.bats" "--shard"; do
    rm -f "$WORK/status"
    # shellcheck disable=SC2086 # the words of one command line
    BIN="$BATS_TEST_TMPDIR/bin" run_selftest $spec
    [ ! -f "$BATS_TEST_TMPDIR/args" ]
    grep -qx "failure usage" "$WORK/status"
    grep -qx "status error" "$WORK/status"
  done
}

@test "the hosted shards cover the real test files, and the seconds file names only files that exist" {
  local n m
  cd "$REPO_ROOT"
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/selftest.sh"
  [ -f "$MG_SELFTEST_SECONDS" ]
  for n in 1 2 3 4 5 6 8 12; do
    for m in $(seq 1 "$n"); do mg_selftest__shard "$m" "$n" tools/tests/*.bats; done >"$BATS_TEST_TMPDIR/seen-$n"
    [ "$(LC_ALL=C sort "$BATS_TEST_TMPDIR/seen-$n")" = "$(printf '%s\n' tools/tests/*.bats | LC_ALL=C sort)" ]
  done
  # A line for a file that was renamed or removed would keep a dead weight.
  while IFS="$(printf '\t')" read -r seconds name; do
    case "$seconds" in '#'*) continue ;; esac
    [[ "$seconds" =~ ^[0-9]+(\.[0-9]+)?$ ]] || { echo "bad seconds: $seconds $name"; return 1; }
    [ -f "tools/tests/$name" ] || { echo "selftest-seconds.tsv lists $name, which is not a test file"; return 1; }
  done <"$MG_SELFTEST_SECONDS"
}
