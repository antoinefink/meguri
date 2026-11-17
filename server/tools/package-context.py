# SPDX-License-Identifier: MIT
"""Assemble only frozen public build inputs; reuse cargo-about's full notices."""
import hashlib
import io
import json
import pathlib
import re
import sys
import tarfile

archive, notices, destination, commit = sys.argv[1:]
root = pathlib.Path(destination)
root.mkdir()
fixed = {
    'server/Dockerfile', 'server/Dockerfile.dockerignore', 'server/Cargo.toml',
    'server/Cargo.lock', 'server/rust-toolchain.toml', 'server/build.rs',
    'server/LICENSE', 'server/NOTICE', 'server/about.toml',
    'server/examples/package-native-smoke.rs', 'server/tools/package-smoke.mjs',
    'core/Cargo.toml', 'core/rust-toolchain.toml',
}
prefixes = ('server/src/', 'server/web/', 'core/crates/meguri-model/')
forbidden = {'.git', '.agents', '.config', 'refs', 'oracle', 'local-data', 'node_modules',
             'target', 'dist', '.test-build', 'playwright-report', 'test-results'}
# Existing hidden-file exclusions also apply. These recognizable nonhidden
# credential/key names must not enter the recursive public source trees.
credential_names = {'id_rsa', 'id_dsa', 'id_ecdsa', 'id_ed25519', 'identity',
                    'credentials', 'credentials.json', 'credentials.toml',
                    'credentials.yaml', 'credentials.yml', 'key.pem',
                    'private-key.pem', 'private_key.pem', 'privkey.pem'}
key_suffixes = {'.key', '.p12', '.pfx', '.jks', '.keystore'}
private_key = re.compile(rb'-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----')
inputs = []
covered = io.BytesIO()
with tarfile.open(archive) as source, tarfile.open(fileobj=covered, mode='w') as mpl:
    for item in source:
        path = pathlib.PurePosixPath(item.name)
        if item.isdir():
            continue
        if (not item.isfile() or path.is_absolute() or '..' in path.parts
                or forbidden.intersection(path.parts)
                or any(part.lower() in credential_names for part in path.parts)
                or path.suffix.lower() in key_suffixes
                or (any(part.startswith('.') for part in path.parts) and item.name != 'server/web/.gitignore')
                or not (item.name in fixed or item.name.startswith(prefixes))):
            raise ValueError('Forbidden build-context input: ' + item.name)
        data = source.extractfile(item).read()
        if private_key.search(data):
            raise ValueError('Forbidden build-context input: ' + item.name)
        output = root / item.name
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(data)
        inputs.append(dict(path=item.name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest()))
        if item.name.startswith('core/crates/meguri-model/') or item.name in {'core/Cargo.toml', 'core/rust-toolchain.toml'}:
            mpl.addfile(item, io.BytesIO(data))
if not fixed.issubset({row['path'] for row in inputs}):
    raise ValueError('Missing fixed build inputs')
for pin in ('core/rust-toolchain.toml', 'server/rust-toolchain.toml'):
    if not re.search(r'channel\s*=\s*"1\.99\.0"', (root / pin).read_text()):
        raise ValueError('Docker Rust pin differs from workspace')
about = (root / 'server/about.toml').read_text()
accepted = json.loads(re.search(r'accepted\s*=\s*(\[[^\]]+\])', about).group(1))
notice_bytes = pathlib.Path(notices).read_bytes()
licenses = json.loads(notice_bytes)['licenses']
if not licenses:
    raise ValueError('Missing Cargo license inventory')
for license in licenses:
    if license['id'] not in accepted or not license.get('text', '').strip() or not license.get('used_by'):
        raise ValueError('Missing or unapproved Cargo license')
(root / 'rust-notices.json').write_bytes(notice_bytes)
(root / 'covered-source.tar').write_bytes(covered.getvalue())
manifest = dict(source_commit=commit, features=[], profile='release',
                source_archive_sha256=hashlib.sha256(pathlib.Path(archive).read_bytes()).hexdigest(),
                rust_notices_sha256=hashlib.sha256(notice_bytes).hexdigest(),
                covered_source_sha256=hashlib.sha256(covered.getvalue()).hexdigest(),
                inputs=sorted(inputs, key=lambda row: row['path']))
(root / 'package-inputs.json').write_text(json.dumps(manifest, indent=2) + '\n')
