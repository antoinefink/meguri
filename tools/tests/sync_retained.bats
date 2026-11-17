#!/usr/bin/env bats
# SPDX-License-Identifier: MIT
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
setup() {
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/sync-retained.sh"
  MG_ROOT="$REPO_ROOT"
  MG_RUN_DIR="$BATS_TEST_TMPDIR"
  mg_status() { printf 'status:%s\n' "$1"; }
  mg_failure() { printf 'failure:%s\n' "$1"; }
}
@test "retained peer dry run performs no Cargo or fixture execution" {
  run "$REPO_ROOT/tools/mg" sync-server-retained --dry-run build
  [ "$status" -eq 0 ]
  result="${lines[${#lines[@]}-1]}"
  run jq -e '.job=="sync-server-retained" and .dry_run==true and .status=="pass"' "$result"
  [ "$status" -eq 0 ]
}
@test "retained peer refuses runtime or arbitrary commands as build modes" {
  run mg_sync_retained execute
  [ "$status" -eq 0 ]
  [[ "$output" == *'failure:usage'* ]]
  [[ "$output" == *'status:error'* ]]
}
@test "retained peer requires exactly one build operation" {
  run mg_sync_retained build extra
  [[ "$output" == *'failure:usage'* ]]
  [[ "$output" == *'status:error'* ]]
}
@test "retained peer preserves fixed case recipes and exact dependency identities" {
  run python3 "$REPO_ROOT/conformance/peers/retained-sync/support.py" check
  [ "$status" -eq 0 ]
}
@test "retained peer admission refuses a final receipt before reading artifacts" {
  run python3 - "$REPO_ROOT" <<'PY'
import importlib.util
import pathlib
import sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
# Deliberately incomplete admission: rejection must precede any artifact access.
peer = dict.fromkeys(('format kind server source_dirty profile features executable_artifact_sha256 '
    'lock_artifact_sha256 mg_build_receipt_sha256 build_invocation execution_binding_sha256 package').split())
peer.update(format='meguri-sync-package-v1', kind='final_package', source_dirty=False, profile='release', features=[])
try:
    support.package_identity(peer, None, {}, False)
except ValueError as error:
    assert str(error) == 'package kind'
else:
    raise AssertionError('full final receipt admitted for execution')
PY
  [ "$status" -eq 0 ]
}
@test "retained peer finalization rejects changed capture and raw bytes before attachment" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import importlib.util
import pathlib
import sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
root = pathlib.Path(sys.argv[2])
captured = root / 'capture'
captured.mkdir()
# A policy-negative input, never a fabricated successful worker observation.
checkpointed = b'{"case_credit":false}\n'
(captured / 'capture.json').write_bytes(checkpointed + b' ')
try:
    support.finalize(captured, root / 'absent-final-inputs', root / 'final', support.digest(checkpointed))
except ValueError as error:
    assert str(error) == 'capture checkpoint'
else:
    raise AssertionError('changed admission accepted')
assert not (root / 'final').exists()
artifacts = root / 'artifacts'
artifacts.mkdir()
raw = support.encoded(dict(format='meguri-retained-error-v1', operation_complete=False,
                          case_credit=False, reason='input_or_public_api_failure'))
sha = support.digest(raw)
(artifacts / sha).write_bytes(raw.replace(b'false', b'true'))
try:
    support.artifact_reader(artifacts)(sha)
except ValueError as error:
    assert str(error) == 'artifact hash'
else:
    raise AssertionError('raw failure mutation accepted')
PY
  [ "$status" -eq 0 ]
}
@test "retained peer final binding refuses changed execution identities and missing after evidence" {
  run python3 - "$REPO_ROOT" <<'PY'
import importlib.util
import pathlib
import sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
# Only neutral equality-policy data; not process/runtime or case evidence.
preflight = dict(kind='final_package_preflight', execution_binding_sha256='before-binding',
                 server={'source_commit': 'source'}, package={'image_id': 'image'})
peer = dict(preflight, kind='final_package', execution_binding_sha256='final-binding')
before = dict(container_id='container', server_process={'pid': 5, 'executable_sha256': 'executable'},
              container_inspection_before_sha256='inspection', run_id='run', endpoint='fixed')
after = dict(before, container_inspection_after_sha256='after-inspection')
support.same_execution(preflight, peer, before, after)
for field, value in [('container_id', 'other'), ('server_process', {'pid': 6, 'executable_sha256': 'executable'}),
                     ('container_inspection_before_sha256', 'other'), ('run_id', 'other'), ('endpoint', 'other')]:
    changed = dict(after, **{field: value})
    try:
        support.same_execution(preflight, peer, before, changed)
    except ValueError:
        pass
    else:
        raise AssertionError('execution identity changed: ' + field)
for changed_peer, changed_after in [(dict(peer, package={'image_id': 'other'}), after),
                                    (dict(peer, server={'source_commit': 'other'}), after),
                                    (peer, before), (dict(peer, kind='final_package_preflight'), after)]:
    try:
        support.same_execution(preflight, changed_peer, before, changed_after)
    except ValueError:
        pass
    else:
        raise AssertionError('non-final or changed package attached')
PY
  [ "$status" -eq 0 ]
}

@test "retained peer finite dispatch attempts every offline preparation before factories after independent refusal" {
  mg_sync_retained__support() {
    if [ "$1" = runtime-prepare ]; then
      printf 'prepare:%s\n' "$3"
      [ "$3" != authentication ]
    else
      printf '%s\n' "$1"
    fi
  }
  mg_sync_retained__instance() { printf 'factory:%s\n' "$2"; }
  run mg_sync_retained__dispatch unused-receipt
  [ "$status" -eq 1 ]
  [ "${#lines[@]}" -eq 17 ]
  local i=0 name
  for name in authentication full-upload full-download normal-no-change normal-bidirectional media-bidirectional media-no-change manual-uncertain-recovery; do
    [ "${lines[$i]}" = "prepare:$name" ]
    [ "${lines[$((i+8))]}" = "factory:$name" ]
    i=$((i+1))
  done
  [ "${lines[16]}" = runtime-assemble ]
}

@test "retained peer checkpoint binds false raw and sanitized wire before measurement and rejects later mutation" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
root = pathlib.Path(sys.argv[2])
for mutation in (False, True):
    directory = root / str(mutation)
    directory.mkdir()
    (directory / 'capture').mkdir()
    # Closed policy data only: no fabricated HTTP or worker success.
    capture = s.encoded(dict(case_credit=False))
    (directory / 'capture/capture.json').write_bytes(capture)
    store = directory / 'checkpoint-artifacts'
    raw = s.encoded(dict(operation_complete=False, case_credit=False))
    wire = s.encoded(dict(measurement_available=False))
    raw_sha, wire_sha = s.cas_put(store, raw), s.cas_put(store, wire)
    checkpoint = dict(raw=dict(files=[dict(path='worker-error.json', bytes=len(raw), sha256=raw_sha)]),
        wire=dict(files=[dict(path='wire.json', bytes=len(wire), sha256=wire_sha)]), capture_sha256=s.digest(capture))
    order = []
    def actual_after_boundary():
        assert (directory / 'checkpoint.sha256').read_text().strip() == s.digest((directory / 'checkpoint.json').read_bytes())
        frozen = s.load_json(directory / 'checkpoint.json')
        assert frozen['raw']['files'][0]['sha256'] == s.digest(raw)
        assert frozen['wire']['files'][0]['sha256'] == s.digest(wire)
        assert s.artifact_reader(store)(raw_sha) == raw
        order.append('after')
        if mutation:
            path = directory / 'checkpoint.json'
            path.chmod(0o600)
            path.write_bytes(b'{}')
        return 'actual-after-measurement'
    try:
        sha, measured = s.checkpoint_after(directory, checkpoint, actual_after_boundary)
    except ValueError as error:
        assert mutation and str(error) == 'runtime checkpoint changed'
    else:
        assert not mutation and measured == 'actual-after-measurement'
        assert sha == s.digest(s.regular(directory / 'checkpoint.json'))
        order.append('offline-finalization')
    assert order == (['after'] if mutation else ['after', 'offline-finalization'])
PY
  [ "$status" -eq 0 ]
}

@test "retained peer observer owns fixed namespace readiness worker-once and graceful cleanup without credentials" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
root = pathlib.Path(sys.argv[2])
inputs, raw, peer = (root / name for name in ('input', 'raw', 'peer'))
for path in (inputs, raw, peer): path.mkdir()
(inputs / 'action.json').write_bytes(s.encoded(dict(case='authentication', action='authenticate')))
(inputs / 'invocation.json').write_bytes(s.encoded(dict(policy_test=True)))
run = s.RetainedRuntime.__new__(s.RetainedRuntime)
run.case, run.cid, run.peer, run.state = 'authentication', 'a' * 64, peer, root / 'owned-state'
run.credentials = root / 'protected-pair'
run.invocation = dict(run_id='outer-run')
order = []
class LifecyclePolicy:
    # Process-control policy double only; no SDK/transport/protocol response.
    # The wire file below is deliberately NOT a valid runtime observation.
    running = False
    def create(self, directory, entrypoint, command, **options):
        assert entrypoint == 'python' and command == ['/opt/reference/driver.py', 'retained-observer', '--input', '/in']
        assert options['network'] == 'container:' + run.cid
        assert all('/secret' not in target and target != '/state' for source, target, ro in options['mounts'])
        assert all(ro or target == '/out' for source, target, ro in options['mounts'])
        self.output = next(source for source, target, ro in options['mounts'] if target == '/out')
        order.append('observer-created')
        return dict(name='owned', owner='nonce')
    def start(self, item):
        self.running = True
        (self.output / 'ready.json').write_bytes(s.encoded(dict(format='meguri-retained-observer-ready-v1',
            case=run.case, action='authenticate', run_id='outer-run', invocation_sha256=s.digest(s.regular(inputs / 'invocation.json')))))
        order.append('ready')
    def identity(self, item): return dict(Id='owned-cid', State=dict(Running=self.running))
    def run(self, directory, entrypoint, command, **options):
        assert order == ['observer-created', 'ready']
        assert entrypoint == '/retained-peer' and command == []
        assert (run.credentials, '/secret/credentials.json', True) in options['mounts']
        assert all(target != '/data' for source, target, ro in options['mounts'])
        order.append('worker-once')
        return 1  # Preserve the failed worker; never retry into success.
    def command(self, *args):
        assert args == ('kill', '--signal', 'TERM', 'owned-cid')
        self.running = False
        order.append('TERM')
    def wait(self, item, directory):
        assert order[-1] == 'TERM'
        (self.output / 'wire.json').write_bytes(b'{}')
        order.append('observer-joined')
        return 0
    def remove(self, item): order.append('owned-cleanup')
run.containers = LifecyclePolicy()
rc, output = run.observe_worker(root, inputs, raw)
assert rc == 1
assert order == ['observer-created', 'ready', 'worker-once', 'TERM', 'observer-joined', 'owned-cleanup']
# Every actual Job, including authentication and normal/media, uses fixed proxy.
source = (s.PEER / 'src/main.rs').read_text()
options = source.split('fn options(', 1)[1].split('\nfn finish', 1)[0]
assert '.proxy(meguri_sync::Proxy::new(LOST_REPLY_PROXY))' in options
assert 'if uncertain' not in options and 'else' not in options
PY
  [ "$status" -eq 0 ]
}

@test "retained peer complete evidence assembly retains fixed eight unavailable cases and full dependency closure" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
root = pathlib.Path(sys.argv[2])
for name in ('peer', 'output', 'preparations', 'instances'): (root / name).mkdir()
# Policy fixture for a wholly incomplete run, never a fabricated case pass.
binary = b'closed-policy-input-not-an-executable'
client = dict(source_commit='a'*40, executable_sha256=s.digest(binary))
(root / 'peer/meguri-retained-sync-peer').write_bytes(binary)
(root / 'peer/artifacts').mkdir()
(root / 'package-artifacts').mkdir()
(root / 'peer/client-build.json').write_bytes(s.encoded(client))
s.runtime_receipt = lambda path: root / 'peer'
for name in ('package-result.json', 'peer-result.json'): (root / name).write_bytes(b'{}')
(root / 'runtime.json').write_bytes(s.encoded(dict(package_receipt=str(root / 'package-result.json'), peer_receipt=str(root / 'peer-result.json'))))
server = dict(kind='meguri_sync', version='0.1.0', build='a'*40, source_commit='a'*40,
    source_sha256='c'*64, lock_sha256='d'*64, executable_sha256='e'*64)
(root / 'package-identity.json').write_bytes(s.encoded(dict(server=server)))
invocation = {k: server[k] for k in ('source_commit', 'source_sha256', 'lock_sha256', 'executable_sha256')}
invocation.update(batch_base='f'*40, mg_job='sync-server-interop', arguments=['--lane', 'retained', '--phase', 'final'])
(root / 'candidate-invocation.json').write_bytes(s.encoded(invocation))
for case in s.CASES:
    directory = root / 'preparations' / case
    directory.mkdir()
    (directory / 'checkpoint.json').write_bytes(s.encoded(dict(case=case, exit_code=2, case_credit=False)))
# One malformed case inventory must not erase the other seven admissions.
(root / 'preparations/authentication/rejected-link').symlink_to(root / 'absent-policy-target')
evidence = s.runtime_assemble(root)
selected = s.load_json(s.SELECTION)['cases']
assert evidence['expected_case_count'] == evidence['selected_case_count'] == 8
assert evidence['completed_case_count'] == 0
assert [c['id'] for c in evidence['cases']] == [c['id'] for c in selected]
read = s.artifact_reader(root / 'output/artifacts')
for row, original in zip(evidence['cases'], selected):
    assert row['actions'] == original['actions'] and row['expected'] == original['expected']
    assert row['status'] == 'unavailable' and row['lane']['kind'] == 'retained_peer'
    assert row['family'] == 'M-I01' and row['classification'] == 'meguri_policy'
    for artifact in row['artifacts']:
        assert len(read(artifact['sha256'])) == artifact['bytes']
        if artifact['role'] == 'neutral_observation':
            for entry in s.json.loads(read(artifact['sha256'])).get('files', []):
                assert len(read(entry['sha256'])) == entry['bytes']
assert s.load_json(root / 'output/review-required.json')['case_credit'] is False
assert all(row['status'] != 'sealed' for row in evidence['cases'])
PY
  [ "$status" -eq 0 ]
}

@test "retained peer companion final attachment rejects a changed expectation before any final package access" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
root = pathlib.Path(sys.argv[2])
inputs, captured = root / 'inputs', root / 'captured'
inputs.mkdir()
captured.mkdir()
# Both versions are syntactically valid local policy inputs. The original
# trusted capture must never authorize a different later expectation.
for name in ('invocation.json', 'preflight.json'): (inputs / name).write_bytes(b'{}')
original = s.encoded(dict(expected_sha256='a'*64))
changed = s.encoded(dict(expected_sha256='b'*64))
(inputs / 'expectations.json').write_bytes(changed)
action = dict(case='full-upload', action='witness_upload')
admission = dict(case=action['case'], action=action['action'], actor='fresh_client',
    invocation_sha256=s.digest(b'{}'), preflight_peer_sha256=s.digest(b'{}'), expectations_sha256=s.digest(original))
data = s.encoded(admission)
(captured / 'capture.json').write_bytes(data)
# Isolate the handoff identity check from the already-covered package and
# context guards. No client, SDK or transport is substituted or executed.
s.companion_context = lambda _: (action, 'fresh_client', {}, {}, {})
try:
    s.companion_finalize(captured, inputs, root / 'absent-final', root / 'out', s.digest(data))
except ValueError as error:
    assert str(error) == 'companion capture input mismatch'
else:
    raise AssertionError('changed expectation attached')
assert not (root / 'out').exists()
# A policy-negative sanitized record cannot authorize a witness to restore
# content. This data is never emitted as actual wire or compatibility evidence.
wire = dict(format='meguri-retained-wire-v1', case='full-upload', action='witness_upload', actor='fresh_client',
    invocation_sha256='a'*64, case_credit=False, totals={}, fault={}, requests=[dict(sequence=1, method='POST',
    path='/sync/upload', request_body_bytes=0, upstream_status=200, upstream_body_bytes=0, delivered_body_bytes=0,
    disposition='forwarded')])
try:
    s.wire_check(wire, action, 'fresh_client', 'a'*64)
except ValueError as error:
    assert str(error) == 'fresh witness attempted content send'
else:
    raise AssertionError('writer request admitted for fresh witness')
PY
  [ "$status" -eq 0 ]
}

@test "retained peer reuses checkpoint CAS after consistent preparation mutation and separates working state" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import copy, importlib.util, pathlib, sys
repo, root = map(pathlib.Path, sys.argv[1:])
spec = importlib.util.spec_from_file_location('retained', repo / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
case = 'authentication'
directory = root / 'preparations' / case
for subdir in ('input', 'raw', 'state/' + case): (directory / subdir).mkdir(parents=True)
# Synthetic preparation-policy bytes, never a database, SDK result or acceptance.
client = dict(policy_test=True)
action = s.encoded(dict(case=case, action='prepare_initial'))
(directory / 'input/action.json').write_bytes(action)
state = b'synthetic original closed state, never executed\n'
(directory / 'state' / case / 'collection.bin').write_bytes(state)
declared = s.encoded(dict(format='meguri-retained-state-inputs-v1', files=[
    dict(path='collection.bin', bytes=len(state), sha256=s.digest(state))]))
(directory / 'raw/state-inputs.json').write_bytes(declared)
snap = dict(format='meguri-retained-snapshot-v1', tables={name: dict(columns=columns.split(), rows=[])
    for name, columns in s.COLUMNS.items()}, media_sha256={'policy.txt': s.digest(b'original')})
snapshot_bytes = s.encoded(snap)
(directory / 'raw/snapshot.json').write_bytes(snapshot_bytes)
result = dict(format='meguri-retained-action-v1', case=case, action='prepare_initial', actor='client_a',
    client_build=client, binding=None, action_sha256=s.digest(action), recipe_sha256=s.digest(s.regular(s.RECIPES)),
    selection_sha256=s.digest(s.regular(s.SELECTION)), state_inputs_sha256=s.digest(declared), started_ms=1, finished_ms=2,
    observed=dict(preparation_only=True, preparation_complete=False, snapshot_sha256=s.digest(snapshot_bytes), snapshot=snap),
    case_credit=False)
(directory / 'raw/action-result.json').write_bytes(s.encoded(result))
store = directory / 'artifacts'
checkpoint = dict(case=case, action='prepare_initial', actor='client_a', exit_code=1, error_kind=None,
    admission_valid=True, preparation_complete=False, case_credit=False,
    inputs=s.tree_inventory(directory / 'input', store), raw=s.tree_inventory(directory / 'raw', store),
    state=s.tree_inventory(directory / 'state', store))
checkpoint_bytes = s.encoded(checkpoint)
s.frozen_file(directory / 'checkpoint.json', checkpoint_bytes)
s.frozen_file(directory / 'checkpoint.sha256', (s.digest(checkpoint_bytes) + '\n').encode())
original = dict(snapshot=snap, state=declared, preparation=checkpoint_bytes, result=result)
changed_snap = copy.deepcopy(snap)
changed_snap['media_sha256']['policy.txt'] = s.digest(b'changed after checkpoint')
changed_result = copy.deepcopy(result)
changed_result['observed'].update(snapshot=changed_snap, snapshot_sha256=s.digest(s.encoded(changed_snap)))
(directory / 'raw/snapshot.json').write_bytes(s.encoded(changed_snap))
(directory / 'raw/action-result.json').write_bytes(s.encoded(changed_result))
# Both live files agree with each other: self-hashing would wrongly admit them.
assert s.preparation_identity(directory, case, 'prepare_initial', client)[1] == changed_snap
# Restoration must also ignore changed live state and consume the saved CAS bytes.
changed_state = b'changed live state after checkpoint, never executed\n'
(directory / 'state' / case / 'collection.bin').write_bytes(changed_state)
# Exercise the real runtime's restoration seam and real checkpoint/CAS readers.
# No admission validator, container, executable or transport is substituted.
working_state = root / 'client-a-state'
s.restore_working_state(directory, case, working_state)
working = working_state / case / 'collection.bin'
assert working_state != directory / 'state' and working.read_bytes() == state
working.write_bytes(b'evolving working state')
assert (directory / 'state' / case / 'collection.bin').read_bytes() == changed_state
assert s.saved_preparation(directory, case, 'prepare_initial', client) == original
# Corrupting original CAS or the checkpoint itself must fail, never self-hash.
cas_snapshot = store / s.digest(snapshot_bytes)
cas_snapshot.chmod(0o600)
cas_snapshot.write_bytes(s.encoded(changed_snap))
try:
    s.saved_preparation(directory, case, 'prepare_initial', client)
except ValueError as error:
    assert str(error) == 'artifact hash'
else:
    raise AssertionError('changed original CAS admitted')
cas_snapshot.write_bytes(snapshot_bytes)
path = directory / 'checkpoint.json'
path.chmod(0o600)
path.write_bytes(checkpoint_bytes + b' ')
try:
    s.saved_preparation(directory, case, 'prepare_initial', client)
except ValueError as error:
    assert str(error) == 'preparation checkpoint changed'
else:
    raise AssertionError('changed preparation checkpoint admitted')
PY
  [ "$status" -eq 0 ]
}

@test "retained runtime persists both stored offline preparations without changing their bytes or closure" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import copy, importlib.util, pathlib, sys
repo, root = map(pathlib.Path, sys.argv[1:])
spec = importlib.util.spec_from_file_location('retained', repo / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)

def files(directory):
    return {path: path.read_bytes() for path in directory.rglob('*') if path.is_file()}

class OfflineProcessPolicy:
    # Only the external offline process is doubled. These are synthetic refused
    # preparation records, never SDK/HTTP success or collection/case evidence.
    preparation_complete = False

    def run(self, directory, entrypoint, command, **kwargs):
        assert entrypoint == '/retained-peer' and command == [] and 'network' not in kwargs
        inputs, output = directory / 'input', directory / 'raw'
        action = s.load_json(inputs / 'action.json')
        case, name = action['case'], action['action']
        state = next(source for source, target, readonly in kwargs['mounts'] if target == '/state')
        owned = state / case
        owned.mkdir(exist_ok=True)
        body = b'synthetic closed state, never executed\x00\xff' + name.encode('ascii')
        (owned / 'collection.bin').write_bytes(body)
        # Deliberately noncanonical JSON: persistence must retain exact bytes.
        declared = b' \n' + s.encoded(dict(format='meguri-retained-state-inputs-v1', files=[
            dict(path='collection.bin', bytes=len(body), sha256=s.digest(body))])) + b'\t'
        snap = dict(format='meguri-retained-snapshot-v1', tables={
            table: dict(columns=columns.split(), rows=[]) for table, columns in s.COLUMNS.items()}, media_sha256={})
        snapshot_bytes = s.encoded(snap)
        result = dict(format='meguri-retained-action-v1', **action, actor='client_a',
            client_build=s.load_json(inputs / 'client-build.json'), binding=None,
            action_sha256=s.digest(s.regular(inputs / 'action.json')),
            recipe_sha256=s.digest(s.regular(s.RECIPES)), selection_sha256=s.digest(s.regular(s.SELECTION)),
            state_inputs_sha256=s.digest(declared), started_ms=1, finished_ms=2,
            observed=dict(preparation_only=True, preparation_complete=self.preparation_complete,
                snapshot_sha256=s.digest(snapshot_bytes), snapshot=snap), case_credit=False)
        for filename, data in [('state-inputs.json', declared), ('snapshot.json', snapshot_bytes),
                               ('action-result.json', s.encoded(result))]:
            s.frozen_file(output / filename, data)
        return 1

class ObservedRuntime(s.RetainedRuntime):
    def offline(self, action):
        record = super().offline(action)
        assert record is not None
        self.captured[action] = record
        self.original_records[action] = copy.deepcopy(record)
        directory = self.directory / record['event']['directory']
        self.original_files.update(files(directory))
        return record

def exercise(case, fault=None):
    outer = root / (case + '-' + (fault or 'persist'))
    peer = outer / 'peer'
    (peer / 'artifacts').mkdir(parents=True)
    client = dict(policy_test=True)
    s.frozen_file(peer / 'client-build.json', s.encoded(client))
    initial = outer / 'preparations' / case
    initial.mkdir(parents=True)
    (initial / 'state').mkdir()
    process = OfflineProcessPolicy()
    assert s.offline_worker(process, peer, initial, case, 'prepare_initial', initial / 'state') == 1
    # Keep real offline_worker, checkpoint, CAS, identity and snapshot validators.
    # Bypass constructor-only service/account setup; network is unavailable.
    runtime = ObservedRuntime.__new__(ObservedRuntime)
    runtime.outer, runtime.case, runtime.peer, runtime.client = outer, case, peer, client
    runtime.containers = process
    runtime.directory = outer / 'instances' / case
    runtime.directory.mkdir(parents=True)
    runtime.state = runtime.directory / 'client-a-state'
    s.restore_working_state(initial, case, runtime.state)
    runtime.serial, runtime.events = 0, []
    runtime.captured, runtime.original_records = {}, {}
    runtime.original_files = files(initial)
    runtime.setup = lambda: None  # External account setup policy boundary only.
    damaged = []
    def unavailable_network(action, expected, baseline=None, companion=False):
        directory = runtime.new_step(action)
        event = runtime.event(action, 'client_b' if companion else 'client_a', directory,
            operation_complete=False, error_kind='policy_network_unavailable', execution_closed=True)
        if action == 'media_send':
            if fault == 'unknown':
                return dict(event=event, unsupported_binary=b'\x00\xff')
            if fault in ('missing', 'tampered'):
                record = runtime.captured['prepare_reverse_media']
                path = runtime.directory / record['event']['directory'] / 'artifacts' / s.digest(record['state'])
                damaged.append(path)
                if fault == 'missing':
                    path.unlink()
                else:
                    path.chmod(0o600)
                    path.write_bytes(b'changed bound CAS, never admitted')
        return None
    runtime.network = unavailable_network
    output = runtime.directory / 'case-observations.json'
    try:
        returned = runtime.run()
    except (TypeError, ValueError) as error:
        assert fault is not None
        assert isinstance(error, TypeError if fault == 'unknown' else ValueError)
        assert not output.exists()
    else:
        assert fault is None
        written = s.load_json(output)
        action = 'prepare_forward' if case == 'normal-bidirectional' else 'prepare_reverse_media'
        record = runtime.captured[action]
        assert returned['results'][action] is record
        assert written['case_credit'] is False
        for key in ('case', 'events', 'initial', 'case_credit'):
            assert written[key] == returned[key]
        assert written['results'].keys() == returned['results'].keys()
        for name, result in returned['results'].items():
            if name != action:
                assert written['results'][name] == result
        projected = written['results'][action]
        assert projected.keys() == record.keys()
        for key, value in record.items():
            if key not in ('state', 'preparation'):
                assert projected[key] == value
        directory = runtime.directory / record['event']['directory']
        for key, original_path in [('state', directory / 'artifacts' / s.digest(record['state'])),
                                   ('preparation', directory / 'checkpoint.json')]:
            ref = projected[key]
            assert set(ref) == {'path', 'sha256', 'bytes'}
            assert runtime.directory / ref['path'] == original_path
            blob = s.regular(original_path)
            assert blob == record[key] and isinstance(record[key], bytes)
            assert ref['sha256'] == s.digest(blob) and ref['bytes'] == len(blob)
        assert record['event']['execution_closed'] is True
        assert record['event']['operation_complete'] is False
        checkpoint = s.load_json(directory / 'checkpoint.json')
        assert checkpoint['admission_valid'] is True and checkpoint['preparation_complete'] is False
    assert runtime.captured == runtime.original_records
    for path, original in runtime.original_files.items():
        if path not in damaged:
            assert s.regular(path) == original
    # Neither source CAS loss nor corruption can be repaired from resident bytes.
    for path in damaged:
        if fault == 'missing':
            assert not path.exists()
        else:
            assert path.read_bytes() == b'changed bound CAS, never admitted'
    if fault is None:
        assert files(initial) == {p: b for p, b in runtime.original_files.items() if p.is_relative_to(initial)}
        directory = runtime.directory / runtime.captured[action]['event']['directory']
        assert files(directory) == {p: b for p, b in runtime.original_files.items() if p.is_relative_to(directory)}

exercise('normal-bidirectional')
exercise('media-bidirectional')
exercise('media-bidirectional', 'unknown')
exercise('media-bidirectional', 'missing')
exercise('media-bidirectional', 'tampered')

# Exercise the existing case schedule with unavailable boundaries only. This
# proves ordering/one attempt and failure propagation, never a runtime outcome.
runtime = s.RetainedRuntime.__new__(s.RetainedRuntime)
runtime.outer = root / 'normal-order'
runtime.directory = runtime.outer / 'instance'
runtime.directory.mkdir(parents=True)
runtime.case, runtime.client, runtime.events = 'normal-no-change', {}, []
calls = []
runtime.setup = lambda: None
def unavailable_offline(action):
    calls.append(('client_a', action))
    return None
def unavailable_network(action, expected, baseline=None, companion=False):
    calls.append(('client_b' if companion else 'client_a', action))
    if action == 'normal_unchanged':
        assert expected is None  # Failed setup cannot supply a measured baseline.
    return None
runtime.offline, runtime.network = unavailable_offline, unavailable_network
returned = runtime.run()
assert calls == [('client_a', 'prepare_snapshot'), ('client_b', 'prepare_rich'),
    ('client_b', 'upload_baseline'), ('client_a', 'import_baseline'), ('client_a', 'establish_baseline'),
    ('client_b', 'settle_normal'), ('client_a', 'settle_normal'), ('client_a', 'prepare_snapshot'),
    ('client_a', 'normal_unchanged')]
assert returned['results']['settle_normal_a'] is None and returned['results']['normal_unchanged'] is None
assert returned['case_credit'] is False

# A successful synthetic A settlement cannot authorize a returned snapshot
# whose process failed after writing complete, valid artifacts. Keep the real
# offline/checkpoint/CAS seam; no SDK/HTTP outcome or case credit is simulated.
runtime = ObservedRuntime.__new__(ObservedRuntime)
runtime.outer, runtime.case = root / 'normal-failed-freeze', 'normal-no-change'
runtime.peer, runtime.client = runtime.outer / 'peer', dict(policy_test=True)
(runtime.peer / 'artifacts').mkdir(parents=True)
s.frozen_file(runtime.peer / 'client-build.json', s.encoded(runtime.client))
initial = runtime.outer / 'preparations' / runtime.case
initial.mkdir(parents=True)
(initial / 'state').mkdir()
runtime.containers = OfflineProcessPolicy()
runtime.containers.preparation_complete = True  # Output claims completion, exit remains unsuccessful.
assert s.offline_worker(runtime.containers, runtime.peer, initial, runtime.case,
    'prepare_initial', initial / 'state') == 1
runtime.directory = runtime.outer / 'instance'
runtime.directory.mkdir()
runtime.state = runtime.directory / 'client-a-state'
s.restore_working_state(initial, runtime.case, runtime.state)
runtime.serial, runtime.events = 0, []
runtime.captured, runtime.original_records = {}, {}
runtime.original_files = files(initial)
runtime.setup = lambda: None
initial_snapshot = s.saved_preparation(initial, runtime.case, 'prepare_initial', runtime.client)['snapshot']
declared_authority = s.with_media(initial_snapshot, s.media_recipe('initial'))
calls, measured = [], []
def settlement_network(action, expected, baseline=None, companion=False):
    calls.append(('client_b' if companion else 'client_a', action))
    if action == 'prepare_rich':
        return dict(authority=declared_authority)
    if action == 'settle_normal':
        directory = runtime.new_step(action)
        event = runtime.event(action, 'client_b' if companion else 'client_a', directory,
            operation_complete=True, error_kind=None, execution_closed=True)
        if not companion:
            assert expected is not None and s.projection(expected) == s.projection(initial_snapshot)
        return dict(event=event)
    if action == 'normal_unchanged':
        record = runtime.captured['prepare_snapshot']
        assert record['snapshot'] is not None
        assert s.projection(record['snapshot']) == s.projection(initial_snapshot)
        assert record['event']['operation_complete'] is False
        assert expected is None and baseline is None
        measured.append((expected, baseline))
    return None
runtime.network = settlement_network
returned = runtime.run()
assert calls == [('client_b', 'prepare_rich'), ('client_b', 'upload_baseline'),
    ('client_a', 'import_baseline'), ('client_a', 'establish_baseline'),
    ('client_b', 'settle_normal'), ('client_a', 'settle_normal'), ('client_a', 'normal_unchanged')]
assert measured == [(None, None)] and runtime.serial == 3  # B settlement, A settlement, then freeze.
assert returned['results']['settle_normal_a']['event']['operation_complete'] is True
assert returned['results']['normal_unchanged'] is None and returned['case_credit'] is False
record = runtime.captured['prepare_snapshot']
directory = runtime.directory / record['event']['directory']
checkpoint = s.load_json(directory / 'checkpoint.json')
assert checkpoint['exit_code'] == 1 and checkpoint['admission_valid'] is True
assert checkpoint['preparation_complete'] is True and checkpoint['case_credit'] is False
assert record['event']['execution_closed'] is True and record['event']['error_kind'] == 'preparation_refused'
assert record['event']['case_credit'] is False and record['event'] in returned['events']
assert s.load_json(directory / 'event.json') == record['event']
assert s.load_json(runtime.directory / 'case-observations.json')['events'] == returned['events']
assert runtime.captured == runtime.original_records
assert files(directory) == {p: b for p, b in runtime.original_files.items() if p.is_relative_to(directory)}
assert files(initial) == {p: b for p, b in runtime.original_files.items() if p.is_relative_to(initial)}

# Failed-offline-only diagnostic admission at the real checkpoint/CAS seam.
error_bytes = s.encoded(dict(format='meguri-retained-error-v1', operation_complete=False,
    case_credit=False, reason='input_or_public_api_failure'))
diagnostic = dict(format='meguri-retained-prepare-forward-diagnostic-v1',
    case='normal-bidirectional', action='prepare_forward', stage='review_head_predicate')
class FailedForwardProcessPolicy:
    def run(self, directory, entrypoint, command, **kwargs):
        assert entrypoint == '/retained-peer' and command == [] and 'network' not in kwargs
        output = directory / 'raw'
        (output / 'worker-error.json').write_bytes(error_bytes)
        if self.payload is not None:
            (output / 'prepare-forward-diagnostic.json').write_bytes(self.payload)
        state = next(source for source, target, readonly in kwargs['mounts'] if target == '/state')
        (state / 'failed-state').write_bytes(b'original failed state, never executable')
        return 2
for name, value in [('valid', diagnostic),
                    ('fsrs-enable', dict(diagnostic, stage='fsrs_enable')),
                    ('fsrs-effects', dict(diagnostic, stage='fsrs_effects')),
                    ('fsrs-policy-pending', dict(diagnostic, stage='review_start_fsrs_policy_pending')),
                    ('preset-unsupported', dict(diagnostic, stage='review_start_preset_unsupported')),
                    ('missing', None), ('extra', dict(diagnostic, payload='forbidden')),
                    ('format', dict(diagnostic, format='other')), ('case', dict(diagnostic, case='authentication')),
                    ('action', dict(diagnostic, action='prepare_snapshot')), ('stage', dict(diagnostic, stage='arbitrary')),
                    ('stage-type', dict(diagnostic, stage=1)), ('missing-key', {k: v for k, v in diagnostic.items() if k != 'stage'})]:
    directory = root / ('failed-forward-' + name)
    peer = directory / 'peer'
    (peer / 'artifacts').mkdir(parents=True)
    (peer / 'client-build.json').write_bytes(b'{}')
    (directory / 'state').mkdir()
    process = FailedForwardProcessPolicy()
    process.payload = None if value is None else b' \n' + s.encoded(value) + b'\t'
    assert s.offline_worker(process, peer, directory, 'normal-bidirectional', 'prepare_forward', directory / 'state') == 1
    checkpoint = s.load_json(directory / 'checkpoint.json')
    assert checkpoint['exit_code'] == 2 and checkpoint['admission_valid'] is False
    assert checkpoint['preparation_complete'] is False and checkpoint['case_credit'] is False
    read = s.artifact_reader(directory / 'artifacts')
    raw = {row['path']: read(row['sha256']) for row in checkpoint['raw']['files']}
    assert raw['worker-error.json'] == error_bytes
    assert [read(row['sha256']) for row in checkpoint['state']['files']] == [b'original failed state, never executable']
    if value is not None:
        assert raw['prepare-forward-diagnostic.json'] == process.payload
    try:
        admitted = s.prepare_forward_diagnostic(directory / 'raw', 'normal-bidirectional', 'prepare_forward', 2)
    except ValueError:
        assert name not in ('valid', 'fsrs-enable', 'fsrs-effects', 'fsrs-policy-pending', 'preset-unsupported', 'missing')
    else:
        assert name in ('valid', 'fsrs-enable', 'fsrs-effects', 'fsrs-policy-pending', 'preset-unsupported', 'missing') and admitted == value

for name in ('valid', 'fsrs-enable', 'fsrs-effects', 'fsrs-policy-pending', 'preset-unsupported'):
    output = root / ('failed-forward-' + name) / 'raw'
    for case, action, rc in [('normal-bidirectional', 'prepare_forward', 0),
                             ('normal-bidirectional', 'prepare_forward', 1),
                             ('normal-bidirectional', 'prepare_snapshot', 2),
                             ('normal-no-change', 'prepare_forward', 2)]:
        try:
            s.prepare_forward_diagnostic(output, case, action, rc)
        except ValueError:
            pass
        else:
            raise AssertionError('diagnostic admitted outside fixed offline exit2 failure')
    for invalid_error in [dict(s.json.loads(error_bytes), reason='other'),
                          dict(s.json.loads(error_bytes), operation_complete=True),
                          dict(s.json.loads(error_bytes), case_credit=True)]:
        (output / 'worker-error.json').write_bytes(s.encoded(invalid_error))
        try:
            s.prepare_forward_diagnostic(output, 'normal-bidirectional', 'prepare_forward', 2)
        except ValueError:
            pass
        else:
            raise AssertionError('diagnostic admitted without original closed failure')
    (output / 'worker-error.json').unlink()
    try:
        s.prepare_forward_diagnostic(output, 'normal-bidirectional', 'prepare_forward', 2)
    except (OSError, ValueError):
        pass
    else:
        raise AssertionError('diagnostic admitted without worker failure file')
PY
  [ "$status" -eq 0 ]
}

@test "retained FSRS prerequisite preserves exact effect boundaries and binds actual preparation bytes without authority on failure" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import copy, importlib.util, pathlib, sys
repo, root = map(pathlib.Path, sys.argv[1:])
spec = importlib.util.spec_from_file_location('retained', repo / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
# Synthetic post-capture admission inputs only, never SDK, scheduler, sync or
# case evidence. Real public-operation ordering is covered by the Rust seam.
def integer(value):
    return dict(integer=str(value))
def text(value):
    return dict(text_hex=value.encode().hex())
def blob(value):
    return dict(blob_hex=value.encode().hex())
before = dict(format='meguri-retained-snapshot-v1', tables={name: dict(columns=columns.split(), rows=[])
    for name, columns in s.COLUMNS.items()}, media_sha256={'original.txt': 'f' * 64})
before['tables']['col']['rows'] = [[integer(1)] + [integer(0)] * 12]
before['tables']['config']['rows'] = [[text('foreign'), integer(7), integer(8), blob('{"opaque":true}')]]
for card_id, data in [(11, '{"pos":7,"lrt":9,"cd":"custom","unknown":{"x":[1, 2]}}'),
                      (12, 'opaque non-JSON card data'), (13, '')]:
    row = [integer(0)] * 18
    row[0], row[1], row[2], row[6], row[8], row[9] = map(integer, (card_id, card_id + 100, 1, 0, 17, 23))
    row[17] = text(data)
    before['tables']['cards']['rows'].append(row)
after = copy.deepcopy(before)
after['tables']['col']['rows'][0][2] = integer(1500)
after['tables']['config']['rows'].append([text('fsrs'), integer(-1), integer(1), blob('true')])
proof = dict(format='meguri-retained-fsrs-prerequisite-v1', case='normal-bidirectional', action='prepare_forward',
    acknowledgment='reviewed_headless_fsrs_only_and_synchronized_effects', operation='enable_fsrs',
    before_sha256=s.digest(s.encoded(before)), after_sha256=s.digest(s.encoded(after)),
    eligible_card_ids=[], started_ms=1000, finished_ms=2000, case_credit=False)
assert s.fsrs_effects(before, after, proof, 1000, 3000) == after
original = copy.deepcopy((before, after, proof))
def rejects(changed_after=None, changed_proof=None, changed_before=None):
    try:
        s.fsrs_effects(changed_before if changed_before is not None else before,
                       changed_after if changed_after is not None else after,
                       changed_proof if changed_proof is not None else proof, 1000, 3000)
    except (ValueError, KeyError, TypeError):
        pass
    else:
        raise AssertionError('undeclared prerequisite effects or proof admitted')
for count in (0, 2, 4):
    changed = copy.deepcopy(before)
    changed['tables']['cards']['rows'] = (changed['tables']['cards']['rows'] +
        [copy.deepcopy(changed['tables']['cards']['rows'][0])])[:count]
    if count == 4:
        changed['tables']['cards']['rows'][3][0] = integer(14)
    rejects(changed_before=changed)
for row_index in range(3):
    for kind in (1, 2, 3):
        changed = copy.deepcopy(before)
        changed['tables']['cards']['rows'][row_index][6] = integer(kind)
        rejects(changed_before=changed)
changed = copy.deepcopy(before)
changed['tables']['revlog']['rows'] = [[integer(v) for v in (21, 11, 0, 3, 23, 20, 2500, 900, 1)]]
rejects(changed_before=changed)
for value in ('false', 'true'):
    changed = copy.deepcopy(before)
    changed['tables']['config']['rows'].append([text('fsrs'), integer(0), integer(0), blob(value)])
    rejects(changed_before=changed)
for name in s.COLUMNS:
    if name not in ('cards', 'config', 'col'):
        changed = copy.deepcopy(after)
        changed['tables'][name]['rows'].append([integer(99)] * len(s.COLUMNS[name].split()))
        rejects(changed)
for row_index in range(3):
    for column in range(18):
        changed = copy.deepcopy(after)
        changed['tables']['cards']['rows'][row_index][column] = integer(999)
        rejects(changed)
for column in range(13):
    if column == 2:
        continue
    changed = copy.deepcopy(after)
    changed['tables']['col']['rows'][0][column] = integer(999)
    rejects(changed)
for column in range(4):
    changed = copy.deepcopy(after)
    changed['tables']['config']['rows'][0][column] = integer(999)
    rejects(changed)
changed = copy.deepcopy(after)
changed['tables']['cards']['rows'][0][17] = text('{"pos":7,"lrt":9,"cd":"custom","unknown":{"x":[1,2]}}')
rejects(changed)  # Even JSON whitespace is preserved, not normalized.
for key, value in [('operation', 'set_config'), ('case', 'normal-no-change'), ('action', 'prepare_initial'),
                   ('acknowledgment', 'app_sheet_appeared'), ('case_credit', True), ('started_ms', True),
                   ('started_ms', 999), ('finished_ms', 999), ('finished_ms', 4000),
                   ('before_sha256', 'a' * 63), ('after_sha256', 'z' * 64),
                   ('eligible_card_ids', ['11']), ('eligible_card_ids', None), ('extra', True)]:
    rejects(changed_proof=dict(proof, **{key: value}))
for key in proof:
    rejects(changed_proof={k: v for k, v in proof.items() if k != key})
try:
    s.decoded('{"operation":"enable_fsrs","operation":"enable_fsrs"}')
except ValueError:
    pass
else:
    raise AssertionError('duplicate prerequisite member admitted')
for name, mutate in [('media', lambda v: v['media_sha256'].clear()),
    ('config-missing', lambda v: v['tables']['config']['rows'].pop()),
    ('config-false', lambda v: v['tables']['config']['rows'][1].__setitem__(3, blob('false'))),
    ('config-storage-class', lambda v: v['tables']['config']['rows'][1].__setitem__(3, text('true'))),
    ('config-usn', lambda v: v['tables']['config']['rows'][1].__setitem__(1, integer(0))),
    ('config-stamp', lambda v: v['tables']['config']['rows'][1].__setitem__(2, integer(9))),
    ('config-early-stamp', lambda v: v['tables']['config']['rows'][1].__setitem__(2, integer(0))),
    ('collection-stamp', lambda v: v['tables']['col']['rows'][0].__setitem__(2, integer(999))),
    ('collection-late-stamp', lambda v: v['tables']['col']['rows'][0].__setitem__(2, integer(2001))),
    ('card-stamp', lambda v: v['tables']['cards']['rows'][0].__setitem__(4, integer(9))),
    ('card-usn', lambda v: v['tables']['cards']['rows'][0].__setitem__(5, integer(-1)))]:
    changed = copy.deepcopy(after)
    mutate(changed)
    rejects(changed)
for stamp in (1000, 2000):
    changed = copy.deepcopy(after)
    changed['tables']['col']['rows'][0][2] = integer(stamp)
    changed['tables']['config']['rows'][1][2] = integer(stamp // 1000)
    assert s.fsrs_effects(before, changed, proof, 1000, 3000) == changed
assert (before, after, proof) == original

# Keep the real offline checkpoint/CAS/identity seam. The process deliberately
# exits unsuccessfully even with structurally complete output: no authority.
class FailedAfterCapture:
    def run(self, directory, entrypoint, command, **kwargs):
        assert entrypoint == '/retained-peer' and command == [] and 'network' not in kwargs
        output, inputs = directory / 'raw', directory / 'input'
        state = next(source for source, target, readonly in kwargs['mounts'] if target == '/state')
        owned = state / 'normal-bidirectional'
        owned.mkdir()
        state_bytes = b'synthetic state, not a collection'
        (owned / 'closed.txt').write_bytes(state_bytes)
        declared = s.encoded(dict(format='meguri-retained-state-inputs-v1', files=[
            dict(path='closed.txt', bytes=len(state_bytes), sha256=s.digest(state_bytes))]))
        snap = s.encoded(after)
        observed = dict(preparation_only=True, preparation_complete=True, limitations=[],
            snapshot_sha256=s.digest(snap), snapshot=after, fsrs_prerequisite=proof)
        result = dict(format='meguri-retained-action-v1', case='normal-bidirectional', action='prepare_forward',
            actor='client_a', client_build={}, binding=None, case_credit=False,
            action_sha256=s.digest(s.regular(inputs / 'action.json')), recipe_sha256=s.digest(s.regular(s.RECIPES)),
            selection_sha256=s.digest(s.regular(s.SELECTION)), state_inputs_sha256=s.digest(declared),
            started_ms=1000, finished_ms=3000, observed=observed)
        for name, data in [('fsrs-before.json', s.encoded(before)), ('fsrs-after.json', s.encoded(after)),
            ('snapshot.json', snap), ('action-result.json', s.encoded(result)), ('state-inputs.json', declared)]:
            s.frozen_file(output / name, data)
        return 1
directory, peer = root / 'failed-complete-forward', root / 'peer'
(peer / 'artifacts').mkdir(parents=True)
s.frozen_file(peer / 'client-build.json', b'{}')
(directory / 'state').mkdir(parents=True)
assert s.offline_worker(FailedAfterCapture(), peer, directory, 'normal-bidirectional', 'prepare_forward', directory / 'state') == 1
checkpoint = s.load_json(directory / 'checkpoint.json')
assert checkpoint['admission_valid'] is True and checkpoint['preparation_complete'] is True
assert checkpoint['exit_code'] == 1 and checkpoint['case_credit'] is False
saved = s.saved_preparation(directory, 'normal-bidirectional', 'prepare_forward', {})
assert saved['fsrs_before'] == before and saved['fsrs_after'] == after
# Bind both actual capture hashes and the action receipt, independently of
# the syntactically valid effect proof. Original frozen CAS stays untouched.
result_path = directory / 'raw/action-result.json'
result_bytes = s.regular(result_path)
for key in ('before_sha256', 'after_sha256', 'action'):
    changed = s.decoded(result_bytes)
    if key == 'action':
        changed[key] = 'prepare_initial'
    else:
        changed['observed']['fsrs_prerequisite'][key] = '0' * 64
    result_path.chmod(0o600)
    result_path.write_bytes(s.encoded(changed))
    try:
        s.preparation_identity(directory, 'normal-bidirectional', 'prepare_forward', {})
    except ValueError:
        pass
    else:
        raise AssertionError('unbound capture hash or action receipt admitted')
result_path.write_bytes(result_bytes)
result_path.chmod(0o444)
saved['event'] = dict(operation_complete=False)
try:
    s.forward_authority(before, saved)
except ValueError as error:
    assert str(error) == 'successful forward event required'
else:
    raise AssertionError('failed frozen event supplied forward authority')
try:
    s.forward_submission(directory, {}, after)
except ValueError as error:
    assert str(error) == 'successful frozen forward checkpoint required'
else:
    raise AssertionError('failed frozen checkpoint supplied submission closure')
# Change resident capture bytes; original CAS still supplies old bytes.
raw = directory / 'raw'
old_bytes = s.regular(raw / 'fsrs-after.json')
(raw / 'fsrs-after.json').chmod(0o600)
(raw / 'fsrs-after.json').write_bytes(old_bytes + b' ')
assert s.saved_preparation(directory, 'normal-bidirectional', 'prepare_forward', {})['fsrs_after'] == after
try:
    s.preparation_identity(directory, 'normal-bidirectional', 'prepare_forward', {})
except ValueError as error:
    assert str(error) == 'actual FSRS prerequisite snapshot hashes'
else:
    raise AssertionError('unbound resident prerequisite bytes admitted')
for case, action in [('normal-no-change', 'prepare_snapshot'), ('normal-bidirectional', 'prepare_initial')]:
    try:
        s.prepare_forward_diagnostic(raw, case, action, 1)
    except ValueError:
        pass
    else:
        raise AssertionError('FSRS artifact filenames admitted outside fixed action')
PY
  [ "$status" -eq 0 ]
}

@test "retained declared edits bind home and original ancestors to one review's statistics in both directions" {
  run python3 - "$REPO_ROOT" <<'PY'
import copy, importlib.util, json, pathlib, sys
repo = pathlib.Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('retained', repo / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
# Public synthetic whole-table policy inputs only: no SDK, HTTP, worker or
# runtime observations. Exercise the real shared proof without substituting it.
recipes = s.load_json(s.RECIPES)
def integer(value):
    return dict(integer=str(value))
def text(value):
    return dict(text_hex=value.encode().hex())
def blob(value):
    return dict(blob_hex=value.hex())
def short_text(tag, value):
    data = value.encode()
    assert 0 < tag < 16 and len(data) < 128
    return bytes([(tag << 3) | 2, len(data)]) + data
def preset(part):
    # Public P6.6 fields 9, 10, 23 and 24; these fixture values fit one byte.
    return bytes([0x48, part['new_per_day'], 0x50, part['reviews_per_day']]) + (
        b'' if part['autoplay'] else b'\xb8\x01\x01') + bytes([0xc0, 0x01, part['maximum_answer_seconds']])
def row(table, **cells):
    return [cells.get(column, integer(0)) for column in s.COLUMNS[table].split()]
def table(value, name):
    return value['tables'][name]['rows']
def identified(value, name, identity):
    return next(r for r in table(value, name) if r[0] == integer(identity))
initial = dict(format='meguri-retained-snapshot-v1',
    tables={name: dict(columns=columns.split(), rows=[]) for name, columns in s.COLUMNS.items()},
    media_sha256={'public-synthetic.txt': 'f' * 64})
table(initial, 'col').append(row('col', id=integer(1), ver=integer(18),
    conf=text('{}'), models=text('{}'), decks=text('{}'), dconf=text('{}'), tags=text('{}')))
for offset, note in enumerate(recipes['rich_initial']['notes']):
    table(initial, 'notes').append(row('notes', id=integer(11 + offset), guid=text('synthetic-' + note['key']),
        mid=integer(21), tags=text(' orbit '), flds=text('\x1f'.join(note['fields'])),
        sfld=text(note['fields'][0]), data=text('')))
    table(initial, 'cards').append(row('cards', id=integer(31 + offset), nid=integer(11 + offset),
        did=integer(41), data=text('{"pos":7,"custom":"public synthetic"}')))
table(initial, 'notetypes').extend([
    row('notetypes', id=integer(21), name=text(recipes['rich_initial']['note_type']), config=blob(short_text(3, recipes['rich_initial']['template']['css']))),
    row('notetypes', id=integer(22), name=text('Other public type'), config=blob(b'\x08\x01'))])
for ordinal, name in enumerate(recipes['rich_initial']['fields_in_order']):
    table(initial, 'fields').append(row('fields', ntid=integer(21), ord=integer(ordinal), name=text(name), config=blob(b'')))
table(initial, 'templates').extend([
    row('templates', ntid=integer(21), name=text(recipes['rich_initial']['template']['name']),
        config=blob(short_text(1, recipes['rich_initial']['template']['question']) + short_text(2, recipes['rich_initial']['template']['answer']))),
    row('templates', ntid=integer(22), name=text(recipes['rich_initial']['template']['name']), config=blob(b'\x0a\x05other'))])
table(initial, 'tags').append(row('tags', tag=text('orbit'), config=blob(b'')))
child_name = recipes['rich_initial']['deck']['name'].replace('::', '\x1f')
parent_name = child_name.rsplit('\x1f', 1)[0]
for did, name, common in [(41, child_name, b'\x10\x01'), (42, parent_name, b'\x08\x01\x10\x01'),
    (43, parent_name + '\x1fSouth', b'\x08\x01\x40\x01'), (44, 'Unrelated', b'\xfa\x0f\x01x'),
    (1, 'Default', b'\x10\x01')]:
    table(initial, 'decks').append(row('decks', id=integer(did), name=text(name),
        common=blob(common), kind=blob(b'\x0a\x02\x08\x3d')))
normal = b'\x08\x3d' + short_text(4, recipes['rich_initial']['deck']['description'])
identified(initial, 'decks', 41)[5] = blob(bytes([0x0a, len(normal)]) + normal)
table(initial, 'deck_config').extend([
    row('deck_config', id=integer(61), name=text(recipes['rich_initial']['preset']['name']), config=blob(preset(recipes['rich_initial']['preset']))),
    row('deck_config', id=integer(62), name=text('Other public preset'), config=blob(b'\x48\x01'))])
table(initial, 'config').extend([
    row('config', KEY=text('meguri_sync_fixture'), val=blob(json.dumps(recipes['rich_initial']['config']['meguri_sync_fixture']).encode())),
    row('config', KEY=text('foreign'), val=blob(b'{"keep": [1, 2]}'))])

def edited(before, direction):
    after = copy.deepcopy(before)
    changes = recipes['rich_' + direction + '_changes']
    delete_id = 12 if direction == 'forward' else 13
    table(after, 'notes')[:] = [r for r in table(after, 'notes') if r[0] != integer(delete_id)]
    table(after, 'cards')[:] = [r for r in table(after, 'cards') if r[1] != integer(delete_id)]
    keep = identified(after, 'notes', 11)
    fields = bytes.fromhex(keep[6]['text_hex']).decode().split('\x1f')
    fields[1 if direction == 'forward' else 2] = changes['keep_answer' if direction == 'forward' else 'keep_context']
    keep[6] = text('\x1f'.join(fields))
    card = identified(after, 'cards', 31)
    card[6], card[7], card[8], card[11] = integer(1), integer(1), integer(100), integer(1 if direction == 'forward' else 2)
    card[17] = text('{"public":"actual scheduler data remains opaque in this proof"}')
    table(after, 'revlog').append([integer(v) for v in
        ((101, 31, -1, 3, -60, 0, 2500, 900, 0) if direction == 'forward' else
         (102, 31, -1, 2, -90, -60, 2500, 1100, 0))])
    table(after, 'graves').extend([[integer(v) for v in values] for values in
        ((delete_id, 1, -1), (delete_id + 20, 0, -1))])
    identified(after, 'notetypes', 21)[4] = blob(short_text(3, changes['css']))
    table(after, 'templates')[0][5] = blob(short_text(1, changes['question']) + short_text(2, changes['answer']))
    child = identified(after, 'decks', 41)
    child[4] = blob(bytes.fromhex('080110012001388407' if direction == 'forward' else '1001200138d00f'))
    normal = b'\x08\x3d' + short_text(4, changes['deck_description'])
    child[5] = blob(bytes([0x0a, len(normal)]) + normal)
    identified(after, 'decks', 42)[4] = blob(bytes.fromhex('080110012001388407' if direction == 'forward' else '08011001200138d00f'))
    identified(after, 'deck_config', 61)[4] = blob(preset(changes['preset']))
    table(after, 'config')[0][3] = blob(json.dumps(changes['config']['meguri_sync_fixture']).encode())
    return after

forward = edited(initial, 'forward')
reverse = edited(forward, 'reverse')
pairs = [(initial, forward, 'forward'), (forward, reverse, 'reverse')]
original_bytes = [s.encoded(v) for v in (initial, forward, reverse)]
for before, after, direction in pairs:
    assert s.declared_edit(before, after, direction) is after
    assert s.declared_edit(before, after, direction, limited=True) is after
    # Only existing protocol stamp exclusions remain in force.
    stamped = copy.deepcopy(after)
    for name, columns in s.COLUMNS.items():
        for r in table(stamped, name):
            for i, column in enumerate(columns.split()):
                if column in s.STAMPS[name]:
                    r[i] = integer(987)
    assert s.declared_edit(before, stamped, direction) is stamped

    def rejects(label, mutate_before=None, mutate_after=None):
        old, new = copy.deepcopy(before), copy.deepcopy(after)
        if mutate_before:
            mutate_before(old)
        if mutate_after:
            mutate_after(new)
        try:
            s.declared_edit(old, new, direction)
        except ValueError:
            pass
        else:
            raise AssertionError(direction + ': admitted ' + label)

    def set_cell(name, identity, column, value):
        return lambda v: identified(v, name, identity).__setitem__(column, value)
    for did in (1, 42, 43, 44):
        for column, value in [(0, integer(999)), (1, text('Changed name')), (5, blob(b'changed kind'))]:
            rejects('deck identity/name/kind', mutate_after=set_cell('decks', did, column, value))
    for did in (1, 43, 44):
        rejects('nonancestor common', mutate_after=set_cell('decks', did, 4, blob(b'\x08\x01\x10\x01\x20\x01\x38\x84\x07')))
    for did in (1, 42, 43, 44):
        # Changing both card copies cannot make another original did authored.
        rejects('wrong original review did', set_cell('cards', 31, 2, integer(did)), set_cell('cards', 31, 2, integer(did)))
    for column, value in [(0, integer(999)), (1, integer(33)), (2, integer(42)), (3, integer(1)), (16, integer(1))]:
        rejects('review card identity', mutate_after=set_cell('cards', 31, column, value))
    for queue in (-3, -2, -1, 4, 99, 1 if direction == 'forward' else 0):
        rejects('ineligible original queue', mutate_before=set_cell('cards', 31, 7, integer(queue)))
    # Complete raw comparisons reject known-field drift, duplicate/default
    # encodings, and unknown/other bytes on either side, not just lost fields.
    for did in (41, 42):
        for which in ('before', 'after', 'both'):
            for suffix in ('1801', '2001', '2801', '3001', '3801', '4001', 'fa0f0178',
                           '0801', '1001', '0800', '1000', '1800', '2000', '2800', '3800', '80'):
                def append_common(value, did=did, suffix=suffix):
                    cell = identified(value, 'decks', did)[4]
                    cell['blob_hex'] += suffix
                rejects('deck ' + str(did) + ' extra raw bytes ' + which,
                    append_common if which in ('before', 'both') else None,
                    append_common if which in ('after', 'both') else None)
    # Keep the parent correct while corrupting only complete HOME bytes.
    # Include structurally valid but wrong counters, elapsed time and flags;
    # the append cases above also cover day, duplicate/default/unknown/other.
    home_raw_cases = (
        [('before', ('', '08011001', '1000', '10012001', '10012801', '10013801')),
         ('after', ('', '080110012002388407', '080110012801388407',
                    '080110012001388507', '10012001388407', '080110002001388407'))]
        if direction == 'forward' else
        [('before', ('', '080110012002388407', '080110012801388407',
                     '080110012001388507', '10012001388407', '080110002001388407')),
         ('after', ('', '1001200238d00f', '1001280138d00f', '1001200138d10f',
                    '08011001200138d00f', '1000200138d00f'))])
    for which, raws in home_raw_cases:
        for raw in raws:
            mutate = set_cell('decks', 41, 4, blob(bytes.fromhex(raw)))
            rejects('correct parent with malformed home ' + which,
                mutate if which == 'before' else None, mutate if which == 'after' else None)
    for raw in ('08011001', '080110012002388407', '080110012801388407',
                '080110012001388507', '10012001388407', '0801100118012001388407'):
        rejects('wrong after counter/category/time/day/flag', mutate_after=set_cell('decks', 42, 4, blob(bytes.fromhex(raw))))
    rejects('wrong original parent baseline', mutate_before=set_cell('decks', 42, 4, blob(b'')))
    for elapsed in (-1, 63001, 901 if direction == 'forward' else 64000):
        rejects('review time bounds', mutate_after=lambda v, elapsed=elapsed: table(v, 'revlog')[-1].__setitem__(7, integer(elapsed)))
    def matched_invalid_time(value):
        # Matching home/parent arithmetic must not excuse a wrong fixed forward
        # elapsed time or a reverse time above the authored 63-second cap.
        table(value, 'revlog')[-1][7] = integer(901 if direction == 'forward' else 63001)
        identified(value, 'decks', 42)[4] = blob(bytes.fromhex(
            '080110012001388507' if direction == 'forward' else '080110012001389df303'))
        identified(value, 'decks', 41)[4] = blob(bytes.fromhex(
            '080110012001388507' if direction == 'forward' else '10012001389df303'))
    rejects('matched but disallowed review time', mutate_after=matched_invalid_time)
    def matched_invalid_queue(value):
        identified(value, 'cards', 31)[7] = integer(1 if direction == 'forward' else 0)
    def matched_invalid_category(value):
        identified(value, 'decks', 42)[4] = blob(bytes.fromhex(
            '08011001388407' if direction == 'forward' else '08011001200238d00f'))
        identified(value, 'decks', 41)[4] = blob(bytes.fromhex(
            '08011001388407' if direction == 'forward' else '1001200238d00f'))
    rejects('matched but disallowed original queue', matched_invalid_queue, matched_invalid_category)
    rejects('time not bound to common', mutate_after=lambda v: table(v, 'revlog')[-1].__setitem__(7, integer(899)))
    for column, value in [(1, integer(33)), (3, integer(4))]:
        rejects('new review identity/grade', mutate_after=lambda v, column=column, value=value: table(v, 'revlog')[-1].__setitem__(column, value))
    def extra_review(value):
        review = copy.deepcopy(table(value, 'revlog')[-1])
        review[0] = integer(999)
        table(value, 'revlog').append(review)
    rejects('extra review', mutate_after=extra_review)
    if direction == 'reverse':
        rejects('original history mutation', mutate_after=lambda v: table(v, 'revlog')[0].__setitem__(7, integer(901)))
        rejects('original history removal', mutate_after=lambda v: table(v, 'revlog').pop(0))
    for name, identity, column, value in [('notes', 11, 10, text('data drift')),
        ('notes', 11, 5, text('tags drift')), ('cards', 31, 11, integer(99)),
        ('notetypes', 22, 4, blob(b'config drift')), ('deck_config', 62, 4, blob(b'config drift'))]:
        rejects('existing data/settings guard', mutate_after=set_cell(name, identity, column, value))
    if direction == 'forward':
        rejects('unreviewed card data', mutate_after=set_cell('cards', 33, 17, text('data drift')))
    for name in ('col', 'fields', 'tags'):
        rejects('whole table drift', mutate_after=lambda v, name=name: table(v, name)[0].__setitem__(-1, blob(b'drift')))
    rejects('other template config', mutate_after=lambda v: table(v, 'templates')[1].__setitem__(5, blob(b'drift')))
    rejects('foreign config bytes', mutate_after=lambda v: table(v, 'config')[1].__setitem__(3, blob(b'{"keep":[1,2]}')))
    rejects('declared config value', mutate_after=lambda v: table(v, 'config')[0].__setitem__(3, blob(b'{}')))
    rejects('whole media drift', mutate_after=lambda v: v['media_sha256'].clear())
    rejects('missing tombstone', mutate_after=lambda v: table(v, 'graves').pop())
    for name, identity, columns in [('notetypes', 21, (4,)), ('deck_config', 61, (4,)), ('decks', 41, (4, 5))]:
        def undo_edit(value, name=name, identity=identity, columns=columns):
            old, new = identified(before, name, identity), identified(value, name, identity)
            for column in columns:
                new[column] = copy.deepcopy(old[column])
        rejects('required selected object edit', mutate_after=undo_edit)

# Reverse category comes from its original queue, regardless of log type.
# These are prospective policy pairs, not a claimed reverse observation.
for elapsed, parent_common, home_common in [
    (0, '080110012001388407', '10012001388407'),
    (63000, '080110012001389cf303', '10012001389cf303')]:
    after = copy.deepcopy(reverse)
    table(after, 'revlog')[-1][7] = integer(elapsed)
    identified(after, 'decks', 42)[4] = blob(bytes.fromhex(parent_common))
    identified(after, 'decks', 41)[4] = blob(bytes.fromhex(home_common))
    assert s.declared_edit(forward, after, 'reverse') is after
    # Regression: correct parent arithmetic alone cannot admit stale HOME time.
    identified(after, 'decks', 41)[4] = copy.deepcopy(identified(reverse, 'decks', 41)[4])
    try:
        s.declared_edit(forward, after, 'reverse')
    except ValueError:
        pass
    else:
        raise AssertionError('reverse elapsed time admitted with stale home')
for queue in (2, 3):
    before, after = copy.deepcopy(forward), copy.deepcopy(reverse)
    identified(before, 'cards', 31)[7] = integer(queue)
    identified(after, 'decks', 42)[4] = blob(bytes.fromhex('080110012001280138d00f'))
    identified(after, 'decks', 41)[4] = blob(bytes.fromhex('10012001280138d00f'))
    assert table(after, 'revlog')[-1][8] == integer(0)
    assert s.declared_edit(before, after, 'reverse') is after
    # A correct parent must not hide a missing HOME review counter.
    identified(after, 'decks', 41)[4] = blob(bytes.fromhex('1001200138d00f'))
    try:
        s.declared_edit(before, after, 'reverse')
    except ValueError:
        pass
    else:
        raise AssertionError('reverse review category admitted with stale home')
    identified(after, 'decks', 41)[4] = blob(bytes.fromhex('10012001280138d00f'))
    identified(after, 'decks', 42)[4] = blob(bytes.fromhex('08011001200138d00f'))
    try:
        s.declared_edit(before, after, 'reverse')
    except ValueError:
        pass
    else:
        raise AssertionError('reverse review category taken from revlog.type')

# Ancestors are resolved from original names; renaming an after-row cannot
# grant an unrelated row permission. Missing immediate parents stop traversal.
before, after = copy.deepcopy(initial), copy.deepcopy(forward)
identified(after, 'decks', 44)[1] = text(parent_name)
try:
    s.declared_edit(before, after, 'forward')
except ValueError:
    pass
else:
    raise AssertionError('after-name ancestor admitted')
for value in (before, after):
    table(value, 'decks')[:] = [r for r in table(value, 'decks') if r[0] != integer(42)]
identified(after, 'decks', 44)[1] = text('Unrelated')
assert s.declared_edit(before, after, 'forward') is after
identified(after, 'decks', 1)[4] = blob(bytes.fromhex('080110012001388407'))
try:
    s.declared_edit(before, after, 'forward')
except ValueError:
    pass
else:
    raise AssertionError('default deck exemption admitted')
assert [s.encoded(v) for v in (initial, forward, reverse)] == original_bytes
PY
  [ "$status" -eq 0 ]
}

@test "retained peer successful offline companion capture checkpoint and final attachment preserve complete closure" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import copy, importlib.util, pathlib, sys
repo, root = map(pathlib.Path, sys.argv[1:])
spec = importlib.util.spec_from_file_location('retained', repo / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
# Reuse the existing public synthetic package fixture and its real validators.
# No container/SDK/HTTP double, execution or compatibility evidence is produced.
saved_argv = sys.argv[:]
sys.argv = ['policy', str(repo / 'server/tools/package-context.py')]
fixture_spec = importlib.util.spec_from_file_location('package_policy', repo / 'tools/tests/sync_package_context_test.py')
fixture = importlib.util.module_from_spec(fixture_spec)
fixture_spec.loader.exec_module(fixture)
sys.argv = saved_argv
proof = fixture.FinalPackageProof()
try:
    proof.setUp()
    inputs, raw = root / 'input', root / 'raw'
    inputs.mkdir()
    raw.mkdir()
    store = inputs / 'artifacts'
    s.copy_cas(proof.artifacts, store)
    invocation = {k: v for k, v in proof.invocation.items() if k not in ('child_phase', 'child_stage')}
    invocation['arguments'] = ['--lane', 'retained', '--phase', 'final']
    binding = copy.deepcopy(proof.binding)
    before_bytes = s.artifact_reader(store)(binding['container_inspection_before_sha256'])
    inspection = s.json.loads(before_bytes)
    inspection[0].update(Args=invocation['server_arguments'], Path='/usr/local/bin/meguri-sync-server')
    before_bytes = s.encoded(inspection)
    binding['container_inspection_before_sha256'] = s.cas_put(store, before_bytes)
    binding['labels'] = inspection[0]['Config']['Labels']
    del binding['container_inspection_after_sha256']
    preflight = dict(proof.peer, kind='final_package_preflight', execution_binding_sha256=s.cas_put(store, s.encoded(binding)))
    action = dict(case='full-upload', action='prepare_rich')
    # A complete synthetic review document, explicitly no actual acceptance.
    synthetic_root = root / 'review-producer'
    review_path = synthetic_root / 'build/final-client-unit/retained-accepted-source-review-20261117.txt'
    review_path.parent.mkdir(parents=True)
    review_bytes = ('SYNTHETIC complete UTF-8 source/preparation review — policy fixture only\n'
        'Scope: entire synthetic package, companion input/output, checkpoint and final closure.\n'
        'Findings: verify exact bytes and unchanged execution binding across offline attachment.\n'
        'Decision: no actual source, preparation, runtime or compatibility acceptance claimed.\n').encode()
    review_path.write_bytes(review_bytes)
    expectations = dict(format='meguri-retained-companion-input-v1', **action, actor='client_b',
        recipe_sha256=s.cas_put(store, s.regular(s.RECIPES)), selection_sha256=s.cas_put(store, s.regular(s.SELECTION)),
        review_sha256=s.cas_put(store, review_bytes), baseline_sha256=None, expected_sha256=None)
    for name, value in [('action.json', action), ('invocation.json', invocation), ('preflight.json', preflight),
                        ('expectations.json', expectations)]:
        s.frozen_file(inputs / name, s.encoded(value))
    s.companion_context(inputs)
    s.strict_package(preflight, store, invocation, False)
    s.copy_cas(store, raw / 'artifacts')
    snap = dict(format='meguri-retained-snapshot-v1', tables={name: dict(columns=columns.split(), rows=[])
        for name, columns in s.COLUMNS.items()}, media_sha256={'policy.txt': s.digest(b'synthetic media')})
    snap_sha = s.cas_put(raw / 'artifacts', s.encoded(snap))
    runtime_sha = s.cas_put(raw / 'artifacts', b'synthetic Desktop runtime identity, never executable\n')
    observation = dict(format='meguri-retained-companion-v1', **action, actor='client_b',
        invocation_sha256=s.digest(s.regular(inputs / 'invocation.json')),
        preflight_peer_sha256=s.digest(s.regular(inputs / 'preflight.json')),
        expectations_sha256=s.digest(s.regular(inputs / 'expectations.json')),
        client=dict(kind='desktop', version='26.09.4', build='c06370ff', runtime_sha256=runtime_sha),
        before_sha256=None, after_sha256=snap_sha, authority_sha256=snap_sha, wire_sha256=None,
        observed=dict(operation_complete=True, error_kind=None, expected_state_equal=None), case_credit=False)
    original = s.encoded(observation)
    for field, changed in [('build', 'another-build'), ('version', '26.09.3')]:
        invalid = copy.deepcopy(observation)
        invalid['client'][field] = changed
        (raw / 'observations.json').write_bytes(s.encoded(invalid))
        try:
            s.companion_capture(raw, inputs, root / ('rejected-' + field))
        except ValueError as error:
            assert str(error) == 'pinned companion client'
        else:
            raise AssertionError('unpinned Desktop admitted: ' + field)
        assert not (root / ('rejected-' + field)).exists()
    s.ROOT = synthetic_root
    assert s.accepted_review() == review_bytes
    s.ROOT = repo
    (raw / 'observations.json').write_bytes(original)
    capture_sha = s.companion_capture(raw, inputs, root / 'capture')
    checkpoint = dict(raw=s.tree_inventory(raw, root / 'checkpoint-artifacts'), wire=None,
        capture_sha256=capture_sha, case_credit=False)
    order = []
    def after_boundary():
        assert s.digest(s.regular(root / 'checkpoint.json')) == (root / 'checkpoint.sha256').read_text().strip()
        assert s.artifact_reader(root / 'capture/artifacts')(s.digest(original)) == original
        assert s.artifact_reader(root / 'capture/artifacts')(expectations['review_sha256']) == review_bytes
        order.append('after-checkpoint')
        # Synthetic inspection bytes only, with the same process and lifetime.
        return binding['server_process'], s.encoded(s.json.loads(before_bytes))
    checkpoint_sha, (process, after_bytes) = s.checkpoint_after(root, checkpoint, after_boundary)
    final_peer = s.package_complete(inputs, root / 'final-input', invocation, after_bytes, process)
    s.companion_finalize(root / 'capture', inputs, root / 'final-input', root / 'final', capture_sha)
    order.append('offline-finalized')
    attachment = s.load_json(root / 'final/final-binding.json')
    assert order == ['after-checkpoint', 'offline-finalized']
    assert checkpoint_sha == s.digest(s.regular(root / 'checkpoint.json'))
    assert attachment['case_credit'] is False and attachment['capture_sha256'] == capture_sha
    assert attachment['execution_binding_sha256'] == final_peer['execution_binding_sha256']
    assert s.regular(root / 'final/observations.json') == original
    read = s.artifact_reader(root / 'final/artifacts')
    assert read(expectations['review_sha256']) == review_bytes
    assert read(runtime_sha) == b'synthetic Desktop runtime identity, never executable\n'
    assert read(snap_sha) == s.encoded(snap)
    for entry in checkpoint['raw']['files']:
        assert len(read(entry['sha256'])) == entry['bytes']
    # Finalization rehashes the accepted original observation, never a new one.
    captured_raw = root / 'capture/artifacts' / s.digest(original)
    captured_raw.chmod(0o600)
    captured_raw.write_bytes(original + b' ')
    try:
        s.companion_finalize(root / 'capture', inputs, root / 'final-input', root / 'rejected-final', capture_sha)
    except ValueError as error:
        assert str(error) == 'artifact hash'
    else:
        raise AssertionError('changed captured observation finalized')
    assert not (root / 'rejected-final').exists()
finally:
    proof.doCleanups()
PY
  [ "$status" -eq 0 ]
}

@test "retained peer measured partial wire completions survive validation but never succeed" {
  run python3 - "$REPO_ROOT" <<'PY'
import copy, importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
action = dict(case='authentication', action='authenticate')
# Synthetic sanitized policy records, not HTTP replies or observed execution.
wire = dict(format='meguri-retained-wire-v1', **action, actor='client_a', invocation_sha256='a'*64, case_credit=False,
    requests=[dict(sequence=1, method='POST', path='/sync/meta', request_body_bytes=7,
        upstream_status=200, upstream_body_bytes=13, delivered_body_bytes=5, disposition='incomplete')],
    totals=dict(request_count=1, upstream_completed_count=0, delivered_completed_count=0,
        request_body_bytes=7, upstream_body_bytes=13, delivered_body_bytes=5, dropped_body_bytes=8),
    fault=dict(actor='fault_driver', armed=False, accepted_upload_ack=False, reply_dropped=False,
        upload_request_count=0, upload_replay_count=0))
partials = []
for upstream_count in (0, 1):
    partial = copy.deepcopy(wire)
    partial['totals']['upstream_completed_count'] = upstream_count
    # The SAME body lengths permit partial upstream or completed upstream with
    # partial downstream; only the independently measured total distinguishes them.
    before = copy.deepcopy(partial)
    assert s.wire_check(partial, action, 'client_a', 'a'*64) == before
    assert partial == before
    partials.append(partial)
assert all(not s.wire_complete(partial) for partial in partials)
# Incomplete upstream can also have sent every currently observed byte.
equal_bytes = copy.deepcopy(wire)
equal_bytes['requests'][0]['delivered_body_bytes'] = 13
equal_bytes['totals'].update(delivered_body_bytes=13, dropped_body_bytes=0)
assert s.wire_check(equal_bytes, action, 'client_a', 'a'*64) == equal_bytes
assert not s.wire_complete(equal_bytes)
complete = copy.deepcopy(equal_bytes)
complete['requests'][0]['disposition'] = 'forwarded'
complete['totals'].update(upstream_completed_count=1, delivered_completed_count=1)
assert s.wire_check(complete, action, 'client_a', 'a'*64) == complete and s.wire_complete(complete)
invalid = []
for key, value in [('upstream_completed_count', 2), ('upstream_completed_count', -1),
                   ('upstream_completed_count', True), ('delivered_completed_count', 1),
                   ('request_count', 2), ('dropped_body_bytes', 7), ('upstream_body_bytes', 14)]:
    changed = copy.deepcopy(wire)
    changed['totals'][key] = value
    invalid.append(changed)
missing_status = copy.deepcopy(wire)
missing_status['requests'][0]['upstream_status'] = None
missing_status['totals']['upstream_completed_count'] = 1
invalid.append(missing_status)
missing_completion = copy.deepcopy(complete)
missing_completion['totals']['upstream_completed_count'] = 0
invalid.append(missing_completion)
for changed in invalid:
    try:
        s.wire_check(changed, action, 'client_a', 'a'*64)
    except ValueError as error:
        assert str(error) == 'wire totals'
    else:
        raise AssertionError('impossible measured wire totals admitted')
PY
  [ "$status" -eq 0 ]
}

@test "retained peer accepted review requires fixed regular nonempty UTF8 bytes without interpreting them" {
  run python3 - "$REPO_ROOT" "$BATS_TEST_TMPDIR" <<'PY'
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location('retained', pathlib.Path(sys.argv[1]) / 'conformance/peers/retained-sync/support.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
s.ROOT = pathlib.Path(sys.argv[2])
path = s.ROOT / 'build/final-client-unit/retained-accepted-source-review-20261117.txt'
path.parent.mkdir(parents=True)
def rejected():
    try:
        s.accepted_review()
    except (OSError, ValueError):
        pass
    else:
        raise AssertionError('unavailable or invalid review accepted')
rejected()  # Missing trusted producer output.
path.mkdir()
rejected()
path.rmdir()
for data in (b'', b' \n\t', b'\xffnot UTF-8'):
    path.write_bytes(data)
    rejected()
path.unlink()
target = path.with_name('synthetic-link-target.txt')
target.write_bytes(b'synthetic nonempty bytes')
path.symlink_to(target)
rejected()
path.unlink()
data = ('SYNTHETIC complete UTF-8 review — no actual acceptance claim\r\n'
    'Scope: every synthetic input and output in this policy fixture.\r\n'
    'Findings: preserve all bytes, including newlines and Unicode.\r\n'
    'Decision: policy test data only.\r\n').encode()
path.write_bytes(data)
assert s.accepted_review() == data
PY
  [ "$status" -eq 0 ]
}

# These exercise the real mode, source/lock/feature admission and report parser
# with public synthetic Cargo process data. They never execute Rust tests.
retained_unit_fixture() {
  MG_ROOT="$BATS_TEST_TMPDIR/unit-repo"
  MG_RUN_DIR="$BATS_TEST_TMPDIR/unit-run"
  UNIT_DATA="$BATS_TEST_TMPDIR/unit-data"
  mkdir -p "$MG_ROOT/conformance/peers/retained-sync/src" "$MG_ROOT/core" "$MG_ROOT/server" \
    "$MG_ROOT/conformance/recordings/scenarios/meguri-sync-server" "$MG_ROOT/tools/mg.d" "$MG_RUN_DIR" "$UNIT_DATA"
  cp "$REPO_ROOT/conformance/peers/retained-sync/support.py" "$MG_ROOT/conformance/peers/retained-sync/"
  cp "$REPO_ROOT/tools/mg.d/sync-retained.sh" "$MG_ROOT/tools/mg.d/"
  cp "$REPO_ROOT/conformance/recordings/scenarios/meguri-sync-server/"{final-fixture-recipes,candidate-retained-final-selection}.json \
    "$MG_ROOT/conformance/recordings/scenarios/meguri-sync-server/"
  python3 - "$MG_ROOT" "$UNIT_DATA" <<'PY'
import json, pathlib, sys
root, data = map(pathlib.Path, sys.argv[1:])
peer = root / 'conformance/peers/retained-sync'
(root / 'core/Cargo.toml').write_text('[workspace]\nmembers = ["crates/*", "tools/*"]\n')
(root / 'server/Cargo.toml').write_text('[workspace]\nmembers = []\n')
(peer / 'Cargo.toml').write_text('[workspace]\nresolver = "3"\n[dependencies.meguri-sync]\nfeatures = ["test-hooks"]\n')
(root / 'core/Cargo.lock').write_text('version = 4\npackage = []\n')
(peer / 'Cargo.lock').write_text('version = 4\n[[package]]\nname = "meguri-retained-sync-peer"\nversion = "0.0.0"\n')
(peer / 'src/main.rs').write_text('// synthetic source: never compiled or executed\n')
# Only controlled compiler/report input: no fabricated runtime acceptance.
exe = data / 'synthetic-binary'
exe.write_bytes(b'synthetic compiler output; never executed\n')
names = ['meguri-engine', 'meguri-model', 'meguri-storage', 'meguri-sync', 'meguri-retained-sync-peer']
packages = [dict(id=name, name=name, version='0.0.0', source=None) for name in names]
(data / 'graph.json').write_text(json.dumps(dict(packages=packages)))
messages = [dict(reason='compiler-artifact', package_id=name, features=['test-hooks'],
                 target=dict(name=name), executable=None) for name in names[:-1]]
messages += [dict(reason='compiler-artifact', package_id=names[-1], features=[],
                  target=dict(name=names[-1]), executable=str(exe)), dict(reason='build-finished', success=True)]
(data / 'build.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in messages))
binary = 'meguri-retained-sync-peer'
identity = dict(crate=binary, test_binary=binary, kind='bin')
names = [binary + '::' + binary + '$fsrs_prerequisite::tests::' + name for name in (
    'captures_bracket_one_real_public_enable_before_forward_mutations',
    'failed_enable_stops_after_before_capture_without_authority_or_retry',
    'empty_bootstrap_cannot_call_enable_or_supply_authority',
    'fixed_input_rejects_nonnew_history_and_present_key_without_parsing_data')]
events = [dict(type='suite', event='started', test_count=len(names), nextest=identity)]
for name in names:
    events += [dict(type='test', event=state, name=name) for state in ('started', 'ok')]
events += [dict(type='suite', event='ok', passed=len(names), failed=0, ignored=0, measured=0,
                filtered_out=0, nextest=identity)]
(data / 'good.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in events))
PY
  git -C "$MG_ROOT" init -q
  git -C "$MG_ROOT" add -A
  git -C "$MG_ROOT" -c user.name=selftest -c user.email=selftest@localhost commit -qm synthetic
  UNIT_STREAM="$UNIT_DATA/good.jsonl"
  UNIT_PROCESS_RC=0
  UNIT_CHANGE_SOURCE=false
  rustc() { [ "$*" = -vV ] && printf 'host: synthetic-host\n'; }
  cargo() {
    printf '%s\n' "$@" >>"$UNIT_DATA/commands"
    case "$1" in
      metadata) cat "$UNIT_DATA/graph.json" ;;
      build) cat "$UNIT_DATA/build.jsonl" ;;
      nextest)
        printf '%s\0' "$@" >"$UNIT_DATA/nextest-args"
        [ "${NEXTEST_EXPERIMENTAL_LIBTEST_JSON:-}" = 1 ] || return 97
        cat "$UNIT_STREAM" || return
        if [ "$UNIT_CHANGE_SOURCE" = true ]; then
          printf '// changed during process\n' >>"$MG_ROOT/conformance/peers/retained-sync/src/main.rs"
        fi
        return "$UNIT_PROCESS_RC" ;;
      *) return 97 ;;
    esac
  }
  mg_artifact() { :; }
  mg_detail() { :; }
  mg_message() { :; }
  mg_count() { printf 'count:%s:%s\n' "$1" "$2"; }
}

@test "retained peer-unit dry run stays in Docker dev without executing the producer" {
  run "$REPO_ROOT/tools/mg" sync-server-retained --dry-run peer-unit
  [ "$status" -eq 0 ]
  result="${lines[${#lines[@]}-1]}"
  run jq -e '.job=="sync-server-retained" and .dry_run==true and .status=="pass" and .service=="dev"' "$result"
  [ "$status" -eq 0 ]
}

@test "retained peer-unit accepts no caller selectors or extra arguments" {
  local extra
  for extra in command output manifest features target filter ''; do
    run mg_sync_retained peer-unit "$extra"
    [[ "$output" == *'failure:usage'* && "$output" == *'status:error'* ]]
  done
  run mg_sync_retained
  [[ "$output" == *'failure:usage'* && "$output" == *'status:error'* ]]
}

@test "retained peer-unit runs the fixed producer and admits four actual completions with retained source and locks" {
  retained_unit_fixture
  run mg_sync_retained peer-unit
  [ "$status" -eq 0 ]
  [[ "$output" == *'status:pass'* && "$output" != *'failure:'* ]]
  [[ "$output" == *'count:total:4'* && "$output" == *'count:passed:4'* ]]
  [[ "$output" == *'count:failed:0'* && "$output" == *'count:skipped:0'* ]]
  cmp "$UNIT_STREAM" "$MG_RUN_DIR/nextest.jsonl"
  cmp "$MG_RUN_DIR/source-before.json" "$MG_RUN_DIR/source-after.json"
  cmp "$MG_ROOT/core/Cargo.lock" "$MG_RUN_DIR/unit-source/core/Cargo.lock"
  cmp "$MG_ROOT/conformance/peers/retained-sync/Cargo.lock" "$MG_RUN_DIR/unit-source/conformance/peers/retained-sync/Cargo.lock"
  cmp "$MG_ROOT/conformance/peers/retained-sync/src/main.rs" "$MG_RUN_DIR/unit-source/conformance/peers/retained-sync/src/main.rs"
  run python3 - "$UNIT_DATA" "$MG_ROOT" "$MG_RUN_DIR" <<'PY'
import json, pathlib, sys
fixture, root, run = map(pathlib.Path, sys.argv[1:])
args = (fixture / 'nextest-args').read_bytes().decode().split('\0')
assert args == ['nextest', 'run', '--locked', '--offline', '--manifest-path',
    str(root / 'conformance/peers/retained-sync/Cargo.toml'), '--bin', 'meguri-retained-sync-peer',
    '--target', 'synthetic-host', '--retries', '0', '--no-fail-fast', '--no-tests=fail',
    '-E', 'test(/^fsrs_prerequisite::tests::/)', '--message-format', 'libtest-json-plus',
    '--message-format-version', '0.1', '']
report = json.loads((run / 'peer-unit-evidence.json').read_text())
assert {key: report[key] for key in ('executed', 'passed', 'failed', 'skipped', 'cancelled')} == dict(
    executed=4, passed=4, failed=0, skipped=0, cancelled=0)
assert report['errors'] == [] and len(report['names']) == 4
receipt = json.loads((run / 'client/client-build.json').read_text())
assert receipt['source_dirty'] is False and receipt['features']
for key in ('source_sha256', 'core_lock_sha256', 'peer_lock_sha256', 'resolved_graph_sha256'):
    assert (run / 'client/artifacts' / receipt[key]).is_file()
PY
  [ "$status" -eq 0 ]
}

@test "retained peer-unit rejects missing extra wrong empty ignored skipped cancelled and malformed actual reports" {
  retained_unit_fixture
  python3 - "$UNIT_DATA" <<'PY'
import copy, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
good = [json.loads(line) for line in (root / 'good.jsonl').read_text().splitlines()]
variants = {}
variants['missing'] = [row for row in good if row.get('name') != good[1]['name']]
variants['unrun'] = [row for row in good if row.get('event') != 'started' or row.get('type') != 'test']
variants['extra'] = good[:-1] + [dict(type='test', event='started', name=good[1]['name'] + '_extra')] + good[-1:]
variants['wrong'] = copy.deepcopy(good)
for row in variants['wrong']:
    if 'name' in row:
        row['name'] = row['name'].replace('fsrs_prerequisite::tests::', 'other::tests::')
variants['empty'] = []
variants['incomplete'] = good[:-1]
variants['duplicate'] = good[:2] + [good[1]] + good[2:]
variants['suite-identity'] = copy.deepcopy(good)
variants['suite-identity'][0]['nextest']['test_binary'] = 'other'
for state in ('ignored', 'skipped', 'cancelled', 'failed'):
    events = copy.deepcopy(good)
    events[2]['event'] = state
    events[-1].update(passed=3, failed=int(state == 'failed'), ignored=int(state in ('ignored', 'skipped')))
    variants[state] = events
for name, value in [('negative', -1), ('boolean', True), ('summary-only', 4.0)]:
    events = copy.deepcopy(good)
    events[-1]['passed'] = value
    variants[name] = events
for name, rows in variants.items():
    (root / (name + '.jsonl')).write_text(''.join(json.dumps(row) + '\n' for row in rows))
(root / 'malformed.jsonl').write_bytes((root / 'good.jsonl').read_bytes() + b'{invalid\n')
(root / 'duplicate-key.jsonl').write_bytes((root / 'good.jsonl').read_bytes().replace(
    b'\"event\": \"ok\"', b'\"event\": \"failed\", \"event\": \"ok\"', 1))
PY
  local variant
  for variant in missing unrun extra wrong empty incomplete duplicate suite-identity ignored skipped cancelled failed negative boolean summary-only malformed duplicate-key; do
    MG_RUN_DIR="$BATS_TEST_TMPDIR/run-$variant"
    mkdir "$MG_RUN_DIR"
    UNIT_STREAM="$UNIT_DATA/$variant.jsonl"
    run mg_sync_retained peer-unit
    [ "$status" -eq 0 ]
    [[ "$output" == *'failure:peer-unit-evidence'* && "$output" == *'status:fail'* ]]
    [[ "$output" != *'status:pass'* ]]
    cmp "$UNIT_STREAM" "$MG_RUN_DIR/nextest.jsonl"
    run jq -e '.errors | length > 0' "$MG_RUN_DIR/peer-unit-evidence.json"
    [ "$status" -eq 0 ]
  done
}

@test "retained peer-unit refuses a failed process despite a complete four-pass report" {
  retained_unit_fixture
  UNIT_PROCESS_RC=71
  run mg_sync_retained peer-unit
  [[ "$output" == *'failure:nextest-exit-71'* && "$output" == *'status:fail'* ]]
  [[ "$output" != *'status:pass'* ]]
  cmp "$MG_RUN_DIR/source-before.json" "$MG_RUN_DIR/source-after.json"
  run jq -e '.executed==4 and .errors==[]' "$MG_RUN_DIR/peer-unit-evidence.json"
  [ "$status" -eq 0 ]
}

@test "retained peer-unit refuses source changes despite a complete four-pass report" {
  retained_unit_fixture
  UNIT_CHANGE_SOURCE=true
  run mg_sync_retained peer-unit
  [[ "$output" == *'failure:peer-unit-source'* && "$output" == *'status:fail'* ]]
  [[ "$output" != *'status:pass'* ]]
  run cmp "$MG_RUN_DIR/source-before.json" "$MG_RUN_DIR/source-after.json"
  [ "$status" -ne 0 ]
  run jq -e '.executed==4 and .errors==[]' "$MG_RUN_DIR/peer-unit-evidence.json"
  [ "$status" -eq 0 ]
}

@test "retained peer-unit preserves the existing feature graph refusal before producing tests" {
  retained_unit_fixture
  python3 - "$UNIT_DATA/build.jsonl" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
rows = [json.loads(line) for line in path.read_text().splitlines()]
rows[0]['features'].append('ankiweb')
path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
PY
  run mg_sync_retained peer-unit
  [[ "$output" == *'failure:retained-build'* && "$output" == *'status:fail'* ]]
  [[ "$output" != *'status:pass'* ]]
  [ ! -e "$UNIT_DATA/nextest-args" ]
}
