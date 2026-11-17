# SPDX-License-Identifier: MIT
"""Exercise the context assembler with public, wholly synthetic tar inputs."""
import io
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
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


class FinalPackageProof(unittest.TestCase):
    """Public synthetic bytes exercise final admission, without image credit."""
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.artifacts = self.root / 'artifacts'
        self.artifacts.mkdir()
        helper = Path(ASSEMBLER).with_name('candidate-output.py')
        spec = importlib.util.spec_from_file_location('candidate_output', helper)
        self.helper = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.helper)
        self.commit, self.run = 'a' * 40, 'synthetic-final'
        self.image = 'sha256:' + hashlib.sha256(b'synthetic image configuration').hexdigest()
        source = {'server/Cargo.toml': b'version = "0.0.0"\n', 'server/Cargo.lock': b'synthetic lock\n'}
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode='w') as archive:
            for name, data in source.items():
                item = tarfile.TarInfo(name)
                item.size = len(data)
                archive.addfile(item, io.BytesIO(data))
        source_hash = self.store(raw.getvalue())
        executable = self.store(b'synthetic executable, never executed\n')
        inputs = dict(source_commit=self.commit, features=[], profile='release',
                      source_archive_sha256=source_hash, covered_source_sha256=self.store(b'synthetic covered source'),
                      rust_notices_sha256=self.store(b'synthetic notices'), inputs=[
                          dict(path=name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
                          for name, data in source.items()])
        inputs_hash = self.store(inputs)
        self.payloads = {
            'index.html': b'<!doctype html><script src="/assets/study-abcdefgh.js"></script>\n',
            'assets_study-abcdefgh.js': b'document.title = "Synthetic public study fixture";\n',
        }
        self.asset_manifest = [dict(path=path, sha256=self.store(self.payloads[name]),
                                    bytes=len(self.payloads[name])) for path, name in
                               [('/index.html', 'index.html'),
                                ('/assets/study-abcdefgh.js', 'assets_study-abcdefgh.js')]]
        assets = self.store(self.asset_manifest)
        runtime = {self.helper.EXECUTABLE: executable,
                   self.helper.RUNTIME_BASE + 'assets.json': assets,
                   self.helper.RUNTIME_BASE + 'package-inputs.json': inputs_hash,
                   self.helper.RUNTIME_BASE + 'covered-source.tar': inputs['covered_source_sha256'],
                   self.helper.RUNTIME_BASE + 'rust-notices.json': inputs['rust_notices_sha256']}
        inventory = {name: dict(sha256=sha, bytes=len((self.artifacts / sha).read_bytes()))
                     for name, sha in runtime.items()}
        inventory_hash = self.store(inventory)
        self.package = dict(image_id=self.image, inputs_sha256=inputs_hash,
                            runtime_inventory_sha256=inventory_hash, assets_sha256=assets,
                            image_inspection_sha256=self.store([dict(Id=self.image, Config=dict(
                                User='10001:10001', Entrypoint=['/' + self.helper.EXECUTABLE],
                                Labels={'org.opencontainers.image.revision': self.commit}))]))
        self.server = dict(kind='meguri_sync', version='0.0.0', build=self.commit, source_commit=self.commit,
                           source_sha256=source_hash, lock_sha256=self.store(source['server/Cargo.lock']),
                           executable_sha256=executable)
        self.receipt = dict(root='/synthetic/root', exit_code=0, failures=[], job='sync-server-package', run_id='synthetic-build', args=[], status='pass', dry_run=False,
                            commit=dict(sha=self.commit, dirty=False), details=dict(package=dict(
                                self.package, source_commit=self.commit, features=[], profile='release')))
        config = dict(User='10001:10001', Entrypoint=['/' + self.helper.EXECUTABLE],
                      Cmd=['serve', '/data/owner', 'https://candidate.example.test', '0.0.0.0:8080',
                           '--trusted-peer', '127.0.0.1'],
                      Labels={'meguri.mg.run-id': self.run, 'meguri.sync.owner': 'b' * 32})
        inspection = [dict(Id='c' * 64, Name='/meguri-local-' + self.run, Image=self.image,
                           Created='synthetic-created', State=dict(Running=True, StartedAt='synthetic-start'), Config=config,
                           HostConfig=dict(ReadonlyRootfs=True, NetworkMode='none', PortBindings={}, CapDrop=['ALL'],
                                           Privileged=False, SecurityOpt=['no-new-privileges:true']),
                           Mounts=[dict(Destination='/data', Type='volume', Name='meguri-sync-data-' + self.run, RW=True)])]
        self.binding = dict(format='meguri-sync-candidate-execution-v1', endpoint='http://127.0.0.1:8080/',
                            server=self.server, run_id=self.run, container_id='c' * 64,
                            labels={'meguri.mg.run-id': self.run}, observed_executable_sha256=executable,
                            package=self.package, server_process=dict(pid=7, executable_sha256=executable),
                            container_inspection_before_sha256=self.store(inspection),
                            container_inspection_after_sha256=self.store(inspection))
        self.peer = dict(format='meguri-sync-package-v1', kind='final_package', server=self.server,
                         source_dirty=False, profile='release', features=[], executable_artifact_sha256=executable,
                         lock_artifact_sha256=self.server['lock_sha256'], mg_build_receipt_sha256=self.store(self.receipt),
                         build_invocation=dict(job='sync-server-package', run_id='synthetic-build', arguments=[]),
                         execution_binding_sha256=self.store(self.binding), package=self.package)
        self.invocation = dict(source_commit=self.commit, batch_base='d' * 40,
                               source_sha256=self.server['source_sha256'],
                               lock_sha256=self.server['lock_sha256'],
                               executable_sha256=self.server['executable_sha256'],
                               mg_job='sync-server-interop', arguments=['--lane', 'desktop', '--phase', 'final'],
                               run_id=self.run, author_session='synthetic-author', root='/synthetic/root',
                               server_arguments=config['Cmd'], data_volume='meguri-sync-data-' + self.run,
                               owner_nonce='b' * 32, child_phase='final-full', child_stage='execute')
        self.helper.final_peer(self.peer, self.artifacts, self.invocation)

    def store(self, value):
        data = value if isinstance(value, bytes) else json.dumps(value).encode()
        sha = hashlib.sha256(data).hexdigest()
        (self.artifacts / sha).write_bytes(data)
        return sha

    def package_fixture(self):
        """Original synthetic package files, not actual producer/image evidence."""
        directory = self.root / 'synthetic-package'
        (directory / 'context').mkdir(parents=True)
        (directory / 'product').mkdir()
        for target, sha in [('source.tar', self.server['source_sha256']),
                            ('context/package-inputs.json', self.package['inputs_sha256']),
                            ('product/runtime-inventory.json', self.package['runtime_inventory_sha256']),
                            ('product/assets.json', self.package['assets_sha256']),
                            ('image.json', self.package['image_inspection_sha256'])]:
            (directory / target).write_bytes((self.artifacts / sha).read_bytes())
        (directory / 'result.json').write_text(json.dumps(self.receipt))
        inventory = self.helper.load(directory / 'product/runtime-inventory.json')
        with tarfile.open(directory / 'runtime.tar', 'w') as archive:
            for name, row in inventory.items():
                data = (self.artifacts / row['sha256']).read_bytes()
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        self.compiled_exports(directory)
        return directory

    def compiled_exports(self, directory, entries=None, manifest=None):
        manifest = ((directory / 'product/assets.json').read_bytes() if manifest is None else manifest)
        if entries is None:
            entries = [('embedded/' + name, data, tarfile.REGTYPE, '')
                       for name, data in self.payloads.items()]
        for filename, members in [('prepared-manifest.tar', [('assets.json', manifest, tarfile.REGTYPE, '')]),
                                  ('prepared-embedded.tar', entries)]:
            with tarfile.open(directory / filename, 'w') as archive:
                if filename == 'prepared-embedded.tar':
                    entry = tarfile.TarInfo('embedded')
                    entry.type = tarfile.DIRTYPE
                    archive.addfile(entry)
                for name, data, kind, link in members:
                    entry = tarfile.TarInfo(name)
                    entry.type, entry.linkname = kind, link
                    entry.size = len(data) if kind == tarfile.REGTYPE else 0
                    archive.addfile(entry, io.BytesIO(data) if kind == tarfile.REGTYPE else None)

    def test_compiled_export_retention_and_preflight_preserve_exact_payloads(self):
        directory = self.package_fixture()
        self.helper.export_package_assets(directory)
        retained = self.root / 'retained'
        retained.mkdir()
        identity = self.helper.retain_package(directory, retained)
        self.assertEqual(identity, dict(server=self.server, package=self.package))
        for row in self.asset_manifest:
            name = row['path'][1:].replace('/', '_')
            self.assertEqual((directory / 'product/frontend-assets' / name).read_bytes(), self.payloads[name])
            self.assertEqual((retained / row['sha256']).read_bytes(), self.payloads[name])
        execute_input = self.root / 'retained-preflight'
        execute_input.mkdir()
        self.helper.prepare_final(execute_input, retained, identity, json.dumps(self.receipt).encode(),
                                  (self.artifacts / self.binding['container_inspection_before_sha256']).read_bytes(),
                                  self.binding['server_process'], self.invocation)
        peer = self.helper.load(execute_input / 'preflight.json')
        self.helper.final_peer(peer, execute_input / 'artifacts', self.invocation, preflight=True)
        for row in self.asset_manifest:
            self.assertEqual(self.helper.artifact(execute_input / 'artifacts', row['sha256']),
                             self.payloads[row['path'][1:].replace('/', '_')])

    def test_old_package_missing_or_tampered_payload_evidence_cannot_retain(self):
        directory = self.package_fixture()
        retained = self.root / 'retained'
        retained.mkdir()
        with self.assertRaisesRegex(ValueError, 'missing compiled frontend payload evidence'):
            self.helper.retain_package(directory, retained)
        self.helper.export_package_assets(directory)
        payload = directory / 'product/frontend-assets/index.html'
        original = payload.read_bytes()
        payload.unlink()
        with self.assertRaisesRegex(ValueError, 'payload evidence'):
            self.helper.retain_package(directory, retained)
        payload.write_bytes(original + b'changed length')
        with self.assertRaisesRegex(ValueError, 'identity or length'):
            self.helper.retain_package(directory, retained)
        payload.write_bytes(b'X' + original[1:])
        with self.assertRaisesRegex(ValueError, 'identity or length'):
            self.helper.retain_package(directory, retained)
        payload.write_bytes(original)
        os.link(payload, self.root / 'linked-payload')
        with self.assertRaisesRegex(ValueError, 'single-link'):
            self.helper.retain_package(directory, retained)

    def test_package_admission_requires_every_transitive_payload_and_exact_length(self):
        path = self.artifacts / self.asset_manifest[0]['sha256']
        original = path.read_bytes()
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            self.helper.final_peer(self.peer, self.artifacts, self.invocation)
        path.write_bytes(b'X' + original[1:])
        with self.assertRaisesRegex(ValueError, 'identity or length'):
            self.helper.final_peer(self.peer, self.artifacts, self.invocation)
        path.write_bytes(original)
        rows = copy.deepcopy(self.asset_manifest)
        rows[0]['bytes'] += 1
        manifest = json.dumps(rows).encode()
        inventory = self.helper.decode((self.artifacts / self.package['runtime_inventory_sha256']).read_bytes())
        inventory[self.helper.RUNTIME_BASE + 'assets.json'] = dict(sha256=self.store(manifest), bytes=len(manifest))
        package = dict(self.package, assets_sha256=self.store(manifest), runtime_inventory_sha256=self.store(inventory))
        receipt = copy.deepcopy(self.receipt)
        receipt['details']['package'].update(package)
        with self.assertRaisesRegex(ValueError, 'identity or length'):
            self.helper.package_artifacts(package, self.server, receipt, self.artifacts)

    def test_export_rejects_manifest_disagreement_and_unsafe_or_incomplete_archive(self):
        directory = self.package_fixture()
        self.compiled_exports(directory, manifest=json.dumps(self.asset_manifest, indent=2).encode())
        with self.assertRaisesRegex(ValueError, 'manifest disagreement'):
            self.helper.export_package_assets(directory)
        good = [('embedded/' + name, data, tarfile.REGTYPE, '') for name, data in self.payloads.items()]
        for bad in [good[:-1], good + [good[0]],
                    good + [('embedded/unexpected.js', b'public extra', tarfile.REGTYPE, '')],
                    [('embedded/index.html', b'', tarfile.SYMTYPE, '/outside')] + good[1:],
                    [('embedded/index.html', b'', tarfile.LNKTYPE, good[1][0])] + good[1:],
                    [('embedded/index.html', b'', tarfile.FIFOTYPE, '')] + good[1:],
                    [('embedded/../index.html', good[0][1], tarfile.REGTYPE, '')] + good[1:]]:
            self.compiled_exports(directory, entries=bad)
            with self.subTest(entries=[row[0:1] + row[2:] for row in bad]), self.assertRaises(ValueError):
                self.helper.export_package_assets(directory)
            self.assertFalse((directory / 'product/frontend-assets').exists())

    def test_manifest_rejects_duplicate_mapping_and_nonoriginal_paths(self):
        for path in ['/index.html', '/assets/../study-abcdefgh.js', '/assets/study.js',
                     '/assets/study-abcdefgh.js.map', '/assets/nested/study-abcdefgh.js']:
            rows = copy.deepcopy(self.asset_manifest)
            rows[1]['path'] = path
            with self.subTest(path=path), self.assertRaises(ValueError):
                self.helper.asset_rows(json.dumps(rows).encode())

    def test_final_rejects_source_image_and_executable_mismatch(self):
        for field in ('source_sha256', 'lock_sha256', 'executable_sha256'):
            with self.subTest(field=field):
                peer = copy.deepcopy(self.peer)
                peer['server'][field] = self.store(b'unrelated actual bytes')
                with self.assertRaises(ValueError):
                    self.helper.final_peer(peer, self.artifacts, self.invocation)
        peer = copy.deepcopy(self.peer)
        peer['package']['image_id'] = 'sha256:' + self.store(b'unrelated image')
        with self.assertRaises(ValueError):
            self.helper.final_peer(peer, self.artifacts, self.invocation)

    def test_final_rejects_debug_relabel_failed_or_noncanonical_producer(self):
        for key, value in (('job', 'sync-server-candidate-build'), ('dry_run', True),
                           ('args', ['--phase', 'full']), ('exit_code', False), ('failures', ['failure'])):
            receipt = dict(self.receipt, **{key: value})
            peer = dict(self.peer, mg_build_receipt_sha256=self.store(receipt))
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.helper.final_peer(peer, self.artifacts, self.invocation)
        receipt = copy.deepcopy(self.receipt)
        receipt['details']['package']['profile'] = 'debug'
        peer = dict(self.peer, mg_build_receipt_sha256=self.store(receipt))
        with self.assertRaisesRegex(ValueError, 'package producer'):
            self.helper.final_peer(peer, self.artifacts, self.invocation)

    def test_final_rejects_changed_retained_dependency(self):
        path = self.artifacts / self.package['assets_sha256']
        path.write_bytes(b'replaced after retention')
        with self.assertRaisesRegex(ValueError, 'artifact bytes'):
            self.helper.final_peer(self.peer, self.artifacts, self.invocation)

    def test_final_rejects_init_pid_as_server_without_measured_executable(self):
        binding = copy.deepcopy(self.binding)
        binding['server_process'] = dict(pid=1, executable_sha256=self.store(b'synthetic init'))
        self.peer['execution_binding_sha256'] = self.store(binding)
        with self.assertRaisesRegex(ValueError, 'server process'):
            self.helper.final_peer(self.peer, self.artifacts, self.invocation)

    def test_preflight_omits_only_after_and_never_admits_as_final(self):
        peer, _ = self.execute_fixture()
        self.helper.final_peer(peer, self.artifacts, self.invocation, preflight=True)
        with self.assertRaisesRegex(ValueError, 'final package receipt'):
            self.helper.final_peer(peer, self.artifacts, self.invocation)
        for field in ('server_process', 'package', 'container_inspection_before_sha256'):
            changed = copy.deepcopy(peer)
            binding = self.helper.decode((self.artifacts / peer['execution_binding_sha256']).read_bytes())
            del binding[field]
            changed['execution_binding_sha256'] = self.store(binding)
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'execution binding'):
                self.helper.final_peer(changed, self.artifacts, self.invocation, preflight=True)
        peer['execution_binding_sha256'] = self.store(self.binding)
        with self.assertRaisesRegex(ValueError, 'execution binding'):
            self.helper.final_peer(peer, self.artifacts, self.invocation, preflight=True)

    def test_final_compares_actual_root_arguments_mount_nonce_and_created(self):
        for key, value in (('root', '/other/root'), ('owner_nonce', 'e' * 32),
                           ('data_volume', 'other-volume'),
                           ('server_arguments', ['serve', '/data/owner', 'https://other.example.test',
                                                 '0.0.0.0:8080', '--trusted-peer', '127.0.0.1'])):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.helper.final_peer(self.peer, self.artifacts, dict(self.invocation, **{key: value}))
        after = self.helper.decode((self.artifacts / self.binding['container_inspection_after_sha256']).read_bytes())
        after[0]['Created'] = 'different-created'
        binding = dict(self.binding, container_inspection_after_sha256=self.store(after))
        with self.assertRaisesRegex(ValueError, 'server changed'):
            self.helper.final_peer(dict(self.peer, execution_binding_sha256=self.store(binding)),
                                   self.artifacts, self.invocation)

    def execute_fixture(self):
        binding = {key: value for key, value in self.binding.items()
                   if key != 'container_inspection_after_sha256'}
        peer = dict(self.peer, kind='final_package_preflight', execution_binding_sha256=self.store(binding))
        preflight_data = json.dumps(peer).encode()
        invocation = dict(self.invocation, selection=[case['id'] for case in
                               self.helper.final_selection(self.invocation['child_phase'])['cases']])
        invocation_data = json.dumps(invocation).encode()
        # Deliberately opaque synthetic bytes: these are not actual client results.
        raw = b'{"synthetic":"unchanged failure output"}'
        (self.root / 'invocation.json').write_bytes(invocation_data)
        (self.root / 'candidate-observations.json').write_bytes(raw)
        admission = dict(format='meguri-sync-final-execute-v1', child_phase=self.invocation['child_phase'],
                         invocation_sha256=self.store(invocation_data),
                         preflight_peer_sha256=self.store(preflight_data), observations_sha256=self.store(raw))
        admission_data = json.dumps(admission).encode()
        (self.root / 'execute-admission.json').write_bytes(admission_data)
        self.store(admission_data)
        return peer, preflight_data

    def test_execute_admission_rehashes_actual_bytes_and_exact_shape(self):
        peer, preflight_data = self.execute_fixture()
        admission = self.helper.execute_admission(self.root, self.invocation, preflight_data)
        self.assertEqual(admission, self.helper.digest((self.root / 'execute-admission.json').read_bytes()))
        for key, value in (('extra', True), ('child_phase', 'final-rich'),
                           ('observations_sha256', '0' * 64)):
            changed = self.helper.load(self.root / 'execute-admission.json')
            changed[key] = value
            (self.root / 'execute-admission.json').write_text(json.dumps(changed))
            with self.subTest(key=key), self.assertRaises((ValueError, OSError)):
                self.helper.execute_admission(self.root, self.invocation, preflight_data)
            self.execute_fixture()
        with self.assertRaisesRegex(ValueError, 'admission bytes'):
            self.helper.execute_admission(self.root, self.invocation,
                                          json.dumps(peer, indent=2).encode())
        with self.assertRaisesRegex(ValueError, 'child invocation'):
            self.helper.execute_admission(self.root, dict(self.invocation, execute_admission_sha256=admission),
                                          preflight_data)
        (self.root / 'candidate-observations.json').write_bytes(b'changed raw output')
        with self.assertRaisesRegex(ValueError, 'admission bytes'):
            self.helper.execute_admission(self.root, self.invocation, preflight_data)

    def record_fixture(self):
        _, preflight_data = self.execute_fixture()
        snapshot = self.helper.execute_snapshot(self.root, self.invocation, preflight_data)
        expected = dict(self.invocation, child_stage='record',
                        execute_admission_sha256=snapshot['admission_sha256'])
        invocation = dict(expected, selection=self.helper.load(self.root / 'invocation.json')['selection'])
        (self.root / 'invocation.json').write_text(json.dumps(invocation))
        return expected, snapshot

    def test_record_requires_same_admission_observations_and_execution(self):
        expected, snapshot = self.record_fixture()
        self.helper.record_handoff(self.root, expected, self.peer, snapshot)
        with self.assertRaisesRegex(ValueError, 'captured execute'):
            self.helper.record_handoff(self.root, dict(expected, execute_admission_sha256='0' * 64),
                                       self.peer, snapshot)
        with self.assertRaisesRegex(ValueError, 'child invocation'):
            self.helper.record_handoff(self.root, {key: value for key, value in expected.items()
                                                  if key != 'execute_admission_sha256'}, self.peer, snapshot)
        for key, value, message in (('child_phase', 'final-rich', 'execute admission'),
                                    ('owner_nonce', 'e' * 32, 'original execute invocation'),
                                    ('arguments', ['--lane', 'desktop', '--phase', 'final', 'altered'],
                                     'original execute invocation')):
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, message):
                self.helper.record_handoff(self.root, dict(expected, **{key: value}), self.peer, snapshot)
        raw = self.root / 'candidate-observations.json'
        raw.write_bytes(b'outcome rewritten')
        with self.assertRaisesRegex(ValueError, 'changed execute observations'):
            self.helper.record_handoff(self.root, expected, self.peer, snapshot)

    def test_record_rejects_replaced_raw_inode_even_with_identical_bytes(self):
        expected, snapshot = self.record_fixture()
        raw = self.root / 'candidate-observations.json'
        replacement = self.root / 'replacement'
        replacement.write_bytes(raw.read_bytes())
        replacement.replace(raw)
        with self.assertRaisesRegex(ValueError, 'changed execute observations'):
            self.helper.record_handoff(self.root, expected, self.peer, snapshot)

    def test_execute_rejects_hardlinked_raw_and_any_final_credit_files(self):
        _, preflight_data = self.execute_fixture()
        raw = self.root / 'candidate-observations.json'
        link = self.root / 'extra-link'
        os.link(raw, link)
        with self.assertRaisesRegex(ValueError, 'single-link'):
            self.helper.execute_snapshot(self.root, self.invocation, preflight_data)
        link.unlink()
        for name in ('evidence.json', 'selection.json', 'summary.json', 'fixture.json', 'fixture-manifest.json'):
            path = self.root / name
            path.write_text('{}')
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'zero final credit'):
                self.helper.execute_snapshot(self.root, self.invocation, preflight_data)
            path.unlink()

    def test_prepare_and_complete_use_only_actual_after_and_preserve_preflight(self):
        execute_input = self.root / 'execute-input'
        execute_input.mkdir()
        identity = dict(server=self.server, package=self.package)
        before = (self.artifacts / self.binding['container_inspection_before_sha256']).read_bytes()
        self.helper.prepare_final(execute_input, self.artifacts, identity,
                                  json.dumps(self.receipt).encode(), before,
                                  self.binding['server_process'], self.invocation)
        self.assertEqual({p.name for p in execute_input.iterdir()}, {'preflight.json', 'artifacts'})
        preflight = self.helper.load(execute_input / 'preflight.json')
        original = self.helper.artifact(execute_input / 'artifacts', preflight['execution_binding_sha256'])
        self.assertNotIn('container_inspection_after_sha256', self.helper.decode(original))
        after = self.helper.decode(before)
        after[0]['State']['Status'] = 'running'
        after_data = json.dumps(after).encode()
        expected = dict(self.invocation, child_stage='record', execute_admission_sha256='f' * 64)
        record_input = self.root / 'record-input'
        record_input.mkdir()
        retained_event = self.helper.retain(execute_input / 'artifacts', b'immutable synthetic execute event')
        self.helper.complete_final(record_input, execute_input, execute_input / 'artifacts', after_data,
                                   self.binding['server_process'], expected)
        self.assertEqual(self.helper.artifact(record_input / 'artifacts', retained_event), b'immutable synthetic execute event')
        peer = self.helper.load(record_input / 'peer.json')
        binding = self.helper.final_peer(peer, record_input / 'artifacts', expected)
        self.assertEqual(binding['container_inspection_after_sha256'], self.helper.digest(after_data))
        self.assertNotEqual(binding['container_inspection_after_sha256'], binding['container_inspection_before_sha256'])
        self.assertEqual(self.helper.artifact(execute_input / 'artifacts', preflight['execution_binding_sha256']), original)
        self.assertEqual({p.name for p in record_input.iterdir()}, {'peer.json', 'artifacts'})
        # A restarted process or replacement container cannot become final evidence.
        for mutation in ('process', 'container', 'created'):
            rejected = self.root / ('reject-' + mutation)
            rejected.mkdir()
            changed = copy.deepcopy(after)
            process = dict(self.binding['server_process'])
            if mutation == 'process': process['pid'] += 1
            if mutation == 'container': changed[0]['Id'] = 'e' * 64
            if mutation == 'created': changed[0]['Created'] = 'different-created'
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.helper.complete_final(rejected, execute_input, execute_input / 'artifacts',
                                           json.dumps(changed).encode(), process, expected)

    def test_fixed_segments_preserve_exact_union_without_reducing_counts(self):
        segments = [self.helper.final_selection(phase)['cases'] for phase in self.helper.FINAL_SEGMENTS]
        self.assertEqual([len(cases) for cases in segments], [11, 10, 4, 1, 1, 1, 1, 1, 1])
        ids = [case['id'] for cases in segments for case in cases]
        self.assertEqual(len(set(ids)), 31)
        self.assertEqual(ids, [case['id'] for case in self.helper.final_selection('final')['cases']])

    def assembly_fixture(self, reuse_runtime=False):
        base = self.root
        segments, controls, output = (base / name for name in ('segments', 'controls', 'aggregate'))
        for path in (segments, controls, output, output / 'artifacts'): path.mkdir()
        common = {key: value for key, value in self.invocation.items() if key in self.helper.INVOCATION_KEYS}
        original = (self.root, self.artifacts, self.invocation, self.peer)
        combined = []
        for index, phase in enumerate(self.helper.FINAL_SEGMENTS):
            instance = 0 if reuse_runtime and index == 1 else index
            self.root = segments / phase
            self.root.mkdir()
            self.artifacts = self.root / 'artifacts'
            shutil.copytree(original[1], self.artifacts)
            nonce, cid, volume = format(instance + 1, '032x'), format(instance + 1, '064x'), 'synthetic-volume-' + str(instance)
            self.invocation = dict(original[2], child_phase=phase, owner_nonce=nonce, data_volume=volume)
            binding = copy.deepcopy(self.binding)
            inspection = self.helper.decode((self.artifacts / binding['container_inspection_before_sha256']).read_bytes())
            inspection[0]['Id'] = cid
            inspection[0]['Config']['Labels']['meguri.sync.owner'] = nonce
            inspection[0]['Mounts'][0]['Name'] = volume
            binding.update(container_id=cid, container_inspection_before_sha256=self.store(inspection),
                           container_inspection_after_sha256=self.store(inspection))
            self.peer = dict(original[3], execution_binding_sha256=self.store(binding))
            # execute_fixture uses this instance's before-only binding.
            saved_binding, self.binding = self.binding, binding
            expected, snapshot = self.record_fixture()
            self.binding = saved_binding
            control = controls / phase
            control.mkdir()
            (control / 'record-input').mkdir()
            for path, value in ((control / 'record-invocation.json', expected),
                                (control / 'execute-snapshot.json', snapshot),
                                (control / 'record-input/peer.json', self.peer)):
                path.write_text(json.dumps(value))
            selection = self.helper.final_selection(phase)
            (self.root / 'selection.json').write_text(json.dumps(selection))
            artifact_data = b'public synthetic evidence fixture; never actual compatibility output'
            sha = self.store(artifact_data)
            artifacts = [dict(role=role, sha256=sha, bytes=len(artifact_data))
                         for role in ('synthetic_input', 'neutral_observation', 'mg_receipt')]
            cases = copy.deepcopy(selection['cases'])
            for case in cases:
                del case['client_kind']
                case.update(status='matched', fixture_sha256=sha, artifacts=artifacts,
                            observed=copy.deepcopy(case['expected']), lane=dict(kind='candidate', server=self.server,
                            client=dict(kind='desktop', version='26.09.4', build='c06370ff', runtime_sha256='2' * 64)))
            evidence = {key: common[key] for key in ('source_commit', 'source_sha256', 'lock_sha256',
                                                    'batch_base', 'mg_job', 'arguments')}
            evidence.update(format='meguri-sync-server-v1', document_kind='evidence', baseline_release='26.09.3',
                            expected_case_count=len(cases), selected_case_count=len(cases), completed_case_count=len(cases), cases=cases)
            summary = dict(total=len(cases), passed=len(cases), failed=0, expected_case_count=len(cases),
                           selected_case_count=len(cases), completed_case_count=len(cases),
                           selection_credit=True, broad_catalog_credit=False)
            for name, value in (('evidence.json', evidence), ('summary.json', summary)):
                (self.root / name).write_text(json.dumps(value))
            combined.extend(cases)
            for path in self.artifacts.iterdir(): shutil.copyfile(path, output / 'artifacts' / path.name)
        self.root, self.artifacts, self.invocation, self.peer = original
        evidence.update(expected_case_count=31, selected_case_count=31, completed_case_count=31, cases=combined)
        summary.update(total=31, passed=31, expected_case_count=31, selected_case_count=31, completed_case_count=31)
        for name, value in (('evidence.json', evidence), ('summary.json', summary),
                            ('selection.json', self.helper.final_selection('final')),
                            ('invocation.json', dict(common, selection=[case['id'] for case in combined]))):
            (output / name).write_text(json.dumps(value))
        return segments, output, common, controls

    def test_assembly_rejects_missing_segments_changed_artifacts_and_outcomes(self):
        segments, output, common, controls = args = self.assembly_fixture()
        self.helper.assembly_handoff(*args)
        extra = segments / 'unexpected'
        extra.mkdir()
        with self.assertRaisesRegex(ValueError, 'exactly nine'):
            self.helper.assembly_handoff(*args)
        extra.rmdir()
        missing = segments / 'final-rich'
        missing.rename(segments.parent / 'held-rich')
        with self.assertRaisesRegex(ValueError, 'exactly nine'):
            self.helper.assembly_handoff(*args)
        (segments.parent / 'held-rich').rename(missing)
        evidence_path = output / 'evidence.json'
        evidence = self.helper.load(evidence_path)
        reordered = copy.deepcopy(evidence)
        reordered['cases'][0], reordered['cases'][1] = reordered['cases'][1], reordered['cases'][0]
        evidence_path.write_text(json.dumps(reordered))
        with self.assertRaisesRegex(ValueError, 'case order'):
            self.helper.assembly_handoff(*args)
        evidence['cases'][0]['actions'][0]['actor'] = 'fresh_client'
        evidence_path.write_text(json.dumps(evidence))
        with self.assertRaisesRegex(ValueError, 'fixed selected contract'):
            self.helper.assembly_handoff(*args)
        # Even an artifact unreferenced by case rows remains part of the execute CAS.
        raw = self.helper.load(segments / 'final-full/execute-admission.json')['observations_sha256']
        (output / 'artifacts' / raw).write_bytes(b'changed raw artifact')
        with self.assertRaisesRegex(ValueError, 'artifact bytes'):
            self.helper.assembly_handoff(*args)

    def test_assembly_rejects_reused_runtime_and_volume_despite_complete_counts(self):
        args = self.assembly_fixture(reuse_runtime=True)
        with self.assertRaisesRegex(ValueError, 'reused a runtime or storage'):
            self.helper.assembly_handoff(*args)

    def test_final_rejects_reduced_case_count(self):
        scenario = Path(ASSEMBLER).parents[2] / 'conformance/recordings/scenarios/meguri-sync-server'
        cases = [case for name in ('full-selection', 'normal-selection', 'media-selection', 'final-additions')
                 for case in json.loads((scenario / ('candidate-' + name + '.json')).read_text())['cases']]
        expected = {key: value for key, value in self.invocation.items()
                    if key in self.helper.INVOCATION_KEYS}
        invocation = dict(expected, selection=[case['id'] for case in cases[:-1]])
        selection = dict(format='meguri-sync-selection-v1', lane_kind='candidate', phase='final', cases=cases)
        (self.root / 'selection.json').write_text(json.dumps(selection))
        with self.assertRaisesRegex(ValueError, 'selection differs'):
            self.helper.validate(self.root, expected, self.peer, selection, {}, invocation)


if __name__ == '__main__':
    unittest.main()
