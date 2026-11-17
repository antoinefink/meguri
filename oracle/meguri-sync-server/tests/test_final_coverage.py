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
    def test_completed_final_payload_keeps_late_wide_values_and_frozen_hashes_exact(self):
        from normal import exact_numbers
        from validation import decode, encoded, same_value, sha
        wide=9007199254740993
        frozen=exact_numbers({'projection':{'notes':[[wide,-wide,17,True]]},'media':{'synthetic.txt':'f'*64}})
        frozen_bytes=encoded(frozen);digest=sha(frozen_bytes)
        output={'projection':copy.deepcopy(frozen['projection']),'media':copy.deepcopy(frozen['media'])}
        output_digest=sha(encoded(output))
        traffic={'normal_response':{'nested':[[wide,-wide,9007199254740991,-9007199254740991,17,True,False,None]]}}
        for phase in ('final-normal','final-rich'):
            with self.subTest(phase=phase):
                result={'phase':phase,'runtime_environment':{'available_cpus':17}}
                if phase == 'final-normal':
                    result=exact_numbers(result)
                    result.update(prepared_inputs=[{'sha256':digest,'value':copy.deepcopy(frozen)}],
                        legacy_events={'events':[{'input_sha256':digest,'output':output,'output_sha256':output_digest,'traffic':[traffic]}]})
                    inputs=result['prepared_inputs'];receipts=result['legacy_events']['events']
                else:
                    result['final_details']={'inputs':[{'sha256':digest,'value':copy.deepcopy(frozen)}],
                        'receipts':[{'input_sha256':digest,'traffic':[traffic]}]}
                    inputs=result['final_details']['inputs'];receipts=result['final_details']['receipts']
                with self.assertRaisesRegex(ValueError,'wide integer'):decode(encoded(result))
                captured=decode(api.result_bytes(result))
                if phase == 'final-normal':
                    captured_inputs=captured['prepared_inputs'];captured_receipts=captured['legacy_events']['events']
                    self.assertEqual(sha(encoded(captured_receipts[0]['output'])),output_digest)
                else:
                    captured_inputs=captured['final_details']['inputs'];captured_receipts=captured['final_details']['receipts']
                self.assertTrue(same_value(captured_receipts[0]['traffic'][0]['normal_response']['nested'][0],
                    [str(wide),str(-wide),9007199254740991,-9007199254740991,17,True,False,None]))
                self.assertEqual(encoded(captured_inputs[0]['value']),frozen_bytes)
                self.assertEqual(sha(encoded(captured_inputs[0]['value'])),digest)
                self.assertEqual(captured_inputs[0]['sha256'],digest)
                self.assertEqual(captured_receipts[0]['input_sha256'],digest)
                self.assertEqual(inputs[0]['value'],frozen)
                self.assertEqual(receipts[0]['traffic'][0]['normal_response']['nested'][0][0],wide)

    def test_failed_media_call_retains_only_sanitized_noncredit_traffic(self):
        from anki.errors import SyncErrorKind
        from types import SimpleNamespace
        from unittest.mock import patch
        from final_desktop import Run
        from validation import decode
        case=candidate_selection('final-migration')['cases'][0]
        record=SimpleNamespace(observations=[{'route':'/sync/upload','status':200}])
        traffic={'method':'POST','route':'/msync/mediaChanges','status':500,'header_members':['k','s','v'],
                 'media_request':{'lastUsn':9007199254740993},'request_bytes':29,'compressed_response_bytes':7}
        secret='synthetic-secret-must-stay-excluded'
        class FailedMedia:
            def sync_media(self,auth):record.observations.append(copy.deepcopy(traffic))
            def media_sync_status(self):raise api.SyncError(secret,None,None,None,SyncErrorKind.OTHER)
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory)
            run=Run(api,root,{'username':secret,'password':secret},case)
            with patch('final_desktop.projection',return_value={'notes':[]}),patch('final_desktop.media_map',return_value={}),\
                 patch.object(api,'FINAL_CAPTURE',None),patch.object(api,'PHASE','stale-runtime-installation-receipt'),\
                 patch.object(api,'FINAL_FAILURE',None):
                with self.assertRaises(api.SyncError):run.call(FailedMedia(),SimpleNamespace(hkey=secret),'client_a',1,'media',record)
                self.assertEqual(run.receipts,[])
                api.retain_final_failure(root)
                payload=(root/'sdk-failure.json').read_bytes();captured=decode(payload)
                self.assertFalse(captured['selection_credit'])
                self.assertEqual(captured['failure']['phase'],'media-sync-status')
                self.assertEqual(captured['failure']['kind'],'SyncError')
                self.assertEqual(captured['failure']['operation'],'media')
                self.assertEqual(captured['failure']['case_id'],case['id'])
                self.assertEqual(captured['failure']['action_index'],1)
                self.assertEqual(captured['failure']['actor'],'client_a')
                expected=copy.deepcopy(traffic);expected['media_request']['lastUsn']='9007199254740993'
                self.assertEqual(captured['failure']['traffic'],[expected])
                self.assertNotIn(secret.encode(),payload)
                self.assertNotIn('driver_frames',captured['failure'])
                self.assertEqual({path.name for path in root.iterdir()},{'sdk-failure.json'})

    def test_migration_upload_resets_before_first_media_and_keeps_frozen_content(self):
        from unittest.mock import patch
        from final_desktop import migration
        # Both services are actual owned pinned SDK references. This characterizes
        # the fixture and HTTP ordering, never candidate31 interoperability credit.
        case=candidate_selection('final-migration')['cases'][0]
        recorder=api.recorder
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory)
            with api.reference_server(root/'candidate') as (port,username,password):
                def reference_upstream(upstream):return recorder(port if upstream == 8080 else upstream)
                target={'username':username,'password':password,'server':api.runtime()['server']}
                with patch.object(api,'recorder',side_effect=reference_upstream):
                    details=migration(api,root/'migration',target,case)
        expected={row['name']:row['value'] for row in case['expected']}
        self.assertEqual(details['values'],expected)
        self.assertEqual(final_values({'final_details':details},case),expected)
        switch=[row for row in details['receipts'] if row['actor'] == 'client_a' and row['action_index'] == 1]
        self.assertEqual([row['operation'] for row in switch],['decision','full','media'])
        self.assertTrue(any(row['route'] == '/sync/upload' and row['status'] == 200 and row['upload_result'] == 'OK' for row in switch[1]['traffic']))
        self.assertFalse(any(row['route'].startswith('/msync/') for receipt in switch[:2] for row in receipt['traffic']))
        rows=switch[2]['traffic']
        self.assertEqual(next(row['media_begin']['usn'] for row in rows if row['route'] == '/msync/begin'),0)
        self.assertEqual(next(row['media_request']['lastUsn'] for row in rows if row['route'] == '/msync/mediaChanges'),0)
        self.assertEqual(sum(row['media_result'][0] for row in rows if row['route'] == '/msync/uploadChanges'),len(recipes()['media_utf8_exact']['initial']))
        inputs={row['sha256']:row['value'] for row in details['inputs']};before=inputs[switch[2]['input_sha256']]
        authority=details['frozen_authorities'][1]
        self.assertEqual(before['projection'],authority['projection'])
        self.assertEqual(before['media'],authority['media'])

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
