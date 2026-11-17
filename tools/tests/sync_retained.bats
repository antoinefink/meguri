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
        assert entrypoint == 'retained-observer' and command == ['--input', '/in']
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
