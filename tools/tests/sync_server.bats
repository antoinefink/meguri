#!/usr/bin/env bats
# Standalone server MG receipts, with synthetic JSONL and stub Cargo only.
# Integrator: tools/mg selftest --filter '^sync-server:'
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/server"
  cp -R "$REPO_ROOT/tools" "$REPO_ROOT/docker" "$t/"
  cp "$REPO_ROOT/.gitignore" "$t/"
  # Synthetic workspace markers; no server sources or compiler are used.
  touch "$t/server/Cargo.toml" "$t/server/Cargo.lock" "$t/server/rust-toolchain.toml"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  local bin="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$bin"
  export MG_CONTAINER=dev CARGO_NET_OFFLINE=true
  export STUB_LOG="$BATS_TEST_TMPDIR/cargo.log" STUB_ARGS="$BATS_TEST_TMPDIR/args"
  export STUB_STREAM="$BATS_TEST_TMPDIR/nextest.jsonl" STUB_BINARY="$bin/server-fixture"
  export STUB_NEXTEST_RC=0
  : >"$STUB_LOG"
  printf '#!/bin/bash\nexit 97\n' >"$STUB_BINARY"
  cat >"$bin/rustc" <<'SH'
#!/bin/bash
printf 'rustc %s\n' "$*" >>"$STUB_LOG"
[ "$*" = -V ] || exit 97
echo 'rustc fixture'
SH
  cat >"$bin/cargo" <<'SH'
#!/bin/bash
printf '%s\n' "$PWD" "target=${CARGO_TARGET_DIR:-}" "$#" "$@" >>"$STUB_LOG"
case "$1 ${2:-}" in
  'nextest --version') echo 'cargo-nextest fixture' ;;
  'build --locked')
    jq -nc --arg path "$STUB_BINARY" '{reason:"compiler-artifact",target:{name:"meguri-sync-server"},executable:$path}' ;;
  'nextest run')
    printf '%s\n' "$@" >"$STUB_ARGS"
    printf 'json=%s\n' "${NEXTEST_EXPERIMENTAL_LIBTEST_JSON:-}" >>"$STUB_LOG"
    cat "$STUB_STREAM"
    exit "$STUB_NEXTEST_RC" ;;
  *) exit 97 ;;
esac
SH
  chmod +x "$bin/cargo" "$bin/rustc" "$STUB_BINARY"
  export PATH="$bin:$PATH"
  single >"$STUB_STREAM"
}

mg() {
  run --separate-stderr "$SB/tools/mg" sync-server-test "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}
field() { jq -r "$@" "$result"; }
validate() { python3 "$SB/tools/tests/validate_result.py" "$result"; }
suite_start() {
  jq -nc --arg bin "$1" --argjson n "$2" \
    '{type:"suite",event:"started",test_count:$n,nextest:{crate:"fixture",test_binary:$bin,kind:"test"}}'
}
suite_end() {
  jq -nc --arg bin "$1" --argjson p "$2" --argjson f "$3" --argjson i "$4" --arg event "${5:-ok}" \
    '{type:"suite",event:$event,passed:$p,failed:$f,ignored:$i,measured:0,filtered_out:0,exec_time:0.1,nextest:{crate:"fixture",test_binary:$bin,kind:"test"}}'
}
test_event() {
  jq -nc --arg name "fixture::$1\$$2" --arg event "$3" \
    '{type:"test",event:$event,name:$name,exec_time:0.1}'
}
single() {
  suite_start alpha 1
  test_event alpha sample started
  test_event alpha sample ok
  suite_end alpha 1 0 0
}
summary() {
  jq -e --argjson p "$1" --argjson f "$2" --argjson i "$3" \
    '.summary == {total:($p+$f+$i),passed:$p,failed:$f,skipped:$i}' "$result"
}
retained() {
  local raw report diagnostics
  raw="$(field '.artifacts[] | select(.kind == "nextest-jsonl") | .path')"
  report="$(field '.artifacts[] | select(.kind == "nextest-evidence") | .path')"
  diagnostics="$(field '.artifacts[] | select(.kind == "nextest-evidence-log") | .path')"
  cmp "$STUB_STREAM" "$raw"
  [ -f "$report" ] && [ -f "$diagnostics" ]
  validate
}
assert_evidence_failure() {
  mg
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  jq -e 'any(.failures[]; .id == "nextest-evidence")' "$result"
  retained
}
replace_stream() {
  jq -c "$1" "$STUB_STREAM" >"$STUB_STREAM.tmp"
  mv "$STUB_STREAM.tmp" "$STUB_STREAM"
}
assert_usage() {
  : >"$STUB_LOG"
  mg "$@"
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = usage ]
  [ ! -s "$STUB_LOG" ]
  validate
}

@test "sync-server: valid single binary records actual counts and immutable Cargo settings" {
  mg
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  summary 1 0 0
  [ "$(field '.failures | length')" -eq 0 ]
  retained
  [ "$(cat "$STUB_ARGS")" = "$(printf '%s\n' nextest run --locked --workspace --all-targets --features test-hooks --retries 0 --no-fail-fast --no-tests=fail --message-format libtest-json-plus --message-format-version 0.1)" ]
  [ "$(sed -n '2p' "$STUB_LOG")" = "$SB/server" ]
  [ "$(sed -n '3p' "$STUB_LOG")" = target=/cargo-target/server ]
  [ "$(tail -n 1 "$STUB_LOG")" = json=1 ]
  grep -Fx -- '--bin' "$STUB_LOG"
  grep -Fx -- 'test-hooks' "$STUB_LOG"
}

@test "sync-server: interleaved binaries and ignored tests reconcile separately" {
  {
    suite_start alpha 2
    suite_start beta 1
    test_event alpha shared started
    test_event beta shared started
    test_event beta shared ok
    suite_end beta 1 0 0
    test_event alpha later ignored
    test_event alpha shared ok
    suite_end alpha 1 0 1
  } >"$STUB_STREAM"
  mg
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  summary 2 0 1
  retained
}

@test "sync-server: malformed or truncated JSONL retains completed counts and cannot pass" {
  local bad
  for bad in '{' '{"type":"suite"' 'not-json' 'null' '[]' ''; do
    single >"$STUB_STREAM"
    printf '%s\n' "$bad" >>"$STUB_STREAM"
    assert_evidence_failure
    summary 1 0 0
  done
}

@test "sync-server: JSON-valid prefixes missing test or suite completion fail" {
  local n
  for n in 1 2 3; do
    single | head -n "$n" >"$STUB_STREAM"
    assert_evidence_failure
    if [ "$n" -eq 3 ]; then summary 1 0 0; else summary 0 0 0; fi
  done
  : >"$STUB_STREAM"
  assert_evidence_failure
  summary 0 0 0
}

@test "sync-server: duplicate and incomplete events and suite identities fail" {
  local mutation
  for mutation in \
    'if .type == "suite" and .event == "started" then ., . else . end' \
    'if .type == "suite" and .event == "ok" then ., . else . end' \
    'if .type == "test" and .event == "started" then ., . else . end' \
    'if .type == "test" and .event == "ok" then ., . else . end' \
    'select(.type != "test" or .event != "started")' \
    'select(.type != "suite" or .event != "started")' \
    'if .type == "test" then .name = "fixture::missing$sample" else . end' \
    'if .type == "test" then del(.name) else . end' \
    'if .type == "test" then .name = 7 else . end' \
    'if .type == "test" then .name = "fixture::alpha$" else . end' \
    'if .type == "suite" then del(.nextest) else . end' \
    'if .type == "suite" then .nextest = [] else . end' \
    'if .type == "suite" and .event == "ok" then .nextest.kind = "other" else . end' \
    'if .type == "test" and .event == "ok" then .event = "unknown" else . end' \
    'if .type == "suite" and .event == "ok" then .event = "unknown" else . end'; do
    single >"$STUB_STREAM"
    replace_stream "$mutation"
    assert_evidence_failure
  done
  # Terminal-before-start and a started test omitted from the suite summary.
  {
    suite_start alpha 1
    test_event alpha sample ok
    test_event alpha sample started
    suite_end alpha 1 0 0
  } >"$STUB_STREAM"
  assert_evidence_failure
  {
    single | head -n 3
    test_event alpha unfinished started
    suite_end alpha 1 0 0
  } >"$STUB_STREAM"
  assert_evidence_failure
}

@test "sync-server: invalid counts and per-binary mismatches cannot cancel each other" {
  local key value
  for key in test_count passed failed ignored measured filtered_out; do
    for value in null -1 0.5 '"1"' true 2147483648; do
      single >"$STUB_STREAM"
      replace_stream "if has(\"$key\") then .[\"$key\"] = $value else . end"
      assert_evidence_failure
    done
  done
  {
    suite_start alpha 1
    suite_start beta 1
    test_event alpha a started
    test_event beta b started
    test_event alpha a ok
    test_event beta b ignored
    suite_end alpha 0 0 1
    suite_end beta 1 0 0
  } >"$STUB_STREAM"
  assert_evidence_failure
  summary 1 0 1
  single >"$STUB_STREAM"
  replace_stream 'if .type == "suite" and .event == "started" then .test_count = 2 else . end'
  assert_evidence_failure
  single >"$STUB_STREAM"
  replace_stream 'if .type == "suite" and .event == "ok" then .measured = 1 else . end'
  assert_evidence_failure
}

@test "sync-server: empty and ignored-only runs never satisfy nonempty execution policy" {
  {
    suite_start alpha 0
    suite_end alpha 0 0 0
  } >"$STUB_STREAM"
  mg
  [ "$status" -eq 1 ]
  summary 0 0 0
  retained
  {
    suite_start alpha 1
    test_event alpha later ignored
    suite_end alpha 0 0 1
  } >"$STUB_STREAM"
  mg
  [ "$status" -eq 1 ]
  summary 0 0 1
  retained
}

@test "sync-server: failed tests and suites fail even with exit zero and bounded diagnostics" {
  {
    suite_start alpha 2
    test_event alpha sample started
    test_event alpha sample ok
    test_event alpha broken started
    test_event alpha broken failed | jq -c '.stdout = ("failure detail " * 300)'
    suite_end alpha 1 1 0 failed
  } >"$STUB_STREAM"
  mg
  [ "$status" -eq 1 ]
  summary 1 1 0
  jq -e 'any(.failures[]; .id == "fixture::alpha$broken" and (.message | startswith("failure detail")) and (.message | length) == 2000)' "$result"
  retained
  single >"$STUB_STREAM"
  replace_stream 'if .type == "suite" and .event == "ok" then .event = "failed" else . end'
  assert_evidence_failure
  summary 1 0 0
  # A long malformed tail keeps raw evidence while capping receipt diagnostics.
  single >"$STUB_STREAM"
  local n=0
  while [ "$n" -lt 60 ]; do printf '{\n' >>"$STUB_STREAM"; n=$((n+1)); done
  assert_evidence_failure
  local report
  report="$(field '.artifacts[] | select(.kind == "nextest-evidence") | .path')"
  jq -e '.errors | length == 20' "$report"
}

@test "sync-server: nonzero nextest exit overrides otherwise passing evidence" {
  STUB_NEXTEST_RC=42 mg
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  summary 1 0 0
  jq -e 'any(.failures[]; .id == "nextest-exit-42")' "$result"
  retained
}

@test "sync-server: parser and count extraction command failures cannot fall through to pass" {
  local bin="$BATS_TEST_TMPDIR/jq-bin" stage
  mkdir -p "$bin"
  export STUB_REAL_JQ="$(command -v jq)"
  cat >"$bin/jq" <<'SH'
#!/bin/bash
for arg in "$@"; do
  case "$STUB_JQ_STAGE:$arg" in
    parser:*'def count:'*|counts:*'[.passed,.failed,.skipped]'*)
      echo 'synthetic evidence reader failure' >&2
      exit 73 ;;
  esac
done
exec "$STUB_REAL_JQ" "$@"
SH
  chmod +x "$bin/jq"
  for stage in parser counts; do
    STUB_JQ_STAGE="$stage" PATH="$bin:$PATH" assert_evidence_failure
    summary 0 0 0
  done
}

@test "sync-server: scope output configuration and policy arguments are refused before Cargo" {
  local option
  for option in --manifest-path --workspace --exclude -p --package --target-dir --target \
    --config --profile --cargo-profile --config-file --tool-config-file --override-version-check \
    --message-format --message-format-version --retries --no-tests --features --all-features \
    --no-default-features --locked --test-threads --run-ignored --no-run --list --lib --bin --; do
    assert_usage -- "$option" value
    assert_usage -- "$option=value"
  done
  assert_usage -- unexpected-positional
  assert_usage -- -E
  assert_usage -- --filter-expr ''
  assert_usage -- --test
  assert_usage -- --test --manifest-path
  assert_usage -- -E --profile
  assert_usage -- -E 'binary(provision)' --manifest-path other/Cargo.toml
}

@test "sync-server: supported focused filters preserve exact expression operands as data" {
  local expr
  for expr in 'test(=sample)' 'binary(provision)' 'test(=name with spaces)' \
    'test(=$(touch injected))' 'test(=`touch injected`)' 'test(=a); touch injected'; do
    mg -- -E "$expr" --test provision --filter-expr 'binary(provision)'
    [ "$status" -eq 0 ]
    [ "$(field .status)" = pass ]
    summary 1 0 0
    [ "$(tail -n 6 "$STUB_ARGS")" = "$(printf '%s\n' -E "$expr" --test provision --filter-expr 'binary(provision)')" ]
    [ ! -e "$SB/server/injected" ] && [ ! -e "$SB/injected" ]
    retained
  done
  mg -E 'binary(provision)'
  [ "$status" -eq 0 ]
}

@test "sync-server: help documents the whitelist and dry runs never invoke Cargo" {
  run "$SB/tools/mg" --list
  [ "$status" -eq 0 ]
  [[ "$output" == *'-E|--filter-expr <expr>'* ]]
  [[ "$output" == *'--test <binary>'* ]]
  mg --dry-run -- -E 'binary(provision)'
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .dry_run)" = true ]
  [ ! -s "$STUB_LOG" ]
  validate
}
