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
    def test_retained_reverse_cli_uses_received_baseline_before_actual_sdk_mutation(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        import retained_companion as companion
        from final_contract import NEUTRAL
        from final_desktop import set_media
        from validation import Artifacts, decode, encoded

        # Only the external final-package receipt audit is outside this offline
        # regression. Fixed CLI paths, input/CAS/owner admission, current pinned
        # SDK, reverse mutation and closed full-state capture are exercised.
        controls=('valid','missing','changed-card','changed-media','expected-only',
                  'changed-cas','wrong-case','wrong-action','wrong-actor','wrong-run','wrong-owner')
        for control in controls:
            with self.subTest(control=control),tempfile.TemporaryDirectory() as directory:
                root=pathlib.Path(directory);incoming=root/'in';incoming.mkdir()
                out=root/'out';out.mkdir();state=root/'state'
                source=Artifacts(incoming/'artifacts')
                self.assertEqual(source.put((NEUTRAL/'final-fixture-recipes.json').read_bytes()),companion.RECIPE_SHA256)
                self.assertEqual(source.put((NEUTRAL/'candidate-retained-final-selection.json').read_bytes()),companion.SELECTION_SHA256)
                review=source.put(b'Offline regression only; no candidate compatibility credit.')
                action={'case':'normal-bidirectional','action':'prepare_reverse'}
                invocation={'source_commit':'0'*40,'batch_base':'1'*40,'source_sha256':'2'*64,
                    'lock_sha256':'3'*64,'mg_job':'sync-server-reference','arguments':['selftest'],
                    'executable_sha256':'4'*64,'run_id':'reverse-offline-'+control,
                    'author_session':'offline-regression','root':'/synthetic','server_arguments':[],
                    'data_volume':'offline-regression','owner_nonce':'5'*32}
                expect={'format':'meguri-retained-companion-input-v1',**action,'actor':'client_b',
                    'recipe_sha256':companion.RECIPE_SHA256,'selection_sha256':companion.SELECTION_SHA256,
                    'review_sha256':review,'baseline_sha256':None,'expected_sha256':None}
                companion.owner(state,action['case'],invocation,expect,new=True)
                col,ids=author(api,state,'client_b',recipes())
                try:
                    set_media(col,recipes()['media_utf8_exact']['initial'])
                    col.set_config('reverse_untouched',{'wide':9007199254740993,'unicode':'Straße'})
                    change(col,ids,recipes(),'forward')
                    self.assertEqual(authored_values(col,ids),expected_values(recipes(),'forward'))
                finally:col.close(downgrade=False)
                profile=state/'client_b';database=profile/'collection.anki2';media=profile/'collection.media'
                original=companion.snapshot(database,media)
                self.assertEqual(set(original['tables']),{row[0] for row in companion.TABLES})
                self.assertEqual(original['media_sha256'],expected_media(recipes()['media_utf8_exact']['forward']))
                baseline=copy.deepcopy(original)
                if control == 'changed-card':baseline['tables']['cards']['rows'][0][12]={'integer':'999999'}
                if control == 'changed-media':baseline['media_sha256']['replace.txt']='0'*64
                baseline_hash=source.put_json(baseline);baseline_bytes=source.read(baseline_hash)
                if control not in {'missing','expected-only'}:expect['baseline_sha256']=baseline_hash
                if control == 'expected-only':expect['expected_sha256']=baseline_hash
                if control == 'changed-cas':(source.root/baseline_hash).write_bytes(b'changed-authority')
                if control == 'wrong-case':expect['case']='full-upload'
                if control == 'wrong-action':expect['action']='receive_forward'
                if control == 'wrong-actor':expect['actor']='client_a'
                if control == 'wrong-owner':
                    owned=decode((state/'owner.json').read_bytes());owned['run_id']='other-run'
                    (state/'owner.json').write_bytes(encoded(owned))
                for name,value in [('action.json',action),('invocation.json',invocation),
                                   ('expectations.json',expect),('preflight.json',{'offline_receipt_audit_stub':True})]:
                    (incoming/name).write_bytes(encoded(value))
                paths={'/in':incoming,'/out':out,'/state':state}
                mapped=SimpleNamespace(Path=lambda value:paths[value] if value in paths else pathlib.Path(value))
                run_id='other-run' if control == 'wrong-run' else invocation['run_id']
                with patch.object(companion,'pathlib',mapped),\
                     patch.dict('os.environ',MG_RUN_ID=run_id),\
                     patch.object(companion,'retained_dependencies',return_value=set()) as audit,\
                     patch.object(companion,'read_credentials',side_effect=AssertionError('offline secret read')) as secrets,\
                     patch.object(companion,'observing',side_effect=AssertionError('offline HTTP')) as network,\
                     patch('final_desktop.change',wraps=change) as mutation:
                    if control in {'changed-cas','wrong-case','wrong-action','wrong-actor','wrong-run'}:
                        with self.assertRaises(ValueError):
                            companion.main(['--input','/in','--credentials','/secret/credentials.json'])
                        self.assertFalse((out/'observations.json').exists())
                    else:
                        status=companion.main(['--input','/in','--credentials','/secret/credentials.json'])
                        observation=decode((out/'observations.json').read_bytes())
                        store=Artifacts(out/'artifacts')
                        self.assertEqual(status,0 if control == 'valid' else 1)
                        self.assertFalse(observation['case_credit']);self.assertIsNone(observation['wire_sha256'])
                        self.assertEqual(observation['case'],action['case'])
                        self.assertEqual(observation['action'],action['action'])
                        self.assertEqual(observation['actor'],'client_b')
                        self.assertEqual(store.json(observation['invocation_sha256']),invocation)
                        self.assertEqual(store.json(observation['expectations_sha256']),expect)
                        self.assertEqual(store.json(observation['preflight_peer_sha256']),{'offline_receipt_audit_stub':True})
                        if control != 'wrong-owner':
                            self.assertEqual(store.json(observation['before_sha256']),original)
                            self.assertEqual(store.json(observation['after_sha256']),companion.snapshot(database,media))
                        if control == 'valid':
                            self.assertIsNone(expect['expected_sha256'])
                            self.assertTrue(observation['observed']['operation_complete'])
                            self.assertIsNone(observation['observed']['error_kind'])
                            self.assertEqual(observation['authority_sha256'],observation['after_sha256'])
                            after=store.json(observation['authority_sha256'])
                            self.assertEqual(set(after['tables']),set(original['tables']))
                            self.assertEqual(after['media_sha256'],original['media_sha256'])
                            self.assertEqual(len(after['tables']['revlog']['rows']),len(original['tables']['revlog']['rows'])+1)
                            self.assertEqual(len(after['tables']['notes']['rows']),1)
                            self.assertEqual(len(after['tables']['cards']['rows']),1)
                            self.assertEqual(after['tables']['fields'],original['tables']['fields'])
                            for table in ('col','config','tags','decks','deck_config','notetypes','templates','notes','cards','revlog','graves'):
                                self.assertEqual(after['tables'][table]['columns'],original['tables'][table]['columns'])
                            col=api.Collection(str(database))
                            try:
                                self.assertEqual(authored_values(col,ids),expected_values(recipes(),'reverse'))
                                self.assertEqual(col.get_config('reverse_untouched'),{'wide':9007199254740993,'unicode':'Straße'})
                            finally:col.close(downgrade=False)
                            mutation.assert_called_once();audit.assert_called_once()
                        else:
                            self.assertFalse(observation['observed']['operation_complete'])
                            self.assertEqual(observation['observed']['error_kind'],'preparation_refused')
                            self.assertIsNone(observation['authority_sha256'])
                    secrets.assert_not_called();network.assert_not_called()
                    if control != 'valid':mutation.assert_not_called()
                if control != 'valid':self.assertEqual(companion.snapshot(database,media),original)
                if control != 'changed-cas':self.assertEqual(source.read(baseline_hash),baseline_bytes)

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
        from final_desktop import migration, _require_migration_media_listing
        from validation import encoded
        api.MIGRATION_MEDIA_DIAGNOSTIC=None
        api.PHASE='setup'
        try:
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
            api.PHASE='migration-test-values-check'
            expected={row['name']:row['value'] for row in case['expected']}
            self.assertEqual(details['values'],expected)
            self.assertEqual(final_values({'final_details':details},case),expected)
            api.PHASE='migration-test-trace-check'
            switch=[row for row in details['receipts'] if row['actor'] == 'client_a' and row['action_index'] == 1]
            self.assertEqual([row['operation'] for row in switch],['decision','full','media'])
            self.assertTrue(any(row['route'] == '/sync/upload' and row['status'] == 200 and row['upload_result'] == 'OK' for row in switch[1]['traffic']))
            self.assertFalse(any(row['route'].startswith('/msync/') for receipt in switch[:2] for row in receipt['traffic']))
            rows=switch[2]['traffic']
            self.assertEqual(next(row['media_begin']['usn'] for row in rows if row['route'] == '/msync/begin'),0)
            _require_migration_media_listing(rows)
            self.assertEqual(sum(row['media_result'][0] for row in rows if row['route'] == '/msync/uploadChanges'),len(recipes()['media_utf8_exact']['initial']))
            # Corrupt only guard inputs; no fabricated SDK exchange or case credit.
            for request in ({'lastUsn':3},{'lastUsn':False},{'lastUsn':0.0},{'lastUsn':'0'},{},None):
                with self.subTest(malformed_listing=request):
                    listing={'route':'/msync/mediaChanges','status':200,'media_error_empty':True,'media_request':request}
                    with self.assertRaises(ValueError):_require_migration_media_listing([listing]+copy.deepcopy(rows))
            absent=[row for row in rows if row['route'] != '/msync/mediaChanges']
            for change in (None,{'status':403},{'media_error_empty':False},{'media_begin':{'usn':3}},
                           {'media_begin':{'usn':False}},{'media_begin':{'usn':0.0}},{'media_begin':{}}):
                with self.subTest(invalid_empty_begin=change):
                    broken=copy.deepcopy(absent)
                    if change is None:broken=[row for row in broken if row['route'] != '/msync/begin']
                    else:next(row for row in broken if row['route'] == '/msync/begin').update(change)
                    with self.assertRaises(ValueError):_require_migration_media_listing(broken)
            _require_migration_media_listing([{'route':'/msync/mediaChanges','status':200,'media_error_empty':True,'media_request':{'lastUsn':0}}]+copy.deepcopy(rows))
            api.PHASE='migration-test-input-check'
            inputs={row['sha256']:row['value'] for row in details['inputs']};before=inputs[switch[2]['input_sha256']]
            authority=details['frozen_authorities'][1]
            self.assertEqual(before['projection'],authority['projection'])
            self.assertEqual(before['media'],authority['media'])
        except BaseException as error:
            phases={'setup','migration-setup','migration-client-setup','migration-source-content-prepare',
                'migration-source-service','migration-source-login','migration-source-witness-login',
                'migration-source-witness-download','migration-source-authority-freeze','migration-target-login',
                'migration-authority-check','migration-upload-content-check','migration-explicit-source-media-reset',
                'migration-reset-content-check','migration-target-media','migration-media-cursor-check',
                'migration-media-publication-check','migration-published-content-check','migration-receiver-b',
                'migration-receiver-b-check','migration-receiver-fresh','migration-receiver-fresh-check',
                'migration-source-cleanup-check','migration-result','migration-test-values-check',
                'migration-test-trace-check','migration-test-input-check','runtime-location',
                'runtime-version-build','runtime-installation-receipt','final-decision','final-full-upload',
                'final-full-download','final-normal','final-media','media-sync-submit','media-sync-status'}
            errors={'ValueError','AssertionError','SyncError','NetworkError','TimeoutError','TypeError','KeyError','OSError'}
            phase=getattr(api,'PHASE','setup');kind=type(error).__name__
            diagnostic={'test':'migration-upload-reset-reference',
                'case':'P-F03:desktop.candidate.migration-explicit-authority',
                'phase':phase if type(phase) is str and phase in phases else 'setup',
                'error_class':kind if kind in errors else 'other','case_credit':False}
            state=getattr(api,'MIGRATION_MEDIA_DIAGNOSTIC',None);media=None
            if type(state) is dict:
                names=('begin_usn','listing_count','first_cursor','successful_upload_count','processed_total')
                media={name:state.get(name) if type(state.get(name)) is int and 0 <= state[name] <= 9007199254740991 else None for name in names}
                order=state.get('route_order');routes={'begin','mediaChanges','uploadChanges','downloadFiles','mediaSanity','other'}
                media['route_order']=[item if type(item) is str and item in routes else 'other' for item in order] if type(order) is list and len(order) <= 64 else None
                media['sanity_ok']=state.get('sanity_ok') if type(state.get('sanity_ok')) is bool else None
            diagnostic['media']=media
            pathlib.Path('/out/migration-reference-diagnostic.json').write_bytes(encoded(diagnostic))
            raise

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
