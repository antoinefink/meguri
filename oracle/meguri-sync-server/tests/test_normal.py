# SPDX-License-Identifier: AGPL-3.0-or-later
import copy
import pathlib
import tempfile
import unittest

from normal import NORMAL_FIXTURE, definitions, exact_numbers, normal_selection
from evidence import record
from validation import decode, encoded


class NormalBoundary(unittest.TestCase):
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
