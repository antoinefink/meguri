#!/usr/bin/env python3
"""Test-only retained build and finite owned runtime; helpers never grant case credit."""
import hashlib
import json
import pathlib
import os
import ipaddress
import subprocess
import sys
import tomllib
import copy
import re
import stat
import time
import importlib.util

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
        str(SELECTION.relative_to(ROOT)), 'tools/mg.d/sync-retained.sh', 'tools/mg.d/meguri-sync.sh']).split(b'\0')
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


# Closed runtime vocabulary. No caller-provided action, URL, command or profile.
CASES = ('authentication', 'full-upload', 'full-download', 'normal-no-change',
         'normal-bidirectional', 'media-bidirectional', 'media-no-change', 'manual-uncertain-recovery')
COMPANION = {
    'prepare_rich': CASES[1:2] + CASES[3:5], 'prepare_right': ('full-download',),
    'upload_baseline': CASES[1:5], 'settle_normal': CASES[3:5],
    'receive_forward': ('normal-bidirectional',), 'prepare_reverse': ('normal-bidirectional',),
    'send_reverse': ('normal-bidirectional',), 'baseline_media': ('media-bidirectional',),
    'prepare_forward_media': ('media-bidirectional',), 'send_forward_media': ('media-bidirectional',),
    'receive_reverse_media': ('media-bidirectional',), 'witness_upload': ('full-upload',),
    'witness_download': ('full-download',), 'witness_committed': ('manual-uncertain-recovery',)}
OFFLINE_COMPANION = {'prepare_rich', 'prepare_right', 'prepare_reverse', 'prepare_forward_media'}
COLUMNS = dict(col='id crt mod scm ver dty usn ls conf models decks dconf tags',
    notes='id guid mid mod usn tags flds sfld csum flags data',
    cards='id nid did ord mod usn type queue due ivl factor reps lapses left odue odid flags data',
    revlog='id cid usn ease ivl lastIvl factor time type', graves='oid type usn',
    deck_config='id name mtime_secs usn config', config='KEY usn mtime_secs val',
    tags='tag usn collapsed config', fields='ntid ord name config',
    templates='ntid ord name mtime_secs usn config', notetypes='id name mtime_secs usn config',
    decks='id name mtime_secs usn common kind')
STAMPS = {name: set(('mod scm usn ls' if name == 'col' else
    'mod usn' if name in ('notes', 'cards') else '' if name == 'fields' else
    'usn' if name in ('revlog', 'graves', 'tags') else 'mtime_secs usn').split()) for name in COLUMNS}
METHODS = ('hostKey meta start applyGraves applyChanges chunk applyChunk sanityCheck2 finish abort '
           'upload download begin mediaChanges uploadChanges downloadFiles mediaSanity').split()


def load_json(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON key')
            result[key] = value
        return result
    return json.loads(regular(path), object_pairs_hook=unique)


def cas_put(directory, data):
    directory.mkdir(parents=True, exist_ok=True)
    sha = digest(data)
    path = directory / sha
    if path.exists():
        require(regular(path) == data, 'CAS changed')
    else:
        frozen_file(path, data)
    return sha


def copy_cas(source, destination):
    for path in sorted(source.iterdir()):
        data = regular(path)
        require(digest(data) == hash_value(path.name), 'CAS closure')
        cas_put(destination, data)


def tree_inventory(root, store):
    # Closed actual files, including sidecars. No inferred or predicted hashes.
    require(root.is_dir() and not root.is_symlink(), 'owned directory')
    files = []
    for path in sorted(root.rglob('*')):
        info = path.lstat()
        require(not stat.S_ISLNK(info.st_mode), 'symlink in closed inventory')
        if stat.S_ISDIR(info.st_mode):
            continue
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == os.getuid(), 'owned single file')
        data = regular(path)
        files.append(dict(path=path.relative_to(root).as_posix(), bytes=len(data), sha256=cas_put(store, data)))
    return dict(files=files)


def snapshot(value):
    exact(value, 'format tables media_sha256')
    require(value['format'] == 'meguri-retained-snapshot-v1' and set(value['tables']) == set(COLUMNS), 'snapshot tables')
    for name, columns in COLUMNS.items():
        table = value['tables'][name]
        exact(table, 'columns rows')
        require(table['columns'] == columns.split() and isinstance(table['rows'], list), 'public column order')
        for row in table['rows']:
            require(isinstance(row, list) and len(row) == len(table['columns']), 'snapshot row')
            for cell in row:
                require(isinstance(cell, dict) and len(cell) == 1, 'cell shape')
                kind, val = next(iter(cell.items()))
                require((kind == 'null' and val is True) or
                    (kind == 'integer' and isinstance(val, str) and re.fullmatch(r'-?(0|[1-9][0-9]*)', val)) or
                    (kind == 'real_bits' and isinstance(val, str) and re.fullmatch(r'[0-9a-f]{16}', val)) or
                    (kind in ('text_hex', 'blob_hex') and isinstance(val, str) and re.fullmatch(r'(?:[0-9a-f]{2})*', val)), 'cell kind')
    require(isinstance(value['media_sha256'], dict), 'whole media map')
    for name, sha in value['media_sha256'].items():
        require(isinstance(name, str) and name and '/' not in name and '\\' not in name, 'media name')
        hash_value(sha)
    return value


def projection(value):
    result = copy.deepcopy(snapshot(value))
    for name, table in result['tables'].items():
        for row in table['rows']:
            for index, column in enumerate(table['columns']):
                if column in STAMPS[name]:
                    row[index] = {'protocol_stamp': True}
    return result


def with_media(authority, media):
    result = copy.deepcopy(snapshot(authority))
    result['media_sha256'] = dict(media)
    return snapshot(result)


def media_recipe(part):
    return {name: digest(body.encode()) for name, body in
            load_json(RECIPES)['media_utf8_exact'][part].items()}


def declared_edit(baseline, authority, direction, limited=False):
    """Retain original cells except declared edits and actual scheduler output.

    Settings blobs remain byte-exact, with changes confined to their declared
    authored row/column. Independent review of those actual settings and the
    preparation rules remains required; they are never normalized or decoded
    through a second SDK implementation here.
    """
    before, after = projection(baseline), projection(authority)
    require(direction in ('forward', 'reverse'), 'declared direction')
    recipes = load_json(RECIPES)
    changes = recipes['rich_' + direction + '_changes']
    originals = {row['key']: row['fields'] for row in recipes['rich_initial']['notes']}
    def text(cell):
        exact(cell, 'text_hex')
        return bytes.fromhex(cell['text_hex']).decode()
    def rows(value, name):
        table = value['tables'][name]
        width = 2 if name in ('graves', 'fields', 'templates') else 1
        return {json.dumps(row[:width], sort_keys=True): row for row in table['rows']}
    notes_before, notes_after = rows(before, 'notes'), rows(after, 'notes')
    delete_prompt = originals[changes['delete_note_key']][0]
    keep_prompt = originals['keep'][0]
    delete_ids, keep_ids = [], []
    for key, row in notes_before.items():
        prompt = text(row[6]).split('\x1f')[0]
        if prompt == delete_prompt:
            delete_ids.append(key)
        if prompt == keep_prompt:
            keep_ids.append(key)
    require(len(delete_ids) == len(keep_ids) == 1, 'original declared notes')
    delete_key, keep_key = delete_ids[0], keep_ids[0]
    require(set(notes_after) == set(notes_before) - {delete_key}, 'only declared note deletion')
    for key, row in notes_before.items():
        if key == delete_key:
            continue
        expected = copy.deepcopy(row)
        if key == keep_key:
            fields = text(row[6]).split('\x1f')
            fields[1 if direction == 'forward' else 2] = changes['keep_answer' if direction == 'forward' else 'keep_context']
            expected[6] = dict(text_hex='\x1f'.join(fields).encode().hex())
        require(notes_after[key] == expected, 'only declared note edit')
    deleted_note = notes_before[delete_key][0]
    kept_note = notes_before[keep_key][0]
    cards_before, cards_after = rows(before, 'cards'), rows(after, 'cards')
    deleted_cards = {key for key, row in cards_before.items() if row[1] == deleted_note}
    reviewed_cards = {key for key, row in cards_before.items() if row[1] == kept_note}
    require(len(reviewed_cards) == 1 and set(cards_after) == set(cards_before) - deleted_cards, 'declared card deletion')
    for key, row in cards_after.items():
        old = cards_before[key]
        if key in reviewed_cards:
            # Scheduling and opaque scheduler data come from the actual review,
            # never predicted. Ownership/deck/ordinal/flags stay original.
            require(all(row[i] == old[i] for i in (0, 1, 2, 3, 16)), 'review card identity')
            require(int(row[11]['integer']) == int(old[11]['integer']) + 1, 'one actual review')
        else:
            require(row == old, 'unrelated scheduling changed')
    logs_before, logs_after = rows(before, 'revlog'), rows(after, 'revlog')
    require(set(logs_before) < set(logs_after) and len(logs_after) == len(logs_before) + 1 and
            all(logs_after[k] == v for k, v in logs_before.items()), 'original history plus one review')
    review = logs_after[next(iter(set(logs_after) - set(logs_before)))]
    kept_card = cards_before[next(iter(reviewed_cards))][0]
    require(review[1] == kept_card and review[3] == dict(integer='3' if direction == 'forward' else '2'), 'declared review identity and grade')
    grave_before, grave_after = rows(before, 'graves'), rows(after, 'graves')
    require(set(grave_before) <= set(grave_after) and all(grave_after[k] == v for k, v in grave_before.items()), 'original deletion history')
    deleted_oids = [deleted_note] + [cards_before[k][0] for k in deleted_cards]
    require(len(set(grave_after) - set(grave_before)) == len(deleted_oids) and
            all(row[0] in deleted_oids for k, row in grave_after.items() if k not in grave_before), 'declared tombstones')
    for name in ('col', 'fields', 'tags'):
        require(before['tables'][name] == after['tables'][name], 'unrelated original table changed')
    for name, mutable, selected_name in (
        ('notetypes', {'config'}, recipes['rich_initial']['note_type']),
        ('templates', {'config'}, recipes['rich_initial']['template']['name']),
        ('decks', {'common', 'kind'}, recipes['rich_initial']['deck']['name'].replace('::', '\x1f')),
        ('deck_config', {'config'}, recipes['rich_initial']['preset']['name']),
        ('config', {'val'}, 'meguri_sync_fixture')):
        old_rows, new_rows = rows(before, name), rows(after, name)
        require(set(old_rows) == set(new_rows), 'authored object identities')
        selected_count, changed = 0, False
        columns = before['tables'][name]['columns']
        name_index = columns.index('KEY' if name == 'config' else 'name')
        authored_ntids = [row[0] for row in before['tables']['notetypes']['rows']
            if text(row[1]) == recipes['rich_initial']['note_type']]
        require(len(authored_ntids) == 1, 'original authored note type')
        for key, old in old_rows.items():
            new = new_rows[key]
            selected = text(old[name_index]) == selected_name
            if name == 'templates':
                selected = selected and old[0] == authored_ntids[0]
            if selected:
                selected_count += 1
            for index, column in enumerate(columns):
                if column in mutable and selected:
                    changed = changed or new[index] != old[index]
                else:
                    require(new[index] == old[index], 'undeclared authored cell changed')
        require(selected_count == 1, 'original authored object')
        require(changed or (limited and name in ('notetypes', 'templates')), 'required declared object edit')
        if name == 'config':
            row = next(row for row in new_rows.values() if text(row[name_index]) == selected_name)
            cell = row[columns.index('val')]
            value = next(iter(cell.values()))
            require(set(cell) in ({'text_hex'}, {'blob_hex'}) and json.loads(bytes.fromhex(value)) == changes['config'][selected_name], 'declared config bytes')
    require(authority['media_sha256'] == baseline['media_sha256'], 'ordinary edit changed media authority')
    return authority


def wire_check(value, action, actor, invocation_sha):
    exact(value, 'format case action actor invocation_sha256 requests totals fault case_credit')
    require(value['format'] == 'meguri-retained-wire-v1' and value['case_credit'] is False and
            value['case'] == action['case'] and value['action'] == action['action'] and
            value['actor'] == actor and value['invocation_sha256'] == invocation_sha, 'wire identity')
    totals = dict.fromkeys(('request_count upstream_completed_count delivered_completed_count request_body_bytes '
                          'upstream_body_bytes delivered_body_bytes dropped_body_bytes').split(), 0)
    require(isinstance(value['requests'], list), 'wire requests')
    drops, forwarded, possibly_completed = 0, 0, 0
    for sequence, row in enumerate(value['requests'], 1):
        exact(row, 'sequence method path request_body_bytes upstream_status upstream_body_bytes delivered_body_bytes disposition')
        require(type(row['sequence']) is int and row['sequence'] == sequence and row['method'] == 'POST' and
                row['path'] in (['/sync/' + method for method in METHODS[:12]] +
                                ['/msync/' + method for method in METHODS[12:]]), 'public wire tokens')
        for key in ('request_body_bytes', 'upstream_body_bytes', 'delivered_body_bytes'):
            require(type(row[key]) is int and row[key] >= 0, 'actual wire byte count')
            totals[key] += row[key]
        status, disposition = row['upstream_status'], row['disposition']
        require(status is None or (type(status) is int and 100 <= status <= 599), 'upstream status')
        require(disposition in ('forwarded', 'upload_reply_dropped', 'incomplete') and
                row['delivered_body_bytes'] <= row['upstream_body_bytes'], 'wire disposition')
        totals['request_count'] += 1
        totals['dropped_body_bytes'] += row['upstream_body_bytes'] - row['delivered_body_bytes']
        if disposition != 'incomplete':
            require(status is not None, 'missing actual completion')
        if disposition == 'forwarded':
            require(row['delivered_body_bytes'] == row['upstream_body_bytes'], 'forwarded body incomplete')
            forwarded += 1
        if disposition == 'incomplete' and status is not None:
            possibly_completed += 1
        if disposition == 'upload_reply_dropped':
            require(row['path'] == '/sync/upload' and status == 200, 'actual successful upload acknowledgement')
            drops += 1
    if actor == 'fresh_client':
        require(not any(row['path'] in ('/sync/upload', '/sync/applyGraves', '/sync/applyChanges',
            '/sync/applyChunk', '/msync/uploadChanges') for row in value['requests']),
            'fresh witness attempted content send')
    exact(value['totals'], ' '.join(totals))
    measured = value['totals']
    require(all(type(v) is int and v >= 0 for v in measured.values()), 'wire totals')
    # Incomplete delivery does not reveal whether upstream completed. Preserve
    # the observer's independent counts; body length cannot supply that fact.
    require(all(measured[k] == v for k, v in totals.items() if k not in
                ('upstream_completed_count', 'delivered_completed_count')) and
            forwarded + drops <= measured['upstream_completed_count'] <= forwarded + drops + possibly_completed and
            measured['delivered_completed_count'] == forwarded, 'wire totals')
    fault = value['fault']
    exact(fault, 'actor armed accepted_upload_ack reply_dropped upload_request_count upload_replay_count')
    require(fault['actor'] == 'fault_driver' and all(type(fault[k]) is bool for k in
            ['armed', 'accepted_upload_ack', 'reply_dropped']), 'fault flags')
    uploads = sum(row['path'] == '/sync/upload' for row in value['requests'])
    require(type(fault['upload_request_count']) is int and type(fault['upload_replay_count']) is int and
            fault['upload_request_count'] == uploads and fault['upload_replay_count'] == max(0, uploads - 1), 'upload counts')
    uncertain = action == dict(case='manual-uncertain-recovery', action='uncertain_upload')
    if uncertain:
        require(fault['armed'] is True and fault['accepted_upload_ack'] == fault['reply_dropped'] == (drops == 1), 'uncertain fault binding')
    else:
        require(not any(fault[k] for k in ['armed', 'accepted_upload_ack', 'reply_dropped']) and drops == 0, 'ordinary pass through')
    return value


def wire_complete(value):
    return value['totals']['request_count'] > 0 and all(
        row['disposition'] != 'incomplete' for row in value['requests'])


def runtime_receipt(path):
    path = path.resolve(strict=True)
    receipt = load_json(path)
    current = source_inputs()
    require(path == ROOT / 'build/mg/sync-server-retained' / receipt['run_id'] / 'result.json', 'canonical peer receipt')
    require(receipt['job'] == 'sync-server-retained' and receipt['status'] == 'pass' and
            receipt['dry_run'] is False and receipt['exit_code'] == 0 and receipt['failures'] == [] and
            receipt['args'] == ['build'] and receipt['root'] == str(ROOT) and
            receipt['commit'] == dict(sha=current['source_commit'], dirty=False), 'current clean peer result')
    client_dir = path.parent / 'client'
    client = load_json(client_dir / 'client-build.json')
    require(client == receipt['details']['retained_client'] and client['source_dirty'] is False and
            client['profile'] == 'dev' and client['source_commit'] == current['source_commit'], 'compiled peer identity')
    read = artifact_reader(client_dir / 'artifacts')
    require(read(client['source_sha256']) == encoded(current) and
            read(client['core_lock_sha256']) == regular(ROOT / 'core/Cargo.lock') and
            read(client['peer_lock_sha256']) == regular(PEER / 'Cargo.lock') and
            digest(regular(client_dir / 'meguri-retained-sync-peer')) == client['executable_sha256'] and
            client['recipe_sha256'] == digest(regular(RECIPES)) and
            client['selection_sha256'] == digest(regular(SELECTION)), 'exact current peer inputs')
    graph = json.loads(read(client['resolved_graph_sha256']))
    require(graph['compiled_features'] == client['features'] and
            sorted({p['name'] for p in client['features'] if 'test-hooks' in p['features']}) ==
            ['meguri-engine', 'meguri-model', 'meguri-storage', 'meguri-sync'] and
            not any(p['name'] == 'meguri-testkit' or 'ankiweb' in p['features'] for p in client['features']), 'peer feature graph')
    verify_lock()
    return client_dir


class Containers:
    """Owned finite children only. No logs, arbitrary endpoints, or server-data mounts."""
    def __init__(self, outer):
        self.outer = outer
        self.config = load_json(outer / 'runtime.json')
        self.run_id = load_json(outer / 'candidate-invocation.json')['run_id']
        self.ledger = outer / 'runtime-owners.jsonl'

    def command(self, *args, output=None, input_bytes=None):
        completed = subprocess.run(['docker', *map(str, args)], input=input_bytes,
            stdout=subprocess.PIPE if output is not None else subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, timeout=180)
        if output is not None:
            frozen_file(output, completed.stdout)
        require(completed.returncode == 0, 'owned docker command failed')
        return completed.stdout if output is not None else b''

    def owners(self):
        if not self.ledger.exists():
            return []
        data = regular(self.ledger)
        # An interrupted final write precedes create, so only that unfinished
        # line can be ignored. Earlier complete creation records still clean.
        lines = data.splitlines(keepends=True)
        if lines and not lines[-1].endswith(b'\n'):
            lines.pop()
        return [json.loads(line) for line in lines]

    def identity(self, item):
        completed = subprocess.run(['docker', 'container', 'ls', '-aq', '--no-trunc', '--filter',
            'name=^/' + item['name'] + '$'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=30)
        require(completed.returncode == 0, 'child ownership observation')
        cid = completed.stdout.decode().strip()
        if not cid:
            return None
        require(re.fullmatch('[0-9a-f]{64}', cid), 'unique child CID')
        inspected = subprocess.run(['docker', 'inspect', cid], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=30)
        require(inspected.returncode == 0, 'child inspection')
        rows = json.loads(inspected.stdout)
        require(len(rows) == 1 and rows[0]['Id'] == cid and rows[0]['Name'] == '/' + item['name'] and
                rows[0]['Config']['Labels']['meguri.mg.run-id'] == self.run_id and
                rows[0]['Config']['Labels']['meguri.sync.owner'] == item['owner'], 'owned child identity')
        return rows[0]

    def remove(self, item):
        row = self.identity(item)
        if row:
            self.command('rm', '-f', row['Id'])
            require(self.identity(item) is None, 'child removal observation')

    def create(self, directory, entrypoint, command, network='none', mounts=(), pid=None, user=None, stdin=False, volumes=()):
        owner = os.urandom(16).hex()
        item = dict(name='meguri-retained-' + self.run_id + '-' + owner, owner=owner)
        # Durable pre-creation ownership record lets the outer MG cancellation
        # hook clean only these children even if create returned ambiguously.
        with self.ledger.open('ab') as ledger:
            ledger.write(json.dumps(item).encode() + b'\n')
            ledger.flush()
            os.fsync(ledger.fileno())
        args = ['create', '--name', item['name'], '--network', network, '--read-only', '--cap-drop', 'ALL',
            '--security-opt', 'no-new-privileges', '--log-driver', 'none', '--user', user or f'{os.getuid()}:{os.getgid()}',
            '--memory', '2g', '--pids-limit', '128', '--label', 'meguri.mg.run-id=' + self.run_id,
            '--label', 'meguri.sync.owner=' + owner, '-e', 'MG_RUN_ID=' + self.run_id,
            '-e', 'TMPDIR=/scratch', '-e', 'TZ=UTC', '-e', 'PYTHONDONTWRITEBYTECODE=1',
            '--tmpfs', '/scratch:exec,mode=1777,size=1g', '--tmpfs', '/tmp:mode=1777,size=64m']
        if stdin:
            args += ['-i']
        if pid:
            args += ['--pid', 'container:' + pid]
        for volume, destination, readonly in volumes:
            args += ['--mount', 'type=volume,src=' + volume + ',dst=' + destination + (',readonly' if readonly else '')]
        for source, destination, readonly in mounts:
            args += ['--mount', 'type=bind,src=' + str(source) + ',dst=' + destination + (',readonly' if readonly else '')]
        args += ['--entrypoint', entrypoint, self.config['image'], *command]
        self.command(*args, output=directory / 'created.txt')
        row = self.identity(item)
        require(row is not None and row['Image'] == self.config['image'] and
                row['HostConfig']['NetworkMode'] == network and row['HostConfig']['ReadonlyRootfs'] is True and
                row['Config']['Entrypoint'] == [entrypoint] and (row['Config']['Cmd'] or []) == list(command), 'actual child command')
        frozen_file(directory / 'container-created.json', encoded([row]))
        return item

    def start(self, item):
        row = self.identity(item)
        require(row is not None, 'owned child absent')
        self.command('start', row['Id'])

    def wait(self, item, directory):
        row = self.identity(item)
        require(row is not None, 'owned child absent')
        self.command('wait', row['Id'], output=directory / 'exit.txt')
        row = self.identity(item)
        require(row is not None and row['State']['Running'] is False, 'child not joined')
        frozen_file(directory / 'container-closed.json', encoded([row]))
        return int(regular(directory / 'exit.txt').strip())

    def run(self, directory, entrypoint, command, stdout=None, input_bytes=None, **kwargs):
        item = self.create(directory, entrypoint, command, stdin=input_bytes is not None, **kwargs)
        try:
            if stdout is not None or input_bytes is not None:
                row = self.identity(item)
                args = ['start', '-a'] + (['-i'] if input_bytes is not None else []) + [row['Id']]
                # A nonzero action exit is an observation, not a command retry.
                completed = subprocess.run(['docker', *args], input=input_bytes,
                    stdout=subprocess.PIPE if stdout is not None else subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL, timeout=180)
                if stdout is not None:
                    frozen_file(stdout, completed.stdout)
            else:
                self.start(item)
            return self.wait(item, directory)
        finally:
            self.remove(item)

    def cleanup(self):
        failed = False
        for item in reversed(self.owners()):
            try:
                self.remove(item)
            except (OSError, ValueError, KeyError, subprocess.SubprocessError):
                failed = True
        require(not failed, 'checked child cleanup failed')


def runtime_prepare(outer, case):
    require(case in CASES, 'fixed case')
    containers = Containers(outer)
    peer = runtime_receipt(pathlib.Path(containers.config['peer_receipt']))
    directory = outer / 'preparations' / case
    directory.mkdir()
    (directory / 'state').mkdir()
    return offline_worker(containers, peer, directory, case, 'prepare_initial', directory / 'state')


def offline_worker(containers, peer, directory, case, action, state):
    require(action in ('prepare_initial', 'prepare_snapshot', 'prepare_forward', 'prepare_reverse_media'), 'offline worker action')
    inputs, output = directory / 'input', directory / 'raw'
    inputs.mkdir()
    output.mkdir()
    copy_cas(peer / 'artifacts', inputs / 'artifacts')
    frozen_file(inputs / 'client-build.json', regular(peer / 'client-build.json'))
    frozen_file(inputs / 'action.json', encoded(dict(case=case, action=action)))
    rc, error_kind = None, None
    try:
        rc = containers.run(directory, '/retained-peer', [], mounts=[(inputs, '/in', True),
            (output, '/out', False), (state, '/state', False), (peer / 'meguri-retained-sync-peer', '/retained-peer', True)])
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        error_kind = 'incomplete'
    # Freeze even refused/error preparations and the COMPLETE closed state.
    store = directory / 'artifacts'
    raw_inventory = tree_inventory(output, store)
    state_inventory = tree_inventory(state, store)
    input_inventory = tree_inventory(inputs, store)
    valid, complete = False, False
    try:
        result, snap = preparation_identity(directory, case, action, load_json(peer / 'client-build.json'))
        declared = load_json(output / 'state-inputs.json')
        exact(declared, 'format files')
        require(declared['format'] == 'meguri-retained-state-inputs-v1', 'closed preparation state format')
        actual = []
        for row in state_inventory['files']:
            require(row['path'].startswith(case + '/'), 'case-owned state only')
            actual.append(dict(row, path=row['path'][len(case) + 1:]))
        require(declared['files'] == actual, 'complete actual preparation state inventory')
        valid, complete = True, result['observed']['preparation_complete'] is True
    except (OSError, ValueError, KeyError, TypeError):
        pass
    checkpoint = encoded(dict(case=case, action=action, actor='client_a',
        exit_code=rc, error_kind=error_kind, admission_valid=valid, preparation_complete=complete,
        inputs=input_inventory, raw=raw_inventory, state=state_inventory, case_credit=False))
    frozen_file(directory / 'checkpoint.json', checkpoint)
    frozen_file(directory / 'checkpoint.sha256', (digest(checkpoint) + '\n').encode())
    return 0 if rc == 0 and valid and complete else 1


def preparation_checkpoint(directory):
    data = regular(directory / 'checkpoint.json')
    require(digest(data) == hash_value(regular(directory / 'checkpoint.sha256').decode('ascii').strip()),
            'preparation checkpoint changed')
    checkpoint = json.loads(data)
    read = artifact_reader(directory / 'artifacts')
    files = {}
    for key, subdir in (('inputs', 'input'), ('raw', 'raw'), ('state', 'state')):
        for row in checkpoint[key]['files']:
            relative = pathlib.PurePosixPath(row['path'])
            require(not relative.is_absolute() and relative.parts and '..' not in relative.parts,
                    'preparation inventory path')
            path = directory / subdir / relative
            require(path not in files, 'duplicate preparation file')
            blob = read(row['sha256'])
            require(type(row['bytes']) is int and len(blob) == row['bytes'], 'preparation inventory bytes')
            files[path] = blob
    return data, checkpoint, files


def restore_working_state(initial, case, state):
    # Original preparation authority stays frozen while this working copy evolves.
    _, checkpoint, files = preparation_checkpoint(initial)
    require(checkpoint['case'] == case and checkpoint['action'] == 'prepare_initial', 'initial checkpoint identity')
    state.mkdir()
    for path, data in files.items():
        if path.is_relative_to(initial / 'state'):
            target = state / path.relative_to(initial / 'state')
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)


def saved_preparation(directory, case, action, client):
    data, checkpoint, files = preparation_checkpoint(directory)
    require(checkpoint['case'] == case and checkpoint['action'] == action and checkpoint['actor'] == 'client_a' and
            checkpoint['case_credit'] is False and checkpoint['admission_valid'] is True, 'closed state admission')
    result, snap = preparation_identity(directory, case, action, client, read_file=files.__getitem__)
    return dict(snapshot=snap, state=files[directory / 'raw/state-inputs.json'], preparation=data, result=result)


def preparation_identity(directory, case, action, client, read_file=regular):
    output, inputs = directory / 'raw', directory / 'input'
    result = json.loads(read_file(output / 'action-result.json'))
    exact(result, 'format case action actor client_build binding action_sha256 recipe_sha256 selection_sha256 '
          'state_inputs_sha256 started_ms finished_ms observed case_credit')
    require(result['format'] == 'meguri-retained-action-v1' and result['case'] == case and result['action'] == action and
        result['actor'] == 'client_a' and result['case_credit'] is False and result['client_build'] == client and
        result['binding'] is None and result['action_sha256'] == digest(read_file(inputs / 'action.json')) and
        result['recipe_sha256'] == digest(regular(RECIPES)) and result['selection_sha256'] == digest(regular(SELECTION)) and
        result['state_inputs_sha256'] == digest(read_file(output / 'state-inputs.json')), 'actual preparation identity')
    snap = snapshot(json.loads(read_file(output / 'snapshot.json')))
    require(result['observed']['preparation_only'] is True and
        type(result['observed']['preparation_complete']) is bool and
        result['observed']['snapshot_sha256'] == digest(read_file(output / 'snapshot.json')) and
        result['observed']['snapshot'] == snap, 'actual preparation snapshot')
    return result, snap


def strict_package(peer, store, invocation, final):
    # Reuse the public first-party package validator itself. Its package check
    # accepts neutral invocation fields; only its separate Desktop builders
    # require child phases. No quarantine module is imported or inspected.
    spec = importlib.util.spec_from_file_location('retained_package_primitives', ROOT / 'server/tools/candidate-output.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    trusted = dict(invocation)
    for key in ('source_commit', 'source_sha256', 'lock_sha256', 'executable_sha256'):
        if key not in trusted:
            trusted[key] = peer['server'][key]
    module.final_peer(peer, store, trusted, preflight=not final)


def package_preflight(inputs, outer, invocation, before_data, process):
    """Retained adapter: same package primitives, no Desktop phase fiction."""
    store = inputs / 'artifacts'
    copy_cas(outer / 'package-artifacts', store)
    identity = load_json(outer / 'package-identity.json')
    receipt_data = regular(pathlib.Path(load_json(outer / 'runtime.json')['package_receipt']))
    receipt = json.loads(receipt_data)
    binding = dict(format='meguri-sync-candidate-execution-v1', endpoint='http://127.0.0.1:8080/',
        server=identity['server'], package=identity['package'], run_id=invocation['run_id'],
        container_id=json.loads(before_data)[0]['Id'], labels=json.loads(before_data)[0]['Config']['Labels'],
        observed_executable_sha256=process['executable_sha256'], server_process=process,
        container_inspection_before_sha256=cas_put(store, before_data))
    peer = dict(format='meguri-sync-package-v1', kind='final_package_preflight', server=identity['server'],
        source_dirty=False, profile='release', features=[], package=identity['package'],
        executable_artifact_sha256=identity['server']['executable_sha256'],
        lock_artifact_sha256=identity['server']['lock_sha256'], mg_build_receipt_sha256=cas_put(store, receipt_data),
        build_invocation=dict(job=receipt['job'], run_id=receipt['run_id'], arguments=receipt['args']),
        execution_binding_sha256=cas_put(store, encoded(binding)))
    package_identity(peer, artifact_reader(store), invocation, False)
    strict_package(peer, store, invocation, False)
    frozen_file(inputs / 'preflight.json', encoded(peer))
    return peer


def package_complete(inputs, final_inputs, invocation, after_data, process):
    final_inputs.mkdir()
    copy_cas(inputs / 'artifacts', final_inputs / 'artifacts')
    read = artifact_reader(final_inputs / 'artifacts')
    preflight = load_json(inputs / 'preflight.json')
    before = package_identity(preflight, read, invocation, False)
    require(process == before['server_process'], 'actual server process changed')
    after = dict(before, container_inspection_after_sha256=cas_put(final_inputs / 'artifacts', after_data))
    peer = dict(preflight, kind='final_package', execution_binding_sha256=cas_put(final_inputs / 'artifacts', encoded(after)))
    package_identity(peer, artifact_reader(final_inputs / 'artifacts'), invocation, True)
    same_execution(preflight, peer, before, after)
    strict_package(peer, final_inputs / 'artifacts', invocation, True)
    frozen_file(final_inputs / 'invocation.json', regular(inputs / 'invocation.json'))
    frozen_file(final_inputs / 'peer.json', encoded(peer))
    return peer


def companion_context(inputs):
    action = load_json(inputs / 'action.json')
    exact(action, 'case action')
    require(action['case'] in COMPANION.get(action['action'], ()), 'fixed companion action')
    actor = 'fresh_client' if action['action'].startswith('witness_') else 'client_b'
    invocation, preflight, expected = [load_json(inputs / name) for name in ('invocation.json', 'preflight.json', 'expectations.json')]
    exact(invocation, 'source_commit batch_base source_sha256 lock_sha256 executable_sha256 mg_job arguments run_id author_session '
          'root server_arguments data_volume owner_nonce')
    exact(expected, 'format case action actor recipe_sha256 selection_sha256 review_sha256 baseline_sha256 expected_sha256')
    require(expected['format'] == 'meguri-retained-companion-input-v1' and expected['case'] == action['case'] and
            expected['action'] == action['action'] and expected['actor'] == actor and
            expected['recipe_sha256'] == digest(regular(RECIPES)) and
            expected['selection_sha256'] == digest(regular(SELECTION)), 'companion expectation identity')
    read = artifact_reader(inputs / 'artifacts')
    for key, value in expected.items():
        if key.endswith('_sha256'):
            if value is None:
                require(action['action'] in OFFLINE_COMPANION and key in ('baseline_sha256', 'expected_sha256'), 'online authority missing')
            else:
                read(hash_value(value))
    for key in ('source_commit', 'source_sha256', 'lock_sha256', 'executable_sha256'):
        require(invocation[key] == preflight['server'][key], 'companion server identity')
    package_identity(preflight, read, invocation, False)
    return action, actor, invocation, preflight, expected


def companion_capture(raw, inputs, captured):
    action, actor, invocation, preflight, expected = companion_context(inputs)
    require({p.name for p in raw.iterdir()} <= {'observations.json', 'artifacts'} and
            (raw / 'observations.json').exists() and (raw / 'artifacts').is_dir(), 'companion output files')
    observation = load_json(raw / 'observations.json')
    exact(observation, 'format case action actor invocation_sha256 preflight_peer_sha256 expectations_sha256 client '
          'before_sha256 after_sha256 authority_sha256 wire_sha256 observed case_credit')
    require(observation['format'] == 'meguri-retained-companion-v1' and observation['case_credit'] is False and
            observation['case'] == action['case'] and observation['action'] == action['action'] and observation['actor'] == actor and
            observation['invocation_sha256'] == digest(regular(inputs / 'invocation.json')) and
            observation['preflight_peer_sha256'] == digest(regular(inputs / 'preflight.json')) and
            observation['expectations_sha256'] == digest(regular(inputs / 'expectations.json')), 'companion raw identity')
    client = observation['client']
    exact(client, 'kind version build runtime_sha256')
    require(client['kind'] == 'desktop' and client['version'] == '26.09.4' and client['build'] == 'c06370ff', 'pinned companion client')
    read = artifact_reader(raw / 'artifacts')
    read(hash_value(client['runtime_sha256']))
    for key in ('before_sha256', 'after_sha256', 'authority_sha256'):
        if observation[key] is not None:
            snapshot(json.loads(read(observation[key])))
    for path in (inputs / 'artifacts').iterdir():
        require(read(path.name) == regular(path), 'companion original closure')
    observed = observation['observed']
    exact(observed, 'operation_complete error_kind expected_state_equal')
    require(type(observed['operation_complete']) is bool and observed['error_kind'] in
            (None, 'preparation_refused', 'public_api_failure', 'state_mismatch', 'incomplete') and
            (observed['expected_state_equal'] is None or type(observed['expected_state_equal']) is bool), 'closed companion outcome')
    offline = action['action'] in OFFLINE_COMPANION
    if offline:
        require(observation['wire_sha256'] is None, 'offline wire forbidden')
    elif observation['wire_sha256'] is not None:
        wire_check(json.loads(read(observation['wire_sha256'])), action, actor, observation['invocation_sha256'])
    if observed['operation_complete']:
        require(observed['error_kind'] is None and observation['after_sha256'] is not None and
                observation['authority_sha256'] is not None, 'complete companion state missing')
        if not offline:
            require(observation['wire_sha256'] is not None and observed['expected_state_equal'] is True and
                    observation['authority_sha256'] == expected['expected_sha256'], 'send/receive frozen authority changed')
            after, authority = [json.loads(read(observation[k])) for k in ('after_sha256', 'authority_sha256')]
            if action['action'] in ('baseline_media', 'send_forward_media', 'receive_reverse_media'):
                require(after['media_sha256'] == authority['media_sha256'], 'actual companion media mismatch')
                if observation['before_sha256'] is not None:
                    before = json.loads(read(observation['before_sha256']))
                    require(projection(before)['tables'] == projection(after)['tables'], 'media changed collection')
            else:
                require(projection(after) == projection(authority), 'actual companion state mismatch')
        else:
            require(json.loads(read(observation['authority_sha256'])) == json.loads(read(observation['after_sha256'])), 'offline actual authority')
    if action['action'] == 'prepare_forward_media' and observation['after_sha256'] is not None:
        after = json.loads(read(observation['after_sha256']))
        require(after['media_sha256'] == media_recipe('forward'), 'declared forward media authority')
        if observation['before_sha256'] is not None:
            before = json.loads(read(observation['before_sha256']))
            require(projection(before)['tables'] == projection(after)['tables'], 'offline media changed collection')
    captured.mkdir()
    copy_cas(raw / 'artifacts', captured / 'artifacts')
    for name in ('action.json', 'invocation.json', 'preflight.json', 'expectations.json'):
        cas_put(captured / 'artifacts', regular(inputs / name))
    sha = cas_put(captured / 'artifacts', regular(raw / 'observations.json'))
    admission = dict(format='meguri-retained-companion-capture-v1', case=action['case'], action=action['action'], actor=actor,
        invocation_sha256=observation['invocation_sha256'], preflight_peer_sha256=observation['preflight_peer_sha256'],
        observations_sha256=sha, expectations_sha256=observation['expectations_sha256'],
        wire_sha256=observation['wire_sha256'], case_credit=False)
    frozen_file(captured / 'capture.json', encoded(admission))
    return digest(encoded(admission))


def companion_finalize(captured, inputs, final_inputs, out, checkpoint):
    require(digest(regular(captured / 'capture.json')) == hash_value(checkpoint), 'companion capture checkpoint')
    action, actor, invocation, preflight, expected = companion_context(inputs)
    admission = load_json(captured / 'capture.json')
    require(admission['case'] == action['case'] and admission['action'] == action['action'] and
            admission['actor'] == actor and admission['invocation_sha256'] == digest(regular(inputs / 'invocation.json')) and
            admission['preflight_peer_sha256'] == digest(regular(inputs / 'preflight.json')) and
            admission['expectations_sha256'] == digest(regular(inputs / 'expectations.json')), 'companion capture input mismatch')
    read = artifact_reader(captured / 'artifacts', final_inputs / 'artifacts')
    observation = json.loads(read(admission['observations_sha256']))
    require(observation['wire_sha256'] == admission['wire_sha256'] and
            observation['invocation_sha256'] == admission['invocation_sha256'] and
            observation['expectations_sha256'] == admission['expectations_sha256'], 'companion immutable raw binding')
    peer = load_json(final_inputs / 'peer.json')
    before = package_identity(preflight, read, invocation, False)
    after = package_identity(peer, read, invocation, True)
    same_execution(preflight, peer, before, after)
    out.mkdir()
    copy_cas(captured / 'artifacts', out / 'artifacts')
    copy_cas(final_inputs / 'artifacts', out / 'artifacts')
    cas_put(out / 'artifacts', regular(captured / 'capture.json'))
    cas_put(out / 'artifacts', regular(final_inputs / 'peer.json'))
    frozen_file(out / 'observations.json', read(admission['observations_sha256']))
    frozen_file(out / 'final-binding.json', encoded(dict(format='meguri-retained-companion-final-binding-v1',
        capture_sha256=checkpoint, case=action['case'], action=action['action'], actor=actor,
        observations_sha256=admission['observations_sha256'], wire_sha256=admission['wire_sha256'],
        final_peer_sha256=digest(regular(final_inputs / 'peer.json')), execution_binding_sha256=peer['execution_binding_sha256'], case_credit=False)))


def checkpoint_after(directory, checkpoint, measure_after):
    # This is the sole runtime transition from closed output to actual AFTER
    # measurement. Verify every frozen raw/wire byte before taking that step.
    read = artifact_reader(directory / 'checkpoint-artifacts')
    for inventory in (checkpoint['raw'], checkpoint['wire']):
        if inventory is not None:
            for entry in inventory['files']:
                require(len(read(entry['sha256'])) == entry['bytes'], 'checkpoint inventory bytes')
    if checkpoint['capture_sha256'] is not None:
        require(digest(regular(directory / 'capture/capture.json')) == checkpoint['capture_sha256'], 'checkpoint capture digest')
    frozen_file(directory / 'checkpoint.json', encoded(checkpoint))
    sha = digest(regular(directory / 'checkpoint.json'))
    frozen_file(directory / 'checkpoint.sha256', (sha + '\n').encode())
    measured = measure_after()
    require(digest(regular(directory / 'checkpoint.json')) == sha, 'runtime checkpoint changed')
    return sha, measured


def accepted_review():
    data = regular(ROOT / 'build/final-client-unit/retained-accepted-source-review-20261117.txt')
    require(bool(data.decode('utf-8').strip()), 'complete accepted source review required')
    return data


class RetainedRuntime:
    def __init__(self, outer, case, credentials):
        require(case in CASES, 'fixed case')
        self.review = accepted_review()
        self.outer, self.case, self.credentials = outer, case, credentials
        self.directory = outer / 'instances' / case
        self.containers = Containers(outer)
        self.peer = runtime_receipt(pathlib.Path(self.containers.config['peer_receipt']))
        self.client = load_json(self.peer / 'client-build.json')
        # Never mount the original preparation state writable. Its checkpoint
        # and CAS remain the original authority as the separate working copy evolves.
        initial = outer / 'preparations' / case
        self.state = self.directory / 'client-a-state'
        restore_working_state(initial, case, self.state)
        self.b_state = self.directory / 'client-b-state'
        self.b_state.mkdir()
        self.events = []
        self.serial = 0
        inspection = load_json(self.directory / 'service-container.json')[0]
        instance = load_json(self.directory / 'instance/instance.json')
        owner = load_json(self.directory / 'instance/container.json')['owner']
        self.cid = inspection['Id']
        self.invocation = dict(load_json(outer / 'candidate-invocation.json'), root=str(ROOT),
            server_arguments=inspection['Config']['Cmd'], data_volume=instance['volume'], owner_nonce=owner)
        require(self.invocation['server_arguments'] == ['serve', '/data/owner', 'https://candidate.example.test',
            '0.0.0.0:8080', '--trusted-peer', '127.0.0.1'], 'fixed owned origin')
        frozen_file(self.directory / 'invocation.json', encoded(self.invocation))
        info = credentials.lstat()
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1 and
                stat.S_IMODE(info.st_mode) == 0o600, 'protected credentials')
        secret = load_json(credentials)
        exact(secret, 'username password')
        require(all(isinstance(v, str) and v for v in secret.values()), 'credential pair')

    def new_step(self, action):
        self.serial += 1
        directory = self.directory / f'{self.serial:02d}-{action}'
        directory.mkdir()
        return directory

    def event(self, action, actor, directory, **values):
        row = dict(case=self.case, action=action, actor=actor, directory=directory.name, case_credit=False, **values)
        frozen_file(directory / 'event.json', encoded(row))
        self.events.append(row)
        return row

    def attempt(self, action, actor, callback):
        directory = self.new_step(action)
        try:
            return callback(directory)
        except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError):
            # Never serialize exceptions: public failures can contain server or
            # credential text. Actual partial files remain attached verbatim.
            self.event(action, actor, directory, operation_complete=False, error_kind='incomplete')
            return None

    def setup(self):
        directory = self.new_step('setup_account')
        helper = ROOT / 'server/tools/candidate-output.py'
        rc = self.containers.run(directory, 'python', ['/owned/candidate-output.py', '--setup-account'],
            network='container:' + self.cid, user='10001:10001', input_bytes=regular(self.credentials),
            mounts=[(helper, '/owned/candidate-output.py', True)], volumes=[(self.invocation['data_volume'], '/data', True)])
        require(rc == 0, 'owned account setup')

    def measure(self, directory, stage):
        tool = directory / ('measure-' + stage)
        tool.mkdir()
        process_path = directory / ('process-' + stage + '.json')
        rc = self.containers.run(tool, 'python', ['/owned/candidate-output.py', '--measure-process', '/owned/invocation.json'],
            pid=self.cid, user='10001:10001', stdout=process_path,
            mounts=[(ROOT / 'server/tools/candidate-output.py', '/owned/candidate-output.py', True),
                    (self.directory / 'invocation.json', '/owned/invocation.json', True)])
        require(rc == 0, 'actual process measurement')
        self.containers.command('inspect', self.cid, output=directory / ('container-' + stage + '.json'))
        return load_json(process_path), regular(directory / ('container-' + stage + '.json'))

    def offline(self, action):
        def execute(directory):
            rc = offline_worker(self.containers, self.peer, directory, self.case, action, self.state)
            prepared = saved_preparation(directory, self.case, action, self.client)
            row = self.event(action, 'client_a', directory, operation_complete=rc == 0,
                error_kind=None if rc == 0 else 'preparation_refused', execution_closed=True,
                snapshot_sha256=prepared['result']['observed']['snapshot_sha256'])
            return dict(prepared, event=row)
        return self.attempt(action, 'client_a', execute)

    def observe_worker(self, directory, inputs, raw):
        observer = directory / 'observer'
        observer.mkdir()
        observer_out = observer / 'raw'
        observer_out.mkdir()
        item = self.containers.create(observer, 'python', ['/opt/reference/driver.py', 'retained-observer', '--input', '/in'], network='container:' + self.cid,
            mounts=[(inputs, '/in', True), (observer_out, '/out', False)])
        observer_rc = None
        try:
            self.containers.start(item)
            deadline = time.monotonic() + 15
            ready = observer_out / 'ready.json'
            while not ready.exists() and time.monotonic() < deadline:
                row = self.containers.identity(item)
                require(row is not None and row['State']['Running'] is True, 'observer stopped before ready')
                time.sleep(0.05)
            value = load_json(ready)
            require(value == dict(format='meguri-retained-observer-ready-v1', case=self.case,
                action=load_json(inputs / 'action.json')['action'], run_id=self.invocation['run_id'],
                invocation_sha256=digest(regular(inputs / 'invocation.json'))), 'observer ready binding')
            worker = directory / 'worker'
            worker.mkdir()
            worker_rc = self.containers.run(worker, '/retained-peer', [], network='container:' + self.cid,
                mounts=[(inputs, '/in', True), (raw, '/out', False), (self.state, '/state', False),
                        (self.peer / 'meguri-retained-sync-peer', '/retained-peer', True),
                        (self.credentials, '/secret/credentials.json', True)])
        finally:
            # The worker is joined (or its failure retained) before graceful
            # observer shutdown. No observer command can replay a request.
            row = self.containers.identity(item)
            if row and row['State']['Running']:
                self.containers.command('kill', '--signal', 'TERM', row['Id'])
            try:
                observer_rc = self.containers.wait(item, observer)
            finally:
                self.containers.remove(item)
        require(observer_rc == 0, 'observer incomplete shutdown')
        require({p.name for p in observer_out.iterdir()} == {'ready.json', 'wire.json'}, 'closed observer files')
        return worker_rc, observer_out

    def network(self, action, expected, baseline=None, companion=False):
        actor = ('fresh_client' if action.startswith('witness_') else 'client_b') if companion else 'client_a'
        def execute(directory):
            require(expected is not None or (companion and action in OFFLINE_COMPANION), 'frozen authority unavailable')
            prepared = None if companion else self.offline('prepare_snapshot')
            if not companion:
                require(prepared is not None, 'worker state unavailable')
            inputs, raw = directory / 'input', directory / 'raw'
            inputs.mkdir()
            raw.mkdir()
            process, inspection = self.measure(directory, 'before')
            peer = package_preflight(inputs, self.outer, self.invocation, inspection, process)
            store = inputs / 'artifacts'
            frozen_file(inputs / 'action.json', encoded(dict(case=self.case, action=action)))
            for data in (regular(RECIPES), regular(SELECTION), self.review):
                cas_put(store, data)
            expected_sha = cas_put(store, encoded(snapshot(expected))) if expected is not None else None
            baseline_sha = cas_put(store, encoded(snapshot(baseline))) if baseline is not None else None
            if companion:
                require(self.case in COMPANION.get(action, ()), 'fixed companion step')
                expectations = dict(format='meguri-retained-companion-input-v1', case=self.case, action=action, actor=actor,
                    recipe_sha256=digest(regular(RECIPES)), selection_sha256=digest(regular(SELECTION)),
                    review_sha256=digest(self.review), baseline_sha256=baseline_sha, expected_sha256=expected_sha)
                invocation = self.invocation
                frozen_file(inputs / 'expectations.json', encoded(expectations))
            else:
                copy_cas(self.peer / 'artifacts', store)
                frozen_file(inputs / 'client-build.json', regular(self.peer / 'client-build.json'))
                manifest = dict(format='meguri-retained-input-v1', case=self.case, action=action,
                    action_sha256=digest(regular(inputs / 'action.json')), state_inputs_sha256=cas_put(store, prepared['state']),
                    before_sha256=cas_put(store, encoded(prepared['snapshot'])), expected_sha256=expected_sha,
                    review_sha256=digest(self.review), client_build_sha256=cas_put(store, regular(self.peer / 'client-build.json')),
                    preflight_peer_sha256=cas_put(store, regular(inputs / 'preflight.json')))
                cas_put(store, prepared['preparation'])
                frozen_file(inputs / 'input-manifest.json', encoded(manifest))
                cas_put(store, regular(inputs / 'action.json'))
                invocation = {key: self.invocation[key] for key in ('run_id', 'root', 'server_arguments', 'data_volume', 'owner_nonce')}
                invocation.update(format='meguri-retained-invocation-v1', case=self.case, action=action, actor='client_a',
                    client_build_sha256=manifest['client_build_sha256'], input_manifest_sha256=cas_put(store, encoded(manifest)),
                    preflight_peer_sha256=manifest['preflight_peer_sha256'])
            frozen_file(inputs / 'invocation.json', encoded(invocation))
            for name in ('action.json', 'preflight.json', 'invocation.json'):
                cas_put(store, regular(inputs / name))
            rc, observer_out, runtime_error = None, None, False
            try:
                if companion:
                    offline = action in OFFLINE_COMPANION
                    state = self.b_state
                    if actor == 'fresh_client':
                        state = directory / 'fresh-state'
                        state.mkdir()  # Empty new collection/media/cursor, never writer state.
                    child = directory / 'companion'
                    child.mkdir()
                    mounts = [(inputs, '/in', True), (raw, '/out', False), (state, '/state', False)]
                    if not offline:
                        mounts.append((self.credentials, '/secret/credentials.json', True))
                    companion_context(inputs)
                    rc = self.containers.run(child, 'python', ['/opt/reference/driver.py', 'retained-companion', '--input', '/in', '--credentials', '/secret/credentials.json'],
                        network='none' if offline else 'container:' + self.cid, mounts=mounts)
                else:
                    context(inputs)
                    rc, observer_out = self.observe_worker(directory, inputs, raw)
            except (OSError, ValueError, KeyError, subprocess.SubprocessError):
                runtime_error = True
            # Freeze all actual output bytes and actual sanitized wire bytes,
            # even if strict admission refuses them. This happens BEFORE after.
            checkpoint_store = directory / 'checkpoint-artifacts'
            inventory = tree_inventory(raw, checkpoint_store)
            state_inventory = tree_inventory(self.b_state if companion and actor == 'client_b' else
                directory / 'fresh-state' if companion else self.state, checkpoint_store)
            observer_path = directory / 'observer/raw'
            wire_inventory = tree_inventory(observer_path, checkpoint_store) if observer_path.exists() else None
            capture_sha, admission_error = None, False
            try:
                if companion:
                    capture_sha = companion_capture(raw, inputs, directory / 'capture')
                else:
                    require(observer_out is not None, 'worker observer unavailable')
                    wire_check(load_json(observer_out / 'wire.json'), load_json(inputs / 'action.json'), actor,
                               digest(regular(inputs / 'invocation.json')))
                    capture_sha = capture(raw, inputs, directory / 'capture')
            except (OSError, ValueError, KeyError, TypeError):
                admission_error = True
            checkpoint = dict(case=self.case, action=action, actor=actor, exit_code=rc, raw=inventory,
                wire=wire_inventory, state=state_inventory, capture_sha256=capture_sha,
                runtime_error=runtime_error, admission_error=admission_error, case_credit=False)
            checkpoint_sha, measured = checkpoint_after(directory, checkpoint, lambda: self.measure(directory, 'after'))
            after_process, after_inspection = measured
            require(tree_inventory(raw, checkpoint_store) == inventory, 'raw changed after capture')
            if wire_inventory is not None:
                require(tree_inventory(observer_path, checkpoint_store) == wire_inventory, 'wire changed after capture')
            package_complete(inputs, directory / 'final-input', invocation, after_inspection, after_process)
            require(capture_sha is not None and not admission_error and not runtime_error, 'strict runtime capture incomplete')
            if companion:
                companion_finalize(directory / 'capture', inputs, directory / 'final-input', directory / 'final', capture_sha)
                result = load_json(raw / 'observations.json')
                read = artifact_reader(raw / 'artifacts')
                after = json.loads(read(result['after_sha256'])) if result['after_sha256'] is not None else None
                authority = json.loads(read(result['authority_sha256'])) if result['authority_sha256'] is not None else None
                wire = json.loads(read(result['wire_sha256'])) if result['wire_sha256'] is not None else None
                observed = result['observed']
            else:
                finalize(directory / 'capture', directory / 'final-input', directory / 'final', capture_sha)
                result = load_json(raw / 'action-result.json') if (raw / 'action-result.json').exists() else None
                observed = result['observed'] if result else dict(operation_complete=False)
                after = observed.get('after')
                authority = expected
                wire = load_json(observer_out / 'wire.json')
            operation_complete = rc == 0 and observed['operation_complete'] is True
            if wire is not None:
                operation_complete = operation_complete and wire_complete(wire)
            error_kind = observed.get('error_kind')
            if wire is not None and any(r['disposition'] == 'incomplete' for r in wire['requests']):
                error_kind = 'incomplete'
            if not companion and result is None:
                error_kind = 'public_api_failure'
            event = self.event(action, actor, directory, operation_complete=operation_complete,
                error_kind=error_kind, execution_closed=True, checkpoint_sha256=checkpoint_sha,
                final_binding_sha256=digest(regular(directory / 'final/final-binding.json')))
            return dict(after=after, authority=authority, observed=observed, wire=wire, result=result, event=event,
                baseline=baseline, expected=expected)
        return self.attempt(action, actor, execute)

    def companion(self, action, expected=None, baseline=None):
        return self.network(action, expected, baseline, companion=True)

    def worker(self, action, expected):
        return self.network(action, expected)

    def run(self):
        initial_path = self.outer / 'preparations' / self.case
        initial = None
        try:
            initial = saved_preparation(initial_path, self.case, 'prepare_initial', self.client)['snapshot']
        except (OSError, ValueError, KeyError, TypeError):
            pass
        # Preserve an unsuccessful initial attempt. A fresh public snapshot
        # can still expose resident storage for later independent operations;
        # it cannot repair or credit the missing initial authoring claim.
        if initial is None:
            resident = self.offline('prepare_snapshot')
            initial = resident['snapshot'] if resident else None
        self.setup()
        results = {}
        def store(name, result):
            results[name] = result
            return result
        def authority(result):
            return result['authority'] if result else None
        def snap(result):
            return result['snapshot'] if result else None
        rich = self.case in ('full-upload', 'normal-no-change', 'normal-bidirectional')
        baseline = None
        if rich or self.case == 'full-download':
            prep = store('prepare_authority', self.companion('prepare_rich' if rich else 'prepare_right'))
            baseline = authority(prep)
            if baseline is not None:
                declared = media_recipe('initial') if rich else {name: digest(body.encode()) for name, body in
                    load_json(RECIPES)['replacement']['expected_server_media_after_collection_only'].items()}
                frozen_file(self.directory / 'server-media-authority.json', encoded(declared))
                if baseline['media_sha256'] != declared:
                    baseline = None
            store('upload_baseline', self.companion('upload_baseline', baseline, baseline))
        if self.case == 'authentication':
            store('authenticate', self.worker('authenticate', initial))
        elif self.case == 'full-upload':
            chosen = with_media(baseline, initial['media_sha256']) if baseline and initial else None
            store('import_baseline', self.worker('import_baseline', chosen))
            store('upload', self.worker('upload', chosen))
            store('witness_upload', self.companion('witness_upload', baseline, baseline))
        elif self.case == 'full-download':
            chosen = with_media(baseline, initial['media_sha256']) if baseline and initial else None
            store('download', self.worker('download', chosen))
            # The independent server media map was frozen by prepare_right,
            # BEFORE upload/download; never fill it from the retained after.
            store('witness_download', self.companion('witness_download', baseline, baseline))
        elif self.case in ('normal-no-change', 'normal-bidirectional'):
            chosen = with_media(baseline, initial['media_sha256']) if baseline and initial else None
            store('import_baseline', self.worker('import_baseline', chosen))
            store('establish_baseline', self.worker('establish_baseline', chosen))
            store('settle_normal', self.companion('settle_normal', baseline, baseline))
            if self.case == 'normal-no-change':
                frozen = self.offline('prepare_snapshot')
                settled = snap(frozen)
                if chosen is None or (settled is not None and projection(settled) != projection(chosen)):
                    settled = None
                store('normal_unchanged', self.worker('normal_unchanged', settled))
            else:
                forward = store('prepare_forward', self.offline('prepare_forward'))
                forward_authority = snap(forward) if chosen is not None else None
                if forward_authority is not None and chosen is not None:
                    try:
                        declared_edit(chosen, forward_authority, 'forward',
                            bool(forward['result']['observed'].get('limitations')))
                    except (ValueError, KeyError, TypeError):
                        forward_authority = None
                store('normal_send', self.worker('normal_send', forward_authority))
                forward_b = with_media(forward_authority, baseline['media_sha256']) if forward_authority and baseline else None
                store('receive_forward', self.companion('receive_forward', forward_b, baseline))
                reverse = store('prepare_reverse', self.companion('prepare_reverse', None, forward_b))
                reverse_authority = authority(reverse) if forward_b is not None else None
                if reverse_authority is not None and forward_b is not None:
                    try:
                        declared_edit(forward_b, reverse_authority, 'reverse',
                            not reverse['event']['operation_complete'])
                    except (ValueError, KeyError, TypeError):
                        reverse_authority = None
                store('send_reverse', self.companion('send_reverse', reverse_authority, forward_b))
                reverse_a = with_media(reverse_authority, initial['media_sha256']) if reverse_authority and initial else None
                store('normal_receive', self.worker('normal_receive', reverse_a))
        elif self.case in ('media-bidirectional', 'media-no-change'):
            store('media_baseline', self.worker('media_baseline', initial))
            if self.case == 'media-no-change':
                frozen = snap(self.offline('prepare_snapshot'))
                if initial is None or (frozen is not None and projection(frozen) != projection(initial)):
                    frozen = None
                store('media_unchanged', self.worker('media_unchanged', frozen))
            else:
                # B has its own collection and fresh media cursor. Collection
                # cells are B's independently prepared state, never client A's.
                store('baseline_media', self.companion('baseline_media', initial, initial))
                prep = store('prepare_forward_media', self.companion('prepare_forward_media', None, initial))
                forward_b = authority(prep)
                store('send_forward_media', self.companion('send_forward_media', forward_b, initial))
                forward_a = with_media(initial, media_recipe('forward')) if initial else None
                store('media_receive', self.worker('media_receive', forward_a))
                reverse = store('prepare_reverse_media', self.offline('prepare_reverse_media'))
                reverse_a = with_media(initial, media_recipe('reverse')) if initial and reverse else None
                store('media_send', self.worker('media_send', reverse_a))
                reverse_b = with_media(forward_b, media_recipe('reverse')) if forward_b else None
                store('receive_reverse_media', self.companion('receive_reverse_media', reverse_b, forward_b))
        else:
            store('uncertain_upload', self.worker('uncertain_upload', initial))
            # Collection-only upload cannot seed local media. The genuinely
            # empty server map is frozen independently, never repaired by A.
            committed = with_media(initial, {}) if initial else None
            store('witness_committed', self.companion('witness_committed', committed, committed))
        serializable = dict(case=self.case, events=self.events, initial=initial, results=results, case_credit=False)
        frozen_file(self.directory / 'case-observations.json', encoded(serializable))
        return serializable


def result_observations(case, observations):
    """Compute assertions from immutable actual outputs; never copy expected values."""
    results = observations['results']
    def result(name):
        return results.get(name) or {}
    def obs(name):
        return result(name).get('observed', {})
    def complete(name):
        return result(name).get('event', {}).get('operation_complete') is True
    def wire(name):
        return result(name).get('wire') or {}
    def paths(name):
        return [r['path'].rsplit('/', 1)[-1] for r in wire(name).get('requests', [])]
    def equal(name):
        item = result(name)
        return item.get('after') is not None and item.get('expected') is not None and (
            projection(item['after']) == projection(item['expected']))
    def no_files(name):
        if not wire(name):
            return None
        return sum(p in ('upload', 'download', 'uploadChanges', 'downloadFiles') for p in paths(name))
    def media_equal(name, part):
        after = result(name).get('after')
        return after is not None and after['media_sha256'] == media_recipe(part)
    def actual_media(name, part):
        return load_json(RECIPES)['media_utf8_exact'][part] if media_equal(name, part) else None
    def full_count(*names):
        if any(not wire(name) for name in names):
            return None
        return sum(p in ('upload', 'download') for name in names for p in paths(name))
    if case == 'authentication':
        return dict(authenticated_request_succeeded=obs('authenticate').get('authenticated_request_succeeded') is True,
            wrong_credentials_refused=obs('authenticate').get('wrong_credentials_refused') is True,
            secret_free_evidence=complete('authenticate') and bool(wire('authenticate')))
    if case == 'full-upload':
        return dict(explicit_authority=obs('upload').get('explicit_authority'), actual_http_transfer='upload' in paths('upload'),
            client_operation_complete=complete('upload'), independent_receiver_equal=complete('witness_upload') and equal('witness_upload'))
    if case == 'full-download':
        return dict(explicit_authority=obs('download').get('explicit_authority'), actual_http_transfer='download' in paths('download'),
            client_operation_complete=complete('download'), chosen_supported_state_equal=equal('download'),
            candidate_preserved=complete('witness_download') and equal('witness_download'))
    if case == 'normal-no-change':
        return dict(normal_completed_without_pending_full=complete('normal_unchanged') and obs('normal_unchanged').get('check_result') == 'NoChanges',
            unchanged_collection_transfers=full_count('normal_unchanged'),
            unchanged_media_transfers=sum(p in ('uploadChanges', 'downloadFiles') for p in paths('normal_unchanged')) if wire('normal_unchanged') else None,
            state_unchanged=obs('normal_unchanged').get('before') == obs('normal_unchanged').get('after') and equal('normal_unchanged'))
    if case == 'normal-bidirectional':
        final = result('normal_receive').get('after')
        duplicate_reviews = None
        deleted = False
        if final is not None:
            rows = final['tables']['revlog']['rows']
            ids = [json.dumps(row[0], sort_keys=True) for row in rows]
            duplicate_reviews = len(ids) - len(set(ids))
            baseline = result('prepare_authority').get('authority')
            if baseline is not None:
                before_ids = {json.dumps(row[0], sort_keys=True) for row in baseline['tables']['notes']['rows']}
                after_ids = {json.dumps(row[0], sort_keys=True) for row in final['tables']['notes']['rows']}
                deleted = len(before_ids - after_ids) == 2 and len(after_ids - before_ids) == 0
        return dict(forward_equal=complete('receive_forward') and equal('receive_forward'),
            reverse_equal=complete('normal_receive') and equal('normal_receive'), deleted_content_absent=deleted,
            duplicate_reviews=duplicate_reviews,
            full_transfer_count=full_count('normal_send', 'receive_forward', 'send_reverse', 'normal_receive'),
            normal_completed_without_pending_full=all(complete(n) for n in ('normal_send', 'receive_forward', 'send_reverse', 'normal_receive')))
    if case == 'media-bidirectional':
        return dict(forward_media_utf8_exact=actual_media('media_receive', 'forward'),
            reverse_media_utf8_exact=actual_media('receive_reverse_media', 'reverse'),
            actual_media_completed=all(complete(n) for n in ('send_forward_media', 'media_receive', 'media_send', 'receive_reverse_media')),
            partial_failure_hidden=False if (obs('media_receive').get('partial_failure_hidden') is False and
                obs('media_send').get('partial_failure_hidden') is False) else None)
    if case == 'media-no-change':
        totals = obs('media_unchanged').get('totals', {})
        return dict(actual_media_completed=complete('media_unchanged'),
            uploaded_mutations=totals.get('uploaded_files', 0) + totals.get('uploaded_deletions', 0) if totals else None,
            downloaded_mutations=totals.get('downloaded_files', 0) + totals.get('downloaded_deletions', 0) if totals else None,
            unchanged_file_transfers=no_files('media_unchanged'),
            state_unchanged=obs('media_unchanged').get('before') is not None and
                obs('media_unchanged').get('before') == obs('media_unchanged').get('after'))
    fault = wire('uncertain_upload').get('fault', {})
    recovery = obs('uncertain_upload').get('recovery', {})
    return dict(actual_reply_loss=all(fault.get(k) is True for k in ('armed', 'accepted_upload_ack', 'reply_dropped')) and
            fault.get('upload_request_count') == 1 and fault.get('upload_replay_count') == 0,
        blind_upload_replay_prevented=recovery.get('blind_upload_replay_prevented') is True,
        explicit_recovery_boundary=recovery.get('explicit_recovery_boundary') is True,
        committed_state_preserved=complete('witness_committed') and equal('witness_committed'))


def runtime_assemble(outer):
    config = load_json(outer / 'runtime.json')
    peer_dir = runtime_receipt(pathlib.Path(config['peer_receipt']))
    client_build = load_json(peer_dir / 'client-build.json')
    identity, invocation = load_json(outer / 'package-identity.json'), load_json(outer / 'candidate-invocation.json')
    selection = load_json(SELECTION)
    require(tuple(c['variants'][0] for c in selection['cases']) == CASES, 'exact selected eight')
    store = outer / 'output/artifacts'
    store.mkdir()
    copy_cas(outer / 'package-artifacts', store)
    copy_cas(peer_dir / 'artifacts', store)
    executable = regular(peer_dir / 'meguri-retained-sync-peer')
    require(digest(executable) == client_build['executable_sha256'], 'assembly peer executable')
    cases = []
    for selected in selection['cases']:
        case = selected['variants'][0]
        row = {key: copy.deepcopy(value) for key, value in selected.items() if key != 'client_kind'}
        row['lane'] = dict(kind='retained_peer', client=dict(kind='retained_meguri', version=tomllib.loads(regular(PEER / 'Cargo.toml').decode())['package']['version'],
            build=client_build['source_commit'], executable_sha256=client_build['executable_sha256']), server=identity['server'])
        row['fixture_sha256'] = digest(regular(RECIPES))
        artifacts = []
        def attach(role, data):
            sha = cas_put(store, data)
            ref = dict(role=role, sha256=sha, bytes=len(data))
            if ref not in artifacts:
                artifacts.append(ref)
        for path in (RECIPES, SELECTION):
            attach('synthetic_input', regular(path))
        attach('package', executable)
        attach('neutral_observation', regular(peer_dir / 'client-build.json'))
        for key in ('package_receipt', 'peer_receipt'):
            attach('mg_receipt', regular(pathlib.Path(config[key])))
        inventory_complete = True
        for root in (outer / 'preparations' / case, outer / 'instances' / case):
            if root.exists():
                try:
                    inventory = tree_inventory(root, store)
                    attach('neutral_observation', encoded(inventory))
                except (OSError, ValueError, KeyError, TypeError):
                    inventory_complete = False
                    attach('neutral_observation', encoded(dict(case=case, inventory_complete=False,
                        boundary=root.parent.name, case_credit=False)))
        observation_path = outer / 'instances' / case / 'case-observations.json'
        preparation_path = outer / 'preparations' / case / 'checkpoint.json'
        factory_path = outer / 'instances' / case / 'factory-result.json'
        try:
            require(inventory_complete, 'incomplete case inventory')
            if observation_path.exists():
                observations = load_json(observation_path)
                actual = result_observations(case, observations)
                events = observations['events']
                factory = load_json(factory_path) if factory_path.exists() else {}
                complete = bool(events) and all(e.get('execution_closed') is True and
                    e.get('error_kind') not in ('incomplete', 'preparation_refused') for e in events)
                preparation = load_json(preparation_path)
                complete = (complete and preparation['exit_code'] == 0 and preparation.get('admission_valid') is True and
                    preparation.get('preparation_complete') is True and factory.get('factory_ready') is True and factory.get('cleanup_complete') is True)
                operations_ok = complete and all(e['operation_complete'] is True for e in events)
                row['observed'] = [dict(name=name, value=value) for name, value in actual.items()]
                expected = {v['name']: v['value'] for v in selected['expected']}
                require(set(actual) == set(expected), 'unchanged assertion names')
                # A missing/failed setup, preparation or final attachment never
                # becomes a matched case merely because a few assertions agree.
                row['status'] = ('matched' if operations_ok and actual == expected else
                    'failed' if complete and any(e.get('error_kind') == 'public_api_failure' for e in events) else
                    'different' if complete else 'unavailable')
                if row['status'] != 'matched':
                    row['reason'] = ('Actual public client operation failed' if row['status'] == 'failed' else
                        'Actual retained assertions differ' if complete else
                        'Required preparation, action, capture, lifetime or cleanup is incomplete')
                attach('neutral_observation', regular(observation_path))
            else:
                row.update(status='unavailable', reason='Owned factory or runtime did not complete; actual partial evidence retained',
                           observed=[dict(name='runtime_complete', value=False)])
        except (OSError, ValueError, KeyError, TypeError, AttributeError):
            row.update(status='unavailable', reason='Case artifact closure or observations are incomplete; independent cases retained',
                       observed=[dict(name='runtime_complete', value=False)])
        row['artifacts'] = artifacts
        cases.append(row)
    require(len({c['id'] for c in cases}) == len(cases) == 8, 'complete fixed selection')
    evidence = dict(format='meguri-sync-server-v1', baseline_release='26.09.3', document_kind='evidence',
        **{k: invocation[k] for k in ('source_commit', 'batch_base', 'source_sha256', 'lock_sha256', 'executable_sha256', 'mg_job', 'arguments')},
        expected_case_count=8, selected_case_count=8, completed_case_count=sum(c['status'] in ('matched', 'different', 'failed') for c in cases), cases=cases)
    frozen_file(outer / 'output/cases.json', encoded(evidence))
    frozen_file(outer / 'output/invocation.json', encoded(invocation))
    frozen_file(outer / 'output/review-required.json', encoded(dict(case_credit=False, sealed=False,
        reason='Parent must independently review actual preparation, runtime identities, semantic assertions and all dependencies')))
    return evidence


def runtime_main(args):
    mode = args[0]
    if mode == 'runtime-receipt' and len(args) == 2:
        runtime_receipt(pathlib.Path(args[1]))
        return 0
    require(len(args) >= 2, 'runtime internal arguments')
    outer = pathlib.Path(args[1]).resolve(strict=True)
    if mode == 'runtime-cleanup' and len(args) == 2:
        Containers(outer).cleanup()
        return 0
    if mode == 'runtime-prepare' and len(args) == 3:
        return runtime_prepare(outer, args[2])
    if mode == 'runtime-case' and len(args) == 4:
        run = RetainedRuntime(outer, args[2], pathlib.Path(args[3]))
        observations = run.run()
        return 0 if all(e['operation_complete'] for e in observations['events']) else 1
    if mode == 'runtime-assemble' and len(args) == 2:
        evidence = runtime_assemble(outer)
        return 0 if all(c['status'] == 'matched' for c in evidence['cases']) else 1
    if mode == 'runtime-counts' and len(args) == 2:
        cases = load_json(outer / 'output/cases.json')['cases']
        print(json.dumps(dict(total=8, passed=sum(c['status'] == 'matched' for c in cases),
            failed=sum(c['status'] in ('different', 'failed') for c in cases),
            skipped=sum(c['status'] == 'unavailable' for c in cases))))
        return 0
    raise ValueError('fixed retained runtime operation')


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1].startswith('runtime-'):
        try:
            raise SystemExit(runtime_main(sys.argv[1:]))
        except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError):
            sys.stderr.write('retained runtime incomplete; actual partial artifacts retained; no case credit\n')
            raise SystemExit(1)

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
