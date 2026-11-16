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
with tarfile.open(archive) as image:
    for item in image:
        name = item.name[2:] if item.name.startswith('./') else item.name
        if item.isdir():
            continue
        if not item.isfile():
            raise ValueError('Nonregular runtime input: ' + name)
        if not (name == 'usr/local/bin/meguri-sync-server' or name.startswith('usr/share/meguri-sync/')
                or name in {'.dockerenv', 'etc/hosts', 'etc/hostname', 'etc/resolv.conf'}):
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
             'licenses/musl.txt', 'licenses/compiler-runtime.txt', 'licenses/GPL-3', 'licenses/rust/COPYRIGHT',
             'licenses/rust/LICENSE-MIT', 'licenses/rust/LICENSE-APACHE', 'web/runtime-notices.txt',
             'web/dependency-notices.txt', 'web/dependency-inventory.json', 'assets.json', 'release.jsonl',
             'target.txt', 'production-features.txt', 'static-packages.txt', 'static-compiler.txt',
             'static-inputs.sha256', 'rustc.txt', 'elf.txt', 'executable.sha256', 'upstream-records.json']:
    if not files.get(base + name, {}).get('bytes'):
        raise ValueError('Missing runtime evidence: ' + name)
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
               'static-inputs.sha256', 'release.jsonl', 'elf.txt', 'assets.json', 'executable.sha256', 'upstream-records.json'}
allowed_extra = {'web/runtime-notices.txt', 'web/dependency-notices.txt', 'web/dependency-inventory.json',
                 'licenses/server-MIT.txt', 'licenses/web-MIT.txt', 'licenses/imported-components.txt',
                 'licenses/musl.txt', 'licenses/compiler-runtime.txt', 'licenses/GPL-3',
                 'licenses/rust/COPYRIGHT', 'licenses/rust/LICENSE-APACHE', 'licenses/rust/LICENSE-MIT'}
for name in files:
    if name.startswith(base):
        relative = name[len(base):]
        if relative not in allowed_top and relative not in allowed_extra:
            raise ValueError('Unexpected runtime material: ' + name)
(out / 'runtime-inventory.json').write_text(json.dumps(files, indent=2) + '\n')
