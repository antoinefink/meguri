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
FIXTURE = ROOT / 'build/fixtures/recipes/typical-1'
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


def node_identity(metadata):
    return metadata.st_dev, metadata.st_ino, metadata.st_mode, metadata.st_nlink


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
            identity = node_identity(os.fstat(child))
            links.append((parent, name, child, identity))
            parent = child

        def verify():
            for directory, name, child, identity in links:
                require(node_identity(os.fstat(child)) == identity
                        and node_identity(os.stat(name, dir_fd=directory, follow_symlinks=False)) == identity,
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


def copy_stream(source_fd, verify_source, verify_source_dir, package, *, rehash_source=False):
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
            require(size > 0 and size == os.fstat(source_fd).st_size,
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
                    '--test-id', METHOD, '--filter', '*' + filename + '*',
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
                    # XCTest appends an index and UUID to the exact attachment name.
                    # Require the delimiter and complete suffix, never a substring.
                    require(attachment.get('suggestedHumanReadableName')
                            == filename + '_0_' + basename,
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


def fixture_identity():
    canonical(FIXTURE)
    require((FIXTURE / 'manifest.json').is_file()
            and (FIXTURE / 'collection.anki2').is_file()
            and (FIXTURE / 'collection.media').is_dir(), 'generate typical-1 before ui-test')
    files = {}
    for path in sorted(FIXTURE.rglob('*')):
        canonical(path)
        require(not path.name.endswith(('-wal', '-shm', '-journal')),
                'expected the closed original generated fixture')
        if not path.is_dir():
            files[path.relative_to(FIXTURE).as_posix()] = file_identity(path)
    require(any(name.startswith('collection.media/') for name in files), 'typical-1 must contain media')
    return {'fixture': 'build/fixtures/recipes/typical-1', 'files': files}


def main():
    global PHASE
    action, value, *args = sys.argv[1:]
    run = own_run(value)
    expected = run / 'expected-source.json'
    destination = run / 'native-media-export'
    if action == 'identity':
        require(not args, 'unexpected identity arguments')
        require(not destination.exists() and not destination.is_symlink(), 'output already exists')
        destination.mkdir()
    canonical(destination)
    if action == 'identity':
        filename = package_name(f'Meguri-Media-{secrets.token_hex(16)}.colpkg')
        require(not expected.exists() and not expected.is_symlink(), 'source identity already exists')
        with expected.open('x') as output:
            output.write(json.dumps(dict(fixture_identity(), native_filename=filename), sort_keys=True, indent=2) + '\n')
        print(filename)
    elif action == 'mac':
        filename, bundle = args
        mac_capture(run, filename, Path(bundle))
    elif action == 'simulator':
        container, device, filename = args
        filename = package_name(filename)
        require(re.fullmatch(UUID, device), 'invalid leased simulator identifier')
        require(re.fullmatch(r'/Users/[A-Za-z0-9._-]+/Library/Developer/CoreSimulator/Devices/'
                             + re.escape(device) + r'/data/Containers/Data/Application/' + UUID,
                             container, re.IGNORECASE), 'unexpected simulator container shape')
        source = Path(container) / 'Documents/RF2 Import Tests' / filename
        copy_package(source, destination / filename)
    elif action == 'receipt':
        PHASE = 'receipt'
        filename, = args
        filename = package_name(filename)
        canonical(expected)
        require(json.loads(expected.read_text()) == dict(fixture_identity(), native_filename=filename),
                'original generated fixture or owned filename changed during the native case')
        identity = file_identity(destination / filename)
        require(identity['bytes'] > 0, 'native destination package is empty')
        receipt = {'native_filename': filename, 'native_destination': identity,
                   'expected_source': 'expected-source.json',
                   'expected_source_identity': file_identity(expected),
                   'external_content_client_proof': 'pending', 'e7_4_acceptance': 'pending'}
        with (run / 'native-media-export.json').open('x') as output:
            output.write(json.dumps(receipt, sort_keys=True, indent=2) + '\n')
    else:
        raise ValueError('unknown capture action')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError):
        # Fixed vocabulary only: never expose subprocess output or source paths.
        sys.exit('FIXED-PHASE:' + PHASE)
