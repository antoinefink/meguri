# SPDX-License-Identifier: AGPL-3.0-or-later
import pathlib
import tempfile
import unittest

import adapter
from evidence import record
from recovery import RECOVERY_FIXTURE, definitions, logical
from synthetic import FIXTURE


class RecoveryTests(unittest.TestCase):
    def test_semantic_digest_preserves_values_and_nonconfig_stamps(self):
        with tempfile.TemporaryDirectory() as directory:
            col,model=adapter.collection(pathlib.Path(directory),'digest',FIXTURE)
            try:
                adapter.add_note(col,model,FIXTURE['notes'][0])
                col.set_config('recovery_authored',{'number':37})
                initial=logical(col)
                col.db.execute('update config set usn=7,mtime_secs=123')
                stamped=logical(col)
                self.assertNotEqual(initial['sha256'],stamped['sha256'])
                self.assertEqual(initial['semantic_sha256'],stamped['semantic_sha256'])
                col.set_config('recovery_authored',{'number':38})
                self.assertNotEqual(stamped['semantic_sha256'],logical(col)['semantic_sha256'])
                before=logical(col)
                col.db.execute('update cards set usn=8')
                self.assertNotEqual(before['semantic_sha256'],logical(col)['semantic_sha256'])
                before=logical(col)
                col.db.execute('update cards set due=due+1')
                self.assertNotEqual(before['semantic_sha256'],logical(col)['semantic_sha256'])
            finally:
                col.close(downgrade=False)

    def test_missing_history_cannot_receive_credit(self):
        result={'probe_kind':'recovery','fixture':RECOVERY_FIXTURE,'runtime':{},
                'recovery_results':dict(definitions())}
        result['recovery_results'].pop('delayed-overtaken')
        with tempfile.TemporaryDirectory() as directory:
            out=pathlib.Path(directory)
            with self.assertRaises(KeyError): record(result,out,{})
            self.assertFalse((out/'summary.json').exists())
            self.assertFalse((out/'evidence.json').exists())
