# SPDX-License-Identifier: AGPL-3.0-or-later
import pathlib
import sqlite3
import subprocess
import sys
import tempfile
import unittest

from synthetic import FIXTURE
from normal import NORMAL_FIXTURE
from media import MEDIA_FIXTURE
from validation import decode, encoded
from adapter import file_stamp


class CandidateEndpoint(unittest.TestCase):
    def test_user_table_hash_keeps_names_near_sqlite_reserved_prefix(self):
        with tempfile.NamedTemporaryFile() as stream:
            connection = sqlite3.connect(stream.name)
            connection.executescript('create table col(mod,scm,usn,ls,crt,ver); insert into col values(1,2,3,4,5,18); create table sqliteXfoo(value); insert into sqliteXfoo values(1);')
            connection.commit()
            before = file_stamp(pathlib.Path(stream.name).read_bytes())
            connection.execute('update sqliteXfoo set value=2')
            connection.commit()
            after = file_stamp(pathlib.Path(stream.name).read_bytes())
            connection.close()
            self.assertNotEqual(before['user_tables_sha256'], after['user_tables_sha256'])

    def run_request(self, request):
        adapter = pathlib.Path(__file__).parents[1] / 'adapter.py'
        return subprocess.run([sys.executable,str(adapter)],input=encoded(request),stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30)

    def test_missing_candidate_never_starts_a_reference_fallback(self):
        for phase,fixture,failure_phase in [('full',FIXTURE,'native-login'),
                                             ('normal',NORMAL_FIXTURE,'normal-sdk'),
                                             ('media',MEDIA_FIXTURE,'media-sdk-setup')]:
            with self.subTest(phase=phase):
                request = {'mode':'candidate','fixture':fixture,'full_only':phase == 'full','phase':phase,
                           'target':{'username':'synthetic','password':'unreachable-only','server':{'kind':'meguri_sync','version':'test','build':'test'}}}
                process = self.run_request(request)
                self.assertNotEqual(process.returncode,0)
                result = decode(process.stdout)
                self.assertEqual(result['failure']['phase'],failure_phase)
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
