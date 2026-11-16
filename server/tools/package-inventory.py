# SPDX-License-Identifier: MIT
"""Check an actual exported runtime, retaining hashes of every regular file."""
import hashlib
import json
import pathlib
import sys
import tarfile

archive, context, output = sys.argv[1:]
root = pathlib.Path(context)
out = pathlib.Path(output)
out.mkdir()
files = {}
# These exact entries were observed in the actual Docker container export.
# They describe empty mount stubs and filesystem layout, not shipped content.
export_entries = {name: (tarfile.REGTYPE, 0o755, 0, 0, 0, '', 0, 0)
                  for name in ['.dockerenv', 'dev/console', 'etc/hostname', 'etc/hosts', 'etc/resolv.conf']}
export_entries['etc/mtab'] = (tarfile.SYMTYPE, 0o777, 0, 0, 0, '/proc/mounts', 0, 0)
export_entries.update({name: (tarfile.DIRTYPE, 0o700, 10001, 10001, 0, '', 0, 0)
                       for name in ['data', 'data/owner']})
export_entries.update({name: (tarfile.DIRTYPE, 0o755, 0, 0, 0, '', 0, 0)
                       for name in ['dev', 'dev/pts', 'dev/shm', 'etc', 'proc', 'sys', 'usr',
                                    'usr/local', 'usr/local/bin', 'usr/share', 'usr/share/meguri-sync',
                                    'usr/share/meguri-sync/licenses', 'usr/share/meguri-sync/licenses/rust',
                                    'usr/share/meguri-sync/web']})
accepted_export_entries = {}
with tarfile.open(archive) as image:
    for item in image:
        name = item.name[2:] if item.name.startswith('./') else item.name
        if name in export_entries:
            metadata = (item.type, item.mode, item.uid, item.gid, item.size,
                        item.linkname, item.devmajor, item.devminor)
            if metadata != export_entries[name]:
                raise ValueError('Unexpected runtime export metadata: ' + name)
            accepted_export_entries[name] = dict(type=item.type.decode('ascii'), mode=item.mode,
                                                uid=item.uid, gid=item.gid, size=item.size,
                                                linkname=item.linkname, devmajor=item.devmajor,
                                                devminor=item.devminor)
            # Validate the link as metadata only; never follow /proc/mounts.
            if item.isdir() or item.issym():
                continue
        if item.isdir():
            raise ValueError('Unexpected runtime directory: ' + name)
        if not item.isfile():
            raise ValueError('Nonregular runtime input: ' + name)
        if not (name == 'usr/local/bin/meguri-sync-server' or name.startswith('usr/share/meguri-sync/')
                or name in accepted_export_entries):
            raise ValueError('Unexpected runtime file: ' + name)
        data = image.extractfile(item).read()
        files[name] = dict(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
        if name.startswith('usr/share/meguri-sync/'):
            relative = name[len('usr/share/meguri-sync/'):]
            target = out / relative
            if '..' in pathlib.PurePosixPath(relative).parts:
                raise ValueError('Invalid runtime path')
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
base = 'usr/share/meguri-sync/'
for source, runtime in [('server/LICENSE', 'licenses/server-MIT.txt'), ('server/web/LICENSE', 'licenses/web-MIT.txt'),
                        ('server/web/THIRD_PARTY_NOTICES.txt', 'licenses/imported-components.txt'),
                        ('server/web/upstream-records.json', 'upstream-records.json'), ('server/NOTICE', 'NOTICE')]:
    if (out / runtime).read_bytes() != (root / source).read_bytes():
        raise ValueError('Runtime first-party/import notice differs: ' + runtime)
for name in ['package-inputs.json', 'rust-notices.json', 'covered-source.tar']:
    if (out / name).read_bytes() != (root / name).read_bytes():
        raise ValueError('Runtime license/source evidence differs: ' + name)
for name in ['NOTICE', 'licenses/server-MIT.txt', 'licenses/web-MIT.txt', 'licenses/imported-components.txt',
             'licenses/musl.txt', 'licenses/compiler-runtime.txt', 'licenses/GPL-3', 'licenses/rust/COPYRIGHT.html',
             'licenses/rust/COPYRIGHT-library.html', 'rust-notice-inputs.sha256', 'web/runtime-notices.txt',
             'web/dependency-notices.txt', 'web/dependency-inventory.json', 'assets.json', 'release.jsonl',
             'target.txt', 'production-features.txt', 'static-packages.txt', 'static-compiler.txt',
             'static-inputs.sha256', 'rustc.txt', 'elf.txt', 'executable.sha256', 'upstream-records.json']:
    if not files.get(base + name, {}).get('bytes'):
        raise ValueError('Missing runtime evidence: ' + name)
# Hashes are measured from the original pinned component, before copying, and
# retain its sysroot paths. Do not substitute one architecture's measured hashes.
rustc = dict(line.split(': ', 1) for line in (out / 'rustc.txt').read_text().splitlines() if ': ' in line)
if (rustc.get('release') != '1.99.0'
        or rustc.get('commit-hash') != 'b940084d7eb6a299eb4bfeb8e34901bc051e7ac4'
        or rustc.get('host') not in {'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu'}):
    raise ValueError('Rust notice compiler identity differs from pinned component')
rust_docs = '/usr/local/rustup/toolchains/1.99.0-' + rustc['host'] + '/share/doc/rust/'
notice_sources = {rust_docs + name: 'licenses/rust/' + name
                  for name in ['COPYRIGHT.html', 'COPYRIGHT-library.html']}
notice_inputs = (out / 'rust-notice-inputs.sha256').read_text().splitlines()
if len(notice_inputs) != len(notice_sources):
    raise ValueError('Incomplete original Rust notice hashes')
for line in notice_inputs:
    digest, source = line.split('  ', 1)
    if source not in notice_sources:
        raise ValueError('Unexpected or duplicate Rust notice source: ' + source)
    runtime = notice_sources.pop(source)
    if digest != files[base + runtime]['sha256']:
        raise ValueError('Runtime Rust notice differs from original component: ' + runtime)
artifacts = [json.loads(line) for line in (out / 'release.jsonl').read_text().splitlines()]
executables = [item for item in artifacts if item.get('reason') == 'compiler-artifact'
               and item.get('target', {}).get('name') == 'meguri-sync-server' and item.get('executable')]
if any('test-hooks' in item.get('features', []) for item in artifacts):
    raise ValueError('Test hooks enabled in production graph')
if files['usr/local/bin/meguri-sync-server']['sha256'] != (out / 'executable.sha256').read_text().strip():
    raise ValueError('Runtime executable differs from compiled release')
if len(executables) != 1 or executables[0]['features'] != [] or executables[0]['profile']['test']:
    raise ValueError('Invalid production artifact features')
if executables[0]['profile']['opt_level'] != '3' or executables[0]['profile']['debug_assertions']:
    raise ValueError('Not a release artifact')
if 'INTERP' in (out / 'elf.txt').read_text():
    raise ValueError('Dynamic runtime dependency')
# Only the required covered-source archive contains source. No source tree,
# Node, Python, reference runtime, test client, compiler or credential is present.
allowed_top = {'NOTICE', 'package-inputs.json', 'rust-notices.json', 'covered-source.tar', 'rustc.txt',
               'target.txt', 'production-features.txt', 'static-packages.txt', 'static-compiler.txt',
               'static-inputs.sha256', 'rust-notice-inputs.sha256', 'release.jsonl', 'elf.txt', 'assets.json',
               'executable.sha256', 'upstream-records.json'}
allowed_extra = {'web/runtime-notices.txt', 'web/dependency-notices.txt', 'web/dependency-inventory.json',
                 'licenses/server-MIT.txt', 'licenses/web-MIT.txt', 'licenses/imported-components.txt',
                 'licenses/musl.txt', 'licenses/compiler-runtime.txt', 'licenses/GPL-3',
                 'licenses/rust/COPYRIGHT.html', 'licenses/rust/COPYRIGHT-library.html'}
for name in files:
    if name.startswith(base):
        relative = name[len(base):]
        if relative not in allowed_top and relative not in allowed_extra:
            raise ValueError('Unexpected runtime material: ' + name)
for name, metadata in accepted_export_entries.items():
    files.setdefault(name, {}).update(metadata)
(out / 'runtime-inventory.json').write_text(json.dumps(files, indent=2) + '\n')
