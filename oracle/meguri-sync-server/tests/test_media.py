# SPDX-License-Identifier: AGPL-3.0-or-later
import pathlib
import tempfile
import unittest

from anki.errors import NetworkError
from anki.sync import SyncAuth

import adapter
from evidence import candidate_selection, record
from media import MEDIA_FIXTURE, definitions
from synthetic import FIXTURE


class MediaTests(unittest.TestCase):
    def test_candidate_media_contract_is_finite_and_sdk_only(self):
        selected = candidate_selection('media')
        self.assertEqual(selected['phase'], 'media')
        self.assertEqual(selected['lane_kind'], 'candidate')
        self.assertEqual(len(selected['cases']), 4)
        self.assertEqual({c['variants'][0] for c in selected['cases']},
                         {'sdk-bidirectional-mutations', 'sdk-no-change',
                          'sdk-26-downloads', 'sdk-1005-listing'})
        self.assertTrue(all('.candidate.' in c['id'] for c in selected['cases']))
        self.assertTrue(all(action['operation'] != 'native_request'
                            for c in selected['cases'] for action in c['actions']))

    def test_public_media_completion_raises_for_unreachable_peer(self):
        with tempfile.TemporaryDirectory() as directory:
            col, _ = adapter.collection(pathlib.Path(directory), 'client', FIXTURE)
            try:
                col.media.write_data('unreachable.bin', b'original unreachable media')
                with self.assertRaises(NetworkError):
                    adapter.media_sync(col, SyncAuth(hkey='synthetic', endpoint='http://127.0.0.1:1/'))
            finally:
                col.close(downgrade=False)

    def test_incomplete_media_results_cannot_receive_credit(self):
        result={'probe_kind':'media','fixture':MEDIA_FIXTURE,'runtime':{},
                'media_results':{variant:values for _,variant,values in definitions()}}
        result['media_results'].pop('sdk-1005-listing')
        with tempfile.TemporaryDirectory() as directory:
            out=pathlib.Path(directory)
            with self.assertRaises(KeyError): record(result,out,{})
            self.assertFalse((out/'summary.json').exists())
            self.assertFalse((out/'evidence.json').exists())
