# SPDX-License-Identifier: AGPL-3.0-or-later
"""Neutral candidate receipt and secret-file boundary; never starts a server."""
import os
import pathlib
import re
import stat

from jsonschema import Draft202012Validator
from validation import Artifacts, decode, require, same_value

ENDPOINT = 'http://127.0.0.1:8080/'


def verify_package(package, store, invocation, preflight=False):
    required = {'format','kind','server','source_dirty','profile','features','executable_artifact_sha256',
                'lock_artifact_sha256','mg_build_receipt_sha256','build_invocation','execution_binding_sha256'}
    kind = package.get('kind')
    if kind in {'final_package','final_package_preflight'}: required = required | {'package'}
    require(set(package) == required and package['format'] == 'meguri-sync-package-v1', 'invalid candidate peer receipt')
    require((kind == 'final_package_preflight' if preflight else kind in {'intermediate_build','final_package'}) and package['source_dirty'] is False, 'unverified or dirty candidate build')
    server = package['server']
    schema = decode(pathlib.Path('/opt/reference/schema.json').read_bytes())
    valid = Draft202012Validator({'$defs':schema['$defs'],'$ref':'#/$defs/server_peer'}).is_valid(server)
    require(valid and server['kind'] == 'meguri_sync', 'invalid candidate peer identity')
    require(package['profile'] in {'debug','release'}, 'unknown candidate build profile')
    features = package['features']
    require(isinstance(features,list) and all(isinstance(value,str) and value for value in features) and features == sorted(set(features)), 'invalid candidate features')
    require(all(same_value(server[key],invocation[key]) for key in ('source_commit','source_sha256','lock_sha256','executable_sha256')), 'candidate invocation source mismatch')
    require(package['executable_artifact_sha256'] == server['executable_sha256'], 'candidate executable digest mismatch')
    require(package['lock_artifact_sha256'] == server['lock_sha256'], 'candidate lock digest mismatch')
    store.read(package['executable_artifact_sha256'])
    store.read(package['lock_artifact_sha256'])
    built = store.json(package['mg_build_receipt_sha256'])
    require(built.get('status') == 'pass' and same_value(built.get('exit_code'),0) and built.get('dry_run') is False, 'candidate build did not pass')
    require(built.get('commit',{}).get('sha') == server['source_commit'] and built['commit'].get('dirty') is False, 'candidate build source mismatch')
    wanted_build = {'job':built['job'],'run_id':built['run_id'],'arguments':built['args']}
    require(same_value(package['build_invocation'],wanted_build), 'candidate MG build invocation mismatch')
    identity = {'server':server,'source_dirty':False,'profile':package['profile'],'features':features}
    if kind == 'intermediate_build':
        require(same_value(built.get('details',{}).get('candidate_build'),identity), 'candidate build fields differ from receipt')
    else:
        from final_package import package_dependencies
        package_dependencies(package,store,invocation,built)
    running = store.json(package['execution_binding_sha256'])
    require(running.get('format') == 'meguri-sync-candidate-execution-v1' and running.get('endpoint') == ENDPOINT, 'candidate endpoint is not fixed')
    require(same_value(running.get('server'),server), 'running candidate identity mismatch')
    require(running.get('run_id') == invocation['run_id'] and running.get('labels',{}).get('meguri.mg.run-id') == invocation['run_id'], 'candidate owned-run label mismatch')
    require(isinstance(running.get('container_id'),str) and re.fullmatch('[0-9a-f]{64}',running['container_id']), 'candidate container identity missing')
    require(running.get('observed_executable_sha256') == server['executable_sha256'], 'running executable differs from build')
    if kind in {'final_package','final_package_preflight'}:
        from final_package import execution_dependencies
        execution_dependencies(package,store,invocation,running,preflight=preflight)
    return package


def read_credentials(path):
    descriptor = os.open(path,os.O_RDONLY | os.O_NOFOLLOW)
    try:
        details = os.fstat(descriptor)
        require(stat.S_ISREG(details.st_mode) and stat.S_IMODE(details.st_mode) == 0o600 and details.st_nlink == 1, 'credential file must be one readonly-mounted 0600 regular file')
        require(details.st_uid == os.geteuid() and details.st_size <= 8192, 'credential file owner or size mismatch')
        with os.fdopen(descriptor,'rb',closefd=False) as source: value = decode(source.read(8193))
    finally: os.close(descriptor)
    require(isinstance(value,dict) and set(value) == {'username','password'}, 'invalid credential input')
    require(all(isinstance(value[key],str) and 0 < len(value[key]) <= 1024 for key in value), 'invalid logical credential fields')
    return value


def load_inputs(root, credential_path, invocation, preflight=False):
    root = pathlib.Path(root)
    name='preflight.json' if preflight else 'peer.json'
    require(not (root / name).is_symlink(), 'linked peer receipt')
    package = decode((root / name).read_bytes())
    store = Artifacts(root / 'artifacts')
    verify_package(package,store,invocation,preflight=preflight)
    credentials = read_credentials(credential_path)
    return package,store,credentials


def retained_dependencies(package, store, invocation, preflight=False):
    """Verify before exporting the complete immutable evidence closure."""
    verify_package(package,store,invocation,preflight=preflight)
    digests = {package[key] for key in ('executable_artifact_sha256','lock_artifact_sha256','mg_build_receipt_sha256','execution_binding_sha256')}
    if package['kind'] in {'final_package','final_package_preflight'}:
        from final_package import package_dependencies, execution_dependencies
        digests |= package_dependencies(package,store,invocation,store.json(package['mg_build_receipt_sha256']))
        digests |= execution_dependencies(package,store,invocation,store.json(package['execution_binding_sha256']),preflight=preflight)
    return digests
