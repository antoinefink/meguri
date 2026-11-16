# SPDX-License-Identifier: AGPL-3.0-or-later
"""Metadata proofs only: these tests earn no reference compatibility credit."""
import copy
import pathlib
import tempfile
import unittest
from evidence import record
from normal_matrix import MATRIX_FIXTURE, definitions, matrix_selection
from validation import Artifacts, decode, encoded, sha

class MatrixBoundary(unittest.TestCase):
 def metadata(self):
  zero='0'*64
  runtime={'format':'meguri-sync-runtime-v1','lock_sha256':zero,'client':{'kind':'desktop','version':'26.09.4','build':'metadata-only','runtime_sha256':zero},'server':{'kind':'reference_runtime','version':'26.09.4','build':'metadata-only','runtime_sha256':zero}}
  invocation={'source_commit':'0'*40,'batch_base':'1'*40,'source_sha256':zero,'lock_sha256':zero,'mg_job':'sync-server-reference','arguments':['selftest'],'run_id':'metadata-only'}
  result={'probe_kind':'normal-matrix','fixture':MATRIX_FIXTURE,'runtime':runtime,'normal_results':{variant:values for _,variant,values in definitions()},'case_inputs':{variant:[{'method':'metadata-only','input_json_hex':encoded({'case':variant}).hex()}] for _,variant,_ in definitions()}}
  return result,invocation

 def test_each_case_retains_exact_action_payload_and_strict_manifest_binding(self):
  result,invocation=self.metadata()
  with tempfile.TemporaryDirectory() as directory:
   out=pathlib.Path(directory);self.assertTrue(record(result,out,invocation))
   evidence=decode((out/'evidence.json').read_bytes());store=Artifacts(out/'artifacts')
   self.assertEqual(len(evidence['cases']),7)
   payloads=[]
   for case in evidence['cases']:
    digests={row['sha256'] for row in case['artifacts'] if row['role']=='synthetic_input'}
    manifest=store.json(case['fixture_sha256']);entries={row['sha256'] for row in manifest['entries']}
    self.assertIn(case['fixture_sha256'],digests)
    self.assertIn(sha(encoded(MATRIX_FIXTURE)),entries)
    exact={action['fixture_sha256'] for action in case['actions']};self.assertEqual(len(exact),1)
    payload=exact.pop();self.assertIn(payload,digests);self.assertIn(payload,entries)
    body=store.json(payload);variant=case['variants'][0]
    self.assertEqual(bytes.fromhex(body['traffic'][0]['input_json_hex']),encoded({'case':variant}))
    self.assertEqual(body['traffic'],result['case_inputs'][variant]);payloads.append(payload)
   self.assertEqual(len(set(payloads)),7)

 def test_missing_action_payload_cannot_get_completion_credit(self):
  result,invocation=self.metadata();del result['case_inputs'][matrix_selection()['cases'][-1]['variants'][0]]
  with tempfile.TemporaryDirectory() as directory:
   out=pathlib.Path(directory)
   with self.assertRaises(KeyError):record(result,out,invocation)
   self.assertFalse((out/'summary.json').exists());self.assertFalse((out/'evidence.json').exists())

 def test_original_modes_keep_their_case_counts(self):
  from normal import normal_selection
  from evidence import selection,candidate_selection
  self.assertEqual([len(fn()['cases']) for fn in (normal_selection,selection)], [12,21])
  self.assertEqual([len(candidate_selection(phase)['cases']) for phase in ('full','complete')],[11,21])
