#!/usr/bin/env python3
"""Current-run Mac Unit inputs only; no signed-agent or credential proof."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import stat
import subprocess
import sys
import tempfile
from contextlib import ExitStack

sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location(
    'swift_helper', Path(__file__).with_name('swift-cloud-service-helper.py'))
helper = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(helper)
r = helper.review
require = r.require
ROOT = r.ROOT
class GuestUnavailable(Exception):
    pass


PRODUCER_ARGS = ['--', '--test', 'ffi_it', '-E',
                 'package(meguri-ffi) & test(=cloud_initial_app_fixture::export_initial_app_adoption_replies_from_real_publication)']


def admitted(value):
    run = Path(value)
    require(run.parent == ROOT / 'build/mg/xcode-check'
            and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', run.name)
            and run.name != 'latest' and run == run.resolve(strict=True)
            and os.environ.get('MG_RUN_DIR') == str(run)
            and os.environ.get('MG_RUN_ID') == run.name
            and os.environ.get('MG_JOB') == 'xcode-check', 'current Xcode MG run required')
    with r.owned_directory(run):
        pass
    return run


def source():
    def git(*args):
        return subprocess.check_output(['git', '-C', str(ROOT), *args])
    require(not git('status', '--porcelain', '-z'), 'clean source required')
    revision = git('rev-parse', 'HEAD').decode().strip()
    require(re.fullmatch('[a-f0-9]{40}', revision), 'source revision')
    digest = hashlib.sha256()
    for raw in sorted(git('ls-files', '-z', '--', 'app', 'core', 'tools').split(b'\0')):
        if not raw:
            continue
        path = ROOT / os.fsdecode(raw)
        # Only named own source paths. Never follow a link into another root.
        require(not any(p in ('refs', 'oracle', 'local-data', '.git') for p in path.parts), 'source scope')
        value = r.identity(path)
        digest.update(raw + b'\0' + json.dumps(value, sort_keys=True).encode() + b'\0')
    return {'root': str(ROOT), 'revision': revision, 'dirty': False, 'sha256': digest.hexdigest()}


def pinned(path, limit=r.CODE_LIMIT, mode=None):
    with r.owned_directory(path.parent) as (directory, verify_directory):
        with r.export.retained_file(directory, path.name) as (fd, verify):
            info = os.fstat(fd)
            require(info.st_uid == os.getuid() and 0 < info.st_size <= limit
                    and info.st_mode & 0o022 == 0, 'input owner/size/mode')
            if mode is not None:
                require(stat.S_IMODE(info.st_mode) == mode, 'input exact mode')
            identity = r.code_hash(fd)
            require(identity['bytes'] == info.st_size, 'input size changed')
            verify()
            verify_directory()
            return {'path': str(path), 'identity': identity, 'pin': r.code_signature(info),
                    'mode': stat.S_IMODE(info.st_mode)}


def record(path):
    pin = pinned(path, 4 * 1024 * 1024)
    value = r.read(path)
    require(pinned(path, 4 * 1024 * 1024) == pin, 'receipt changed')
    return value, pin


def verify_inputs(run, producer):
    authority, _ = record(run / 'xcode-unit-source.json')
    require(source() == authority, 'source changed during production')
    require(producer.parent.parent == ROOT / 'build/mg/core-test'
            and producer.name == 'result.json' and producer == producer.resolve(strict=True)
            and producer.parent.name != 'latest', 'exact producer result required')
    receipt, producer_pin = record(producer)
    require(receipt['job'] == 'core-test' and receipt['status'] == 'pass'
            and receipt['dry_run'] is False and receipt['root'] == str(ROOT)
            and receipt['run_id'] == producer.parent.name
            and receipt['commit'] == {'sha': authority['revision'], 'dirty': False}
            and receipt['args'] == PRODUCER_ARGS
            and receipt['summary'] == {'total': 1, 'passed': 1, 'failed': 0, 'skipped': 0},
            'real clean passing producer required')
    # The bridge calls this producer synchronously after source admission.
    require(producer.stat().st_mtime_ns >= (run / 'xcode-unit-source.json').stat().st_mtime_ns,
            'producer predates admission')
    fixture = pinned(run / 'initial-adoption-fixture.json', 16 * 1024 * 1024)
    publication, publication_pin = record(run / 'cloud-service-code.json')
    binary = pinned(run / 'cloud-service-code/meguri-cli', mode=0o500)
    build = pinned(run / 'cloud-service-build.jsonl', 16 * 1024 * 1024)
    compiler = pinned_compiler(Path(publication['compiler']))
    require(publication['run'] == str(run) and publication['published'] == binary['path']
            and publication['compiled'] == publication['executable'] == binary['identity'] == compiler['identity']
            and publication['compiler_pin'] == compiler['pin']
            and publication['published_pin'] == binary['pin'] and publication['mode'] == 0o500
            and publication['build_profile'] == 'dev' and publication['build'] == build['identity'],
            'helper publication mismatch')
    messages = [json.loads(line) for line in Path(build['path']).read_text().splitlines()]
    require(messages and all(m.get('reason') != 'compiler-message'
            or m['message']['level'] not in ('warning', 'error') for m in messages), 'helper diagnostics')
    executables = [m['executable'] for m in messages if m.get('reason') == 'compiler-artifact'
                   and m.get('target', {}).get('name') == 'meguri-cli' and m.get('executable') is not None]
    require(executables == [publication['compiler']] and pinned(Path(build['path']), 16 * 1024 * 1024) == build,
            'helper build mismatch')
    # Retain the actual receipt, without inventing a producer result.
    with r.owned_directory(run):
        r.export.copy_package(producer, run / 'initial-adoption-producer.json')
    require(r.identity(run / 'initial-adoption-producer.json') == producer_pin['identity'], 'receipt copy mismatch')
    r.write(run / 'xcode-unit-inputs.json', {
        'version': 1, 'run': str(run), 'source': authority, 'producer': producer_pin,
        'publication': publication_pin, 'build': build, 'fixture': fixture, 'helper': binary})


def pinned_compiler(path):
    # Cargo's original may be hardlinked; the run-owned publication may not.
    require(path.is_absolute(), 'absolute native compiler path')
    with r.export.retained_directory(path.parent) as (directory, verify_directory):
        fd = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        try:
            before = os.fstat(fd)
            require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                    and before.st_mode & 0o022 == 0 and before.st_mode & 0o111
                    and 0 < before.st_size <= r.CODE_LIMIT, 'compiler mode/size/owner')
            actual = r.code_hash(fd)
            require(r.code_signature(before) == r.code_signature(os.fstat(fd))
                    == r.code_signature(os.stat(path.name, dir_fd=directory, follow_symlinks=False)),
                    'compiler changed')
            verify_directory()
            return {'identity': actual, 'pin': r.code_signature(before)}
        finally:
            os.close(fd)


def checked(run):
    value, _ = record(run / 'xcode-unit-inputs.json')
    require(value['version'] == 1 and value['run'] == str(run)
            and value['source'] == source(), 'input source authority changed')
    for key, path, limit, mode in (
        ('fixture', run / 'initial-adoption-fixture.json', 16 * 1024 * 1024, None),
        ('helper', run / 'cloud-service-code/meguri-cli', r.CODE_LIMIT, 0o500),
        ('publication', run / 'cloud-service-code.json', 4 * 1024 * 1024, None),
        ('build', run / 'cloud-service-build.jsonl', 16 * 1024 * 1024, None)):
        require(pinned(path, limit, mode) == value[key], 'published input changed')
    require(pinned(Path(value['producer']['path']), 4 * 1024 * 1024) == value['producer']
            and r.identity(run / 'initial-adoption-producer.json') == value['producer']['identity'],
            'producer receipt changed')
    return value


# Sent as code through the existing SSH wrapper. It has no caller-selected
# destination: only these two leaves under this admitted MG run are writable.
# No payload or native exception text is printed. Guest checks run again in
# the process that sets TEST_RUNNER_* and execs the original xcodebuild argv.
GUEST = r'''
import hashlib, json, os, stat, sys
from contextlib import ExitStack

def need(ok):
    if not ok: raise ValueError()

def meta(s):
    need(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid == os.getuid())
    return [s.st_dev, s.st_ino, s.st_mode, s.st_nlink, s.st_uid, s.st_gid,
            s.st_size, s.st_mtime_ns, s.st_ctime_ns]

def digest(fd):
    os.lseek(fd, 0, 0)
    h = hashlib.sha256(); size = 0
    for b in iter(lambda: os.read(fd, 1024 * 1024), b''):
        size += len(b); need(size <= 512 * 1024 * 1024); h.update(b)
    return {'sha256': h.hexdigest(), 'bytes': size}

try:
    action, root, name, encoded = sys.argv[1:5]
    need(root.startswith('/') and all(p not in ('.', '..', '') for p in root.split('/')[1:]))
    import re
    need(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', name) and name != 'latest')
    expected = json.loads(encoded)
    base = root + '/build/mg/xcode-check/' + name
    paths = [base + '/vm-unit-inputs/initial-adoption-fixture.json', base + '/vm-unit-inputs/meguri-cli']
    with ExitStack() as stack:
        parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        stack.callback(os.close, parent)
        links = []
        parts = (base + '/vm-unit-inputs').split('/')[1:]
        for i, part in enumerate(parts):
            if action == 'deliver' and i >= len(root.split('/')) - 1:
                try: os.mkdir(part, 0o700, dir_fd=parent)
                except FileExistsError:
                    need(i != len(parts) - 1)  # Exclusive delivery, never adopt old inputs.
            fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
            stack.callback(os.close, fd)
            s = os.fstat(fd)
            # Ancestors outside the checkout (e.g. /tmp) need not be owned.
            if i >= len(root.split('/')) - 2:
                need(s.st_uid == os.getuid() and s.st_mode & 0o022 == 0)
            links.append((parent, part, fd, (s.st_dev, s.st_ino, s.st_mode)))
            parent = fd
        def directories():
            for p, n, fd, pin in links:
                for s in (os.fstat(fd), os.stat(n, dir_fd=p, follow_symlinks=False)):
                    need((s.st_dev, s.st_ino, s.st_mode) == pin)
        files = []
        for leaf, spec, mode in zip(('initial-adoption-fixture.json', 'meguri-cli'), expected['files'], (0o400, 0o500)):
            if action == 'deliver':
                need(0 < spec['bytes'] <= (16 * 1024 * 1024 if mode == 0o400 else 512 * 1024 * 1024))
                fd = os.open(leaf, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
                stack.callback(os.close, fd)
                remaining = spec['bytes']
                while remaining:
                    b = sys.stdin.buffer.read(min(1024 * 1024, remaining)); need(b)
                    remaining -= len(b); pending = memoryview(b)
                    while pending:
                        count = os.write(fd, pending); need(count > 0); pending = pending[count:]
                os.fchmod(fd, mode); os.fsync(fd)
            else:
                need(action == 'launch')
                fd = os.open(leaf, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
                stack.callback(os.close, fd)
            pin = meta(os.fstat(fd)); need(stat.S_IMODE(pin[2]) == mode)
            need(digest(fd) == spec and meta(os.fstat(fd)) == pin
                 == meta(os.stat(leaf, dir_fd=parent, follow_symlinks=False)))
            files.append((leaf, fd, pin, spec))
        if action == 'deliver': need(not sys.stdin.buffer.read(1))
        for leaf, fd, pin, spec in files:
            need(digest(fd) == spec and meta(os.fstat(fd)) == pin
                 == meta(os.stat(leaf, dir_fd=parent, follow_symlinks=False)))
        directories()
        pins = [f[2] for f in files]
        if action == 'deliver':
            print(json.dumps({'files': expected['files'], 'pins': pins, 'paths': paths}, sort_keys=True))
        else:
            need(expected['pins'] == pins and expected['paths'] == paths)
            need(sys.argv[5] == 'test-without-building')
            env = dict(os.environ)
            env['TEST_RUNNER_MEGURI_INITIAL_ADOPTION_FIXTURE'] = paths[0]
            env['TEST_RUNNER_MEGURI_CLOUD_SERVICE_CLI'] = paths[1]
            os.execvpe('xcodebuild', ['xcodebuild', *sys.argv[5:]], env)
except (OSError, ValueError, KeyError, TypeError, IndexError):
    print('xcode_project_check: Mac Unit input guest verification failed', file=sys.stderr)
    sys.exit(70)
'''


def guest_command(run, action, value):
    return shlex.join(['python3', '-c', GUEST, action, str(ROOT), run.name,
                       json.dumps(value, sort_keys=True, separators=(',', ':'))])


def deliver(run, ssh):
    value = checked(run)
    expected = {'files': [value[k]['identity'] for k in ('fixture', 'helper')]}
    with ExitStack() as stack:
        streams = []
        for key in ('fixture', 'helper'):
            path = Path(value[key]['path'])
            directory, verify_dir = stack.enter_context(r.owned_directory(path.parent))
            fd, verify_file = stack.enter_context(r.export.retained_file(directory, path.name))
            streams.append((fd, verify_dir, verify_file, value[key]))
        errors = stack.enter_context(tempfile.TemporaryFile())
        process = subprocess.Popen([ssh, guest_command(run, 'deliver', expected)], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=errors)
        try:
            for fd, verify_dir, verify_file, item in streams:
                require(r.code_signature(os.fstat(fd)) == item['pin'], 'input changed before transfer')
                while True:
                    block = os.read(fd, 1024 * 1024)
                    if not block:
                        break
                    process.stdin.write(block)
                require(r.code_hash(fd) == item['identity'], 'input changed during transfer')
                verify_file(); verify_dir()
            process.stdin.close()
            output = process.stdout.read(16385)
            code = process.wait()
            errors.seek(0)
            diagnostic = errors.read(16384).decode('utf-8', errors='replace')
            if code == 255 or (code != 0 and re.search(
                    r'exec request failed|fork(?: failed)?: (?:retry: )?resource temporarily unavailable',
                    diagnostic, re.IGNORECASE)):
                raise GuestUnavailable()
            require(len(output) <= 16384 and code == 0, 'guest delivery failed')
        except BrokenPipeError:
            code = process.wait()
            errors.seek(0)
            diagnostic = errors.read(16384).decode('utf-8', errors='replace')
            if code == 255 or re.search(
                    r'exec request failed|fork(?: failed)?: (?:retry: )?resource temporarily unavailable',
                    diagnostic, re.IGNORECASE):
                raise GuestUnavailable()
            raise ValueError('guest delivery failed')
        finally:
            if process.poll() is None:
                process.kill(); process.wait()
        receipt = json.loads(output)
        require(receipt['files'] == expected['files'] and len(receipt['pins']) == 2
                and receipt['paths'] == [str(run / 'vm-unit-inputs' / n)
                                        for n in ('initial-adoption-fixture.json', 'meguri-cli')], 'delivery receipt mismatch')
        require(checked(run) == value, 'inputs changed during delivery')
        r.write(run / 'xcode-unit-delivery.json', receipt)


def main():
    action = sys.argv[1]
    run = admitted(sys.argv[2])
    if action == 'start' and len(sys.argv) == 3:
        for leaf in ('initial-adoption-fixture.json', 'cloud-service-code', 'cloud-service-code.json',
                     'cloud-service-build.jsonl', 'initial-adoption-producer.json',
                     'xcode-unit-inputs.json', 'xcode-unit-delivery.json'):
            require(not os.path.lexists(run / leaf), 'fresh publication required')
        r.write(run / 'xcode-unit-source.json', source())
    elif action == 'verify-inputs' and len(sys.argv) == 4:
        verify_inputs(run, Path(sys.argv[3]))
    elif action == 'deliver' and len(sys.argv) == 4:
        deliver(run, sys.argv[3])
    elif action == 'command' and len(sys.argv) == 3:
        value = checked(run)
        receipt, _ = record(run / 'xcode-unit-delivery.json')
        require(receipt['files'] == [value[k]['identity'] for k in ('fixture', 'helper')], 'delivery input mismatch')
        print(guest_command(run, 'launch', receipt))
    else:
        raise ValueError()


if __name__ == '__main__':
    try:
        main()
    except GuestUnavailable:
        print('xcode_project_check: Mac Unit input VM delivery unavailable', file=sys.stderr)
        sys.exit(72)
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError):
        print('xcode_project_check: Mac Unit input admission failed', file=sys.stderr)
        sys.exit(70)
