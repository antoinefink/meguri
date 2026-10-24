#!/usr/bin/env bats
# Pure helper tests, run in Docker by tools/mg selftest --filter 'local-sync:'.

setup() {
  export LOCAL_SYNC_SCRIPT="$BATS_TEST_DIRNAME/../scripts/local_sync.py"
  export LOCAL_SYNC_TEST_ROOT="$BATS_TEST_TMPDIR/private-service"
}

@test "local-sync: foreign names refuse up and down without mutation" {
  run python3 - <<'PY'
import importlib.util, os
from pathlib import Path
from unittest.mock import patch
s = importlib.util.spec_from_file_location('local_sync', os.environ['LOCAL_SYNC_SCRIPT'])
m = importlib.util.module_from_spec(s); s.loader.exec_module(m)
directory = Path(os.environ['LOCAL_SYNC_TEST_ROOT']); directory.mkdir()
for action in ('up', 'down'):
    calls = []
    def fake(*args, **kwargs):
        calls.append(args)
        assert args == ('info', '--format', '{{.ServerVersion}}')
        return 'installed'
    with patch.object(m, 'docker', fake), patch.object(m, 'inspect', return_value={}):
        try:
            m.operate(directory, action, 'sha256:' + 'a' * 64)
            raise AssertionError('foreign object was accepted')
        except m.ServiceError as error:
            assert 'foreign' in str(error)
    assert len(calls) == 1
assert not list(directory.iterdir())
PY
  [ "$status" -eq 0 ]
}

@test "local-sync: repeated up retains credentials and private permissions" {
  run python3 - <<'PY'
import contextlib, hashlib, importlib.util, io, json, os, stat, sys
from pathlib import Path
from unittest.mock import patch
s = importlib.util.spec_from_file_location('local_sync', os.environ['LOCAL_SYNC_SCRIPT'])
m = importlib.util.module_from_spec(s); s.loader.exec_module(m)
root = Path(os.environ['LOCAL_SYNC_TEST_ROOT']); image = 'sha256:' + 'a' * 64
def inspect(kind, name, expression):
    return image if kind == 'image' else None
def start(directory, config, found):
    config['port'] = 49152
    m.save_config(directory, config)
captured = io.StringIO()
with patch.object(m, 'docker', return_value=''), patch.object(m, 'inspect', inspect), \
     patch.object(m, 'start', start), patch.object(sys, 'argv', ['local_sync', '--root', str(root), 'up', '--image', image]), \
     contextlib.redirect_stdout(captured):
    assert m.main() == 0
    directory = root / 'build/local-sync'
    first = {p.name: p.read_bytes() for p in directory.iterdir()}
    assert m.main() == 0
    assert first == {p.name: p.read_bytes() for p in directory.iterdir()}
assert stat.S_IMODE(directory.stat().st_mode) == 0o700
assert all(stat.S_IMODE(p.stat().st_mode) == 0o600 for p in directory.iterdir())
client = json.loads((directory / 'client.json').read_text())
env = (directory / 'server.env').read_text()
assert len(client['password']) >= 40
assert client['password'] not in env
assert 'PASSWORDS_HASHED=1\n' in env
assert '$pbkdf2-sha256$i=600000,l=32$' in env
assert client['password'] not in captured.getvalue()
assert '$pbkdf2' not in captured.getvalue()
PY
  [ "$status" -eq 0 ]
}

@test "local-sync: down sends SIGINT only to owned service and preserves data" {
  run python3 - <<'PY'
import importlib.util, json, os
from pathlib import Path
from unittest.mock import patch
s = importlib.util.spec_from_file_location('local_sync', os.environ['LOCAL_SYNC_SCRIPT'])
m = importlib.util.module_from_spec(s); s.loader.exec_module(m)
directory = Path(os.environ['LOCAL_SYNC_TEST_ROOT']); directory.mkdir(mode=0o700)
config = m.new_config(directory, 'sha256:' + 'a' * 64)
before = {p.name: p.read_bytes() for p in directory.iterdir()}
calls = []
def fake(*args, **kwargs):
    calls.append(args)
    return ''
def inspect(kind, name, expression):
    if expression in ('.Config.Labels', '.Labels'): return {m.LABEL: config['owner']}
    if expression == '.State': return {'Status': 'exited', 'Running': False}
    if expression == '.Id': return 'container-id'
    raise AssertionError(expression)
with patch.object(m, 'docker', fake), patch.object(m, 'inspect', inspect):
    report = m.operate(directory, 'down', None)
assert calls == [('info', '--format', '{{.ServerVersion}}'),
                 ('container', 'stop', '--signal', 'SIGINT', '--timeout', '20', m.CONTAINER)]
assert report['state'] == 'exited' and not report['healthy']
assert before == {p.name: p.read_bytes() for p in directory.iterdir()}
PY
  [ "$status" -eq 0 ]
}

@test "local-sync: failed initialization recovers and down up retains fixed endpoint and data" {
  run python3 - <<'PY'
import importlib.util, json, os
from pathlib import Path
from unittest.mock import patch
s = importlib.util.spec_from_file_location('local_sync', os.environ['LOCAL_SYNC_SCRIPT'])
m = importlib.util.module_from_spec(s); s.loader.exec_module(m)
directory = Path(os.environ['LOCAL_SYNC_TEST_ROOT']); directory.mkdir(mode=0o700)
image = 'sha256:' + 'a' * 64
resources, calls = {}, []
initializations = 0
def inspect(kind, name, expression):
    if kind == 'image': return image
    resource = resources.get(kind)
    if resource is None: return None
    if expression in ('.Config.Labels', '.Labels'): return resource['labels']
    if expression == '.Image': return image
    if expression == '.HostConfig.PortBindings':
        return {'8080/tcp': [{'HostIp': '127.0.0.1', 'HostPort': resource['requested_port']}]}
    if expression == '.NetworkSettings.Ports':
        return {'8080/tcp': [{'HostIp': '127.0.0.1', 'HostPort': str(resource['port'])}]}
    if expression == '.State':
        return {'Status': 'running' if resource['running'] else 'exited', 'Running': resource['running']}
    if expression == '.Id': return resource['id']
    raise AssertionError(expression)
def docker(*args, **kwargs):
    global initializations
    calls.append(args)
    if args == ('info', '--format', '{{.ServerVersion}}'): return 'installed'
    labels = dict(args[i + 1].split('=', 1) for i, arg in enumerate(args) if arg == '--label')
    if args[:2] in (('network', 'create'), ('volume', 'create')):
        kind = args[0]
        assert kind not in resources
        resources[kind] = {'id': kind + '-id', 'labels': labels}
        if kind == 'volume':
            resources[kind]['data'] = {'account/collection': b'retained-test-data'}
        return kind + '-id'
    if args[0] == 'run':
        assert labels['meguri.mg.run-id'] == os.environ['MG_RUN_ID']
        owner = json.loads((directory / 'config.json').read_text())['owner']
        assert resources['volume']['labels'] == {m.LABEL: owner}
        if '--rm' in args:
            assert args[-2:] == ('-c', 'chown 10001:10001 /data')
            initializations += 1
            if initializations == 1:
                raise m.ServiceError('Test initialization interrupted after volume creation.')
            resources['volume']['root_owner'] = '10001:10001'
            return ''
        assert resources['volume']['root_owner'] == '10001:10001'
        assert 'container' not in resources
        address, requested_port, target_port = args[args.index('--publish') + 1].split(':')
        assert address == '127.0.0.1' and target_port == '8080'
        resources['container'] = {'id': 'container-id', 'labels': labels, 'running': True,
                                  'requested_port': requested_port,
                                  'port': int(requested_port) if requested_port else 49152}
        return 'container-id'
    if args == ('container', 'start', m.CONTAINER):
        container = resources['container']
        # Docker allocates another port on restart when HostConfig kept an empty one.
        if not container['requested_port']: container['port'] += 1
        container['running'] = True
        return 'container-id'
    if args == ('container', 'stop', '--signal', 'SIGINT', '--timeout', '20', m.CONTAINER):
        resources['container']['running'] = False
        return 'container-id'
    raise AssertionError(args)
with patch.object(m, 'docker', docker), patch.object(m, 'inspect', inspect), \
     patch.object(m, 'health', return_value=True):
    with patch.dict(os.environ, {'MG_RUN_ID': 'failed-start'}):
        try:
            m.operate(directory, 'up', image)
            raise AssertionError('initialization failure was ignored')
        except m.ServiceError as error:
            assert 'interrupted' in str(error)
    credentials = json.loads((directory / 'client.json').read_text())
    server_env = (directory / 'server.env').read_bytes()
    config = json.loads((directory / 'config.json').read_text())
    volume = resources['volume']
    assert 'container' not in resources and 'root_owner' not in volume
    with patch.dict(os.environ, {'MG_RUN_ID': 'recovered-start'}):
        report = m.operate(directory, 'up', None)
    assert report['healthy'] and report['container_id'] == 'container-id'
    client = json.loads((directory / 'client.json').read_text())
    assert all(client[key] == credentials[key] for key in ('username', 'password'))
    assert (directory / 'server.env').read_bytes() == server_env
    retained_config = json.loads((directory / 'config.json').read_text())
    assert all(retained_config[key] == config[key] for key in ('owner', 'image'))
    private_files = {p.name: p.read_bytes() for p in directory.iterdir()}
    with patch.dict(os.environ, {'MG_RUN_ID': 'later-start'}):
        assert m.operate(directory, 'down', None)['state'] == 'exited'
        assert m.operate(directory, 'up', None)['healthy']
    assert private_files == {p.name: p.read_bytes() for p in directory.iterdir()}
assert resources['volume'] is volume and volume['id'] == 'volume-id'
assert volume['data'] == {'account/collection': b'retained-test-data'}
assert resources['container']['labels']['meguri.mg.run-id'] == 'recovered-start'
assert resources['container']['requested_port'] != ''
assert resources['container']['port'] == retained_config['port']
assert initializations == 3
assert sum(args[:2] == ('volume', 'create') for args in calls) == 1
assert sum(args[0] == 'run' and '--detach' in args for args in calls) == 1
assert credentials['password'] not in repr(calls)
PY
  [ "$status" -eq 0 ]
}

@test "local-sync: retained dynamic or changed port binding refuses startup without mutation" {
  run python3 - <<'PY'
import importlib.util, os
from pathlib import Path
from unittest.mock import patch
s = importlib.util.spec_from_file_location('local_sync', os.environ['LOCAL_SYNC_SCRIPT'])
m = importlib.util.module_from_spec(s); s.loader.exec_module(m)
directory = Path(os.environ['LOCAL_SYNC_TEST_ROOT']); directory.mkdir(mode=0o700)
image = 'sha256:' + 'a' * 64
config = m.new_config(directory, image)
config['port'] = 49152
m.save_config(directory, config)
before = {p.name: p.read_bytes() for p in directory.iterdir()}
for requested_port in ('', '49153'):
    def inspect(kind, name, expression):
        if expression in ('.Id', '.Image'): return image
        assert expression == '.HostConfig.PortBindings'
        return {'8080/tcp': [{'HostIp': '127.0.0.1', 'HostPort': requested_port}]}
    with patch.object(m, 'docker') as docker, patch.object(m, 'inspect', inspect), \
         patch.dict(os.environ, {'MG_RUN_ID': 'refused-start'}):
        try:
            m.start(directory, config, {'container': True, 'network': True, 'volume': True})
            raise AssertionError('unstable retained port was accepted')
        except m.ServiceError as error:
            assert 'fixed loopback port' in str(error)
        docker.assert_not_called()
    assert before == {p.name: p.read_bytes() for p in directory.iterdir()}
PY
  [ "$status" -eq 0 ]
}

@test "local-sync: failed durability sync cannot acknowledge a private write" {
  run python3 - <<'PY'
import importlib.util, os, stat
from pathlib import Path
from unittest.mock import patch
s = importlib.util.spec_from_file_location('local_sync', os.environ['LOCAL_SYNC_SCRIPT'])
m = importlib.util.module_from_spec(s); s.loader.exec_module(m)
directory = Path(os.environ['LOCAL_SYNC_TEST_ROOT']); directory.mkdir(mode=0o700)
path = directory / 'config.json'
m.private_write(path, 'previous')
real_fsync = os.fsync
for failing_kind in ('file', 'directory'):
    synced = []
    def fsync(fd):
        kind = 'directory' if stat.S_ISDIR(os.fstat(fd).st_mode) else 'file'
        synced.append(kind)
        if kind == failing_kind:
            raise OSError('test durable write failure')
        real_fsync(fd)
    with patch.object(m.os, 'fsync', fsync):
        try:
            m.private_write(path, 'replacement')
            raise AssertionError('durability failure was acknowledged as success')
        except OSError:
            pass
    assert synced == (['file'] if failing_kind == 'file' else ['file', 'directory'])
    assert path.read_text() == ('previous' if failing_kind == 'file' else 'replacement')
    assert stat.S_IMODE(path.stat().st_mode) == 0o600
    assert not path.with_name(path.name + '.new').exists()
PY
  [ "$status" -eq 0 ]
}

@test "local-sync: failed Docker command exits without raw diagnostics or credentials" {
  run python3 - <<'PY'
import contextlib, importlib.util, io, os, subprocess, sys
from unittest.mock import patch
s = importlib.util.spec_from_file_location('local_sync', os.environ['LOCAL_SYNC_SCRIPT'])
m = importlib.util.module_from_spec(s); s.loader.exec_module(m)
secret = 'private-value-should-never-appear'
out, err = io.StringIO(), io.StringIO()
with patch.object(m.subprocess, 'run', return_value=subprocess.CompletedProcess(['docker'], 1, secret, secret)), \
     patch.object(sys, 'argv', ['local_sync', '--root', os.environ['LOCAL_SYNC_TEST_ROOT'], 'status']), \
     contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
    assert m.main() == 1
assert secret not in out.getvalue() + err.getvalue()
assert out.getvalue() == ''
assert err.getvalue().strip() == 'Docker operation failed; inspect the named service locally.'
PY
  [ "$status" -eq 0 ]
}
