# SPDX-License-Identifier: MIT
"""Exercise the context assembler with public, wholly synthetic tar inputs."""
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
    def assemble(self, extra, rejected=None):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            inputs = dict.fromkeys(FIXED, b'synthetic public input\n')
            inputs.update({'server/about.toml': b'accepted = ["MPL-2.0"]\n',
                           'core/rust-toolchain.toml': b'channel = "1.99.0"\n',
                           'server/rust-toolchain.toml': b'channel = "1.99.0"\n',
                           'server/web/.gitignore': b'node_modules\ndist\n'})
            inputs.update(extra)
            with tarfile.open(root / 'source.tar', 'w') as archive:
                for name, data in inputs.items():
                    item = tarfile.TarInfo(name)
                    item.size = len(data)
                    archive.addfile(item, io.BytesIO(data))
            (root / 'notices.json').write_text(json.dumps({'licenses': [
                {'id': 'MPL-2.0', 'text': 'Synthetic license text', 'used_by': ['synthetic']}
            ]}))
            result = subprocess.run([sys.executable, ASSEMBLER, str(root / 'source.tar'),
                                     str(root / 'notices.json'), str(root / 'context'), 'synthetic'],
                                    capture_output=True, text=True)
            if rejected:
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Forbidden build-context input', result.stderr)
                self.assertFalse((root / 'context' / rejected).exists())
            else:
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((root / 'context/server/web/.gitignore').read_bytes(),
                                 inputs['server/web/.gitignore'])

    def test_public_gitignore(self):
        self.assemble({})

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
