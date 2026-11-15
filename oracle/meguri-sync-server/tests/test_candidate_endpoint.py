# SPDX-License-Identifier: AGPL-3.0-or-later
import pathlib
import subprocess
import sys
import unittest

from synthetic import FIXTURE
from validation import decode, encoded


class CandidateEndpoint(unittest.TestCase):
    def test_missing_candidate_never_starts_a_reference_fallback(self):
        adapter = pathlib.Path(__file__).parents[1] / 'adapter.py'
        request = {'mode':'candidate','fixture':FIXTURE,'full_only':True,
                   'target':{'username':'synthetic','password':'unreachable-only','server':{'kind':'meguri_sync','version':'test','build':'test'}}}
        process = subprocess.run([sys.executable,str(adapter)],input=encoded(request),stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30)
        self.assertNotEqual(process.returncode,0)
        result = decode(process.stdout)
        self.assertEqual(result['failure']['phase'],'native-login')
        self.assertNotIn(b'unreachable-only',process.stdout)
