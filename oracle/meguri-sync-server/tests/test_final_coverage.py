# SPDX-License-Identifier: AGPL-3.0-or-later
"""Offline SDK fixtures and tamper checks, never candidate compatibility credit."""
import copy
import pathlib
import tempfile
import unittest

import adapter as api
from evidence import candidate_selection
from final_checks import final_values
from final_contract import recipes
from final_desktop import author, authored_values, change, expected_values, expected_media, projection


class FinalCoverage(unittest.TestCase):
    def test_final31_preserves_each_historical_case_and_order(self):
        final=candidate_selection('final')['cases']
        unchanged=sum([candidate_selection(phase)['cases'] for phase in ('full','normal','media')],[])
        self.assertEqual(final[:25],unchanged)
        self.assertEqual(len(final),31)
        self.assertEqual(len({case['id'] for case in final}),31)
        self.assertEqual([len(candidate_selection('final-'+phase)['cases']) for phase in ('full','normal','media','migration','replacement-upload','replacement-download','metadata-only','fresh-fresh','rich')],[11,10,4,1,1,1,1,1,1])

    def test_actual_sdk_can_author_nonempty_metadata_without_cards_or_notes(self):
        recipe=recipes()
        with tempfile.TemporaryDirectory() as directory:
            col,ids=author(api,pathlib.Path(directory),'metadata',recipe,empty=True)
            try:
                wanted=expected_values(recipe,'initial');wanted['notes']={}
                self.assertEqual(authored_values(col,ids),wanted)
                self.assertEqual(col.db.scalar('select count(*) from cards'),0)
                self.assertEqual(col.db.scalar('select count(*) from notes'),0)
                self.assertEqual(col.db.scalar('pragma integrity_check'),'ok')
            finally:col.close(downgrade=False)

    def test_rich_comparison_refuses_template_corruption_despite_true_claim(self):
        recipe=recipes();case=candidate_selection('final-rich')['cases'][0]
        with tempfile.TemporaryDirectory() as directory:
            col,ids=author(api,pathlib.Path(directory),'author',recipe)
            try:
                authorities=[{'projection':projection(col)}];values=[authored_values(col,ids)]
                for direction in ('forward','reverse'):
                    change(col,ids,recipe,direction)
                    authorities.append({'projection':projection(col)});values.append(authored_values(col,ids))
                rows=[{'observed_projection':copy.deepcopy(authorities[index]['projection']),
                    'authored_values':copy.deepcopy(values[index]),'expected_authored_values':expected_values(recipe,checkpoint)}
                    for index,checkpoint in [(0,'initial'),(1,'forward'),(2,'reverse'),(2,'reverse')]]
                result={'final_details':{'comparisons':rows,'frozen_authorities':authorities,'logical_ids':ids,
                    'final_media_maps':[expected_media(recipe['media_utf8_exact']['reverse'])]*3,'post_settlement_projection':copy.deepcopy(authorities[0]['projection'])}}
                before=final_values(result,case)
                self.assertTrue(before['full_supported_values_match_recipe'])
                self.assertTrue(before['history_scheduling_equal'])
                model=next(row for row in rows[0]['observed_projection']['objects']['models'] if row['id'] == ids['model'])
                model['tmpls'][0]['qfmt']='corrupted candidate template'
                self.assertFalse(final_values(result,case)['full_supported_values_match_recipe'])
                self.assertTrue(before['full_supported_values_match_recipe'])
            finally:col.close(downgrade=False)

    def test_retained_writer_cannot_hide_missing_candidate_media_before_reconcile(self):
        from final_desktop import require_media_preserved, media_map
        recipe=recipes();expected=recipe['replacement']['server_media_utf8_exact']
        repaired=[]
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);root.joinpath('retained').mkdir()
            writer=api.Collection(str(root/'retained'/'collection.anki2'))
            try:
                for name,value in expected.items():writer.media.write_data(name,value.encode())
                self.assertEqual(media_map(writer),expected_media(expected))
                # A cleared candidate yields nothing to a genuinely empty reader.
                observation={'initial_media':{},'first_listing_cursor':0,'observed_hashes':{}}
                with self.assertRaisesRegex(ValueError,'candidate media changed'):
                    require_media_preserved(observation,expected)
                    repaired.append('ordinary writer reconciliation must not execute')
                self.assertEqual(repaired,[])
                self.assertEqual(media_map(writer),expected_media(expected))
            finally:writer.close(downgrade=False)

    def test_legacy_actual_source_vectors_reject_missing_swapped_actor_and_order(self):
        from unittest.mock import patch
        from final_desktop import LegacyTrace
        from final_checks import verify_legacy
        from synthetic import FIXTURE
        contract=candidate_selection('final-full');trace=LegacyTrace('final-full',contract)
        # Actual public SDK reference exchanges generate these vectors. They are
        # harness checks, never final-image or candidate interoperability credit.
        with patch.object(api,'FINAL_LEGACY',trace):
            result=api.smoke(FIXTURE,full_only=True)
        result.update(phase='final-full',prepared_inputs=trace.inputs,legacy_events=trace.result(),preparation_code_sha256=trace.preparation)
        verify_legacy(result,contract['cases'][0])
        events=result['legacy_events']['events'];position=next(i for i,row in enumerate(events) if row['operation'] == 'full-upload')
        for kind in ('missing','swap','actor','order'):
            with self.subTest(kind=kind):
                broken=copy.deepcopy(result);event=broken['legacy_events']['events'][position]
                if kind == 'missing':broken['prepared_inputs']=[row for row in broken['prepared_inputs'] if row['sha256'] != event['input_sha256']]
                elif kind == 'swap':event['input_sha256']=broken['legacy_events']['events'][position-1]['input_sha256']
                elif kind == 'actor':event['actor']='client_b'
                else:event['submit_sequence']=event['capture_sequence']-1
                with self.assertRaises(ValueError):verify_legacy(broken,contract['cases'][0])

    def test_common_corruption_cannot_become_rich_authority(self):
        recipe=recipes();case=candidate_selection('final-rich')['cases'][0]
        with tempfile.TemporaryDirectory() as directory:
            col,ids=author(api,pathlib.Path(directory),'author',recipe)
            try:
                authorities=[{'projection':projection(col)}];values=[authored_values(col,ids)]
                col.set_config('untouched_supported',{'original':17})
                authorities[0]={'projection':projection(col)}
                for direction in ('forward','reverse'):
                    change(col,ids,recipe,direction);authorities.append({'projection':projection(col)});values.append(authored_values(col,ids))
                rows=[{'observed_projection':copy.deepcopy(authorities[index]['projection']),'authored_values':copy.deepcopy(values[index]),'expected_authored_values':expected_values(recipe,checkpoint)}
                      for index,checkpoint in [(0,'initial'),(1,'forward'),(2,'reverse'),(2,'reverse')]]
                base={'final_details':{'comparisons':rows,'frozen_authorities':authorities,'logical_ids':ids,
                    'post_settlement_projection':copy.deepcopy(authorities[0]['projection']),
                    'final_media_maps':[expected_media(recipe['media_utf8_exact']['reverse'])]*3}}
                self.assertTrue(final_values(base,case)['fresh_supported_values_match_recipe'])
                for corruption in ('preset','config'):
                    with self.subTest(corruption=corruption):
                        broken=copy.deepcopy(base);details=broken['final_details']
                        affected=[details['post_settlement_projection']]+[row['projection'] for row in details['frozen_authorities'][1:]]+[row['observed_projection'] for row in details['comparisons'][1:]]
                        for state in affected:
                            if corruption == 'preset':next(row for row in state['objects']['presets'] if row['id'] != ids['preset'])['new']['perDay']=999
                            else:state['objects']['config']['untouched_supported']={'original':999}
                        with self.assertRaisesRegex(ValueError,'settlement absorbed'):final_values(broken,case)
            finally:col.close(downgrade=False)
