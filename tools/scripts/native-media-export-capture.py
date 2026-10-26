#!/usr/bin/env python3
"""Bounded E7.4 destination capture; no package production or content proof."""
import hashlib
import json
import os
import re
import secrets
import stat
import subprocess
import sys
from contextlib import ExitStack, contextmanager
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PARENT = ROOT / 'build/fixtures/recipes/typical-1'
CLOCK = {'now': '2030-01-15T10:00:00+01:00', 'tz': 'Europe/Paris'}
METHOD = 'CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace()'
PHASE = 'export'
UUID = r'[A-Fa-f0-9]{8}(?:-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical(path):
    require(path.is_absolute() and path.resolve(strict=True) == path,
            'missing or noncanonical capture path')


def own_run(value):
    run = Path(value)
    require(run.parent == ROOT / 'build/mg/ui-test'
            and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', run.name),
            'capture requires this checkout and its own ui-test run')
    canonical(run)
    return run


def package_name(value):
    require(re.fullmatch(r'Meguri-Media-[a-f0-9]{32}\.colpkg', value),
            "expected this run's safe media package filename")
    return value


def simulator_source(container, device, filename):
    filename = package_name(filename)
    require(re.fullmatch(UUID, device), 'invalid leased simulator identifier')
    require(re.fullmatch(r'/Users/[A-Za-z0-9._-]+/Library/Developer/CoreSimulator/Devices/'
                         + re.escape(device) + r'/data/Containers/Data/Application/' + UUID,
                         container, re.IGNORECASE), 'unexpected simulator container shape')
    # Files keeps the product filename; only this run's validated nonce child
    # may be read. Never inspect or enumerate the surrounding app container.
    return (Path(container) / 'Documents/RF2 Import Tests'
            / Path(filename).stem / 'Meguri-Collection.colpkg')


def node_identity(metadata):
    return metadata.st_dev, metadata.st_ino, metadata.st_mode, metadata.st_nlink


def directory_identity(metadata):
    # Directory link counts can change when ordinary files are added (APFS).
    require(stat.S_ISDIR(metadata.st_mode), 'expected a directory')
    return metadata.st_dev, metadata.st_ino, metadata.st_mode


def file_metadata(metadata):
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1,
            'expected a regular file with one link')
    return (*node_identity(metadata), metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns)


@contextmanager
def retained_directory(path):
    """Walk only named components, retaining nofollow authority at every level."""
    require(path.is_absolute() and '..' not in path.parts, 'expected an absolute directory')
    with ExitStack() as stack:
        parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        stack.callback(os.close, parent)
        links = []
        for name in path.parts[1:]:
            child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
            stack.callback(os.close, child)
            identity = directory_identity(os.fstat(child))
            links.append((parent, name, child, identity))
            parent = child

        def verify():
            for directory, name, child, identity in links:
                require(directory_identity(os.fstat(child)) == identity
                        and directory_identity(os.stat(name, dir_fd=directory, follow_symlinks=False)) == identity,
                        'capture directory was replaced')

        verify()
        yield parent, verify
        verify()


@contextmanager
def retained_file(directory, name):
    # NONBLOCK prevents a substituted FIFO from blocking before fstat refuses it.
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
    try:
        before = file_metadata(os.fstat(fd))

        def verify():
            require(file_metadata(os.fstat(fd)) == before
                    and file_metadata(os.stat(name, dir_fd=directory, follow_symlinks=False)) == before,
                    'capture file was replaced or mutated')

        verify()
        yield fd, verify
        verify()
    finally:
        os.close(fd)


def hash_fd(fd):
    digest = hashlib.sha256()
    size = 0
    for block in iter(lambda: os.read(fd, 1024 * 1024), b''):
        digest.update(block)
        size += len(block)
    return {'sha256': digest.hexdigest(), 'bytes': size}


def file_identity(path):
    with retained_directory(path.parent) as (directory, _):
        with retained_file(directory, path.name) as (fd, verify):
            identity = hash_fd(fd)
            verify()
            require(identity['bytes'] == os.fstat(fd).st_size, 'capture file size changed')
    return identity


def copy_package(source, package):
    """Copy exactly one retained regular source into an exclusive own-run file."""
    with retained_directory(source.parent) as (source_dir, verify_source_dir):
        with retained_file(source_dir, source.name) as (source_fd, verify_source):
            copy_stream(source_fd, verify_source, verify_source_dir, package)


def copy_stream(source_fd, verify_source, verify_source_dir, package, *, rehash_source=False, allow_empty=False):
    """Stream a pinned source; Mac also verifies its retained attachment twice."""
    with retained_directory(package.parent) as (output_dir, verify_output_dir):
        output_fd = os.open(package.name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                            0o600, dir_fd=output_dir)
        owned = node_identity(os.fstat(output_fd))
        try:
            digest = hashlib.sha256()
            size = 0
            for block in iter(lambda: os.read(source_fd, 1024 * 1024), b''):
                digest.update(block)
                size += len(block)
                remaining = memoryview(block)
                while remaining:
                    written = os.write(output_fd, remaining)
                    require(written > 0, 'native destination write made no progress')
                    remaining = remaining[written:]
            verify_source()
            verify_source_dir()
            require((allow_empty or size > 0) and size == os.fstat(source_fd).st_size,
                    'missing package or changed source size')
            os.fsync(output_fd)
            output_before = file_metadata(os.fstat(output_fd))
            os.lseek(output_fd, 0, os.SEEK_SET)
            require(hash_fd(output_fd) == {'sha256': digest.hexdigest(), 'bytes': size},
                    'native destination digest or size differs')
            require(file_metadata(os.fstat(output_fd)) == output_before
                    and file_metadata(os.stat(package.name, dir_fd=output_dir,
                                              follow_symlinks=False)) == output_before,
                    'native destination was replaced or mutated')
            verify_source()
            verify_source_dir()
            if rehash_source:
                os.lseek(source_fd, 0, os.SEEK_SET)
                require(hash_fd(source_fd) == {'sha256': digest.hexdigest(), 'bytes': size},
                        'runner capture source digest or size changed')
                verify_source()
                verify_source_dir()
                require(file_metadata(os.fstat(output_fd)) == output_before
                        and file_metadata(os.stat(package.name, dir_fd=output_dir,
                                                  follow_symlinks=False)) == output_before,
                        'native destination changed during source rehash')
            verify_output_dir()
        except BaseException:
            # Remove only the file we created, never a substituted name.
            try:
                if node_identity(os.stat(package.name, dir_fd=output_dir,
                                         follow_symlinks=False)) == owned:
                    os.unlink(package.name, dir_fd=output_dir)
            except FileNotFoundError:
                pass
            raise
        finally:
            os.close(output_fd)


def attachment_basename(value):
    require(isinstance(value, str)
            and re.fullmatch(UUID + r'\.colpkg', value),
            'expected an exported package basename')
    return value


def mac_capture(run, filename, bundle):
    """Export only the native case's one named package from this run's result."""
    global PHASE
    PHASE = 'export'
    filename = package_name(filename)
    require(bundle == run / 'mac.xcresult', "expected this run's Mac result bundle")
    canonical(bundle)
    destination = run / 'native-media-export' / filename
    exported = run / 'native-media-attachments'
    # Exclusive creation refuses stale, foreign or linked export directories.
    exported.mkdir()
    with retained_directory(run) as (run_fd, verify_run):
        with retained_file(run_fd, 'expected-source.json') as (expected_fd, verify_expected):
            with os.fdopen(os.dup(expected_fd)) as source:
                expected = json.load(source)
                require(isinstance(expected, dict) and expected.get('native_filename') == filename,
                        'attachment filename is not owned by this run')
            verify_expected()
            with retained_directory(bundle) as (_, verify_bundle), \
                    retained_directory(exported) as (directory, verify_directory):
                subprocess.run([
                    'xcrun', 'xcresulttool', 'export', 'attachments',
                    '--path', str(bundle), '--output-path', str(exported),
                    '--test-id', METHOD, '--filter', '*.colpkg',
                ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                verify_bundle()
                verify_directory()
                verify_run()
                PHASE = 'manifest'
                with retained_file(directory, 'manifest.json') as (manifest_fd, verify_manifest):
                    with os.fdopen(os.dup(manifest_fd)) as source:
                        manifest = json.load(source)
                    require(isinstance(manifest, list) and len(manifest) == 1,
                            'expected exactly one attachment test record')
                    record = manifest[0]
                    require(isinstance(record, dict) and record.get('testIdentifier') == METHOD,
                            'attachment belongs to a different method')
                    attachments = record.get('attachments')
                    require(isinstance(attachments, list) and len(attachments) == 1
                            and isinstance(attachments[0], dict),
                            'expected exactly one native package attachment')
                    attachment = attachments[0]
                    basename = attachment_basename(attachment.get('exportedFileName'))
                    require(attachment.get('isAssociatedWithFailure') is False,
                            'failure attachment is not native destination evidence')
                    # XCTest replaces the extension with an index and independent UUID.
                    # Require the run's exact stem, delimiter and complete suffix.
                    human_name = attachment.get('suggestedHumanReadableName')
                    require(isinstance(human_name, str)
                            and re.fullmatch(re.escape(Path(filename).stem)
                                             + '_0_' + UUID + r'\.colpkg', human_name),
                            "attachment does not match this run's exact filename")

                    def verify_exports():
                        verify_directory()
                        verify_run()
                        verify_expected()
                        verify_manifest()
                        require(set(os.listdir(directory)) == {'manifest.json', basename},
                                'unexpected selective attachment exports')

                    verify_exports()
                    with retained_file(directory, basename) as (fd, verify_source):
                        require(os.fstat(fd).st_size > 0, 'empty native attachment')
                        PHASE = 'copy'
                        copy_stream(fd, verify_source, verify_exports, destination,
                                    rehash_source=True)


def fixture_files(fixture, copy_to=None):
    """Retain nofollow tree/file authority; copy only closed, owned inputs."""
    files = {}
    with retained_directory(fixture) as (directory, verify_root):
        def walk(fd, relative, output):
            require(os.fstat(fd).st_uid == os.getuid(), 'fixture directory is not owned')
            names = sorted(os.listdir(fd))
            for name in names:
                require(not name.endswith(('-wal', '-shm', '-journal')), 'fixture is not closed')
                child = relative / name
                metadata = os.stat(name, dir_fd=fd, follow_symlinks=False)
                require(metadata.st_uid == os.getuid(), 'fixture file is not owned')
                if stat.S_ISDIR(metadata.st_mode):
                    with retained_directory(fixture / child) as (nested, verify_nested):
                        require(directory_identity(os.fstat(nested)) == directory_identity(metadata),
                                'fixture directory changed')
                        target = None if output is None else output / name
                        if target is not None:
                            with retained_directory(output) as (target_parent, _):
                                os.mkdir(name, mode=0o700, dir_fd=target_parent)
                        walk(nested, child, target)
                        verify_nested()
                else:
                    with retained_file(fd, name) as (source, verify_source):
                        files[child.as_posix()] = hash_fd(source)
                        if output is not None:
                            os.lseek(source, 0, os.SEEK_SET)
                            copy_stream(source, verify_source, verify_root, output / name,
                                        rehash_source=True, allow_empty=True)
                        verify_source()
            require(names == sorted(os.listdir(fd)), 'fixture entries changed')
        walk(directory, Path(), copy_to)
        verify_root()
    require({'manifest.json', 'collection.anki2'} <= files.keys()
            and any(name.startswith('collection.media/') for name in files),
            'generate typical-1 before ui-test')
    return files


def read_record(path):
    with retained_directory(path.parent) as (directory, _):
        with retained_file(directory, path.name) as (fd, verify):
            require(os.fstat(fd).st_size <= 16 * 1024 * 1024, 'record too large')
            with os.fdopen(os.dup(fd)) as source:
                record = json.load(source)
            verify()
    return record


def stage_fixture(run):
    parent = fixture_files(PARENT)
    staged = run / 'native-media-parent'
    with retained_directory(run) as (run_fd, verify_run):
        os.mkdir(staged.name, mode=0o700, dir_fd=run_fd)
        with retained_directory(staged) as (_, verify_staged):
            fixture_files(PARENT, copy_to=staged)
            verify_staged()
        require(fixture_files(staged) == parent == fixture_files(PARENT), 'generated parent changed')
        os.mkdir('native-media-baseline', mode=0o700, dir_fd=run_fd)
        out = os.open('native-media-parent.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                      0o600, dir_fd=run_fd)
        with os.fdopen(out, 'w') as output:
            json.dump({'fixture': PARENT.relative_to(ROOT).as_posix(), 'files': parent,
                       'manifest': read_record(PARENT / 'manifest.json')}, output, sort_keys=True)
        verify_run()


def prepared_fixture(run):
    return run / 'native-media-baseline/fixture'


def fixture_identity(run):
    parent = read_record(run / 'native-media-parent.json')
    require(parent == {'fixture': PARENT.relative_to(ROOT).as_posix(),
                       'files': fixture_files(PARENT),
                       'manifest': read_record(PARENT / 'manifest.json')}, 'generated parent changed')
    require(fixture_files(run / 'native-media-parent') == parent['files'], 'staged parent changed')
    preparation = run / 'native-media-baseline/preparation.json'
    record = read_record(preparation)
    fixture = prepared_fixture(run)
    files = fixture_files(fixture)
    require(record['parent_files'] == parent['files'] and record['prepared_files'] == files
            and record['clock'] == CLOCK, 'unbound native media preparation')
    row = record['inserted_row']
    require(set(row) == {'KEY', 'usn', 'mtime_secs', 'val_hex', 'storage_class'}
            and row['KEY'] == 'localOffset' and type(row['usn']) is int and row['usn'] == -1
            and type(row['mtime_secs']) is int and row['val_hex'] == '2d3630'
            and row['storage_class'] == 'blob', 'unexpected native clock baseline')
    require(files.keys() == parent['files'].keys()
            and all(files[name] == identity for name, identity in parent['files'].items()
                    if name != 'collection.anki2'), 'non-database fixture bytes changed')
    return {'fixture': fixture.relative_to(ROOT).as_posix(), 'files': files,
            'parent': parent, 'manifest_provenance': 'parent', 'clock': CLOCK,
            'inserted_row': row, 'preparation_identity': file_identity(preparation)}


def main():
    global PHASE
    action, value, *args = sys.argv[1:]
    run = own_run(value)
    expected = run / 'expected-source.json'
    destination = run / 'native-media-export'
    if action == 'stage':
        require(not args, 'unexpected staging arguments')
        stage_fixture(run)
        return
    if action == 'identity':
        require(not args, 'unexpected identity arguments')
        require(not destination.exists() and not destination.is_symlink(), 'output already exists')
        destination.mkdir()
    canonical(destination)
    if action == 'identity':
        filename = package_name(f'Meguri-Media-{secrets.token_hex(16)}.colpkg')
        require(not expected.exists() and not expected.is_symlink(), 'source identity already exists')
        # Freeze hashes before VM handoff. Keep copyable file modes: the
        # unchanged installer preserves them in the app's writable collection.
        frozen = dict(fixture_identity(run), native_filename=filename)
        with retained_directory(run) as (directory, _):
            fd = os.open(expected.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o400, dir_fd=directory)
            with os.fdopen(fd, 'w') as output:
                output.write(json.dumps(frozen, sort_keys=True, indent=2) + '\n')
        print(filename)
    elif action == 'mac':
        filename, bundle = args
        mac_capture(run, filename, Path(bundle))
    elif action == 'simulator':
        container, device, filename = args
        filename = package_name(filename)
        source = simulator_source(container, device, filename)
        copy_package(source, destination / filename)
    elif action == 'receipt':
        PHASE = 'receipt'
        filename, = args
        filename = package_name(filename)
        canonical(expected)
        require(read_record(expected) == dict(fixture_identity(run), native_filename=filename),
                'prepared fixture, generated parent or owned filename changed during the native case')
        identity = file_identity(destination / filename)
        require(identity['bytes'] > 0, 'native destination package is empty')
        receipt = {'native_filename': filename, 'native_destination': identity,
                   'expected_source': 'expected-source.json',
                   'expected_source_identity': file_identity(expected),
                   'fixture': prepared_fixture(run).relative_to(ROOT).as_posix(),
                   'fixture_identity': fixture_identity(run),
                   'external_content_client_proof': 'pending', 'e7_4_acceptance': 'pending'}
        with (run / 'native-media-export.json').open('x') as output:
            output.write(json.dumps(receipt, sort_keys=True, indent=2) + '\n')
    else:
        raise ValueError('unknown capture action')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        # Fixed vocabulary only: never expose subprocess output or source paths.
        sys.exit('FIXED-PHASE:' + PHASE)
