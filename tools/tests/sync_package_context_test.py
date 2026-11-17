# SPDX-License-Identifier: MIT
"""Exercise the context assembler with public, wholly synthetic tar inputs."""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

ASSEMBLER = sys.argv.pop()
FIXED = '''server/Dockerfile server/Dockerfile.dockerignore server/Cargo.toml
server/Cargo.lock server/rust-toolchain.toml server/build.rs server/LICENSE
server/NOTICE server/about.toml server/examples/package-native-smoke.rs
server/tools/package-smoke.mjs core/Cargo.toml core/rust-toolchain.toml'''.split()


class ContextProof(unittest.TestCase):
    def assemble(self, extra, rejected=None, licenses=None, license_error=None):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            inputs = dict.fromkeys(FIXED, b'synthetic public input\n')
            inputs.update({'server/about.toml': b'accepted = ["MIT"]\n',
                           'core/rust-toolchain.toml': b'channel = "1.99.0"\n',
                           'server/rust-toolchain.toml': b'channel = "1.99.0"\n',
                           'server/web/.gitignore': b'node_modules\ndist\n'})
            inputs.update(extra)
            with tarfile.open(root / 'source.tar', 'w') as archive:
                for name, data in inputs.items():
                    item = tarfile.TarInfo(name)
                    item.size = len(data)
                    archive.addfile(item, io.BytesIO(data))
            if licenses is None:
                licenses = [{'id': 'MIT', 'text': 'Synthetic license text', 'used_by': ['synthetic']}]
            notice_bytes = json.dumps({'licenses': licenses}).encode()
            (root / 'notices.json').write_bytes(notice_bytes)
            result = subprocess.run([sys.executable, ASSEMBLER, str(root / 'source.tar'),
                                     str(root / 'notices.json'), str(root / 'context'), 'synthetic'],
                                    capture_output=True, text=True)
            if rejected:
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Forbidden build-context input', result.stderr)
                self.assertFalse((root / 'context' / rejected).exists())
            elif license_error:
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(license_error, result.stderr)
                for output in ('rust-notices.json', 'covered-source.tar', 'package-inputs.json'):
                    self.assertFalse((root / 'context' / output).exists())
            else:
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((root / 'context/server/web/.gitignore').read_bytes(),
                                 inputs['server/web/.gitignore'])
                self.assertEqual((root / 'context/rust-notices.json').read_bytes(), notice_bytes)
                manifest = json.loads((root / 'context/package-inputs.json').read_text())
                for field, path in [('source_archive_sha256', root / 'source.tar'),
                                    ('rust_notices_sha256', root / 'notices.json'),
                                    ('covered_source_sha256', root / 'context/covered-source.tar')]:
                    self.assertEqual(manifest[field], hashlib.sha256(path.read_bytes()).hexdigest())
                self.assertEqual(manifest['inputs'], [
                    dict(path=name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
                    for name, data in sorted(inputs.items())])
                with tarfile.open(root / 'context/covered-source.tar') as source:
                    model_inputs = {name: data for name, data in inputs.items()
                                    if name.startswith('core/crates/meguri-model/')
                                    or name in {'core/Cargo.toml', 'core/rust-toolchain.toml'}}
                    self.assertEqual(source.getnames(), list(model_inputs))
                    for name, data in model_inputs.items():
                        self.assertEqual(source.extractfile(name).read(), data)

    def test_public_gitignore(self):
        self.assemble({})

    def test_mit_only_context(self):
        self.assemble({'core/crates/meguri-model/src/synthetic.rs': b'// Synthetic public model input\n'})

    def test_empty_license_inventory(self):
        self.assemble({}, licenses=[], license_error='Missing Cargo license inventory')

    def test_invalid_license_entries(self):
        valid = dict(id='MIT', text='Synthetic license text', used_by=['synthetic'])
        for field, values in [('id', ['MIT OR Apache-2.0']),
                              ('text', [None, '', ' \n']), ('used_by', [None, []])]:
            for value in values:
                with self.subTest(field=field, value=value):
                    license = valid.copy()
                    if value is None:
                        del license[field]
                    else:
                        license[field] = value
                    self.assemble({}, licenses=[license],
                                  license_error='Missing or unapproved Cargo license')

    def test_context_allowlist(self):
        path = 'server/unlisted.txt'
        self.assemble({path: b'Synthetic public input\n'}, path)

    def test_recognizable_credentials(self):
        for name in ['id_rsa', 'tls/private-key.pem', 'tls/key.pem', 'service.key',
                     'credentials.json', '.env', '.npmrc', '.ssh/config']:
            with self.subTest(name=name):
                path = 'server/web/' + name
                self.assemble({path: b'SYNTHETIC credential filename fixture\n'}, path)

    def test_private_key_under_public_looking_name(self):
        path = 'server/web/public-input.txt'
        self.assemble({path: b'-----BEGIN PRIVATE KEY-----\nSYNTHETIC-NOT-A-KEY\n'}, path)


if __name__ == '__main__':
    unittest.main()
