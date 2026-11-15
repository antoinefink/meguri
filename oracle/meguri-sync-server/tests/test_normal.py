# SPDX-License-Identifier: AGPL-3.0-or-later
import copy
import pathlib
import tempfile
import unittest

from normal import NORMAL_FIXTURE, definitions, exact_numbers, normal_selection
from evidence import record
from validation import Artifacts, decode, encoded, sha


class NormalBoundary(unittest.TestCase):
    def test_record_retains_action_inputs_with_exact_synthetic_descriptors(self):
        # Metadata-only producer regression; no SDK execution or compatibility claim.
        zero = '0' * 64
        runtime = {'format':'meguri-sync-runtime-v1', 'lock_sha256':zero,
                   'client':{'kind':'desktop','version':'26.09.4','build':'metadata-only','runtime_sha256':zero},
                   'server':{'kind':'reference_runtime','version':'26.09.4','build':'metadata-only','runtime_sha256':zero}}
        result = {'probe_kind':'normal','fixture':NORMAL_FIXTURE,'runtime':runtime,
                  'normal_results':{variant:values for _,variant,values in definitions()}}
        invocation = {'source_commit':'0'*40,'batch_base':'1'*40,'source_sha256':zero,'lock_sha256':zero,
                      'mg_job':'sync-server-reference','arguments':['selftest'],'run_id':'metadata-only'}
        with tempfile.TemporaryDirectory() as directory:
            out = pathlib.Path(directory)
            self.assertTrue(record(result,out,invocation))
            manifest = decode((out/'evidence.json').read_bytes())
            store = Artifacts(out/'artifacts')
            digest = sha(encoded(NORMAL_FIXTURE))
            self.assertEqual(store.read(digest), encoded(NORMAL_FIXTURE))
            for case in manifest['cases']:
                inputs = [row['sha256'] for row in case['artifacts'] if row['role'] == 'synthetic_input']
                self.assertEqual(len(inputs), len(set(inputs)))
                self.assertIn(case['fixture_sha256'], inputs)
                self.assertEqual({action['fixture_sha256'] for action in case['actions']}, {digest})
                self.assertIn(digest, inputs)

    def test_wide_values_are_exact_and_types_are_preserved(self):
        value={'rows':[[9007199254740993,-9007199254740993,17,True]],'text':'9007199254740993'}
        original=copy.deepcopy(value)
        self.assertEqual(decode(encoded(exact_numbers(value))),{'rows':[['9007199254740993','-9007199254740993',17,True]],'text':'9007199254740993'})
        self.assertEqual(value,original)

    def test_missing_completed_observation_cannot_receive_counts_or_credit(self):
        result={'probe_kind':'normal','fixture':NORMAL_FIXTURE,
                'runtime':{'client':{},'server':{}},
                'normal_results':{variant:values for _,variant,values in definitions()}}
        result['normal_results'].pop(normal_selection()['cases'][-1]['variants'][0])
        with tempfile.TemporaryDirectory() as directory:
            out=pathlib.Path(directory)
            with self.assertRaises(KeyError): record(result,out,{})
            self.assertFalse((out/'summary.json').exists())
            self.assertFalse((out/'evidence.json').exists())
