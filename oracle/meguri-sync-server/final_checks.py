# SPDX-License-Identifier: AGPL-3.0-or-later
"""Recompute final assertions from frozen independent inputs and exact outputs."""
from anki.sync import SyncOutput
from final_contract import RECIPE_SHA256, recipes
from validation import encoded, require, same_value, sha, unique_named


def authored(snapshot,ids):
    objects=snapshot['objects'];model=next(row for row in objects['models'] if row['id'] == ids['model'])
    deck=next(row for row in objects['decks'] if row['id'] == ids['deck']);preset=next(row for row in objects['presets'] if row['id'] == ids['preset'])
    notes={row[0]:row[4].split('\x1f') for row in snapshot['notes']}
    return {'note_type':model['name'],'fields_in_order':[row['name'] for row in model['flds']],
        'template':{'name':model['tmpls'][0]['name'],'question':model['tmpls'][0]['qfmt'],'answer':model['tmpls'][0]['afmt'],'css':model['css']},
        'deck':{'name':deck['name'],'description':deck['desc'],'collapsed':deck['collapsed']},
        'preset':{'name':preset['name'],'new_per_day':preset['new']['perDay'],'reviews_per_day':preset['rev']['perDay'],
                  'maximum_answer_seconds':preset['maxTaken'],'autoplay':preset['autoplay']},
        'config':{'meguri_sync_fixture':objects['config'].get('meguri_sync_fixture')},'notes':{key:notes[ident] for key,ident in ids['notes'].items() if ident in notes}}


def media_hashes(values):return {name:sha(value.encode()) for name,value in values.items()}


def final_values(result,case):
    from final_desktop import expected_values
    recipe=recipes();details=result['final_details'];rows=details['comparisons'];authorities=details.get('frozen_authorities',[])
    variant=case['variants'][0];ids=details.get('logical_ids')
    def equal(index,authority,checkpoint):
        row=rows[index];expected=expected_values(recipe,checkpoint)
        require(same_value(authored(authority['projection'],ids),expected),'frozen authority differs from original recipe')
        return same_value(row['observed_projection'],authority['projection']) and same_value(row['authored_values'],expected) and same_value(row['expected_authored_values'],expected)
    if variant == 'rich-final-fresh-receiver':
        require(len(authorities) == 3 and len(rows) == 4,'rich checkpoints missing');initial,forward,reverse=authorities
        from final_desktop import require_declared_mutation
        require(same_value(details['post_settlement_projection'],initial['projection']),'settlement absorbed candidate corruption')
        require_declared_mutation(initial['projection'],forward['projection'],ids,recipe,'forward')
        require_declared_mutation(forward['projection'],reverse['projection'],ids,recipe,'reverse')
        full,fw,rv,fresh=equal(0,initial,'initial'),equal(1,forward,'forward'),equal(2,reverse,'reverse'),equal(3,reverse,'reverse')
        deleted={ids['notes'][key] for key in ('delete_forward','delete_reverse')};last=rows[3]['observed_projection']
        return {'full_supported_values_match_recipe':full,'forward_supported_values_match_recipe':fw,'reverse_supported_values_match_recipe':rv,
            'fresh_supported_values_match_recipe':fresh,'history_scheduling_equal':same_value(last,reverse['projection']) and len(reverse['projection']['reviews']) == 2,
            'deleted_content_absent':all(row[0] not in deleted for row in last['notes']) and all(row[1] not in deleted for row in last['cards']),
            'final_media_matches_recipe':len(details['final_media_maps']) == 3 and all(value == media_hashes(recipe['media_utf8_exact']['reverse']) for value in details['final_media_maps'])}
    if variant == 'migration-explicit-authority':
        require(len(authorities) == 2 and len(rows) == 2,'migration authorities missing');source=details['source'];authority=authorities[1]
        require(source.get('runtime',{}).get('client',{}).get('build') == 'c06370ff','source runtime mismatch')
        return {'source_sync_completed':same_value(source['witness_projection'],authorities[0]['projection']) and source['media'] == media_hashes(recipe['media_utf8_exact']['initial']),
            'distinct_owned_source':type(source.get('owned_pid')) is int and source['owned_pid'] > 0 and source.get('endpoint') == 'http://127.0.0.1:8081/' and source.get('cleanup_complete') is True,
            'authority':'upload','history_scheduling_equal':equal(0,authority,'initial') and equal(1,authority,'initial'),
            'media_equal':all(row['media'] == media_hashes(recipe['media_utf8_exact']['initial']) for row in rows),
            'supported_values_match_initial_recipe':equal(0,authority,'initial') and equal(1,authority,'initial')}
    if variant in {'replacement-upload-explicit-authority','replacement-download-explicit-authority'}:
        require(len(authorities) == 2 and len(rows) == 2,'replacement authorities missing')
        for authority,logical,side in zip(authorities,details['authority_ids'],('right','left')):
            wanted=expected_values(recipe,'initial');wanted['note_type']=recipe['replacement'][side+'_objects']['note_type'];wanted['deck']['name']=recipe['replacement'][side+'_objects']['deck'];wanted['preset']['name']=recipe['replacement'][side+'_objects']['preset'];wanted['config']=recipe['replacement'][side+'_objects']['config'];wanted['notes']={side:recipe['replacement'][side+'_fields']}
            require(same_value(authored(authority['projection'],logical),wanted),'replacement source differs from original recipe')
        upload=variant.startswith('replacement-upload');right,left=authorities;chosen=left if upload else right;actual=rows[0]['observed_projection']
        fields=[row[4].split('\x1f') for row in actual['notes']];replacement=recipe['replacement'];maps=details['media_maps']
        from final_desktop import require_media_preserved
        observations=details['media_observations'];require(len(observations) == 2,'replacement empty observers missing')
        for observation,index,key in zip(observations,(2,4),('expected_server_media_after_collection_only','expected_server_media_after_ordinary_reconcile')):
            require(observation.get('action_index') == index and observation.get('actor') == case['actions'][index]['actor'],'replacement observer actor/action mismatch')
            require_media_preserved(observation,replacement[key])
        require(observations[0]['observed_hashes'] == media_hashes(maps['after_collection']) and observations[1]['observed_hashes'] == media_hashes(maps['after_reconcile']),'replacement maps differ from empty receivers')
        values={'full_required_before_authority':details['initial_decision'] == SyncOutput.FULL_SYNC,'authority':'upload' if upload else 'download',
            'chosen_fields':fields[0] if len(fields) == 1 else fields,
            'displaced_collection_content_absent':same_value(actual,chosen['projection']) and replacement['right_fields' if upload else 'left_fields'] not in fields,
            'history_scheduling_equal':same_value(actual,chosen['projection']) and same_value(rows[1]['observed_projection'],chosen['projection']),
            'server_media_after_collection_only':maps['after_collection'],'server_media_after_reconcile':maps['after_reconcile']}
        if not upload:values['candidate_not_overwritten']=same_value(actual,right['projection']) and same_value(rows[1]['observed_projection'],right['projection'])
        return values
    if variant == 'no-card-nonempty-preservation':
        require(len(authorities) == 1 and len(rows) == 1,'metadata authority missing')
        wanted=expected_values(recipe,'initial');wanted['notes']={}
        require(same_value(authored(authorities[0]['projection'],ids),wanted),'metadata source differs from original recipe')
        actual=rows[0]['observed_projection']
        return {'cards':len(actual['cards']),'notes':len(actual['notes']),'objects_config_equal':same_value(actual['objects'],authorities[0]['projection']['objects']),
            'media_utf8_exact':details['media_utf8_exact'],'valid_client_state':details['integrity'] == ['ok','ok']}
    if variant == 'fresh-fresh-explicit-direction':
        decision=details['initial_decision'];authority=details['explicit_authority'];last=details['blank_final']
        return {'decision_observed':decision in {SyncOutput.NO_CHANGES,SyncOutput.FULL_UPLOAD,SyncOutput.FULL_DOWNLOAD,SyncOutput.FULL_SYNC},
            'explicit_authority_matches_decision':authority == ('none' if decision == SyncOutput.NO_CHANGES else 'upload' if decision == SyncOutput.FULL_UPLOAD else 'download'),
            'blank_state_valid':last['notes'] == last['cards'] == 0 and last['integrity'] == 'ok',
            'ordinary_completed':details['receipts'][-1]['operation'] == 'normal' and details['receipts'][-1]['decision'] == SyncOutput.NO_CHANGES}
    raise ValueError('unknown final assertion')


def verify_result(result,case,store,fixture):
    require(result.get('phase','').startswith('final-'),'final result phase missing');store.read(RECIPE_SHA256)
    details=result.get('final_details');inputs=result.get('prepared_inputs',details.get('inputs',[]) if details else [])
    require(bool(inputs),'no actual frozen input');hashes=[]
    for row in inputs:
        require(sha(encoded(row['value'])) == row['sha256'] and store.read(row['sha256']) == encoded(row['value']),'frozen input changed');hashes.append(row['sha256'])
    entries={entry['sha256'] for entry in fixture['entries']}
    for index,action in enumerate(case['actions']):
        bound=store.json(action['fixture_sha256'])
        require(bound.get('format') == 'meguri-sync-final-action-manifest-v1' and bound.get('case_id') == case['id'] and bound.get('action_index') == index and bound.get('actor') == action['actor'],'final action identity mismatch')
        selected=action_input_hashes(result,case,index) if 'legacy_events' in result else hashes
        require(bound.get('recipe_sha256') == RECIPE_SHA256 and bound.get('frozen_input_sha256') == selected and set(selected) <= entries,'unbound final action inputs')
    import pathlib
    for name,digest in result.get('preparation_code_sha256',{}).items():
        require(name in {'adapter.py','normal.py','media.py','final_desktop.py','final_checks.py','final_contract.py'} and sha(pathlib.Path(__file__).with_name(name).read_bytes()) == digest,'preparation code changed')
    require(len(result.get('preparation_code_sha256',{})) == 6,'preparation code identity missing')
    if details is None:
        require('legacy_events' in result,'legacy actual event map missing')
        verify_legacy(result,case)
        return
    receipts=details.get('receipts',[])
    require([row.get('sequence') for row in receipts] == list(range(len(receipts))) and bool(receipts),'final actual receipt sequence missing')
    for receipt in receipts:
        index=receipt['action_index'];require(0 <= index < len(case['actions']) and receipt['actor'] == case['actions'][index]['actor'],'final actual actor/action mismatch')
        require(receipt.get('case_id') == case['id'] and receipt.get('input_sha256') in hashes,'final actual input binding missing')
        require(receipt.get('traffic') and all(row.get('status') == 200 for row in receipt['traffic']),'actual final traffic missing or failed')
        if receipt['operation'] == 'normal':require(receipt['decision'] == SyncOutput.NO_CHANGES,'final ordinary completion left pending full')
    require(all(any(row['action_index'] == index for row in receipts) for index in range(len(case['actions']))),'missing actual final action')
    computed=final_values(result,case)
    require(same_value(computed,details['values']) and same_value(computed,result['final_results'][case['variants'][0]]) and same_value(computed,unique_named(case['observed'])),'final booleans differ from retained exact values')


def action_input_hashes(result,case,index):
    trace=result['legacy_events'];digests=[]
    for event in trace['events']:
        if any(row['case_id'] == case['id'] and row['action_index'] == index for row in event['bindings']):
            digests.append(event['input_sha256'])
            if 'authoritative_input_sha256' in event:digests.append(event['authoritative_input_sha256'])
            digests.append(trace['baselines'][case['id']])
    require(bool(digests),'legacy action has no actual event input')
    return list(dict.fromkeys(digests))


def verify_legacy(result,case):
    from evidence import candidate_selection
    from final_desktop import NORMALIZATION
    trace=result['legacy_events'];contract={row['id']:row for row in candidate_selection(result['phase'])['cases']}
    require(set(trace['baselines']) == set(contract),'legacy baseline selection differs')
    inputs={row['sha256']:row['value'] for row in result['prepared_inputs']}
    require(len(inputs) == len(result['prepared_inputs']),'legacy duplicate captured input identity')
    require(trace['phase'] == result['phase'] and trace['preparation_identity'] == result['preparation_code_sha256'],'legacy preparation identity changed')
    events=trace['events'];require([row['sequence'] for row in events] == list(range(len(events))),'legacy event sequence missing or swapped')
    require(len({value['capture_sequence'] for value in inputs.values()}) == len(inputs),'legacy capture identity reused')
    baseline=trace['baselines'][case['id']];initial=inputs[baseline]
    require(initial['operation'] == 'prepare' and initial['actor'] == 'client_a' and initial['baselines'].get(case['id']) is None and any(row['case_id'] == case['id'] and row['action_index'] == 0 for row in initial['bindings']),'legacy initial baseline is not independent preparation')
    last=-1;covered=set()
    for event in events:
        require(event.get('input_sha256') in inputs,'legacy actual captured input missing')
        source=inputs[event['input_sha256']]
        require(source['phase'] == trace['phase'] and source['operation'] == event['operation'] and source['normalization'] == NORMALIZATION,'legacy input operation/normalization changed')
        require(event['capture_sequence'] == source['capture_sequence'] and event['capture_sequence'] > last,'legacy capture/completion order changed')
        require(source['preparation_identity'] == event['preparation_identity'] == trace['preparation_identity'],'legacy capture preparation mismatch')
        require(source['actor'] == event['actor'] and source['bindings'] == event['bindings'],'legacy actual actor/action snapshot swapped')
        require(event['completion_sequence'] > event['capture_sequence'],'legacy completion precedes capture')
        require(len({(row['case_id'],row['action_index']) for row in event['bindings']}) == len(event['bindings']),'legacy duplicate action binding')
        for row in event['bindings']:
            require(row['case_id'] in contract and 0 <= row['action_index'] < len(contract[row['case_id']]['actions']) and contract[row['case_id']]['actions'][row['action_index']]['actor'] == row['actor'],'legacy event has unknown case/action/actor')
            require(row['actor'] == event['actor'] and event['baselines'][row['case_id']] == trace['baselines'][row['case_id']],'legacy baseline/actual actor mismatch')
            if row['case_id'] == case['id']:
                index=row['action_index'];require(0 <= index < len(case['actions']) and row['actor'] == case['actions'][index]['actor'],'legacy declared action mismatch');covered.add(index)
        if event['operation'] == 'prepare':
            require(event['submit_sequence'] is None and event['input_sha256'] in trace['baselines'].values(),'legacy preparation submitted as client action')
        else:
            require(event['capture_sequence'] < event['submit_sequence'] < event['completion_sequence'],'legacy first-submit/completion order invalid')
            require(event.get('authoritative_input_sha256') in inputs,'legacy unchanged authoritative input missing')
            authority=inputs[event['authoritative_input_sha256']]
            if event['comparison'] in {'state','media'}:
                require(authority['operation'] == 'local-authority','legacy authority is not a pre-submit local event')
                require(all(authority['baselines'].get(row['case_id']) == trace['baselines'][row['case_id']] for row in event['bindings']),'legacy authority belongs to another baseline/case')
            require(authority['capture_sequence'] <= event['capture_sequence'] and authority['preparation_identity'] == trace['preparation_identity'],'legacy authority captured after first submit')
            require(event['traffic'] and all(row.get('status') == 200 for row in event['traffic']),'legacy actual HTTP completion missing or failed')
            output=event['output'];require(sha(encoded(output)) == event['output_sha256'],'legacy output changed')
            comparison=event['comparison']
            expected_mode='unchanged' if event['operation'] in {'login','decision'} else 'media' if event['operation'] == 'media' else 'blank' if trace['phase'] == 'final-full' and event['actor'] == 'fresh_client' else 'state'
            require(comparison == expected_mode,'legacy comparison does not match actual operation')
            if comparison == 'state':matched=same_value(output['projection'],authority['projection'])
            elif comparison == 'media':matched=output['media'] == authority['media'] and same_value(output['projection'],source['projection'])
            elif comparison == 'blank':matched=not output['projection']['notes'] and not output['projection']['cards'] and output.get('integrity') == 'ok'
            else:matched=same_value(output['projection'],source['projection']) and output['media'] == source['media']
            require(event.get('matched') is True and matched,'legacy output not equal to unchanged authoritative input')
            if event['operation'] == 'normal':require(event.get('required') == SyncOutput.NO_CHANGES,'legacy normal completion left pending full')
            if result['phase'] == 'final-full' and event['operation'] == 'full-upload':
                require(same_value(authority['projection'],initial['projection']),'full authority changed from original prepared baseline')
        last=event['completion_sequence']
    require(covered == set(range(len(case['actions']))),'legacy action/capture coverage incomplete')
