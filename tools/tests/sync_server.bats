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
  # Isolate synthetic runner/report tests from real browser asset preparation.
  # Loaded after meguri-sync.sh; no prepared assets or build evidence are faked.
  cat >"$t/tools/mg.d/zz-sync-server-web-fixture.sh" <<'SH'
mg_sync_server__web() { :; }
SH
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
  export STUB_NEXTEST_RC=0 STUB_SELECTION=false
  export STUB_SECOND_STREAM="$BATS_TEST_TMPDIR/beta.jsonl"
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
    if [ "$STUB_SELECTION" = true ]; then
      all=false
      selected=()
      shift 2
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --all-targets) all=true ;;
          --test) selected+=("$2"); shift ;;
        esac
        shift
      done
      # Model Cargo's precedence: --all-targets defeats explicit selectors.
      if [ "$all" = true ] || [ "${#selected[@]}" -eq 0 ]; then
        cat "$STUB_STREAM" "$STUB_SECOND_STREAM"
      else
        for target in "${selected[@]}"; do
          case "$target" in
            alpha) cat "$STUB_STREAM" ;;
            beta) cat "$STUB_SECOND_STREAM" ;;
            *) echo 'no test target with that name' >&2; exit 101 ;;
          esac
        done
      fi
    else
      cat "$STUB_STREAM"
    fi
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

@test "sync-server: explicit test targets select only requested binaries and missing targets fail without credit" {
  export STUB_SELECTION=true
  {
    suite_start beta 1
    test_event beta other started
    test_event beta other ok
    suite_end beta 1 0 0
  } >"$STUB_SECOND_STREAM"
  mg
  [ "$status" -eq 0 ]
  summary 2 0 0
  grep -Fx -- '--all-targets' "$STUB_ARGS"
  mg -- --test alpha
  [ "$status" -eq 0 ]
  summary 1 0 0
  ! grep -Fx -- '--all-targets' "$STUB_ARGS"
  [ "$(cat "$STUB_ARGS")" = "$(printf '%s\n' nextest run --locked --workspace --features test-hooks --retries 0 --no-fail-fast --no-tests=fail --message-format libtest-json-plus --message-format-version 0.1 --test alpha)" ]
  mg -- --test alpha --test beta
  [ "$status" -eq 0 ]
  summary 2 0 0
  ! grep -Fx -- '--all-targets' "$STUB_ARGS"
  mg -- --test does-not-exist
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  summary 0 0 0
  jq -e 'any(.failures[]; .id == "nextest-exit-101")' "$result"
  validate
}

# These fixtures exercise only the source-owned finite wrapper seams. No client,
# reference inputs, compiler, credentials from disk, or container is used.
candidate_shell() {
  run bash -c '
    mg_job() { :; }
    . "$1/tools/mg.d/meguri-sync.sh"
    export MG_ROOT="$1" MG_RUN_DIR="$2" MG_JOB=sync-server-interop MG_RUN_ID=20261115T000000Z-1
    mg_failure() { printf "%s\n" "$1"; }
    mg_status() { printf "status:%s\n" "$1"; }
    mg_detail() { :; }
    mg_sync_server__clean() { mg_failure fixture-before-runtime; return 1; }
    mg_session_role() { echo integration; }
    shift 2
    "$@"
  ' fixture "$SB" "$BATS_TEST_TMPDIR" "$@"
}

producer_ledger() {
  mkdir -p "$SB/docs/process/provenance"
  printf '{"session":"producer-fixture"}\n' >"$SB/docs/process/provenance/sessions.jsonl"
  git -C "$SB" add docs/process/provenance/sessions.jsonl
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm 'synthetic producer ledger'
}

# Public synthetic build measurements, with no compiler or container execution.
candidate_build_fixture() {
  producer_ledger
  mkdir -p "$SB/core/crates/meguri-model" "$SB/server/tools"
  touch "$SB/core/crates/meguri-model/fixture" "$SB/core/Cargo.toml" "$SB/core/rust-toolchain.toml" "$SB/LICENSE" "$SB/server/tools/candidate-setup.py"
  printf 'version = "0.0.0"\n' >"$SB/server/Cargo.toml"
  git -C "$SB" add core server LICENSE
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm 'synthetic build inputs'
  CANDIDATE_BUILD="$SB/build/mg/sync-server-candidate-build/20261115T000000Z-2"
  mkdir -p "$CANDIDATE_BUILD/source" "$CANDIDATE_BUILD/product"
  mkdir -m 700 "$CANDIDATE_BUILD/target"
  mkdir "$CANDIDATE_BUILD/target/debug"
  bash -c 'mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"; MG_ROOT="$1"; mg_sync_server__archive HEAD' fixture "$SB" >"$CANDIDATE_BUILD/source.tar"
  tar -xf "$CANDIDATE_BUILD/source.tar" -C "$CANDIDATE_BUILD/source"
  printf 'synthetic build executable; never executed\n' >"$CANDIDATE_BUILD/product/meguri-sync-server"
  cp "$CANDIDATE_BUILD/product/meguri-sync-server" "$CANDIDATE_BUILD/target/debug/meguri-sync-server"
  local script
  script="$(bash -c 'mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"; mg_sync_server__build_script' fixture "$SB")"
  python3 - "$SB" "$CANDIDATE_BUILD" "$(git -C "$SB" rev-parse HEAD)" "$script" <<'PY'
import hashlib, json, pathlib, stat, sys
root, base = map(pathlib.Path, sys.argv[1:3])
commit, script = sys.argv[3:]
def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def write(name, value): (base/name).write_text(json.dumps(value))
run, nonce, image = '20261115T000000Z-2', '9'*32, 'sha256:'+'8'*64
server = dict(kind='meguri_sync', version='0.0.0', build=commit, source_commit=commit,
              source_sha256=digest(base/'source.tar'), lock_sha256=digest(base/'source/server/Cargo.lock'),
              executable_sha256=digest(base/'product/meguri-sync-server'))
s = (base/'target').lstat()
owner = dict(device=s.st_dev, inode=s.st_ino, uid=s.st_uid, mode=stat.S_IMODE(s.st_mode))
inputs = dict(command=['cargo','build','--locked','--no-default-features','--bin','meguri-sync-server',
                      '--message-format','json-render-diagnostics','--target-dir','/candidate-target','--profile','dev'],
              working_directory='/work/server',target_directory='/candidate-target',target_host_directory=str(base/'target'),
              target_initially_empty=True,target_owner=owner,container_owner=nonce,
              source_sha256=server['source_sha256'],lock_sha256=server['lock_sha256'],image_id=image,
              profile='debug',features=[],rustflags='',encoded_rustflags='')
write('result.json',dict(job='sync-server-candidate-build',status='pass',dry_run=False,root=str(root),run_id=run,
                       commit=dict(sha=commit,dirty=False),
                       args=['--lane','desktop','--phase','full','--author-session','producer-fixture'],
                       details=dict(author_session='producer-fixture',candidate_build=dict(server=server,source_dirty=False,profile='debug',features=[]),
                                    candidate_build_inputs=inputs,build_image=dict(id=image))))
write('build-container.json',[dict(Id='7'*64,Name='/meguri-candidate-build-'+run,Image=image,State=dict(ExitCode=0),
                                 Config=dict(Labels={'meguri.mg.run-id':run,'meguri.sync.owner':nonce},
                                             Cmd=['bash','-c',script],Env=['CARGO_TARGET_DIR=/candidate-target']),
                                 Mounts=[dict(Type='bind',Source=str(base/host),Destination=dest,RW=rw)
                                         for host,dest,rw in [('source','/work',False),('product','/candidate-out',True),('target','/candidate-target',True)]])])
write('product/build.jsonl',dict(reason='compiler-artifact',target=dict(name='meguri-sync-server'),fresh=False,features=[],
                               profile=dict(test=False,debug_assertions=True,opt_level='0'),executable='/candidate-target/debug/meguri-sync-server'))
PY
}

candidate_build_check() {
  run bash -c '
    mg_job() { :; }
    . "$1/tools/mg.d/meguri-sync.sh"
    export MG_ROOT="$1" MG_RUN_DIR="$2" MG_RUN_ID=20261115T000000Z-1
    mg_failure() { echo "$1"; }
    mg_detail() { :; }
    # Stop only after every retained-build prerequisite passes, before resources.
    mktemp() { echo fixture-ready-for-runtime >&2; return 97; }
    docker() { echo unexpected-docker >&2; return 98; }
    mg_sync_server__interop_owned "$3" "[]" producer-fixture
  ' fixture "$SB" "$BATS_TEST_TMPDIR" "$CANDIDATE_BUILD/result.json"
}

candidate_output_fixture() {
  export CANDIDATE_FIXTURE="$BATS_TEST_TMPDIR/candidate"
  mkdir -p "$CANDIDATE_FIXTURE/out/artifacts"
  python3 - "$REPO_ROOT" "$CANDIDATE_FIXTURE" "${1:-full}" <<'PY'
import copy, hashlib, json, pathlib, sys
root, base = map(pathlib.Path, sys.argv[1:3])
phase = sys.argv[3]
selection = json.loads((root / f'conformance/recordings/scenarios/meguri-sync-server/candidate-{phase}-selection.json').read_text())
def write(path, value):
    (base / path).write_text(json.dumps(value))
args = ['--lane', 'desktop', '--phase', phase, '--build-receipt', 'owned/result.json', '--author-session', 'producer-fixture']
invocation = dict(source_commit='a'*40, batch_base='b'*40, source_sha256='c'*64,
                  lock_sha256='d'*64, executable_sha256='1'*64, mg_job='sync-server-interop', arguments=args,
                  run_id='20261115T000000Z-1', author_session='producer-fixture')
write('expected.json', invocation)
write('selection.json', selection)
server = dict(kind='meguri_sync', version='0.0.0', build='a'*40, source_commit='a'*40,
              source_sha256='c'*64, lock_sha256='d'*64, executable_sha256='1'*64)
write('peer.json', dict(server=server))
invocation['selection'] = [case['id'] for case in selection['cases']]
write('out/invocation.json', invocation)
artifacts = []
for role in ('synthetic_input', 'neutral_observation', 'mg_receipt'):
    data = ('finite synthetic output fixture: ' + role).encode()
    digest = hashlib.sha256(data).hexdigest()
    (base / 'out/artifacts' / digest).write_bytes(data)
    artifacts.append(dict(role=role, sha256=digest, bytes=len(data)))
data = b'finite synthetic output fixture: action'
digest = hashlib.sha256(data).hexdigest()
(base / 'out/artifacts' / digest).write_bytes(data)
artifacts.append(dict(role='synthetic_input', sha256=digest, bytes=len(data)))
cases = []
for contract in selection['cases']:
    case = copy.deepcopy(contract)
    del case['client_kind']
    case.update(status='matched', observed=copy.deepcopy(case['expected']),
                lane=dict(kind='candidate', server=server, client=dict(kind='desktop', version='26.09.4', build='c06370ff', runtime_sha256='2'*64)),
                fixture_sha256=artifacts[0]['sha256'], artifacts=artifacts)
    for action in case['actions']:
        action['fixture_sha256'] = artifacts[-1]['sha256']
    cases.append(case)
evidence = {key: invocation[key] for key in ('source_commit', 'batch_base', 'source_sha256', 'lock_sha256', 'mg_job', 'arguments')}
evidence.update(format='meguri-sync-server-v1', document_kind='evidence', baseline_release='26.09.3',
                expected_case_count=len(cases), selected_case_count=len(cases), completed_case_count=len(cases), cases=cases)
write('out/evidence.json', evidence)
write('out/summary.json', dict(total=len(cases), passed=len(cases), failed=0, expected_case_count=len(cases),
                            selected_case_count=len(cases), completed_case_count=len(cases),
                            selection_credit=True, broad_catalog_credit=False))
PY
}

candidate_output_check() {
  run python3 "$REPO_ROOT/server/tools/candidate-output.py" "$CANDIDATE_FIXTURE/out" \
    "$CANDIDATE_FIXTURE/expected.json" "$CANDIDATE_FIXTURE/peer.json" "$CANDIDATE_FIXTURE/selection.json"
}

candidate_mutate() {
  python3 - "$CANDIDATE_FIXTURE" "$1" <<'PY'
import json, pathlib, sys
base = pathlib.Path(sys.argv[1])
summary = json.loads((base/'out/summary.json').read_text())
invocation = json.loads((base/'out/invocation.json').read_text())
evidence = json.loads((base/'out/evidence.json').read_text())
exec(sys.argv[2])
for name, value in [('summary', summary), ('invocation', invocation), ('evidence', evidence)]:
    (base / 'out' / (name + '.json')).write_text(json.dumps(value))
PY
}

@test "sync-server: candidate producer is explicit unique and committed before runtime" {
  producer_ledger
  local job
  touch "$BATS_TEST_TMPDIR/receipt.json"
  for job in mg_sync_server_candidate_build mg_sync_server_interop; do
    local args=(--lane desktop --phase full)
    [ "$job" != mg_sync_server_interop ] || args+=(--build-receipt "$BATS_TEST_TMPDIR/receipt.json")
    candidate_shell "$job" "${args[@]}"
    [[ "$output" == *usage* ]] && [[ "$output" != *fixture-before-runtime* ]]
    candidate_shell "$job" "${args[@]}" --author-session unknown
    [[ "$output" == *candidate-author-session* ]] && [[ "$output" != *fixture-before-runtime* ]]
    candidate_shell "$job" "${args[@]}" --author-session producer-fixture
    [[ "$output" == *fixture-before-runtime* ]]
  done
  printf '{"session":"uncommitted-only"}\n' >>"$SB/docs/process/provenance/sessions.jsonl"
  candidate_shell mg_sync_server__author_session uncommitted-only
  [ "$status" -ne 0 ] && [[ "$output" == *candidate-author-session* ]]
  printf '{"session":"producer-fixture"}\n' >>"$SB/docs/process/provenance/sessions.jsonl"
  git -C "$SB" add docs/process/provenance/sessions.jsonl
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm 'synthetic duplicate record'
  candidate_shell mg_sync_server__author_session producer-fixture
  [ "$status" -ne 0 ] && [[ "$output" == *candidate-author-session* ]]
}

@test "sync-server: complete phase reaches its guard with valid session arguments and retained build receipt" {
  candidate_build_fixture
  candidate_build_check
  [ "$status" -ne 0 ] && [[ "$output" == *fixture-ready-for-runtime* ]]
  [[ "$output" != *unexpected-docker* ]]
  local job
  for job in mg_sync_server_candidate_build mg_sync_server_interop; do
    local args=(--lane desktop --phase full)
    [ "$job" != mg_sync_server_interop ] || args+=(--build-receipt "$CANDIDATE_BUILD/result.json")
    args+=(--author-session producer-fixture)
    candidate_shell "$job" "${args[@]}"
    [[ "$output" == *fixture-before-runtime* ]] && [[ "$output" != *usage* ]]
    args[3]=media
    candidate_shell "$job" "${args[@]}"
    if [ "$job" = mg_sync_server_interop ]; then
      [[ "$output" == *fixture-before-runtime* ]] && [[ "$output" != *candidate-phase* ]]
    else
      [[ "$output" == *candidate-phase* ]] && [[ "$output" == *status:error* ]]
    fi
    # Change only the phase; preserve the valid producer and every receipt operand.
    args[3]=complete
    candidate_shell "$job" "${args[@]}"
    [[ "$output" == *candidate-phase* ]] && [[ "$output" == *status:error* ]]
    [[ "$output" != *usage* ]] && [[ "$output" != *candidate-author-session* ]]
    [[ "$output" != *fixture-before-runtime* ]]
  done
}

@test "sync-server: empty owned build target image command mount and copied executable are revalidated" {
  mkdir -m 700 "$BATS_TEST_TMPDIR/fresh-target"
  run bash -c 'mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"; mg_sync_server__target_identity "$2" empty' fixture "$SB" "$BATS_TEST_TMPDIR/fresh-target"
  [ "$status" -eq 0 ]
  candidate_build_fixture
  candidate_build_check
  [[ "$output" == *fixture-ready-for-runtime* ]]
  cp "$CANDIDATE_BUILD/result.json" "$BATS_TEST_TMPDIR/build-receipt-original.json"
  cp "$CANDIDATE_BUILD/build-container.json" "$BATS_TEST_TMPDIR/build-container-original.json"
  cp "$CANDIDATE_BUILD/product/build.jsonl" "$BATS_TEST_TMPDIR/build-artifact-original.json"
  local mutation
  for mutation in "receipt['details']['candidate_build_inputs']['target_initially_empty']=False" \
    "receipt['details']['candidate_build_inputs']['target_host_directory']='/shared/cache'" \
    "receipt['details']['candidate_build_inputs']['target_owner']['inode']+=1" \
    "receipt['details']['candidate_build_inputs']['command'].pop()" \
    "receipt['details']['candidate_build_inputs']['lock_sha256']='0'*64" \
    "container[0]['Image']='sha256:'+'0'*64" \
    "container[0]['Config']['Env']=['CARGO_TARGET_DIR=/cargo-target/server-candidate']" \
    "container[0]['Mounts'][2]['Source']='/shared/cache'" \
    "container[0]['Config']['Labels']['meguri.sync.owner']='unowned'" \
    "artifact['fresh']=True" "artifact['features']=['test-hooks']" \
    "artifact['profile']['opt_level']='3'" \
    "(base/'target/debug/meguri-sync-server').write_bytes(b'unrelated cached binary')"; do
    cp "$CANDIDATE_BUILD/product/meguri-sync-server" "$CANDIDATE_BUILD/target/debug/meguri-sync-server"
    python3 - "$CANDIDATE_BUILD" "$BATS_TEST_TMPDIR" "$mutation" <<'PY'
import json, pathlib, sys
base, originals = map(pathlib.Path, sys.argv[1:3])
receipt = json.loads((originals/'build-receipt-original.json').read_text())
container = json.loads((originals/'build-container-original.json').read_text())
artifact = json.loads((originals/'build-artifact-original.json').read_text())
exec(sys.argv[3])
for path, value in [('result.json',receipt),('build-container.json',container),('product/build.jsonl',artifact)]:
    (base/path).write_text(json.dumps(value))
PY
    candidate_build_check
    [ "$status" -ne 0 ] && [[ "$output" != *fixture-ready-for-runtime* ]]
    [[ "$output" == *build-command* || "$output" == *build-measurement* ]]
  done
  run bash -c 'mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"; mg_sync_server__target_identity "$2" empty' fixture "$SB" "$CANDIDATE_BUILD/target"
  [ "$status" -ne 0 ] && [[ "$output" == *'not empty'* ]]
}

@test "sync-server: image discovery resolves configured tags without existing containers" {
  run bash -c '
    mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"
    MG_ROOT="$1"
    # The service config may name an explicit tag or the Compose default tag.
    mg_compose() {
      printf "%s\n" "$@" >>"$log"
      case "$*" in *"config --images dev") echo configured-dev-tag ;;
        *"config --images sync-reference") echo configured-reference-tag ;; *) return 97 ;; esac
    }
    docker() {
      [ "$1 $2 $3 $4" = "image inspect --format {{.Id}}" ] || return 98
      case "$5" in configured-dev-tag|configured-reference-tag) printf "sha256:%064d\n" 8 ;; *) return 99 ;; esac
    }
    log="$2"
    mg_sync_server__image dev && mg_sync_server__image sync-reference -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml"
  ' fixture "$SB" "$BATS_TEST_TMPDIR/image-config"
  [ "$status" -eq 0 ]
  [ "$(grep -c '^config$' "$BATS_TEST_TMPDIR/image-config")" -eq 2 ]
  ! grep -Fx images "$BATS_TEST_TMPDIR/image-config"
}

@test "sync-server: candidate actual output preserves final selection and distinct runtime and peer identities" {
  candidate_output_fixture
  candidate_output_check
  [ "$status" -eq 0 ]
  jq -e '.errors==[] and (.invocation.selection|length)==11 and .invocation.executable_sha256==("1"*64) and .summary.total==11 and .summary.selection_credit and (.summary.broad_catalog_credit|not)' <<<"$output"
  candidate_mutate "[action.pop('fixture_sha256') for case in evidence['cases'] for action in case['actions']]"
  candidate_output_check
  [ "$status" -eq 0 ]
  jq -e '.errors==[]' <<<"$output"
  local field
  for field in source_commit batch_base source_sha256 lock_sha256 executable_sha256 mg_job arguments run_id author_session; do
    candidate_output_fixture
    candidate_mutate "invocation['$field'] = 'stale-identity'"
    candidate_output_check
    [ "$status" -ne 0 ]
  done
}

@test "sync-server: normal and media candidate counts and phases stay bound to their selected contracts" {
  local phase count
  for phase in normal media; do
    count=10
    [ "$phase" != media ] || count=4
    candidate_output_fixture "$phase"
    candidate_output_check
    [ "$status" -eq 0 ]
    jq -e --argjson n "$count" '.errors==[] and .summary.total==$n and (.invocation.selection|length)==$n' <<<"$output"
    candidate_mutate "summary['completed_case_count']=$((count - 1))"
    candidate_output_check
    [ "$status" -ne 0 ]
    candidate_output_fixture "$phase"
    candidate_mutate "expected=json.loads((base/'expected.json').read_text()); expected['arguments'][3]='full'; (base/'expected.json').write_text(json.dumps(expected)); invocation['arguments'][3]='full'; evidence['arguments'][3]='full'"
    candidate_output_check
    [ "$status" -ne 0 ]
    jq -e '.errors==["invalid fixed selection"]' <<<"$output"
  done
}

@test "sync-server: candidate executable binds expected final and optional evidence identities" {
  candidate_output_fixture
  candidate_mutate "evidence['executable_sha256']=invocation['executable_sha256']"
  candidate_output_check
  [ "$status" -eq 0 ]
  local mutation
  for mutation in "invocation['executable_sha256']='0'*64" \
    "expected=json.loads((base/'expected.json').read_text()); expected['executable_sha256']='0'*64; (base/'expected.json').write_text(json.dumps(expected)); invocation['executable_sha256']='0'*64" \
    "evidence['executable_sha256']='0'*64" \
    "del invocation['executable_sha256']" \
    "evidence['unknown']=True" "del evidence['source_commit']" \
    "invocation['unknown']=True"; do
    candidate_output_fixture
    candidate_mutate "$mutation"
    candidate_output_check
    [ "$status" -ne 0 ]
    case "$mutation" in
      expected=*) jq -e '.errors==["invocation executable differs from owned candidate"]' <<<"$output" ;;
      "evidence['executable_sha256']="*) jq -e '.errors==["evidence executable differs from owned candidate"]' <<<"$output" ;;
    esac
  done
}

@test "sync-server: candidate invocation source and server lock cannot use unrelated reference identities" {
  local field
  for field in source_commit source_sha256 lock_sha256; do
    candidate_output_fixture
    candidate_mutate "expected=json.loads((base/'expected.json').read_text()); expected['$field']='0'*len(expected['$field']); (base/'expected.json').write_text(json.dumps(expected)); invocation['$field']=expected['$field']; evidence['$field']=expected['$field']"
    candidate_output_check
    [ "$status" -ne 0 ]
    jq -e '.errors==["invocation source or lock differs from owned candidate"]' <<<"$output"
  done
}

@test "sync-server: candidate rejects missing zero incomplete duplicate or failed execution" {
  local mutation
  for mutation in "summary['total']=0" "summary['selected_case_count']=0" \
    "summary['completed_case_count']=10" "summary['total']=True" "summary['skipped']=1" \
    "summary['broad_catalog_credit']=True" "summary['selection_credit']=False" \
    "evidence['cases'].pop()" "evidence['cases'][1]=evidence['cases'][0]" \
    "evidence['cases'][0]['status']='unavailable'" "evidence['cases'][0]['status']='failed'" \
    "invocation['selection'].pop()" "invocation['selection'][1]=invocation['selection'][0]"; do
    candidate_output_fixture
    candidate_mutate "$mutation"
    candidate_output_check
    [ "$status" -ne 0 ]
  done
  local file
  for file in summary.json invocation.json evidence.json; do
    candidate_output_fixture
    rm "$CANDIDATE_FIXTURE/out/$file"
    candidate_output_check
    [ "$status" -ne 0 ]
  done
}

@test "sync-server: candidate binds selected case contracts peers observations and artifact bytes" {
  local mutation
  for mutation in "evidence['arguments']=[]" "evidence['source_sha256']='0'*64" \
    "evidence['cases'][0]['lane']['server']['source_sha256']='0'*64" \
    "evidence['cases'][0]['lane']['kind']='reference'" \
    "evidence['cases'][0]['lane']['client']['kind']='service'" \
    "evidence['cases'][0]['variants']=['unselected']" \
    "evidence['cases'][0]['family']='P-F01'" \
    "action=evidence['cases'][0]['actions'][0]; key=next(key for key in action if key!='fixture_sha256'); action[key]={'wrong_static_value':True}" \
    "action=evidence['cases'][0]['actions'][0]; del action[next(key for key in action if key!='fixture_sha256')]" \
    "evidence['cases'][0]['actions'].append(evidence['cases'][0]['actions'][0])" \
    "evidence['cases'][0]['actions'][0]['unknown']=True" \
    "evidence['cases'][0]['actions'][0]['fixture_sha256']='not-a-digest'" \
    "evidence['cases'][0]['actions'][0]['fixture_sha256']='0'*64" \
    "evidence['cases'][0]['actions'][0]['fixture_sha256']=evidence['cases'][0]['artifacts'][1]['sha256']" \
    "evidence['cases'][0]['artifacts'].pop()" \
    "(base/'out/artifacts'/evidence['cases'][0]['actions'][0]['fixture_sha256']).unlink()" \
    "(base/'out/artifacts'/evidence['cases'][0]['actions'][0]['fixture_sha256']).write_bytes(b'changed action fixture')" \
    "evidence['cases'][0]['observed'][0]['value']='mismatch'" \
    "evidence['cases'][0]['observed'].append(evidence['cases'][0]['observed'][0])" \
    "evidence['cases'][0]['artifacts'][0]['bytes']=0" \
    "evidence['cases'][0]['fixture_sha256']='0'*64" \
    "(base/'out/artifacts'/evidence['cases'][0]['artifacts'][0]['sha256']).write_bytes(b'changed')" \
    "evidence['cases'][0]['artifacts']=[a for a in evidence['cases'][0]['artifacts'] if a['role']!='mg_receipt']"; do
    candidate_output_fixture
    candidate_mutate "$mutation"
    candidate_output_check
    [ "$status" -ne 0 ]
  done
}

@test "sync-server: candidate receipt cannot self-assert unrelated source while naming current HEAD" {
  producer_ledger
  mkdir -p "$SB/core/crates/meguri-model" "$BATS_TEST_TMPDIR/verify"
  touch "$SB/core/crates/meguri-model/fixture" "$SB/core/Cargo.toml" "$SB/core/rust-toolchain.toml" "$SB/LICENSE"
  printf 'version = "0.0.0"\n' >"$SB/server/Cargo.toml"
  git -C "$SB" add core server/Cargo.toml LICENSE
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm 'synthetic frozen inputs'
  local dir="$SB/build/mg/sync-server-candidate-build/20261115T000000Z-2"
  mkdir -p "$dir/source/server" "$dir/product"
  cp "$SB/server/Cargo.lock" "$dir/source/server/Cargo.lock"
  printf 'unrelated archive' >"$dir/source.tar"
  printf 'synthetic executable' >"$dir/product/meguri-sync-server"
  jq -nc --arg root "$SB" --arg commit "$(git -C "$SB" rev-parse HEAD)" \
    --arg unrelated "$(shasum -a 256 "$dir/source.tar" | awk '{print $1}')" '
    {job:"sync-server-candidate-build",status:"pass",dry_run:false,root:$root,
     run_id:"20261115T000000Z-2",commit:{sha:$commit,dirty:false},
     args:["--lane","desktop","--phase","full","--author-session","producer-fixture"],
     details:{author_session:"producer-fixture",candidate_build:{source_dirty:false,features:[],profile:"debug",
       server:{kind:"meguri_sync",version:"0.0.0",build:$commit,source_commit:$commit,source_sha256:$unrelated}}}}
  ' >"$dir/result.json"
  candidate_shell mg_sync_server__interop_owned "$dir/result.json" '[]' producer-fixture
  [ "$status" -ne 0 ] && [[ "$output" == *build-source* ]]
  # Even after a current archive is supplied, a stale retained lock is refused.
  bash -c 'mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"; MG_ROOT="$1"; mg_sync_server__archive HEAD' fixture "$SB" >"$dir/source.tar"
  printf 'unrelated lock' >"$dir/source/server/Cargo.lock"
  candidate_shell mg_sync_server__interop_owned "$dir/result.json" '[]' producer-fixture
  [ "$status" -ne 0 ] && [[ "$output" == *build-source* ]]
}

@test "sync-server: candidate publishes final actual outputs counts artifacts and runtime failure" {
  candidate_output_fixture
  mkdir -p "$SB/server/tools" "$SB/conformance/recordings/scenarios/meguri-sync-server"
  cp "$REPO_ROOT/server/tools/candidate-output.py" "$SB/server/tools/"
  cp "$CANDIDATE_FIXTURE/selection.json" "$SB/conformance/recordings/scenarios/meguri-sync-server/candidate-full-selection.json"
  local pair rc validation_rc
  for pair in '0 0' '19 0' '0 23' '19 23'; do
    read -r rc validation_rc <<<"$pair"
    local run_dir="$BATS_TEST_TMPDIR/publisher-$rc-$validation_rc"
    mkdir -p "$run_dir/out" "$run_dir/input"
    cp "$CANDIDATE_FIXTURE/peer.json" "$run_dir/input/peer.json"
    printf '{"username":"synthetic","password":"synthetic-fixture"}\n' >"$run_dir/credentials.json"
    chmod 600 "$run_dir/credentials.json"
    run bash -c '
      mg_job() { :; }
      . "$1/tools/mg.d/host.sh"
      . "$1/tools/mg.d/meguri-sync.sh"
      export MG_ROOT="$1" MG_RUN_DIR="$2" MG_JOB=sync-server-interop MG_RUN_ID=20261115T000000Z-1
      fixture_rc="$3" fixture_validation_rc="$4"
      mg_failure() { echo "$1" >>"$MG_RUN_DIR/failures"; }
      mg_detail() { jq -nc --arg key "$1" --argjson value "$2" "{(\$key):\$value}" >>"$MG_RUN_DIR/details"; }
      mg_count() { printf "%s=%s\n" "$1" "$2" >>"$MG_RUN_DIR/counts"; }
      mg_artifact() { printf "%s=%s\n" "$1" "$2" >>"$MG_RUN_DIR/artifacts"; }
      mg_sync_server__author_session() { [ "$1" = producer-fixture ]; }
      mg_sync_server__reference_identity() { printf "{\"source_sha256\":\"%064d\",\"lock_sha256\":\"%064d\"}\n" 0 0; }
      mg_status() { echo "$1" >>"$MG_RUN_DIR/status"; }
      mg_compose() {
        case "$*" in *"build sync-reference") return 0 ;;
          *"config --images sync-reference") echo synthetic-image ;; *) return 97 ;; esac
      }
      docker() {
        if [ "$1" = image ]; then printf "sha256:%064d\n" 0; return; fi
        if [ "$1 $2" = "container ls" ]; then return 0; fi
        [ "$1" = run ] || return 97
        local mode=client arg
        for arg in "$@"; do [ "$arg" != validate ] || mode=validation; done
        # Hooks must already be registered before either fallible run call.
        printf "%s\n" "${MG_EXIT_CMDS[@]}" | grep -F "meguri-candidate-$mode-$MG_RUN_ID" >/dev/null || return 98
        printf "%s\n" "$@" >"$MG_RUN_DIR/$mode-argv"
        if [ "$mode" = validation ]; then
          python3 - "$MG_RUN_DIR" <<PY
import json, pathlib, sys
base = pathlib.Path(sys.argv[1])
validation = base/"validation"
assert json.loads((validation/"invocation.json").read_text()) == json.loads((base/"out/invocation.json").read_text())
# Deliberately distinct metadata proves validation cannot overwrite execution or
# add a second set of counts. This is a stub, not test-tool acceptance evidence.
(validation/"summary.json").write_text(json.dumps(dict(total=999, passed=999, failed=0)))
invocation = json.loads((validation/"invocation.json").read_text())
invocation["selection"] = ["validation-fixture"]
(validation/"invocation.json").write_text(json.dumps(invocation))
PY
          return "$fixture_validation_rc"
        fi
        # Synthetic client augments the actual input invocation, as the public
        # runtime interface does. No original driver is loaded or executed.
        python3 - "$CANDIDATE_FIXTURE" "$MG_RUN_DIR/out" <<PY
import json, pathlib, shutil, sys
base, output = map(pathlib.Path, sys.argv[1:])
invocation = json.loads((output/"invocation.json").read_text())
invocation["selection"] = json.loads((base/"out/invocation.json").read_text())["selection"]
(output/"invocation.json").write_text(json.dumps(invocation))
evidence = json.loads((base/"out/evidence.json").read_text())
for key in ("source_commit", "batch_base", "source_sha256", "lock_sha256", "mg_job", "arguments"):
    evidence[key] = invocation[key]
(output/"evidence.json").write_text(json.dumps(evidence))
shutil.copyfile(base/"out/summary.json", output/"summary.json")
shutil.copytree(base/"out/artifacts", output/"artifacts")
PY
        return "$fixture_rc"
      }
      server="$(jq -c .server "$MG_RUN_DIR/input/peer.json")"
      arguments="$(jq -c .arguments "$CANDIDATE_FIXTURE/expected.json")"
      mg_sync_server_candidate_client owned-container "$MG_RUN_DIR/input" "$MG_RUN_DIR/credentials.json" "$MG_RUN_DIR/out" "$server" "$arguments" producer-fixture
    ' fixture "$SB" "$run_dir" "$rc" "$validation_rc"
    local credit=false
    if [ "$rc" -eq 0 ] && [ "$validation_rc" -eq 0 ]; then
      [ "$status" -eq 0 ]; credit=true
    else [ "$status" -ne 0 ]; fi
    [ "$rc" -eq 0 ] || grep -Fx candidate-client-exit-19 "$run_dir/failures"
    [ "$validation_rc" -eq 0 ] || grep -Fx candidate-validation-exit-23 "$run_dir/failures"
    jq -se --argjson credit "$credit" 'add | .candidate_compatibility_credit==$credit and
      (.reference_invocation.selection|length)==11 and .reference_invocation.author_session=="producer-fixture" and
      .reference_invocation.source_commit==("a"*40) and .reference_invocation.source_sha256==("c"*64) and
      .reference_invocation.lock_sha256==("d"*64) and .reference_invocation.executable_sha256==("1"*64) and
      .candidate_reference_inputs.source_sha256==("0"*64) and .reference_evidence.total==11' "$run_dir/details"
    jq -e '.executable_sha256==("1"*64)' "$run_dir/candidate-invocation.json"
    [ "$(cat "$run_dir/counts")" = "$(printf 'total=11\npassed=11\nfailed=0')" ]
    grep -Fx "reference-output=$run_dir/out" "$run_dir/artifacts"
    grep -Fx "reference-evidence=$run_dir/out/evidence.json" "$run_dir/artifacts"
    grep -Fx /opt/reference/driver.py "$run_dir/client-argv"
    grep -Fx -- --read-only "$run_dir/client-argv"
    grep -Fx container:owned-container "$run_dir/client-argv"
    grep -Fx 'meguri.mg.run-id=20261115T000000Z-1' "$run_dir/client-argv"
    grep -Fx "candidate-semantic-validation=$run_dir/validation" "$run_dir/artifacts"
    jq -e '(.selection|length)==11' "$run_dir/out/invocation.json"
    jq -e '.total==999' "$run_dir/validation/summary.json"
    grep -Fx "type=bind,src=$run_dir/out,dst=/in,readonly" "$run_dir/validation-argv"
    grep -Fx "type=bind,src=$run_dir/validation,dst=/out" "$run_dir/validation-argv"
    grep -Fx /opt/reference/driver.py "$run_dir/validation-argv"
    [ "$(tail -n 3 "$run_dir/validation-argv")" = "$(printf 'validate\n--input\n/in')" ]
    [ "$(sed -n '/^--network$/{n;p;}' "$run_dir/validation-argv")" = none ]
    ! grep -E '/secret|credentials|/candidate$|/owned|/data|container:' "$run_dir/validation-argv"
    local argv
    for argv in "$run_dir/client-argv" "$run_dir/validation-argv"; do
      grep -Fx -- --read-only "$argv"
      [ "$(sed -n '/^--cap-drop$/{n;p;}' "$argv")" = ALL ]
      [ "$(sed -n '/^--security-opt$/{n;p;}' "$argv")" = no-new-privileges ]
      [ "$(sed -n '/^--memory$/{n;p;}' "$argv")" = 2g ]
      [ "$(sed -n '/^--pids-limit$/{n;p;}' "$argv")" = 128 ]
      [ "$(sed -n '/^--user$/{n;p;}' "$argv")" = "$(id -u):$(id -g)" ]
      grep -Fx /scratch:exec,mode=1777,size=1g "$argv"
      grep -Fx /tmp:mode=1777,size=64m "$argv"
      grep -Fx 'meguri.mg.run-id=20261115T000000Z-1' "$argv"
    done
  done
}

@test "sync-server: real MG job owns exchange hooks and retains cleanup failures after later success" {
  # A normal MG fixture job uses host.sh's real registration, EXIT runner and
  # result writer. Only the finite container/client interface is synthetic.
  cat >"$SB/tools/mg.d/zz-sync-owned-fixture.sh" <<'SH'
mg_job sync-owned-fixture host sync_owned_fixture "synthetic owned exchange"
sync_owned_fixture() {
  # Real EXIT hooks run after this function returns; keep their fixture state
  # in the owning job shell rather than in this function's dynamic scope.
  fixture_shell="$BASH_SUBSHELL" fixture_mode="$1" fixture_receipt="$2"
  fixture_phase=full fixture_count=11
  case "$fixture_mode" in
    normal) fixture_phase=normal; fixture_count=10 ;;
    media) fixture_phase=media; fixture_count=4 ;;
    unknown) fixture_phase=unknown ;;
    wrong-count) fixture_phase=media ;;
  esac
  local arguments
  arguments="$(mg_json_array --lane desktop --phase "$fixture_phase" --build-receipt "$fixture_receipt" --author-session producer-fixture)"
  if ! mg_sync_server__interop_owned "$fixture_receipt" "$arguments" producer-fixture; then mg_status fail; fi
  mg_detail fixture_hooks "${#MG_EXIT_CMDS[@]}"
  printf 'returned\n' >>"$MG_RUN_DIR/events"
}
mg_sync_server_candidate_client() {
  local kind name owner=11111111111111111111111111111111
  printf '%s\n' "$BASH_SUBSHELL" >"$MG_RUN_DIR/client-shell"
  printf '%s\n' "$fixture_shell" >"$MG_RUN_DIR/owner-shell"
  printf '%s\n' "${3%/*}" >"$MG_RUN_DIR/secret-path"
  [ -f "$3" ] && [ -f "$MG_RUN_DIR/$(printf '%064d' 7).live" ] || return 1
  for kind in client validation; do
    name="meguri-candidate-$kind-$MG_RUN_ID"
    mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
    docker create --name "$name" --label "meguri.sync.owner=$owner" >/dev/null || return 1
  done
  mg_count total "$fixture_count"; mg_count passed "$fixture_count"; mg_count failed 0
  mg_detail reference_invocation '{}'; mg_detail reference_evidence '{}'
}
docker() {
  local name owner id file kind
  case "$1" in
    create)
      shift
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --name) name="$2"; shift ;;
          --label) case "$2" in meguri.sync.owner=*) owner="${2#*=}" ;; esac; shift ;;
        esac
        shift
      done
      case "$name" in
        "meguri-candidate-$MG_RUN_ID") id="$(printf '%064d' 7)" ;;
        "meguri-candidate-client-$MG_RUN_ID") id="$(printf '%064d' 6)" ;;
        "meguri-candidate-validation-$MG_RUN_ID") id="$(printf '%064d' 5)" ;;
        *) return 97 ;;
      esac
      # Actual registrations must precede each fallible creation, including
      # the parent's secret hook and the client's/validator's own hooks.
      printf '%s\n' "${MG_EXIT_CMDS[@]}" | grep -F "$name" >/dev/null || return 98
      [ "${#MG_EXIT_CMDS[@]}" -ge 2 ] || return 98
      jq -nc --arg id "$id" --arg name "/$name" --arg run "$MG_RUN_ID" --arg owner "$owner" \
        '[{Id:$id,Name:$name,State:{Running:true},Config:{Labels:{"meguri.mg.run-id":$run,"meguri.sync.owner":$owner}},HostConfig:{NetworkMode:"none",PortBindings:{}}}]' >"$MG_RUN_DIR/$id.json"
      touch "$MG_RUN_DIR/$id.live"; echo "$id" ;;
    start) [ -f "$MG_RUN_DIR/$2.live" ] ;;
    inspect) [ -f "$MG_RUN_DIR/$2.live" ] && cat "$MG_RUN_DIR/$2.json" ;;
    exec)
      case "$*" in
        *'sha256sum /proc/1/exe') jq -r '.details.candidate_build.server.executable_sha256' "$fixture_receipt" ;;
        "exec -i "*" python3 /owned/setup.py") return 0 ;;
        *) return 97 ;;
      esac ;;
    container)
      [ "$2 $3 $4 $5" = 'ls -aq --no-trunc --filter' ] || return 97
      name="${6#name=^/}"; name="${name%\$}"
      for file in "$MG_RUN_DIR"/*.live; do
        [ -f "$file" ] || continue
        jq -r --arg name "/$name" '.[] | select(.Name==$name) | .Id' "${file%.live}.json"
      done ;;
    rm)
      [ "$2" = -f ] || return 97
      name="$(jq -r '.[0].Name' "$MG_RUN_DIR/$3.json")"
      case "$name" in
        "/meguri-candidate-$MG_RUN_ID") kind=candidate ;;
        "/meguri-candidate-client-$MG_RUN_ID") kind=client ;;
        "/meguri-candidate-validation-$MG_RUN_ID") kind=validation ;;
        *) return 97 ;;
      esac
      printf 'remove:%s\n' "$kind" >>"$MG_RUN_DIR/events"
      printf '%s\n' "$BASH_SUBSHELL" >>"$MG_RUN_DIR/cleanup-shells"
      if [ "$fixture_mode" = "$kind" ] && [ ! -e "$MG_RUN_DIR/failed-once" ]; then
        touch "$MG_RUN_DIR/failed-once"; return 99
      fi
      rm "$MG_RUN_DIR/$3.live" ;;
    *) return 97 ;;
  esac
}
SH
  git -C "$SB" add tools/mg.d/zz-sync-owned-fixture.sh
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm 'synthetic owned job'
  candidate_build_fixture
  local mode run_dir cleanup_shell remaining
  for mode in pass normal media unknown wrong-count candidate client validation; do
    run --separate-stderr "$SB/tools/mg" sync-owned-fixture "$mode" "$CANDIDATE_BUILD/result.json"
    result="${lines[${#lines[@]} - 1]:-}"
    if [ "$mode" = pass ] || [ "$mode" = normal ] || [ "$mode" = media ]; then
      [ "$status" -eq 0 ]; [ "$(field .status)" = pass ]
      [ "$(field '.failures | length')" -eq 0 ]
    elif [ "$mode" = unknown ] || [ "$mode" = wrong-count ]; then
      [ "$status" -eq 1 ]; [ "$(field .status)" = fail ]
      jq -e 'any(.failures[]; .id=="candidate-interop")' "$result"
    else
      [ "$status" -eq 1 ]; [ "$(field .status)" = fail ]
      jq -e 'any(.failures[]; .id=="candidate-cleanup")' "$result"
    fi
    run_dir="$(dirname "$result")"
    [ "$(field .details.fixture_hooks)" -eq 4 ]
    cmp "$run_dir/owner-shell" "$run_dir/client-shell"
    while IFS= read -r cleanup_shell; do
      [ "$cleanup_shell" = "$(cat "$run_dir/owner-shell")" ]
    done <"$run_dir/cleanup-shells"
    # Only normal parent cleanup precedes the return. Client/validator hooks
    # run at job exit, then successful parent/secret hooks cannot erase a fail.
    [ "$(head -n 3 "$run_dir/events")" = "$(printf 'remove:candidate\nreturned\nremove:validation')" ]
    [ "$(tail -n 1 "$run_dir/events")" = "remove:client" ] || [ "$mode" = candidate ]
    [ "$mode" != candidate ] || [ "$(tail -n 1 "$run_dir/events")" = 'remove:candidate' ]
    [ ! -d "$(cat "$run_dir/secret-path")" ]
    remaining="$(find "$run_dir" -name '*.live' | wc -l | tr -d ' ')"
    case "$mode" in
      client) [ "$remaining" -eq 1 ]; [ -f "$run_dir/$(printf '%064d' 6).live" ] ;;
      validation) [ "$remaining" -eq 1 ]; [ -f "$run_dir/$(printf '%064d' 5).live" ] ;;
      *) [ "$remaining" -eq 0 ] ;;
    esac
    validate
  done
}

@test "sync-server: reference cleanup publication failures persist through real MG EXIT hooks and later success" {
  # Use MG's real status/failure sinks, host.sh EXIT runner and result writer.
  # Faults affect only one cleanup publication; a later real hook succeeds.
  cat >"$SB/tools/mg.d/zz-sync-reference-cleanup-fixture.sh" <<'SH'
mg_job sync-reference-cleanup-fixture host sync_reference_cleanup_fixture "synthetic reference cleanup publication"
eval "$(declare -f mg_detail | sed '1s/mg_detail/mg_fixture_detail/')"
fixture_publication_failure() {
  [ "$fixture_phase" = "$fixture_stage" ] && [ "$fixture_fault" = "$1" ] &&
    [ ! -e "$MG_RUN_DIR/publication-failed" ] || return 1
  touch "$MG_RUN_DIR/publication-failed"
  printf 'publication:%s:%s\n' "$fixture_phase" "$1" >>"$MG_RUN_DIR/events"
}
mg_detail() {
  if [ "$1" = reference_cleanup ]; then
    fixture_publication_failure sink && return 89
    printf 'detail:%s\n' "$fixture_phase" >>"$MG_RUN_DIR/events"
  fi
  mg_fixture_detail "$@"
}
jq() {
  # Fail the actual cleanup JSON construction, before mg_detail is called.
  if [ "$#" -eq 5 ] && [ "$1" = -nc ] && [ "$2" = --argjson ] && [ "$3" = rc ]; then
    fixture_publication_failure encode && return 88
  fi
  command jq "$@"
}
sync_reference_cleanup_fixture() {
  fixture_stage="$1" fixture_fault="$2" fixture_previous="$3" fixture_phase=normal
  fixture_owner=11111111111111111111111111111111
  fixture_name="meguri-reference-$MG_RUN_ID-$fixture_owner"
  mkdir -m 700 "$MG_RUN_DIR/reference-control" || return 1
  (umask 077; jq -nc --arg run "$MG_RUN_ID" --arg name "$fixture_name" --arg nonce "$fixture_owner" \
    '{run:$run,name:$name,nonce:$nonce}' >"$MG_RUN_DIR/reference-control/owner.json") || return 1
  touch "$MG_RUN_DIR/present"
  mg_on_exit mg_sync_reference__cancel_cleanup
  mg_on_exit mg_sync_reference__cancel_cleanup
  mg_on_exit 'fixture_phase=exit'
  case "$fixture_previous" in
    execution|both) mg_failure reference-execution "Synthetic earlier execution failure" ;;
  esac
  local rc=0
  mg_sync_reference__cancel_cleanup || rc=$?
  mg_detail fixture_normal_cleanup "$rc"
  if [ "$rc" -ne 0 ] || [ "$fixture_previous" != none ]; then mg_status fail; else mg_status pass; fi
  printf 'returned\n' >>"$MG_RUN_DIR/events"
}
docker() {
  printf '%s\n' "$*" >>"$MG_RUN_DIR/docker-calls"
  case "$1 ${2:-}" in
    'container ls')
      [ "$#" -eq 6 ] && [ "$3 $4 $5" = '-aq --no-trunc --filter' ] &&
        [ "$6" = "name=^/$fixture_name$" ] || return 97
      [ ! -e "$MG_RUN_DIR/present" ] || printf '%064d\n' 7
      return 0 ;;
    'inspect '*)
      [ "$#" -eq 2 ] && [ "$2" = "$(printf '%064d' 7)" ] || return 97
      jq -nc --arg id "$2" --arg name "/$fixture_name" --arg run "$MG_RUN_ID" --arg owner "$fixture_owner" \
        '[{Id:$id,Name:$name,Config:{Labels:{"meguri.mg.run-id":$run,"meguri.sync.owner":$owner}}}]' ;;
    'rm -f')
      [ "$#" -eq 3 ] && [ "$3" = "$(printf '%064d' 7)" ] || return 97
      case "$fixture_previous" in
        cleanup|both)
          if [ ! -e "$MG_RUN_DIR/removal-failed" ]; then touch "$MG_RUN_DIR/removal-failed"; return 98; fi ;;
      esac
      rm "$MG_RUN_DIR/present" ;;
    *) return 97 ;;
  esac
}
SH
  local scenario stage fault previous run_dir expected_rc
  for scenario in exit:none:none normal:sink:none exit:sink:none normal:encode:none exit:encode:none \
    exit:sink:execution exit:sink:cleanup exit:encode:both; do
    IFS=: read -r stage fault previous <<<"$scenario"
    run --separate-stderr "$SB/tools/mg" sync-reference-cleanup-fixture "$stage" "$fault" "$previous"
    result="${lines[${#lines[@]} - 1]:-}"
    run_dir="$(dirname "$result")"
    if [ "$fault" = none ]; then
      [ "$status" -eq 0 ] && [ "$(field .status)" = pass ]
      jq -e '.failures==[]' "$result"
    else
      [ "$status" -eq 1 ] && [ "$(field .status)" = fail ]
      jq -e '[.failures[] | select(.id=="reference-cleanup-publication")] | length==1' "$result"
      grep -Fx "publication:$stage:$fault" "$run_dir/events"
    fi
    # A failing EXIT hook return is discarded by the runner; the durable
    # failure/status must survive both that and the final successful hook.
    [ "$(tail -n 1 "$run_dir/events")" = detail:exit ]
    if [ "$stage" = exit ] && [ "$fault" != none ]; then
      [ "$(head -n 2 "$run_dir/events")" = "$(printf 'detail:normal\nreturned')" ]
    fi
    expected_rc=0
    if [ "$stage" = normal ] || [ "$previous" = cleanup ] || [ "$previous" = both ]; then expected_rc=1; fi
    jq -e --argjson rc "$expected_rc" '.details.fixture_normal_cleanup==$rc and
      .details.reference_cleanup=={checked:true,exit_code:0,absence_verified:true,late_create_excluded:false}' "$result"
    case "$previous" in
      execution|both) jq -e 'any(.failures[]; .id=="reference-execution")' "$result" ;;
    esac
    case "$previous" in
      cleanup|both) jq -e 'any(.failures[]; .id=="candidate-cleanup")' "$result" ;;
    esac
    [ ! -e "$run_dir/present" ]
    validate
  done
}

@test "sync-server: container cleanup checks ownership and propagates removal or verification failures" {
  local mode
  for mode in owned unowned wrong-nonce wrong-name wrong-id missing-run missing-nonce inspect-failed invalid-id multiple-ids remove-failed still-present list-failed absent; do
    local dir="$BATS_TEST_TMPDIR/cleanup-$mode"
    mkdir "$dir"
    [ "$mode" = absent ] || touch "$dir/present"
    run bash -c '
      mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"
      MG_RUN_ID=20261115T000000Z-1
      dir="$2" mode="$3" owner=synthetic-owner
      mg_failure() { echo "$1"; }
      mg_status() { echo "status:$1"; }
      docker() {
        printf "%s\n" "$*" >>"$dir/commands"
        case "$1 $2" in
          "container ls")
            [ "$mode" != list-failed ] || return 97
            [ "$mode" != invalid-id ] || { echo not-an-id; return 0; }
            [ "$mode" != multiple-ids ] || { printf "%064d\n%064d\n" 7 8; return 0; }
            [ ! -e "$dir/present" ] || printf "%064d\n" 7
            return 0 ;;
          "inspect "*)
            [ "$mode" != inspect-failed ] || return 97
            local run="$MG_RUN_ID" label_owner="$owner" inspected_id="$2" inspected_name="/meguri-candidate-$MG_RUN_ID"
            [ "$mode" != unowned ] || run=someone-else
            [ "$mode" != wrong-nonce ] || label_owner=someone-else
            [ "$mode" != wrong-name ] || inspected_name=/foreign
            [ "$mode" != wrong-id ] || inspected_id="$(printf "%064d" 8)"
            jq -nc --arg id "$inspected_id" --arg name "$inspected_name" --arg run "$run" --arg owner "$label_owner" --arg mode "$mode" \
              "[{Id:\$id,Name:\$name,Config:{Labels:{\"meguri.mg.run-id\":\$run,\"meguri.sync.owner\":\$owner}}}] |
                if \$mode==\"missing-run\" then del(.[0].Config.Labels[\"meguri.mg.run-id\"])
                elif \$mode==\"missing-nonce\" then del(.[0].Config.Labels[\"meguri.sync.owner\"]) else . end" ;;
          "rm -f")
            [ "$mode" != remove-failed ] || return 98
            [ "$mode" = still-present ] || rm "$dir/present" ;;
          *) return 99 ;;
        esac
      }
      mg_sync_server__remove_container "meguri-candidate-$MG_RUN_ID" "$owner"
    ' fixture "$SB" "$dir" "$mode"
    case "$mode" in
      owned|absent) [ "$status" -eq 0 ]; [ ! -e "$dir/present" ] ;;
      *) [ "$status" -ne 0 ]; [[ "$output" == *candidate-cleanup* ]]; [[ "$output" == *status:fail* ]]; [ -e "$dir/present" ] ;;
    esac
    case "$mode" in unowned|wrong-nonce|wrong-name|wrong-id|missing-run|missing-nonce|inspect-failed|invalid-id|multiple-ids|list-failed|absent) ! grep -q '^rm ' "$dir/commands" ;; esac
  done
}

@test "sync-server: omitted empty selftest and identity cancellation enforce bounded nonce ownership without sweeping" {
  # Source MG's actual functions in the synthetic checkout, omitting only its
  # final dispatcher call. No production predicates or helper bodies are copied.
  sed '$d' "$SB/tools/mg" >"$SB/tools/mg-functions"
  local mode selector dir
  for selector in default empty selftest identity; do
    for mode in owned record-limit oversized-control uncertain-create interrupted-attach absent unpublished invalid-record record-symlink record-permissions wrong-record-run foreign missing-nonce wrong-name wrong-id list-failed inspect-failed remove-failed still-present; do
      dir="$BATS_TEST_TMPDIR/reference-cancel-$selector-$mode"
      mkdir -p "$dir/run"
      run bash -c '
        . "$1/tools/mg-functions"
        mg__load_families
        MG_RUN_DIR="$2/run" MG_RUN_ID=reference-cancel MG_JOB=sync-server-reference
        mode="$3" selector="$4" dir="$2"
        case "$selector" in
          default) args=() ;;
          empty) args=("") ;;
          *) args=("$selector") ;;
        esac
        mg_failure() { echo "$1"; }
        mg_status() { echo "status:$1"; }
        mg_detail() { printf "%s=%s\n" "$1" "$2" >>"$dir/details"; }
        mg__remove_unreserved_run_containers() { echo sweep >>"$dir/events"; }
        owner=11111111111111111111111111111111
        name="meguri-reference-$MG_RUN_ID-$owner"
        if [ "$mode" != unpublished ]; then
          mkdir -m 700 "$MG_RUN_DIR/reference-control"
          (umask 077; jq -nc --arg run "$MG_RUN_ID" --arg name "$name" --arg nonce "$owner" \
            "{run:\$run,name:\$name,nonce:\$nonce}" >"$MG_RUN_DIR/reference-control/owner.json")
          case "$mode" in
            record-limit|oversized-control)
              limit=4096
              [ "$mode" != oversized-control ] || limit=4097
              size="$(wc -c <"$MG_RUN_DIR/reference-control/owner.json")"
              printf "%*s" "$((limit-size))" "" >>"$MG_RUN_DIR/reference-control/owner.json" ;;
            invalid-record) printf "{}\n" >"$MG_RUN_DIR/reference-control/owner.json" ;;
            record-symlink)
              mv "$MG_RUN_DIR/reference-control/owner.json" "$dir/other.json"
              ln -s "$dir/other.json" "$MG_RUN_DIR/reference-control/owner.json" ;;
            record-permissions) chmod 644 "$MG_RUN_DIR/reference-control/owner.json" ;;
            wrong-record-run)
              jq ".run=\"foreign\"" "$MG_RUN_DIR/reference-control/owner.json" >"$dir/changed"
              cat "$dir/changed" >"$MG_RUN_DIR/reference-control/owner.json" ;;
          esac
        fi
        case "$mode" in absent|unpublished) ;; *) touch "$dir/present" ;; esac
        docker() {
          printf "%s\n" "$*" >>"$dir/events"
          case "$1 $2" in
            "container ls")
              [ "$6" = "name=^/$name$" ] || return 96
              [ "$mode" != list-failed ] || return 97
              [ ! -e "$dir/present" ] || printf "%064d\n" 7
              return 0 ;;
            "inspect "*)
              [ "$mode" != inspect-failed ] || return 97
              local run="$MG_RUN_ID" nonce="$owner" actual_name="/$name" actual_id="$2"
              [ "$mode" != foreign ] || run=foreign
              [ "$mode" != missing-nonce ] || nonce=
              [ "$mode" != wrong-name ] || actual_name=/foreign
              [ "$mode" != wrong-id ] || actual_id="$(printf "%064d" 8)"
              local state=exited
              [ "$mode" != uncertain-create ] || state=created
              [ "$mode" != interrupted-attach ] || state=running
              jq -nc --arg id "$actual_id" --arg name "$actual_name" --arg run "$run" --arg owner "$nonce" \
                --arg state "$state" \
                "[{Id:\$id,Name:\$name,State:{Status:\$state,Running:(\$state==\"running\")},
                   Config:{Labels:{\"meguri.mg.run-id\":\$run,\"meguri.sync.owner\":\$owner}}}]" ;;
            "rm -f")
              [ "$mode" != remove-failed ] || return 98
              [ "$mode" = still-present ] || rm "$dir/present" ;;
            *) return 99 ;;
          esac
        }
        mg__remove_run_containers
      ' fixture "$SB" "$dir" "$mode" "$selector"
      case "$mode" in
        owned|record-limit|uncertain-create|interrupted-attach|absent) [ "$status" -eq 0 ]; [ ! -e "$dir/present" ] ;;
        *) [ "$status" -ne 0 ] ;;
      esac
      [ ! -e "$dir/events" ] || ! grep -Fx sweep "$dir/events"
      case "$mode" in
        unpublished|oversized-control|invalid-record|record-symlink|record-permissions|wrong-record-run)
          [ ! -e "$dir/events" ]
          [[ "$output" == *reference-cleanup* ]] && [[ "$output" == *status:fail* ]]
          [ "$mode" = unpublished ] || [ -e "$dir/present" ] ;;
        foreign|missing-nonce|wrong-name|wrong-id|list-failed|inspect-failed|absent)
          ! grep -q '^rm ' "$dir/events" ;;
      esac
    done
  done
}

@test "sync-server: other cancellation modes retain generic cleanup" {
  sed '$d' "$SB/tools/mg" >"$SB/tools/mg-functions"
  local mode
  for mode in deps lock recovery-discover recovery recovery-dirty smoke edges normal normal-sdk normal-matrix media catalog validate seal; do
    run bash -c '
      . "$1/tools/mg-functions"
      MG_JOB=sync-server-reference MG_RUN_DIR="$2" args=("$3")
      mg_sync_reference__cancel_cleanup() { echo forbidden-delegation; return 97; }
      mg__remove_unreserved_run_containers() { echo generic-cleanup; }
      mg__remove_run_containers
    ' fixture "$SB" "$BATS_TEST_TMPDIR" "$mode"
    [ "$status" -eq 0 ] && [ "$output" = generic-cleanup ]
  done
  # An extra argument must not accidentally opt into retained cancellation,
  # including the explicit empty spelling of the default mode.
  for mode in '' selftest identity; do
    run bash -c '
      . "$1/tools/mg-functions"
      MG_JOB=sync-server-reference MG_RUN_DIR="$2" args=("$3" unexpected)
      mg_sync_reference__cancel_cleanup() { echo forbidden-delegation; return 97; }
      mg__remove_unreserved_run_containers() { echo generic-cleanup; }
      mg__remove_run_containers
    ' fixture "$SB" "$BATS_TEST_TMPDIR" "$mode"
    [ "$status" -eq 0 ] && [ "$output" = generic-cleanup ]
  done
}

@test "sync-server: secret cleanup refuses replaced directories and removes only owned synthetic credentials" {
  local mode
  for mode in owned replaced; do
    local dir="$BATS_TEST_TMPDIR/secret-$mode"
    mkdir -m 700 "$dir"
    printf '{"username":"synthetic","password":"synthetic-fixture"}\n' >"$dir/credentials.json"
    chmod 600 "$dir/credentials.json"
    python3 - "$dir" "$BATS_TEST_TMPDIR/secret-owner-$mode.json" <<'PY'
import json, os, pathlib, sys
s = os.lstat(sys.argv[1])
pathlib.Path(sys.argv[2]).write_text(json.dumps([s.st_dev, s.st_ino, s.st_uid]))
PY
    if [ "$mode" = replaced ]; then mv "$dir" "$dir-original"; mkdir -m 700 "$dir"; fi
    run bash -c '
      mg_job() { :; }; . "$1/tools/mg.d/meguri-sync.sh"
      mg_failure() { echo "$1"; }; mg_status() { echo "status:$1"; }
      mg_sync_server__remove_secret "$2" "$3"
    ' fixture "$SB" "$dir" "$BATS_TEST_TMPDIR/secret-owner-$mode.json"
    if [ "$mode" = owned ]; then
      [ "$status" -eq 0 ] && [ ! -e "$dir" ]
    else
      [ "$status" -ne 0 ] && [ -d "$dir" ] && [ -f "$dir-original/credentials.json" ]
      [[ "$output" == *candidate-secret-cleanup* ]] && [[ "$output" == *status:fail* ]]
    fi
  done
}
