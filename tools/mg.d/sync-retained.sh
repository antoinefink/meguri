# SPDX-License-Identifier: MIT
# Separate retained public HTTP worker. No service/client runtime on the host.
mg_job sync-server-retained docker:dev mg_sync_retained \
  "Retained public HTTP peer workspace: prepare-lock, build, check, peer-unit (Docker only; no compatibility credit)"

mg_sync_retained() {
  local mode="${1:-}" peer="$MG_ROOT/conformance/peers/retained-sync" graph compiler_host
  [ "$#" -eq 1 ] || { mg_failure usage "sync-server-retained needs prepare-lock, build, check or peer-unit"; mg_status error; return; }
  case "$mode" in prepare-lock|build|check|peer-unit) ;; *) mg_failure usage "Unknown retained peer operation"; mg_status error; return ;; esac
  if ! python3 "$peer/support.py" check; then
    mg_failure retained-inputs "Committed selection, recipes or workspace boundaries differ"; mg_status fail; return
  fi
  if [ "$mode" = check ]; then
    mg_status pass
    return
  fi
  if [ "$mode" = prepare-lock ]; then
    if [ -e "$peer/Cargo.lock" ]; then
      mg_failure lock-exists "Prepare-lock only creates the absent separate lock; retain existing input"; mg_status fail; return
    fi
    cp "$MG_ROOT/core/Cargo.lock" "$peer/Cargo.lock" || return 1
    # Metadata minimally extends the existing locked resolution with this root.
    # Do not use generate-lockfile: it would resolve all packages afresh.
    if ! (cd "$MG_ROOT/core" && cargo metadata --offline --format-version 1 --manifest-path "$peer/Cargo.toml") >"$MG_RUN_DIR/graph.json" ||
        ! python3 "$peer/support.py" verify-lock; then
      mg_failure retained-lock "Offline lock preparation failed; inspect the retained lock before any build"; mg_status fail; return
    fi
    mg_artifact peer-lock "$peer/Cargo.lock" "Separate prepared lock; review and commit before locked verification"
    mg_message "Lock preparation only; no compiled or compatibility evidence"
    mg_status pass
    return
  fi
  if [ ! -f "$peer/Cargo.lock" ]; then
    mg_failure lock-missing "Run sync-server-retained prepare-lock in Docker, then review and commit its separate lock"; mg_status fail; return
  fi
  if [ -n "$(git -C "$MG_ROOT" status --porcelain=v1 --untracked-files=all)" ]; then
    mg_failure source-dirty "Peer builds require clean committed source and separate reviewed lock"; mg_status fail; return
  fi
  graph="$MG_RUN_DIR/graph.json"
  compiler_host="$(cd "$MG_ROOT/core" && rustc -vV | sed -n 's/^host: //p')" || return 1
  [ -n "$compiler_host" ] || { mg_failure toolchain "Compiler host identity missing"; mg_status fail; return; }
  if ! python3 "$peer/support.py" source "$MG_RUN_DIR/source-before.json" ||
      ! python3 "$peer/support.py" verify-lock ||
      ! (cd "$MG_ROOT/core" && cargo metadata --locked --offline --filter-platform "$compiler_host" --format-version 1 --manifest-path "$peer/Cargo.toml") >"$graph" ||
      ! (cd "$MG_ROOT/core" && cargo build --locked --offline --target "$compiler_host" --message-format=json --manifest-path "$peer/Cargo.toml") >"$MG_RUN_DIR/cargo-build.jsonl" ||
      ! python3 "$peer/support.py" freeze "$MG_RUN_DIR/client" "$graph" "$MG_RUN_DIR/source-before.json" "$MG_RUN_DIR/cargo-build.jsonl"; then
    if [ -f "$MG_RUN_DIR/cargo-build.jsonl" ]; then mg_artifact compilation "$MG_RUN_DIR/cargo-build.jsonl" "Actual Cargo artifact messages and compiler diagnostics"; fi
    mg_failure retained-build "Locked retained peer build failed; no runtime or compatibility credit"; mg_status fail; return
  fi
  mg_artifact retained-client "$MG_RUN_DIR/client" "Actual executable, exact core/peer locks, source and resolved features"
  mg_detail retained_client "$(cat "$MG_RUN_DIR/client/client-build.json")"
  if [ "$mode" = peer-unit ]; then
    mg_sync_retained__peer_unit "$compiler_host"
    return
  fi
  mg_message "Build only. Parent owns reviewed action inputs, owned final image/namespace, Desktop/fault steps and all eight case verdicts"
  mg_status pass
}

# Fixed policy-unit lane in the separate peer workspace. Reuse the build's
# lock/feature admission and receipt, retaining actual source before execution.
mg_sync_retained__unit_source() {
  python3 - "$MG_ROOT" "$MG_RUN_DIR" <<'PY'
import hashlib, json, pathlib, sys
root, run = map(pathlib.Path, sys.argv[1:])
source = json.loads((run / 'source-before.json').read_bytes())
files = {entry['path']: entry for entry in source['files']}
for lock in ('core/Cargo.lock', 'conformance/peers/retained-sync/Cargo.lock'):
    if lock not in files:
        raise ValueError('missing source lock')
for name, entry in files.items():
    relative = pathlib.PurePosixPath(name)
    if relative.is_absolute() or '..' in relative.parts:
        raise ValueError('invalid source path')
    path = root / name
    if path.is_symlink() or not path.is_file():
        raise ValueError('nonregular source')
    data = path.read_bytes()
    if len(data) != entry['bytes'] or hashlib.sha256(data).hexdigest() != entry['sha256']:
        raise ValueError('source changed before tests')
    target = run / 'unit-source' / name
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
PY
}

mg_sync_retained__unit_report() {
  python3 - "$MG_RUN_DIR" <<'PY'
import json, pathlib, sys
run = pathlib.Path(sys.argv[1])
# Frozen from the accepted FSRS5 source; no caller-supplied filter or names.
binary = 'meguri-retained-sync-peer'
identity = dict(crate=binary, test_binary=binary, kind='bin')
prefix = binary + '::' + binary + '$fsrs_prerequisite::tests::'
expected = {prefix + name for name in (
    'captures_bracket_one_real_public_enable_before_forward_mutations',
    'failed_enable_stops_after_before_capture_without_authority_or_retry',
    'empty_bootstrap_cannot_call_enable_or_supply_authority',
    'fixed_input_rejects_nonnew_history_and_present_key_without_parsing_data')}
started, completed = set(), {}
suite_started = suite_done = False
counts = dict(executed=0, passed=0, failed=0, skipped=0, cancelled=0)
errors = []
def need(ok, reason):
    if not ok:
        raise ValueError(reason)
def integer(value):
    return type(value) is int and 0 <= value <= 2147483647
def unique(pairs):
    result = {}
    for key, value in pairs:
        need(key not in result, 'duplicate JSON key')
        result[key] = value
    return result
try:
    with (run / 'nextest.jsonl').open() as stream:
        for line in stream:
            event = json.loads(line, object_pairs_hook=unique)
            need(isinstance(event, dict), 'nonobject event')
            kind, state = event.get('type'), event.get('event')
            if kind == 'suite':
                need(event.get('nextest') == identity, 'wrong suite identity')
                if state == 'started':
                    need(not suite_started and integer(event.get('test_count')) and
                         event['test_count'] == 4, 'duplicate or wrong suite start/count')
                    suite_started = True
                elif state in ('ok', 'failed'):
                    need(suite_started and not suite_done, 'suite completion without unique start')
                    for key in ('passed', 'failed', 'ignored', 'measured', 'filtered_out'):
                        need(integer(event.get(key)), 'invalid suite count: ' + key)
                    need(event['passed'] == counts['passed'] and event['failed'] == counts['failed'] and
                         event['ignored'] == counts['skipped'] and event['measured'] == 0 and
                         event['filtered_out'] == 0, 'inconsistent suite counts')
                    need(state == 'ok' and set(completed) == expected and
                         all(value == 'ok' for value in completed.values()), 'incomplete or unsuccessful suite')
                    suite_done = True
                else:
                    raise ValueError('unsupported suite event')
            elif kind == 'test':
                name = event.get('name')
                need(suite_started and not suite_done and name in expected, 'unexpected test or inactive suite')
                if state == 'started':
                    need(name not in started and name not in completed, 'duplicate test start')
                    started.add(name)
                else:
                    need(state in ('ok', 'failed', 'ignored', 'skipped', 'cancelled'), 'unsupported test event')
                    need(name not in completed, 'duplicate test completion')
                    need(state in ('ignored', 'skipped') or name in started, 'test completion without start')
                    completed[name] = state
                    key = {'ok':'passed', 'failed':'failed', 'ignored':'skipped',
                           'skipped':'skipped', 'cancelled':'cancelled'}[state]
                    counts[key] += 1
                    if state in ('ok', 'failed'):
                        counts['executed'] += 1
            else:
                raise ValueError('unsupported nextest record')
    need(suite_done and started == expected and set(completed) == expected, 'empty or incomplete test run')
    need(counts == dict(executed=4, passed=4, failed=0, skipped=0, cancelled=0), 'four actual passes required')
except (OSError, ValueError, TypeError, KeyError) as error:
    errors.append(str(error))
report = dict(counts, names=sorted(completed), errors=errors)
(run / 'peer-unit-evidence.json').write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
if errors:
    sys.stderr.write('retained peer-unit evidence refused: ' + errors[0] + '\n')
    raise SystemExit(1)
PY
}

mg_sync_retained__peer_unit() {
  local compiler_host="$1" rc=0 evidence_ok=true dirty report counts passed failed skipped
  report="$MG_RUN_DIR/peer-unit-evidence.json"
  if ! mg_sync_retained__unit_source; then
    mg_failure peer-unit-source "Could not retain exact source and both locks before tests"; mg_status fail; return
  fi
  mg_artifact peer-unit-source "$MG_RUN_DIR/unit-source" "Actual pre-test source bytes including core and separate peer locks"
  mg_artifact source-before "$MG_RUN_DIR/source-before.json" "Build and unit source inventory"
  (cd "$MG_ROOT/core" && NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run \
    --locked --offline --manifest-path "$MG_ROOT/conformance/peers/retained-sync/Cargo.toml" \
    --bin meguri-retained-sync-peer --target "$compiler_host" \
    --retries 0 --no-fail-fast --no-tests=fail \
    -E 'test(/^fsrs_prerequisite::tests::/)' \
    --message-format libtest-json-plus --message-format-version 0.1) >"$MG_RUN_DIR/nextest.jsonl" || rc=$?
  mg_artifact nextest-jsonl "$MG_RUN_DIR/nextest.jsonl" "Actual fixed peer-unit process report"
  mg_detail peer_unit_exit_code "$rc"
  # Check after execution even when the process/report failed. Neither a clean
  # process exit nor a matching summary alone grants admission.
  if ! python3 "$MG_ROOT/conformance/peers/retained-sync/support.py" source "$MG_RUN_DIR/source-after.json" ||
      ! cmp -s "$MG_RUN_DIR/source-before.json" "$MG_RUN_DIR/source-after.json" ||
      ! python3 "$MG_ROOT/conformance/peers/retained-sync/support.py" verify-lock ||
      ! dirty="$(git -C "$MG_ROOT" status --porcelain=v1 --untracked-files=all)" || [ -n "$dirty" ]; then
    mg_failure peer-unit-source "Source or locks changed during peer-unit execution"; evidence_ok=false
  fi
  mg_artifact source-after "$MG_RUN_DIR/source-after.json" "Post-test source inventory; must equal before"
  if ! mg_sync_retained__unit_report 2>"$MG_RUN_DIR/peer-unit-evidence.log"; then
    mg_failure peer-unit-evidence "Invalid, incomplete or unsuccessful fixed-four nextest report"; evidence_ok=false
  fi
  mg_artifact peer-unit-evidence "$report"
  mg_artifact peer-unit-evidence-log "$MG_RUN_DIR/peer-unit-evidence.log"
  if [ "$rc" -ne 0 ]; then
    mg_failure "nextest-exit-$rc" "Fixed peer-unit nextest process failed"; evidence_ok=false
  fi
  if counts="$(jq -er '[.passed,.failed,.skipped] | if all(.[]; type == "number" and . >= 0 and . <= 2147483647 and floor == .) then @tsv else error("invalid counts") end' "$report")" &&
      IFS=$'\t' read -r passed failed skipped <<<"$counts"; then
    mg_count total "$((passed+failed+skipped))"; mg_count passed "$passed"; mg_count failed "$failed"; mg_count skipped "$skipped"
  else
    mg_failure peer-unit-evidence "Could not extract actual nextest counts"; evidence_ok=false
  fi
  if [ "$evidence_ok" = true ]; then
    mg_detail peer_unit "$(cat "$report")"
    mg_message "Four FSRS prerequisite policy unit tests only; no protocol, eight-case, device, image or compatibility acceptance"
    mg_status pass
  else mg_status fail; fi
}

# Finite retained/final HOST branch of sync-server-interop. Build jobs above
# remain docker:dev. Internal case arguments come only from this fixed list.
mg_sync_retained__support() {
  python3 "$MG_ROOT/conformance/peers/retained-sync/support.py" "$@"
}
mg_sync_retained__dispatch() {
  local case_name rc=0
  # All eight actual offline attempts precede ANY package/account construction.
  for case_name in authentication full-upload full-download normal-no-change normal-bidirectional \
      media-bidirectional media-no-change manual-uncertain-recovery; do
    mg_sync_retained__support runtime-prepare "$MG_RUN_DIR" "$case_name" || rc=1
  done
  for case_name in authentication full-upload full-download normal-no-change normal-bidirectional \
      media-bidirectional media-no-change manual-uncertain-recovery; do
    mg_sync_retained__instance "$1" "$case_name" || rc=1
  done
  # Assembly includes every selected case, including unavailable factories.
  mg_sync_retained__support runtime-assemble "$MG_RUN_DIR" || rc=1
  [ "$rc" -eq 0 ]
}
mg_sync_retained__owned() {
  local receipt="$1" peer_receipt="$2" arguments="$3" session="$4" identity after image rc=0 commit
  commit="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  mg_detail candidate_compatibility_credit false
  mg_sync_package__receipt "$receipt" || { mg_failure build-receipt "Fresh canonical clean package required"; return 1; }
  mg_sync_retained__support runtime-receipt "$peer_receipt" || {
    mg_failure retained-client-receipt "Fresh current canonical retained build and exact source/locks/features required"; return 1;
  }
  receipt="$(cd "$(dirname "$receipt")" && pwd -P)/result.json" || return 1
  peer_receipt="$(cd "$(dirname "$peer_receipt")" && pwd -P)/result.json" || return 1
  mkdir "$MG_RUN_DIR/package-artifacts" "$MG_RUN_DIR/preparations" "$MG_RUN_DIR/instances" "$MG_RUN_DIR/output" || return 1
  mg_sync_package__candidate_input "$receipt" "$MG_RUN_DIR/package-artifacts" >"$MG_RUN_DIR/package-identity.json" || return 1
  identity="$(mg_sync_server__reference_identity)" || return 1
  mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" --progress plain build sync-reference || return 1
  after="$(mg_sync_server__reference_identity)" || return 1
  [ "$identity" = "$after" ] || return 1
  image="$(mg_sync_server__image sync-reference -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml")" || return 1
  [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]] || return 1
  jq -nc --argjson server "$(jq -c .server "$MG_RUN_DIR/package-identity.json")" \
    --arg job "$MG_JOB" --argjson args "$arguments" --arg run "$MG_RUN_ID" --arg session "$session" '
    {source_commit:$server.source_commit,batch_base:"480df4526514e294f5a3a16dd4ab83a1fbcc7f47",
     source_sha256:$server.source_sha256,lock_sha256:$server.lock_sha256,executable_sha256:$server.executable_sha256,
     mg_job:$job,arguments:$args,run_id:$run,author_session:$session}' >"$MG_RUN_DIR/candidate-invocation.json" || return 1
  jq -nc --arg receipt "$receipt" --arg peer "$peer_receipt" --arg image "$image" \
    --argjson reference "$identity" '{package_receipt:$receipt,peer_receipt:$peer,image:$image,reference:$reference}' \
    >"$MG_RUN_DIR/runtime.json" || return 1
  # Register before the first child container. The helper removes only names
  # with the exact outer-run and per-creation nonce in its closed owner ledger.
  mg_on_exit "$(printf '%q ' mg_sync_retained__support runtime-cleanup "$MG_RUN_DIR")"
  mg_sync_retained__dispatch "$receipt" || rc=1
  mg_sync_retained__support runtime-cleanup "$MG_RUN_DIR" || rc=1
  after="$(mg_sync_server__reference_identity)" || rc=1
  [ "$identity" = "$after" ] || rc=1
  mg_sync_server__clean || rc=1
  [ "$(git -C "$MG_ROOT" rev-parse HEAD)" = "$commit" ] || rc=1
  mg_artifact retained-evidence "$MG_RUN_DIR/output" "Fixed eight automated observations; independent artifact review required; no seal"
  mg_artifact retained-preparations "$MG_RUN_DIR/preparations" "All eight actual independent offline attempts"
  mg_artifact retained-instances "$MG_RUN_DIR/instances" "Per-case package, lifetime, worker/companion/wire checkpoints and failures"
  mg_sync_retained__support runtime-counts "$MG_RUN_DIR" >"$MG_RUN_DIR/retained-counts.json" || return 1
  local count_key
  for count_key in total passed failed skipped; do
    mg_count "$count_key" "$(jq -r --arg key "$count_key" '.[$key]' "$MG_RUN_DIR/retained-counts.json")"
  done
  [ "$rc" -eq 0 ] || { mg_failure retained-runtime "Finite retained execution has failures or incomplete claims; artifacts preserved"; return 1; }
  mg_message "Automated fixed8 artifacts only; no compatibility credit until independent actual-artifact review"
  mg_status pass
}
mg_sync_retained__instance() {
  local receipt="$1" case_name="$2" outer="$MG_RUN_DIR" secret_dir rc=0 runtime_rc=0 cleanup_rc=0 factory_ready=false
  local MG_RUN_DIR="$outer/instances/$case_name" MG_SYNC_INSTANCE="$outer/instances/$case_name/instance"
  local MG_SYNC_INSTANCE_SUFFIX="retained-$case_name" MG_SYNC_SETUP_FILE=/dev/null
  local MEGURI_SYNC_ORIGIN=https://candidate.example.test MEGURI_SYNC_TRUSTED_PEER=127.0.0.1 MEGURI_SYNC_NETWORK=none
  mkdir "$MG_RUN_DIR" || return 1
  secret_dir="$(mktemp -d "${TMPDIR:-/tmp}/meguri-retained.XXXXXX")" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json")"
  python3 - "$secret_dir" "$MG_RUN_DIR/secret-owner.json" <<'PY'
import json, os, pathlib, sys
secret, receipt = map(pathlib.Path, sys.argv[1:])
s = secret.lstat()
with receipt.open('x') as f: json.dump([s.st_dev, s.st_ino, s.st_uid], f)
os.chmod(secret, 0o700)
fd = os.open(secret / 'credentials.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, 'w') as f:
    json.dump(dict(username='synthetic.' + os.urandom(12).hex(), password=os.urandom(32).hex()), f)
PY
  if [ "$?" -ne 0 ]; then
    mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json"
    return 1
  fi
  mg_on_exit "$(printf '%q ' mg_sync_package__volume_remove "$MG_SYNC_INSTANCE")"
  mg_on_exit "local MG_SYNC_INSTANCE=$(printf '%q' "$MG_SYNC_INSTANCE"); mg_sync_package__stop"
  if mg_sync_package__init "$receipt" && mg_sync_package__start; then
    factory_ready=true
    mg_sync_retained__support runtime-case "$outer" "$case_name" "$secret_dir/credentials.json" || runtime_rc=$?
    [ "$runtime_rc" -eq 0 ] || rc=1
  else rc=1; fi
  mg_sync_package__stop || { rc=1; cleanup_rc=1; }
  if mg_sync_package__volume_remove "$MG_SYNC_INSTANCE"; then
    if [ -f "$MG_SYNC_INSTANCE/instance.json" ]; then
      mv "$MG_SYNC_INSTANCE/instance.json" "$MG_SYNC_INSTANCE/retired-instance.json" || { rc=1; cleanup_rc=1; }
    fi
  else rc=1; cleanup_rc=1; fi
  mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json" || { rc=1; cleanup_rc=1; }
  jq -nc --arg case_name "$case_name" --argjson rc "$rc" --argjson runtime_rc "$runtime_rc" \
    --argjson ready "$factory_ready" --argjson cleanup "$cleanup_rc" \
    '{case:$case_name,exit_code:$rc,runtime_exit_code:$runtime_rc,factory_ready:$ready,cleanup_complete:($cleanup==0),case_credit:false}' >"$MG_RUN_DIR/factory-result.json" || return 1
  [ "$rc" -eq 0 ]
}
