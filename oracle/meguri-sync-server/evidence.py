# SPDX-License-Identifier: AGPL-3.0-or-later
"""Produce neutral, content-addressed observations with executable selections."""
from __future__ import annotations

import copy
import pathlib

from validation import Artifacts, binding, decode, encoded, require, same_value, sha, unique_named, validate
from synthetic import FIXTURE, EDGE_FIXTURE


def selection():
    definitions = [
        ('P-T01', 'identity', 'sync/server-transport#T2', {'version': '26.09.4', 'build': 'c06370ff', 'protocol': [11]}),
        ('P-T02', 'raw-zstd', 'sync/server-transport#T3', {'all_requests_raw_zstd': True, 'all_original_sizes_match': True}),
        ('P-T02', 'chunked', 'sync/server-transport#T2', {'observed_framing': ['chunked']}),
        ('P-T02', 'no-argument-padding', 'sync/server-transport#T2', {'observed_no_argument_bodies': [{'_pad': None}]}),
        ('P-F01', 'initial-meta', 'sync/server-collection#C1', {'initial_meta': {'mod': 0, 'usn': 0, 'media_usn': 0, 'empty': True, 'cont': True, 'hostNum': 0}}),
        ('P-F01', 'no-card-metadata', 'sync/server-collection#C1', {'blank_receiver_counts': [0, 0], 'blank_download_schema': 18}),
        ('P-F03', 'cards-local', 'sync/server-collection#C1', {'initial_direction': 'full_upload'}),
        ('P-F02', 'full-round-trip', 'sync/server-collection#C6', {'full_content_equal': True, 'receiver_integrity': 'ok'}),
        ('P-F02', 'upload-file-form', 'sync/server-collection#C6', {'upload_schema': 18, 'upload_response': 'OK'}),
        ('P-F02', 'download-file-form', 'sync/server-collection#C6', {'download_schemas': [18, 18]}),
        ('P-F02', 'download-stamps', 'sync/server-collection#C6', {'download_usn_deltas': [1, 1], 'schema_creation_stamps_preserved': True, 'served_ls_preserved': True}),
        ('P-N10', 'no-change', 'sync/server-collection#C2', {'no_change_routes': ['/sync/meta'], 'normal_content_equal': True}),
        ('P-M01', 'envelopes', 'sync/server-media#M1', {'media_envelopes': [['data', 'err']], 'all_media_errors_empty': True}),
        ('P-M01', 'empty-sk', 'sync/server-media#M1', {'empty_sk_observed': True, 'response_mutations': ['empty-sk'], 'fresh_media_equal': True}),
        ('P-M04', 'add', 'sync/server-media#M3', {'media_upload_results': [[1, 1], [1, 2]]}),
        ('P-M07', 'two-way', 'sync/server-media#M1', {'forward_media_equal': True, 'reverse_media_equal': True}),
        ('P-M07', 'no-change-no-retransmission', 'sync/server-media#M1', {'no_change_media_routes': ['/msync/begin']}),
    ]
    for direction in ('upload', 'download'):
        for same in (True, False):
            definitions.append(('P-F06', direction + ('-same-key-round' if same else '-other-key-round'),
                                'sync/server-lifecycle#L3', {'start_status': 200, 'pending_grave_status': 200,
                                'full_transfer': 'ok', 'full_transfer_sessions':[same], 'receiver_graves': 0, 'stale_finish_status': 409}))
    return {'format': 'meguri-sync-selection-v1', 'lane_kind': 'reference',
            'scope': 'Desktop synthetic smoke and explicitly named current-server observations only; broad family obligations remain unobserved.',
            'cases': [{'id': f'{family}:desktop.reference.{variant}', 'family': family, 'classification': 'current_probe',
                       'spec_ids': [spec], 'variants': [variant], 'client_kind': 'desktop',
                       'expected': [{'name': name, 'value': value} for name, value in values.items()],
                       'actions': case_actions(family, variant)}
                      for family, variant, spec, values in definitions]}


def case_actions(family, variant):
    actions = [{'actor':'client_a','operation':'prepare_synthetic','description':'Create original synthetic clients and media from the retained input recipe.'}]
    if family == 'P-F06':
        actions += [{'actor':'fault_driver','operation':'native_request','method':'start','description':'Open an unfinished collection round using a generated session.'},
                    {'actor':'fault_driver','operation':'native_request','method':'applyGraves','description':'Apply the fixture grave inside that unfinished round.'}]
        if '-same-' in variant:
            actions.append({'actor':'fault_driver','operation':'native_request','phase':'request-session-mutation','description':'Rewrite the actual Desktop full-transfer session to the unfinished round session.'})
        method = 'upload' if variant.startswith('upload') else 'download'
        actions += [{'actor':'client_a' if method == 'upload' else 'client_b','operation':'client_sync','method':method,'description':'Perform the actual Desktop full transfer and inspect its receiver.'},
                    {'actor':'fault_driver','operation':'native_request','method':'finish','description':'Attempt to finish the retired round and observe its refusal.'}]
    elif family == 'P-M01' and variant == 'empty-sk':
        actions += [{'actor':'fault_driver','operation':'native_request','method':'begin','phase':'response-mutation','description':'Rewrite only sk to an empty string in the reference begin reply; recompress it and update the original-size header.'},
                    {'actor':'fresh_client','operation':'client_sync','description':'Run actual Desktop media sync through the marked response mutation and verify both fixture file hashes.'}]
    else:
        actions.append({'actor':'client_a','operation':'client_sync','description':'Run the actual pinned Desktop synthetic upload, download, collection and media pipeline; observe the named assertion.'})
    return actions


def edge_selection():
    definitions = [
        ('P-M04','identical-noop','sync/server-media#M3',{'original_result':[1,1],'replayed_result':[1,1],'replay_status':200,
                                                      'request_mutation':'replay-upload','receiver_hash':sha(EDGE_FIXTURE['repeat_contents'].encode())}),
        ('P-M04','absent-noop','sync/server-media#M3',{'original_result':[1,2],'replayed_result':[1,2],'replay_status':200,
                                                    'request_mutation':'replay-upload','receiver_deleted':True}),
    ]
    for count in EDGE_FIXTURE['page_counts']:
        pages = [] if count == 0 else ([count,0] if count <= 1000 else [1000,1,0])
        hashes = {f"{EDGE_FIXTURE['media_prefix']}-{index:04}.bin":sha((EDGE_FIXTURE['media_payload_prefix']+str(index)).encode()) for index in range(count)}
        definitions.append(('P-M05','page-'+str(count),'sync/server-media#M3',{'page_lengths':pages,'change_usns':list(range(1,count+1)),'file_hashes':hashes}))
    definitions.append(('P-N01','idle-expiry','sync/server-lifecycle#L2',{
        'requested_idle_seconds':EDGE_FIXTURE['idle_seconds'],'sdk_timeout_seconds':EDGE_FIXTURE['sdk_timeout_seconds'],
        'reference_idle_status':200,'reference_client_result':'no_changes','reference_elapsed_at_least_boundary':True,
        'explicit_expiry_mutation':'idle-expire','explicit_abort_status':200,'expired_request_status':409,
        'expired_client_failed':True,'expired_local_content_preserved':True,'expired_pending_edit_rolled_back':True}))
    cases = []
    for family,variant,spec,values in definitions:
        actions = [{'actor':'client_a','operation':'prepare_synthetic','description':'Create original synthetic clients from the retained edge recipe.'}]
        if family == 'P-M04':
            actions += [{'actor':'client_a','operation':'client_sync','description':'Run actual Desktop media sync for the original addition or deletion.'},
                        {'actor':'fault_driver','operation':'native_request','method':'uploadChanges','phase':'request-replay','description':'Replay the exact SDK upload frame once before returning its original acknowledgment.'},
                        {'actor':'client_b','operation':'client_sync','description':'Run actual receiver media sync and verify the bytes or deletion.'}]
        elif family == 'P-M05':
            actions += [{'actor':'client_a','operation':'client_sync','description':'Upload the exact count of generated files with the actual Desktop SDK.'},
                        {'actor':'fresh_client','operation':'client_sync','description':'Use a fresh actual Desktop receiver; retain ordered listing pages and every received file hash.'}]
        else:
            actions += [{'actor':'client_a','operation':'client_sync','description':'Run actual Desktop normal sync with the retained longer request timeout.'},
                        {'actor':'fault_driver','operation':'native_request','method':'chunk','phase':'request-delay','description':'Delay the actual SDK chunk request for exactly the retained boundary duration before forwarding it unchanged.'},
                        {'actor':'fault_driver','operation':'native_request','method':'abort','phase':'expiry-injection','description':'In a separate round, explicitly abort at that boundary before forwarding the delayed actual SDK request; do not claim automatic reference expiry.'},
                        {'actor':'fresh_client','operation':'client_sync','description':'Download committed state with the actual SDK and verify rollback after the expired-round refusal.'}]
        cases.append({'id':f'{family}:desktop.reference.{variant}','family':family,'classification':'current_probe','spec_ids':[spec],
                      'variants':[variant],'client_kind':'desktop','expected':[{'name':name,'value':value} for name,value in values.items()], 'actions':actions})
    return {'format':'meguri-sync-selection-v1','lane_kind':'reference',
            'scope':'Actual Desktop pagination and replay probes; bounded reference idle observation and explicitly injected expiry only.', 'cases':cases}


def candidate_selection(phase):
    require(phase in {'full','complete'},'unknown candidate phase')
    contract = copy.deepcopy(selection())
    contract['lane_kind'] = 'candidate'
    contract['phase'] = phase
    contract['scope'] = 'Actual Desktop against the verified job-owned candidate; intermediate build interoperability only, not production package acceptance.'
    if phase == 'full':
        contract['cases'] = [case for case in contract['cases'] if case['family'] in {'P-T01','P-T02','P-F01','P-F02','P-F03'}]
        full_actions = [{'actor':'client_a','operation':'prepare_synthetic','description':'Create original synthetic collections from the retained recipe.'},
                        {'actor':'client_a','operation':'client_sync','method':'hostKey','description':'Log in the actual Desktop clients using the protected synthetic credentials.'},
                        {'actor':'client_a','operation':'client_sync','method':'meta','description':'Observe the initial metadata and transfer direction with the actual SDK.'},
                        {'actor':'fresh_client','operation':'client_sync','method':'download','description':'Download the blank account with the actual SDK.'},
                        {'actor':'client_a','operation':'client_sync','method':'upload','description':'Upload the original synthetic collection with the actual SDK.'},
                        {'actor':'client_b','operation':'client_sync','method':'download','description':'Download twice with the actual SDK and inspect content, integrity and stamps.'}]
        for case in contract['cases']: case['actions'] = copy.deepcopy(full_actions)
    for case in contract['cases']: case['id'] = case['id'].replace('.reference.','.candidate.')
    return contract


def edge_observations(result):
    uploads = [row for row in result['noops']['traffic'] if row['route'] == '/msync/uploadChanges']
    actual = {
        ('P-M04','identical-noop'):{'original_result':uploads[0]['media_result'],'replayed_result':uploads[0]['replay_media_result'],
                                  'replay_status':uploads[0]['replay_status'],'request_mutation':uploads[0]['request_mutation'],'receiver_hash':result['noops']['received_hash']},
        ('P-M04','absent-noop'):{'original_result':uploads[1]['media_result'],'replayed_result':uploads[1]['replay_media_result'],
                                'replay_status':uploads[1]['replay_status'],'request_mutation':uploads[1]['request_mutation'],'receiver_deleted':result['noops']['receiver_deleted']},
    }
    for row in result['pagination']:
        actual[('P-M05','page-'+str(row['count']))] = {'page_lengths':[len(page) for page in row['pages']],
                                                    'change_usns':[change[1] for page in row['pages'] for change in page],'file_hashes':row['file_hashes']}
    idle,expired = result['idle'],result['expiry']
    idle_row = next(row for row in idle['traffic'] if row.get('request_mutation') == 'idle-delay')
    expired_row = next(row for row in expired['traffic'] if row.get('request_mutation') == 'idle-expire')
    actual[('P-N01','idle-expiry')] = {'requested_idle_seconds':idle['requested_idle_seconds'],'sdk_timeout_seconds':result['fixture']['sdk_timeout_seconds'],
        'reference_idle_status':idle_row['status'],'reference_client_result':idle['client_result'],
        'reference_elapsed_at_least_boundary':idle_row['idle_elapsed_ms'] >= result['fixture']['idle_seconds']*1000,
        'explicit_expiry_mutation':expired_row['request_mutation'],'explicit_abort_status':expired_row['injected_abort_status'],
        'expired_request_status':expired_row['status'],'expired_client_failed':expired['client_result'] == 'error',
        'expired_local_content_preserved':expired['local_content_unchanged'],'expired_pending_edit_rolled_back':expired['server_pending_edit_rolled_back']}
    return actual


def _unique(values):
    output = []
    for value in values:
        if value not in output: output.append(value)
    return output


def observations(result):
    traffic, outcomes, admission = result['traffic'], result['outcomes'], result.get('session_admission',{})
    upload_index = next(index for index, row in enumerate(traffic) if 'upload_stamp' in row)
    uploaded = traffic[upload_index]
    downloads = [row['download_stamp'] for row in traffic[upload_index + 1:] if 'download_stamp' in row][:2]
    initial_meta = next(row['meta'] for row in traffic if 'meta' in row)
    blank = next(row['download_stamp'] for row in traffic if 'download_stamp' in row)
    media = [row for row in traffic if 'media_envelope_members' in row]
    values = {
        ('P-T01', 'identity'): {'version': result['runtime']['client']['version'], 'build': result['runtime']['client']['build'], 'protocol': sorted(set(row['protocol'] for row in traffic))},
        ('P-T02', 'raw-zstd'): {'all_requests_raw_zstd': all(row['raw_zstd'] for row in traffic), 'all_original_sizes_match': all(row.get('original_size_matches') for row in traffic if row['status'] == 200)},
        ('P-T02', 'chunked'): {'observed_framing': sorted(set(row['framing'] for row in traffic))},
        ('P-T02', 'no-argument-padding'): {'observed_no_argument_bodies': _unique([row['no_argument_body'] for row in traffic if 'no_argument_body' in row])},
        ('P-F01', 'initial-meta'): {'initial_meta': {key: initial_meta[key] for key in ('mod','usn','media_usn','empty','cont','hostNum')}},
        ('P-F01', 'no-card-metadata'): {'blank_receiver_counts': outcomes['blank_counts'], 'blank_download_schema': blank['ver']},
        ('P-F03', 'cards-local'): {'initial_direction': outcomes['initial_requirement']},
        ('P-F02', 'full-round-trip'): {'full_content_equal': outcomes['full_equal'], 'receiver_integrity': outcomes['integrity']},
        ('P-F02', 'upload-file-form'): {'upload_schema': uploaded['upload_stamp']['ver'], 'upload_response': uploaded['upload_result']},
        ('P-F02', 'download-file-form'): {'download_schemas': [row['ver'] for row in downloads]},
        ('P-F02', 'download-stamps'): {'download_usn_deltas': [downloads[0]['usn'] - uploaded['upload_stamp']['usn'], downloads[1]['usn'] - downloads[0]['usn']],
                                      'schema_creation_stamps_preserved': all(all(row[key] == uploaded['upload_stamp'][key] for key in ('scm', 'crt')) for row in downloads),
                                      'served_ls_preserved': all(row['ls'] == uploaded['upload_stamp']['ls'] for row in downloads)},
    }
    if result.get('full_only'): return values
    values.update({
        ('P-N10', 'no-change'): {'no_change_routes': outcomes['no_change_routes'], 'normal_content_equal': outcomes['normal_equal']},
        ('P-M01', 'envelopes'): {'media_envelopes': _unique([row['media_envelope_members'] for row in media]), 'all_media_errors_empty': all(row['media_error_empty'] for row in media)},
        ('P-M01', 'empty-sk'): {'empty_sk_observed': any(row.get('media_begin', {}).get('sk_empty') for row in media),
                               'response_mutations': _unique([row['response_mutation'] for row in media if 'response_mutation' in row]),
                               'fresh_media_equal': outcomes['fresh_media_hashes'] == outcomes['media_hashes']},
        ('P-M04', 'add'): {'media_upload_results': [row['media_result'] for row in media if row['route'].endswith('/uploadChanges')]},
        ('P-M07', 'two-way'): {'forward_media_equal': outcomes['media_forward'], 'reverse_media_equal': outcomes['media_reverse']},
        ('P-M07', 'no-change-no-retransmission'): {'no_change_media_routes': outcomes['no_change_media_routes']},
    })
    for direction in ('upload', 'download'):
        for same in (True, False):
            row = admission[direction + ('_same_session' if same else '_other_session')]
            values[('P-F06', direction + ('-same-key-round' if same else '-other-key-round'))] = {
                'start_status': row['start']['status'], 'pending_grave_status': row['pending_grave']['status'],
                'full_transfer': row['full_transfer'], 'full_transfer_sessions':row['full_transfer_sessions'],
                'receiver_graves': row.get('receiver_graves'), 'stale_finish_status': row['stale_finish']['status']}
    return values


def record(result, out, invocation, package=None, input_store=None):
    if result.get('probe_kind') == 'candidate': contract = candidate_selection(result['phase'])
    else: contract = edge_selection() if result.get('probe_kind') == 'edges' else selection()
    store = Artifacts(out / 'artifacts')
    package_hash = None
    if package is not None:
        for key in ('executable_artifact_sha256','lock_artifact_sha256','mg_build_receipt_sha256','execution_binding_sha256'):
            store.put(input_store.read(package[key]))
        package_hash = store.put_json(package)
    expected_fixture = EDGE_FIXTURE if result.get('probe_kind') == 'edges' else FIXTURE
    require(same_value(result['fixture'],expected_fixture),'returned fixture differs from the supplied original input')
    fixture_bytes = encoded(expected_fixture)
    fixture_input = store.put(fixture_bytes)
    fixture_manifest = {'format': 'meguri-sync-fixtures-v1', 'entries': [{'sha256': fixture_input, 'bytes': len(fixture_bytes), 'kind': 'original-synthetic-json'}]}
    fixture_hash = store.put_json(fixture_manifest)
    runtime_hash = store.put_json(result['runtime'])
    actual = edge_observations(result) if result.get('probe_kind') == 'edges' else observations(result)
    invocation['selection'] = [row['id'] for row in contract['cases']]
    manifest = {key: copy.deepcopy(invocation[key]) for key in ('source_commit','batch_base','source_sha256','lock_sha256','mg_job','arguments')}
    if package is not None: manifest['executable_sha256'] = package['server']['executable_sha256']
    manifest.update(format='meguri-sync-server-v1', baseline_release='26.09.3', document_kind='evidence',
                    expected_case_count=len(contract['cases']), selected_case_count=len(contract['cases']), completed_case_count=len(contract['cases']), cases=[])
    for selected in contract['cases']:
        case = {key: copy.deepcopy(selected[key]) for key in ('id','family','classification','spec_ids','variants','expected')}
        case['lane'] = {'kind':contract['lane_kind'], 'client': result['runtime']['client'], 'server': result['runtime']['server']}
        case['fixture_sha256'] = fixture_hash
        case['actions'] = [{**copy.deepcopy(action), 'fixture_sha256':fixture_input} for action in selected['actions']]
        case['observed'] = [{'name': name, 'value': value} for name, value in actual[(case['family'], case['variants'][0])].items()]
        case['status'] = 'matched' if same_value(unique_named(case['expected']), unique_named(case['observed'])) else 'different'
        if case['status'] == 'different': case['reason'] = 'Named current-reference observations differ from the selected contract.'
        bound = binding(manifest, case)
        observation_hash = store.put_json({'format': 'meguri-sync-neutral_observation-v1', 'binding': bound, 'details': result})
        receipt_hash = store.put_json({'format': 'meguri-sync-mg_receipt-v1', 'binding': bound, 'run_id': invocation['run_id'],
                                      'selection': invocation['selection'], 'completed_ids': invocation['selection'], 'runtime_receipt_sha256': runtime_hash})
        case['artifacts'] = [{'role': role, 'sha256': digest, 'bytes': len(store.read(digest))} for role, digest in (
            ('synthetic_input', fixture_hash), ('neutral_observation', observation_hash), ('mg_receipt', receipt_hash))]
        if package_hash is not None: case['artifacts'].append({'role':'package','sha256':package_hash,'bytes':len(store.read(package_hash))})
        manifest['cases'].append(case)
    (out / 'selection.json').write_bytes(encoded(contract))
    (out / 'invocation.json').write_bytes(encoded(invocation))
    (out / 'evidence.json').write_bytes(encoded(manifest))
    (out / 'fixture.json').write_bytes(fixture_bytes)
    (out / 'fixture-manifest.json').write_bytes(encoded(fixture_manifest))
    schema = decode(pathlib.Path('/opt/reference/schema.json').read_bytes())
    summary = validate(manifest, contract, schema, store, invocation, result['runtime'], require_credit=False)
    (out / 'summary.json').write_bytes(encoded({'total': summary['selected'], 'passed': sum(case['status'] == 'matched' for case in manifest['cases']),
                                             'failed': sum(case['status'] != 'matched' for case in manifest['cases']), 'selection_credit': summary['credit'],
                                             'expected_case_count': summary['expected'], 'selected_case_count': summary['selected'], 'completed_case_count': summary['completed'],
                                             'broad_catalog_credit': False}))
    return summary['credit']


def validate_existing(source, out, current, trusted_runtime, seal):
    manifest = decode((source / 'evidence.json').read_bytes())
    contract = decode((source / 'selection.json').read_bytes())
    invocation_path = source / 'execution-invocation.json'
    invocation = decode((invocation_path if invocation_path.exists() else source / 'invocation.json').read_bytes())
    if contract['lane_kind'] == 'candidate': expected_contract = candidate_selection(contract['phase'])
    else: expected_contract = edge_selection() if invocation['arguments'][0] == 'edges' else selection()
    require(same_value(contract, expected_contract), 'retained selection differs from the executable contract')
    require(current['source_sha256'] == manifest['source_sha256'] and current['lock_sha256'] == manifest['lock_sha256'], 'retained evidence source bytes are stale')
    schema = decode(pathlib.Path('/opt/reference/schema.json').read_bytes())
    old_store = Artifacts(source / 'artifacts')
    validate(manifest, contract, schema, old_store, invocation, trusted_runtime)
    if seal:
        require(all(case['status'] == 'matched' for case in manifest['cases']), 'seal accepts newly matched execution evidence only')
        final_result = decode((source / 'result.json').read_bytes())
        review = decode((source / 'review.json').read_bytes())
        require(review.get('format') == 'meguri-sync-review-batch-v1' and review.get('outcome') == 'accepted', 'accepted independent review required')
        require(review.get('role') == 'oracle-maintainer', 'oracle-maintainer review required')
        require(review.get('reviewer_session') and review['reviewer_session'] != invocation['author_session'], 'review is not independent')
        expected_bindings = [binding(manifest, case) for case in manifest['cases']]
        require(same_value(review.get('bindings'), expected_bindings), 'review binds different source or cases')
        store = Artifacts(out / 'artifacts')
        for path in old_store.root.iterdir():
            store.put(old_store.read(path.name))
        review_hash = store.put_json(review)
        result_hash = store.put_json(final_result)
        for case in manifest['cases']:
            case['status'] = 'sealed'
            for artifact in case['artifacts']:
                if artifact['role'] == 'mg_receipt':
                    receipt = store.json(artifact['sha256'])
                    receipt['final_result_sha256'] = result_hash
                    artifact['sha256'] = store.put_json(receipt)
                    artifact['bytes'] = len(store.read(artifact['sha256']))
            per_case = {'format': 'meguri-sync-review-v1', 'binding': binding(manifest, case), 'outcome': 'accepted',
                        'reviewer_session': review['reviewer_session'], 'review_batch_sha256': review_hash}
            digest = store.put_json(per_case)
            case['artifacts'].append({'role': 'independent_review', 'sha256': digest, 'bytes': len(store.read(digest))})
        summary = validate(manifest, contract, schema, store, invocation, trusted_runtime)
        for name, value in [('evidence.json',manifest),('selection.json',contract),('execution-invocation.json',invocation)]:
            (out / name).write_bytes(encoded(value))
    else:
        summary = validate(manifest, contract, schema, old_store, invocation, trusted_runtime)
    (out / 'summary.json').write_bytes(encoded({'total': summary['selected'], 'passed': summary['selected'], 'failed': 0,
                                             'expected_case_count': summary['expected'], 'selected_case_count': summary['selected'], 'completed_case_count': summary['completed'],
                                             'selection_credit': summary['credit'], 'broad_catalog_credit': False}))
    return summary['credit']
