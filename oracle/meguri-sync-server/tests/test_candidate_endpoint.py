# SPDX-License-Identifier: AGPL-3.0-or-later
import pathlib
import subprocess
import sys
import unittest

from synthetic import FIXTURE
from validation import decode, encoded


class CandidateEndpoint(unittest.TestCase):
    def run_request(self, request):
        adapter = pathlib.Path(__file__).parents[1] / 'adapter.py'
        return subprocess.run([sys.executable,str(adapter)],input=encoded(request),stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30)

    def test_missing_candidate_never_starts_a_reference_fallback(self):
        adapter = pathlib.Path(__file__).parents[1] / 'adapter.py'
        request = {'mode':'candidate','fixture':FIXTURE,'full_only':True,
                   'target':{'username':'synthetic','password':'unreachable-only','server':{'kind':'meguri_sync','version':'test','build':'test'}}}
        process = subprocess.run([sys.executable,str(adapter)],input=encoded(request),stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30)
        self.assertNotEqual(process.returncode,0)
        result = decode(process.stdout)
        self.assertEqual(result['failure']['phase'],'native-login')
        self.assertNotIn(b'unreachable-only',process.stdout)

    def test_missing_candidate_target_never_selects_reference(self):
        process = self.run_request({'mode':'candidate','fixture':FIXTURE,'full_only':True,'target':None})
        self.assertNotEqual(process.returncode,0)

    def test_full_transfer_branch_emits_no_media_or_normal_round(self):
        process = self.run_request({'mode':'smoke','fixture':FIXTURE,'full_only':True})
        self.assertEqual(process.returncode,0)
        result = decode(process.stdout)
        self.assertTrue(result.get('full_only'))
        self.assertEqual(result['runtime']['server']['kind'],'reference_runtime')
        self.assertTrue(result['outcomes']['full_equal'])
        routes = {row['route'] for row in result['traffic']}
        self.assertFalse(any(route.startswith('/msync/') for route in routes))
        self.assertNotIn('/sync/start',routes)
