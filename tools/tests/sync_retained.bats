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
