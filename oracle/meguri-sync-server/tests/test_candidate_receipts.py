# SPDX-License-Identifier: AGPL-3.0-or-later
"""Synthetic receipt-validator fixtures; no candidate compatibility evidence."""
import copy
import os
import pathlib
import tempfile
import unittest

from candidate import ENDPOINT, read_credentials, verify_package
from validation import Artifacts


class CandidateReceipts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.store = Artifacts(pathlib.Path(self.temp.name) / 'artifacts')
        executable = self.store.put(b'candidate-validator-only')
        lock = self.store.put(b'lock-validator-only')
        self.server = {'kind':'meguri_sync','version':'0.1.0','build':'validator-fixture','source_commit':'0'*40,
                       'source_sha256':'1'*64,'lock_sha256':lock,'executable_sha256':executable}
        self.invocation = {**self.server,'run_id':'validator-candidate'}
        self.build = {'job':'sync-server-build','run_id':'validator-build','args':['--profile','debug'], 'status':'pass','exit_code':0,'dry_run':False,
                      'commit':{'sha':self.server['source_commit'],'dirty':False},
                      'details':{'candidate_build':{'server':self.server,'source_dirty':False,'profile':'debug','features':[]}}}
        self.running = {'format':'meguri-sync-candidate-execution-v1','endpoint':ENDPOINT,'server':self.server,
                        'run_id':self.invocation['run_id'],'container_id':'2'*64,'labels':{'meguri.mg.run-id':self.invocation['run_id']},
                        'observed_executable_sha256':executable}
        self.package = {'format':'meguri-sync-package-v1','kind':'intermediate_build','server':self.server,'source_dirty':False,'profile':'debug','features':[],
                        'executable_artifact_sha256':executable,'lock_artifact_sha256':lock,'mg_build_receipt_sha256':self.store.put_json(self.build),
                        'build_invocation':{'job':self.build['job'],'run_id':self.build['run_id'],'arguments':self.build['args']},
                        'execution_binding_sha256':self.store.put_json(self.running)}

    def check(self): return verify_package(self.package,self.store,self.invocation)

    def test_bound_intermediate_receipt_fields_are_accepted(self):
        self.assertEqual(self.check()['kind'],'intermediate_build')

    def test_wrong_source_or_dirty_build_is_rejected(self):
        self.package['source_dirty'] = True
        with self.assertRaises(ValueError): self.check()
        self.package['source_dirty'] = False
        self.invocation['source_commit'] = '3'*40
        with self.assertRaises(ValueError): self.check()

    def test_running_endpoint_and_label_are_fixed(self):
        self.running['endpoint'] = 'http://example.invalid/'
        self.package['execution_binding_sha256'] = self.store.put_json(self.running)
        with self.assertRaises(ValueError): self.check()
        self.running['endpoint'] = ENDPOINT
        self.running['labels']['meguri.mg.run-id'] = 'another-job'
        self.package['execution_binding_sha256'] = self.store.put_json(self.running)
        with self.assertRaises(ValueError): self.check()

    def test_actual_binary_bytes_must_match_the_receipt(self):
        (self.store.root / self.server['executable_sha256']).write_bytes(b'changed')
        with self.assertRaises(ValueError): self.check()

    def test_profile_features_and_mg_invocation_are_bound(self):
        self.package['features'] = ['different-feature']
        with self.assertRaises(ValueError): self.check()
        self.package['features'] = []
        self.package['build_invocation']['arguments'] = ['another-build']
        with self.assertRaises(ValueError): self.check()

    def test_credentials_require_0600_and_closed_fields(self):
        path = pathlib.Path(self.temp.name) / 'credentials.json'
        path.write_text('{"username":"synthetic","password":"test-only"}')
        path.chmod(0o644)
        with self.assertRaises(ValueError): read_credentials(path)
        path.chmod(0o600)
        self.assertEqual(read_credentials(path)['username'],'synthetic')
        path.write_text('{"username":"synthetic","password":"test-only","endpoint":"unexpected"}')
        with self.assertRaises(ValueError): read_credentials(path)

    def test_credentials_never_follow_a_link(self):
        target = pathlib.Path(self.temp.name) / 'target.json'; target.write_text('{}'); target.chmod(0o600)
        path = pathlib.Path(self.temp.name) / 'credentials.json'; path.symlink_to(target)
        with self.assertRaises(OSError): read_credentials(path)
