#!/usr/bin/env python3
"""Retained build/capture/finalization only; no client execution or case credit."""
import hashlib
import json
import pathlib
import os
import ipaddress
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[3]
PEER = ROOT / 'conformance/peers/retained-sync'
RECIPES = ROOT / 'conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json'
SELECTION = RECIPES.with_name('candidate-retained-final-selection.json')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, indent=2) + '\n').encode()


def audit_lock(core, peer):
    def external(lock):
        return {(p['name'], p['version'], p['source'], p.get('checksum'))
                for p in lock['package'] if 'source' in p}
    if not external(peer) <= external(core):
        raise ValueError('peer lock introduced an external package identity')
    if any(p['name'] == 'meguri-retained-sync-peer' for p in core['package']):
        raise ValueError('peer entered the core graph')
    roots = [p for p in peer['package'] if p['name'] == 'meguri-retained-sync-peer']
    if len(roots) != 1:
        raise ValueError('peer lock requires exactly one worker package')


def check():
    selection = json.loads(SELECTION.read_bytes())
    names = ['authentication', 'full-upload', 'full-download', 'normal-no-change',
             'normal-bidirectional', 'media-bidirectional', 'media-no-change', 'manual-uncertain-recovery']
    cases = selection['cases']
    assert selection['lane_kind'] == 'retained_peer' and selection['phase'] == 'final'
    assert selection['fixture_recipes_sha256'] == digest(RECIPES.read_bytes())
    assert [c['variants'] for c in cases] == [[name] for name in names]
    assert len({c['id'] for c in cases}) == 8
    assert all(c['family'] == 'M-I01' and c['classification'] == 'meguri_policy'
               and c['client_kind'] == 'retained_meguri' for c in cases)
    core = tomllib.loads((ROOT / 'core/Cargo.toml').read_text())
    server = tomllib.loads((ROOT / 'server/Cargo.toml').read_text())
    assert core['workspace']['members'] == ['crates/*', 'tools/*']
    assert 'meguri-retained-sync-peer' not in json.dumps(server)
    manifest = tomllib.loads((PEER / 'Cargo.toml').read_text())
    assert manifest['workspace'] == {'resolver': '3'}
    assert manifest['dependencies']['meguri-sync']['features'] == ['test-hooks']
    assert not (PEER / 'Cargo.lock').exists() or verify_lock()
    return True


def verify_lock():
    audit_lock(tomllib.loads((ROOT / 'core/Cargo.lock').read_text()),
               tomllib.loads((PEER / 'Cargo.lock').read_text()))
    return True


def source_inputs():
    paths = subprocess.check_output(['git', '-C', str(ROOT), 'ls-files', '-z', '--',
        'core', 'conformance/peers/retained-sync', str(RECIPES.relative_to(ROOT)),
        str(SELECTION.relative_to(ROOT)), 'tools/mg.d/sync-retained.sh']).split(b'\0')
    source = []
    for raw in paths:
        if not raw:
            continue
        relative = raw.decode()
        path = ROOT / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError('nonregular source input')
        data = path.read_bytes()
        source.append(dict(path=relative, bytes=len(data), sha256=digest(data)))
    return dict(source_commit=subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
                files=sorted(source, key=lambda p: p['path']))


def freeze(out, graph, before, compilation):
    check()
    verify_lock()
    source = source_inputs()
    if encoded(source) != before.read_bytes():
        raise ValueError('source changed while compiling')
    if subprocess.check_output(['git', '-C', str(ROOT), 'status', '--porcelain=v1', '-z', '--untracked-files=all']):
        raise ValueError('source became dirty while compiling')
    out.mkdir(exist_ok=True, parents=True)
    artifacts = out / 'artifacts'
    artifacts.mkdir(exist_ok=True)

    def retain(data):
        sha = digest(data)
        target = artifacts / sha
        if target.exists() and target.read_bytes() != data:
            raise ValueError('artifact collision')
        target.write_bytes(data)
        return sha

    metadata = json.loads(graph.read_bytes())
    packages = {p['id']: p for p in metadata['packages']}
    messages = [json.loads(line) for line in compilation.read_text().split('\n') if line]
    artifacts_built = [m for m in messages if m['reason'] == 'compiler-artifact']
    executables = [m['executable'] for m in artifacts_built
        if m['target']['name'] == 'meguri-retained-sync-peer' and m.get('executable')]
    if len(executables) != 1 or not any(m['reason'] == 'build-finished' and m['success'] for m in messages):
        raise ValueError('missing unique successful Cargo executable artifact')
    executable = pathlib.Path(executables[0])
    resolved_by_bytes = {encoded(dict(name=packages[m['package_id']]['name'],
        version=packages[m['package_id']]['version'], source=packages[m['package_id']]['source'],
        features=sorted(m['features']))) for m in artifacts_built}
    resolved = sorted([json.loads(data) for data in resolved_by_bytes],
        key=lambda p: (p['name'], p['version'], str(p['source']), p['features']))
    hooks = sorted({p['name'] for p in resolved if 'test-hooks' in p['features']})
    if hooks != ['meguri-engine', 'meguri-model', 'meguri-storage', 'meguri-sync']:
        raise ValueError('unexpected hook propagation')
    if any(p['name'] == 'meguri-testkit' or 'ankiweb' in p['features'] for p in resolved):
        raise ValueError('unexpected stub or production service switch')
    data = executable.read_bytes()
    retained = out / 'meguri-retained-sync-peer'
    retained.write_bytes(data)
    retained.chmod(0o755)
    result = dict(format='meguri-retained-build-v1',
        author_session='s-20261116T225101Z-bbe6c19b',
        source_commit=source['source_commit'], source_sha256=retain(encoded(source)),
        core_lock_sha256=retain((ROOT / 'core/Cargo.lock').read_bytes()),
        peer_lock_sha256=retain((PEER / 'Cargo.lock').read_bytes()),
        resolved_graph_sha256=retain(encoded(dict(metadata=metadata, compiled_features=resolved))), executable_sha256=digest(data),
        recipe_sha256=digest(RECIPES.read_bytes()), selection_sha256=digest(SELECTION.read_bytes()),
        profile='dev', source_dirty=False, features=resolved)
    (out / 'client-build.json').write_bytes(encoded(result))


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def exact(value, names):
    require(isinstance(value, dict) and set(value) == set(names.split()), 'object keys')


def hash_value(value):
    require(isinstance(value, str) and len(value) == 64 and
            all(c in '0123456789abcdef' for c in value), 'hash shape')
    return value


def regular(path):
    require(not path.is_symlink() and path.is_file(), 'regular file required')
    return path.read_bytes()


def artifact_reader(*directories):
    def read(sha):
        hash_value(sha)
        matches = [regular(d / sha) for d in directories if (d / sha).exists()]
        require(matches and all(digest(data) == sha for data in matches), 'artifact hash')
        return matches[0]
    return read


def package_identity(peer, read, invocation, final):
    # Neutral package checks only. Canonical result/ownership measurements are
    # supplied and independently checked by the trusted parent MG job.
    load = lambda sha: json.loads(read(sha))
    exact(peer, 'format kind server source_dirty profile features executable_artifact_sha256 '
          'lock_artifact_sha256 mg_build_receipt_sha256 build_invocation execution_binding_sha256 package')
    require(peer['format'] == 'meguri-sync-package-v1' and
            peer['kind'] == ('final_package' if final else 'final_package_preflight') and
            peer['source_dirty'] is False and peer['profile'] == 'release' and peer['features'] == [], 'package kind')
    server, package = peer['server'], peer['package']
    require(server['kind'] == 'meguri_sync' and server['executable_sha256'] == peer['executable_artifact_sha256']
            and server['lock_sha256'] == peer['lock_artifact_sha256'], 'server identity')
    read(peer['executable_artifact_sha256'])
    read(peer['lock_artifact_sha256'])
    build = load(peer['mg_build_receipt_sha256'])
    exact(peer['build_invocation'], 'job run_id arguments')
    require(build['job'] == 'sync-server-package' and build['status'] == 'pass' and build['dry_run'] is False
            and build['args'] == [] and build['commit']['dirty'] is False
            and build['commit']['sha'] == server['source_commit'] and build['root'] == invocation['root']
            and peer['build_invocation'] == dict(job=build['job'], run_id=build['run_id'], arguments=build['args']), 'build receipt')
    exact(package, 'image_id inputs_sha256 runtime_inventory_sha256 image_inspection_sha256 assets_sha256')
    image, inputs, inventory = [load(package[k]) for k in
                              ['image_inspection_sha256', 'inputs_sha256', 'runtime_inventory_sha256']]
    assets = read(package['assets_sha256'])
    require(isinstance(image, list) and len(image) == 1 and image[0]['Id'] == package['image_id']
            and image[0]['Config']['User'] == '10001:10001'
            and image[0]['Config']['Entrypoint'] == ['/usr/local/bin/meguri-sync-server']
            and all(build['details']['package'][k] == package[k] for k in
                    ['image_id', 'inputs_sha256', 'runtime_inventory_sha256']), 'immutable package')
    require(inputs['source_commit'] == server['source_commit'] and inputs['profile'] == 'release'
            and inputs['features'] == [] and inputs['source_archive_sha256'] == server['source_sha256']
            and inventory['usr/local/bin/meguri-sync-server']['sha256'] == server['executable_sha256'], 'package source')
    read(inputs['source_archive_sha256'])
    locks = [i for i in inputs['inputs'] if i['path'] == 'server/Cargo.lock']
    require(len(locks) == 1 and locks[0]['sha256'] == server['lock_sha256'], 'package lock')
    shipped_assets = [v for k, v in inventory.items() if k.endswith('/assets.json')]
    require(len(shipped_assets) == 1 and shipped_assets[0]['sha256'] == digest(assets), 'shipped assets')
    binding = load(peer['execution_binding_sha256'])
    fields = ('format endpoint server run_id container_id labels observed_executable_sha256 package '
              'server_process container_inspection_before_sha256')
    exact(binding, fields + (' container_inspection_after_sha256' if final else ''))
    require(binding['format'] == 'meguri-sync-candidate-execution-v1' and binding['endpoint'] == 'http://127.0.0.1:8080/'
            and binding['server'] == server and binding['package'] == package
            and binding['run_id'] == invocation['run_id']
            and binding['observed_executable_sha256'] == server['executable_sha256']
            and binding['labels']['meguri.mg.run-id'] == binding['run_id']
            and binding['labels']['meguri.sync.owner'] == invocation['owner_nonce'], 'execution identity')
    hash_value(binding['container_id'])
    exact(binding['server_process'], 'pid executable_sha256')
    process = binding['server_process']
    require(type(process['pid']) is int and 1 <= process['pid'] <= 2147483647
            and process['executable_sha256'] == server['executable_sha256'], 'process identity')
    inspections = [load(binding[k]) for k in ['container_inspection_before_sha256'] +
                   (['container_inspection_after_sha256'] if final else [])]
    for inspection in inspections:
        require(isinstance(inspection, list) and len(inspection) == 1, 'inspection count')
        c = inspection[0]
        require(c['Id'] == binding['container_id'] and c['Image'] == package['image_id']
                and c['Config']['User'] == '10001:10001' and c['Config']['Labels'] == binding['labels']
                and c['Config']['Entrypoint'] == ['/usr/local/bin/meguri-sync-server']
                and c['Config']['Cmd'] == invocation['server_arguments'] and c['Args'] == invocation['server_arguments']
                and c['Path'] == '/usr/local/bin/meguri-sync-server' and c['HostConfig']['ReadonlyRootfs'] is True
                and isinstance(c['Created'], str) and c['Created'] and c['State']['Running'] is True
                and isinstance(c['State']['StartedAt'], str) and c['State']['StartedAt'], 'owned inspection')
        mounts = [m for m in c['Mounts'] if m['Destination'] == '/data']
        require(len(mounts) == 1 and mounts[0]['Type'] == 'volume'
                and mounts[0]['Name'] == invocation['data_volume'] and mounts[0]['RW'] is True, 'owned data mount')
    require(all(i[0]['Created'] == inspections[0][0]['Created'] and
                i[0]['State']['StartedAt'] == inspections[0][0]['State']['StartedAt']
                for i in inspections), 'same container process lifetime')
    return binding


def context(inputs):
    blobs = {name: regular(inputs / name) for name in
             ['action.json', 'client-build.json', 'input-manifest.json', 'preflight.json', 'invocation.json']}
    action, client, manifest, peer, invocation = [json.loads(v) for v in blobs.values()]
    exact(action, 'case action')
    allowed = {
        'authentication': ['authenticate'], 'full-upload': ['upload', 'import_baseline'],
        'full-download': ['download'], 'normal-no-change': ['establish_baseline', 'import_baseline', 'normal_unchanged'],
        'normal-bidirectional': ['establish_baseline', 'import_baseline', 'normal_send', 'normal_receive'],
        'media-bidirectional': ['media_baseline', 'media_receive', 'media_send'],
        'media-no-change': ['media_baseline', 'media_unchanged'], 'manual-uncertain-recovery': ['uncertain_upload']}
    require(action['action'] in allowed.get(action['case'], []), 'retained case action')
    exact(manifest, 'format case action action_sha256 state_inputs_sha256 before_sha256 expected_sha256 '
          'review_sha256 client_build_sha256 preflight_peer_sha256')
    require(manifest['format'] == 'meguri-retained-input-v1' and
            all(manifest[k] == action[k] for k in ['case', 'action']), 'input identity')
    for field, name in [('action_sha256', 'action.json'), ('client_build_sha256', 'client-build.json'),
                        ('preflight_peer_sha256', 'preflight.json')]:
        require(manifest[field] == digest(blobs[name]), 'input bytes')
    read = artifact_reader(inputs / 'artifacts')
    for key, value in manifest.items():
        if key.endswith('_sha256'):
            read(value)
    require(client['format'] == 'meguri-retained-build-v1', 'client build')
    for field in ['source_sha256', 'core_lock_sha256', 'peer_lock_sha256', 'resolved_graph_sha256']:
        read(client[field])
    exact(invocation, 'format run_id root server_arguments data_volume owner_nonce case action actor '
          'client_build_sha256 input_manifest_sha256 preflight_peer_sha256')
    require(invocation['format'] == 'meguri-retained-invocation-v1' and invocation['actor'] == 'client_a'
            and all(invocation[k] == action[k] for k in ['case', 'action'])
            and isinstance(invocation['run_id'], str) and invocation['run_id']
            and isinstance(invocation['root'], str) and invocation['root'].startswith('/')
            and isinstance(invocation['data_volume'], str) and invocation['data_volume'], 'invocation identity')
    for field, name in [('client_build_sha256', 'client-build.json'), ('input_manifest_sha256', 'input-manifest.json'),
                        ('preflight_peer_sha256', 'preflight.json')]:
        require(invocation[field] == digest(blobs[name]), 'invocation bytes')
    nonce, args = invocation['owner_nonce'], invocation['server_arguments']
    require(isinstance(nonce, str) and len(nonce) == 32 and all(c in '0123456789abcdef' for c in nonce), 'owner nonce')
    require(isinstance(args, list) and len(args) == 6 and all(isinstance(a, str) for a in args)
            and args[:2] == ['serve', '/data/owner'] and args[2].startswith('https://')
            and args[3:5] == ['0.0.0.0:8080', '--trusted-peer'], 'serve arguments')
    ipaddress.ip_address(args[5])
    package_identity(peer, read, invocation, False)
    return blobs, action, client, manifest, peer, invocation


def same_execution(preflight, peer, before, after):
    common = dict(peer, kind='final_package_preflight', execution_binding_sha256=preflight['execution_binding_sha256'])
    require(peer['kind'] == 'final_package' and 'container_inspection_after_sha256' in after
            and 'container_inspection_after_sha256' not in before
            and common == preflight and {k: v for k, v in after.items()
            if k != 'container_inspection_after_sha256'} == before, 'same preflight final execution')


def frozen_file(path, data):
    with path.open('xb') as file:
        file.write(data)
        file.flush()
        os.fsync(file.fileno())
    path.chmod(0o444)


def retain_artifacts(destination, blobs):
    destination.mkdir()
    for sha, data in sorted(blobs.items()):
        require(digest(data) == hash_value(sha), 'retained hash')
        frozen_file(destination / sha, data)


def capture(raw, inputs, out):
    # Called and its printed digest retained by MG BEFORE actual after measurement.
    blobs, action, client, manifest, peer, invocation = context(inputs)
    allowed = {'action-result.json', 'worker-error.json', 'state-inputs.json', 'snapshot.json'}
    names = {p.name for p in raw.iterdir()}
    require(names <= allowed and names & {'action-result.json', 'worker-error.json'}, 'worker output files')
    data = {name: regular(raw / name) for name in names}
    if 'action-result.json' in data:
        result = json.loads(data['action-result.json'])
        exact(result, 'format case action actor client_build binding action_sha256 recipe_sha256 selection_sha256 '
              'state_inputs_sha256 started_ms finished_ms observed case_credit')
        require(result['format'] == 'meguri-retained-action-v1' and result['case_credit'] is False
                and result['actor'] == 'client_a' and all(result[k] == action[k] for k in ['case', 'action'])
                and result['client_build'] == client and result['action_sha256'] == digest(blobs['action.json'])
                and result['recipe_sha256'] == client['recipe_sha256'] and result['selection_sha256'] == client['selection_sha256']
                and result['binding'] == dict(preflight=peer, input=manifest, invocation=invocation)
                and result['state_inputs_sha256'] == digest(data['state-inputs.json']), 'raw action identity')
    if 'worker-error.json' in data:
        error = json.loads(data['worker-error.json'])
        require(error == dict(format='meguri-retained-error-v1', operation_complete=False, case_credit=False,
                              reason='input_or_public_api_failure'), 'raw error identity')
    retained = {digest(b): b for b in list(blobs.values()) + list(data.values())}
    for path in (inputs / 'artifacts').iterdir():
        artifact = regular(path)
        require(digest(artifact) == hash_value(path.name), 'input artifact')
        retained[path.name] = artifact
    admission = dict(format='meguri-retained-capture-v1', case=action['case'], action=action['action'], actor='client_a',
                     invocation_sha256=digest(blobs['invocation.json']), preflight_peer_sha256=digest(blobs['preflight.json']),
                     inputs={name: digest(b) for name, b in sorted(blobs.items())},
                     raw={name: digest(b) for name, b in sorted(data.items())}, case_credit=False)
    out.mkdir()
    retain_artifacts(out / 'artifacts', retained)
    frozen_file(out / 'capture.json', encoded(admission))
    return digest(encoded(admission))


def finalize(captured, final_inputs, out, admission_sha):
    admission_bytes = regular(captured / 'capture.json')
    require(digest(admission_bytes) == hash_value(admission_sha), 'capture checkpoint')
    admission = json.loads(admission_bytes)
    exact(admission, 'format case action actor invocation_sha256 preflight_peer_sha256 inputs raw case_credit')
    require(admission['format'] == 'meguri-retained-capture-v1' and admission['case_credit'] is False, 'capture kind')
    read = artifact_reader(captured / 'artifacts', final_inputs / 'artifacts')
    exact(admission['inputs'], 'action.json client-build.json input-manifest.json preflight.json invocation.json')
    original = {name: json.loads(read(sha)) for name, sha in admission['inputs'].items()}
    require(admission['invocation_sha256'] == admission['inputs']['invocation.json'] and
            admission['preflight_peer_sha256'] == admission['inputs']['preflight.json'], 'capture input hashes')
    invocation, preflight = original['invocation.json'], original['preflight.json']
    require(regular(final_inputs / 'invocation.json') == read(admission['invocation_sha256'])
            and all(admission[k] == invocation[k] for k in ['case', 'action', 'actor']), 'unchanged invocation')
    raw = {name: read(sha) for name, sha in admission['raw'].items()}
    require(set(raw) <= {'action-result.json', 'worker-error.json', 'state-inputs.json', 'snapshot.json'}
            and set(raw) & {'action-result.json', 'worker-error.json'}, 'raw files')
    final_bytes = regular(final_inputs / 'peer.json')
    peer = json.loads(final_bytes)
    before = package_identity(preflight, read, invocation, False)
    after = package_identity(peer, read, invocation, True)
    same_execution(preflight, peer, before, after)
    # Never modify the raw result or re-evaluate its outcome, even when false.
    attachment = dict(format='meguri-retained-final-binding-v1', capture_sha256=admission_sha,
                      case=admission['case'], action=admission['action'], actor=admission['actor'],
                      inputs=admission['inputs'], raw=admission['raw'], final_peer_sha256=digest(final_bytes),
                      execution_binding_sha256=peer['execution_binding_sha256'], case_credit=False)
    retained = {admission_sha: admission_bytes, digest(final_bytes): final_bytes}
    for directory in [captured / 'artifacts', final_inputs / 'artifacts']:
        for path in directory.iterdir():
            data = regular(path)
            require(digest(data) == hash_value(path.name), 'artifact changed')
            retained[path.name] = data
    out.mkdir()
    retain_artifacts(out / 'artifacts', retained)
    (out / 'raw').mkdir()
    for name, data in raw.items():
        frozen_file(out / 'raw' / name, data)
        require(digest(regular(out / 'raw' / name)) == admission['raw'][name], 'raw output changed')
    frozen_file(out / 'final-binding.json', encoded(attachment))
    return digest(encoded(attachment))


def selftest():
    # These test the audit policy, not a candidate result.
    p = dict(name='existing', version='1', source='registry+fixed', checksum='a')
    core = dict(package=[p])
    peer = dict(package=[p, dict(name='meguri-retained-sync-peer', version='0.0.0')])
    audit_lock(core, peer)
    for field, value in [('version', '2'), ('checksum', 'b'), ('source', 'registry+other')]:
        changed = dict(p, **{field: value})
        try:
            audit_lock(core, dict(package=[changed, peer['package'][1]]))
        except ValueError:
            pass
        else:
            raise AssertionError('lock identity substitution accepted')
    check()


if __name__ == '__main__':
    if sys.argv[1:] == ['check']:
        selftest()
    elif sys.argv[1:] == ['verify-lock']:
        verify_lock()
    elif len(sys.argv) == 5 and sys.argv[1] == 'capture':
        print(capture(*map(pathlib.Path, sys.argv[2:])))
    elif len(sys.argv) == 6 and sys.argv[1] == 'finalize':
        print(finalize(*map(pathlib.Path, sys.argv[2:5]), sys.argv[5]))
    elif len(sys.argv) == 3 and sys.argv[1] == 'source':
        pathlib.Path(sys.argv[2]).write_bytes(encoded(source_inputs()))
    elif len(sys.argv) == 6 and sys.argv[1] == 'freeze':
        freeze(*map(pathlib.Path, sys.argv[2:]))
    else:
        raise SystemExit('fixed retained support operation required')
