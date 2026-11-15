# SPDX-License-Identifier: AGPL-3.0-or-later
"""Produce neutral, content-addressed observations with executable selections."""
from __future__ import annotations

import copy
import pathlib

from validation import Artifacts, binding, decode, encoded, require, same_value, sha, unique_named, validate


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
        ('P-M01', 'empty-sk', 'sync/server-media#M1', {'empty_sk_observed': True, 'fresh_media_equal': True}),
        ('P-M04', 'add', 'sync/server-media#M3', {'media_upload_results': [[1, 1], [1, 2]]}),
        ('P-M07', 'two-way', 'sync/server-media#M1', {'forward_media_equal': True, 'reverse_media_equal': True}),
        ('P-M07', 'no-change-no-retransmission', 'sync/server-media#M1', {'no_change_media_routes': ['/msync/begin']}),
        ('P-N01', 'same-key-start', 'sync/server-lifecycle#L2', {'start_statuses': [200, 200]}),
        ('P-N01', 'other-key-start', 'sync/server-lifecycle#L2', {'replacement_status': 200, 'displaced_chunk_status': 409}),
        ('P-N01', 'abort-absent', 'sync/server-lifecycle#L2', {'abort_statuses': [200, 200], 'abort_results': [None, None]}),
    ]
    for direction in ('upload', 'download'):
        for same in (True, False):
            definitions.append(('P-F06', direction + ('-same-key-round' if same else '-other-key-round'),
                                'sync/server-lifecycle#L3', {'start_status': 200, 'pending_grave_status': 200,
                                'full_transfer': 'ok', 'receiver_graves': 0, 'stale_finish_status': 409}))
    return {'format': 'meguri-sync-selection-v1', 'lane_kind': 'reference',
            'scope': 'Desktop synthetic smoke and explicitly named current-server observations only; broad family obligations remain unobserved.',
            'cases': [{'id': f'{family}:desktop.reference.{variant}', 'family': family, 'classification': 'current_probe',
                       'spec_ids': [spec], 'variants': [variant], 'client_kind': 'desktop',
                       'expected': [{'name': name, 'value': value} for name, value in values.items()]}
                      for family, variant, spec, values in definitions]}


def _unique(values):
    output = []
    for value in values:
        if value not in output: output.append(value)
    return output


def observations(result):
    traffic, outcomes, admission = result['traffic'], result['outcomes'], result['session_admission']
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
        ('P-N10', 'no-change'): {'no_change_routes': outcomes['no_change_routes'], 'normal_content_equal': outcomes['normal_equal']},
        ('P-M01', 'envelopes'): {'media_envelopes': _unique([row['media_envelope_members'] for row in media]), 'all_media_errors_empty': all(row['media_error_empty'] for row in media)},
        ('P-M01', 'empty-sk'): {'empty_sk_observed': any(row.get('media_begin', {}).get('sk_empty') for row in media), 'fresh_media_equal': outcomes['fresh_media_hashes'] == outcomes['media_hashes']},
        ('P-M04', 'add'): {'media_upload_results': [row['media_result'] for row in media if row['route'].endswith('/uploadChanges')]},
        ('P-M07', 'two-way'): {'forward_media_equal': outcomes['media_forward'], 'reverse_media_equal': outcomes['media_reverse']},
        ('P-M07', 'no-change-no-retransmission'): {'no_change_media_routes': outcomes['no_change_media_routes']},
        ('P-N01', 'same-key-start'): {'start_statuses': [admission['first_start']['status'], admission['same_session_start']['status']]},
        ('P-N01', 'other-key-start'): {'replacement_status': admission['other_session_start']['status'], 'displaced_chunk_status': admission['displaced_chunk']['status']},
        ('P-N01', 'abort-absent'): {'abort_statuses': [admission['owning_abort']['status'], admission['absent_abort']['status']], 'abort_results': [admission['owning_abort'].get('value'), admission['absent_abort'].get('value')]},
    }
    for direction in ('upload', 'download'):
        for same in (True, False):
            row = admission[direction + ('_same_session' if same else '_other_session')]
            values[('P-F06', direction + ('-same-key-round' if same else '-other-key-round'))] = {
                'start_status': row['start']['status'], 'pending_grave_status': row['pending_grave']['status'],
                'full_transfer': row['full_transfer'], 'receiver_graves': row.get('receiver_graves'), 'stale_finish_status': row['stale_finish']['status']}
    return values


def record(result, out, invocation):
    contract = selection()
    store = Artifacts(out / 'artifacts')
    fixture_bytes = encoded(result['fixture'])
    fixture_input = store.put(fixture_bytes)
    fixture_manifest = {'format': 'meguri-sync-fixtures-v1', 'entries': [{'sha256': fixture_input, 'bytes': len(fixture_bytes), 'kind': 'original-synthetic-json'}]}
    fixture_hash = store.put_json(fixture_manifest)
    runtime_hash = store.put_json(result['runtime'])
    actual = observations(result)
    invocation['selection'] = [row['id'] for row in contract['cases']]
    manifest = {key: copy.deepcopy(invocation[key]) for key in ('source_commit','batch_base','source_sha256','lock_sha256','mg_job','arguments')}
    manifest.update(format='meguri-sync-server-v1', baseline_release='26.09.3', document_kind='evidence',
                    expected_case_count=len(contract['cases']), selected_case_count=len(contract['cases']), completed_case_count=len(contract['cases']), cases=[])
    for selected in contract['cases']:
        case = {key: copy.deepcopy(selected[key]) for key in ('id','family','classification','spec_ids','variants','expected')}
        case['lane'] = {'kind': 'reference', 'client': result['runtime']['client'], 'server': result['runtime']['server']}
        case['fixture_sha256'] = fixture_hash
        case['actions'] = [{'actor': 'client_a', 'operation': 'prepare_synthetic', 'description': 'Create original synthetic clients and media from the retained input recipe.', 'fixture_sha256': fixture_input},
                           {'actor': 'fault_driver' if selected['family'] in {'P-F06','P-N01'} else 'client_a', 'operation': 'observe',
                            'description': 'Run the named Desktop observation; session edge requests are explicitly injected by the local fault driver.', 'fixture_sha256': fixture_input}]
        case['observed'] = [{'name': name, 'value': value} for name, value in actual[(case['family'], case['variants'][0])].items()]
        case['status'] = 'matched' if same_value(unique_named(case['expected']), unique_named(case['observed'])) else 'different'
        if case['status'] == 'different': case['reason'] = 'Named current-reference observations differ from the selected contract.'
        bound = binding(manifest, case)
        observation_hash = store.put_json({'format': 'meguri-sync-neutral_observation-v1', 'binding': bound, 'details': result})
        receipt_hash = store.put_json({'format': 'meguri-sync-mg_receipt-v1', 'binding': bound, 'run_id': invocation['run_id'],
                                      'selection': invocation['selection'], 'completed_ids': invocation['selection'], 'runtime_receipt_sha256': runtime_hash})
        case['artifacts'] = [{'role': role, 'sha256': digest, 'bytes': len(store.read(digest))} for role, digest in (
            ('synthetic_input', fixture_hash), ('neutral_observation', observation_hash), ('mg_receipt', receipt_hash))]
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
    require(same_value(contract, selection()), 'retained selection differs from the executable contract')
    invocation_path = source / 'execution-invocation.json'
    invocation = decode((invocation_path if invocation_path.exists() else source / 'invocation.json').read_bytes())
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
