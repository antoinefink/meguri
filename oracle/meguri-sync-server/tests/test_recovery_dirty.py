# SPDX-License-Identifier: AGPL-3.0-or-later
import pathlib
import tempfile
import unittest

from evidence import record
from recovery_dirty import DIRTY_FIXTURE, dirty_selection


class DirtyRecoveryTests(unittest.TestCase):
    def test_failed_inplace_fact_is_separate_from_clean_recovery(self):
        cases={case['variants'][0]:{row['name']:row['value'] for row in case['expected']}
               for case in dirty_selection()['cases']}
        self.assertFalse(cases['dirty-inplace-observed']['media_snapshot_equal'])
        self.assertTrue(cases['dirty-clean-receiver']['media_snapshot_equal'])
        self.assertNotIn('dirty-inplace',cases)

    def test_missing_clean_receiver_cannot_receive_credit(self):
        results={case['variants'][0]:{row['name']:row['value'] for row in case['expected']}
                 for case in dirty_selection()['cases']}
        results.pop('dirty-clean-receiver')
        value={'probe_kind':'recovery-dirty','fixture':DIRTY_FIXTURE,'runtime':{},'dirty_results':results}
        with tempfile.TemporaryDirectory() as directory:
            out=pathlib.Path(directory)
            with self.assertRaises(KeyError):record(value,out,{})
            self.assertFalse((out/'evidence.json').exists())
            self.assertFalse((out/'summary.json').exists())
