# SPDX-License-Identifier: AGPL-3.0-or-later
"""Bounded public-interface discovery; never inspect runtime implementation source."""
import inspect
import pathlib
import tempfile


def discover(api):
    with tempfile.TemporaryDirectory(prefix='recovery-discovery-') as scratch:
        col,_=api.collection(pathlib.Path(scratch),'discover',api.FIXTURE)
        try:
            interfaces={}
            for label,obj in [('collection',col),('media',col.media)]:
                names={}
                for name in dir(obj):
                    if name.startswith('_') or not any(word in name.lower() for word in ('sync','media','reset','check','scan','full','reopen')):
                        continue
                    value=getattr(obj,name)
                    if not callable(value): continue
                    try: signature=str(inspect.signature(value))
                    except (ValueError,TypeError): signature='signature unavailable'
                    names[name]=signature
                interfaces[label]=names
            return {'runtime':api.runtime(),'public_interfaces':interfaces,'source_inspection':False}
        finally:
            col.close(downgrade=False)

# P-M07 reference mechanisms only. C8's P-R families require a candidate lane.
RECOVERY_FIXTURE = {
    'format':'meguri-sync-recovery-v1', 'base':__import__('synthetic').FIXTURE,
    'histories':['ahead','equal-reused','delayed-overtaken'],
    'media_names':['recovery-one.bin','recovery-two.bin'],
    'config_key':'recovery_authored', 'config_value':{'source':'Recovery','number':37},
    'snapshot_payloads':['Recovery snapshot 0','Recovery snapshot 1'],
    'abandoned_payloads':['Recovery abandoned 0','Recovery abandoned 1'],
    'current_payloads':['Recovery current 0','Recovery current 1','Recovery current 2','Recovery current 3'],
    'offline_edit':'Recovery offline edit','seed_review_ease':3,'offline_review_ease':2,'schema_delta_ms':1000,
    'limits':['Reference SDK mechanism only; no candidate restore, GUI or Mobile acceptance.',
              'Stopped whole-root rollback and credential replacement are controlled fixtures.',
              'Explicit server-authoritative download intentionally discards offline edits/review.']}


def definitions():
    return [(history,{
        'cursor_precondition':True,'equal_mod_different_scm':True,
        'old_key_rejected':True,'renewed_credentials':True,
        'explicit_download_count':1,'no_offline_upload':True,
        'collection_hash_equal':True,'review_schedule_equal':True,
        'media_hashes_equal':True,'reset_listing_starts_at_zero':True,
        'fresh_receiver_equal':True,'reopened_equal':True,'ordinary_media_hashes_equal':False,'offline_review_added':True,
        'config_value_bytes_equal':True,'config_changed_columns':['usn','mtime_secs']}) for history in RECOVERY_FIXTURE['histories']]


def recovery_selection():
    return {'format':'meguri-sync-selection-v1','lane_kind':'reference',
      'scope':'Three finite SDK reset/full-download mechanisms; no P-R recovery acceptance or GUI/Mobile credit.',
      'cases':[{'id':'P-M07:desktop.reference.recovery-'+history,'family':'P-M07','classification':'current_probe',
        'spec_ids':['sync/server-media#M5','sync/server-lifecycle#L6'], 'variants':[history],'client_kind':'desktop',
        'expected':[{'name':key,'value':value} for key,value in expected.items()],
        'actions':[{'actor':'fault_driver','operation':'prepare_synthetic','description':'Prepare stopped whole-root rollback, changed credentials and measured cursor/stamp histories on original synthetic data.'},
                   {'actor':'client_b','operation':'client_sync','description':'Authenticate again, explicitly download the authoritative collection, invoke the public media reset and synchronize.'},
                   {'actor':'fresh_client','operation':'client_sync','description':'Independently receive and compare collection semantic hashes, reviews, scheduling and every media byte hash.'}]}
      for history,expected in definitions()]}


def logical(col):
    from validation import encoded,sha
    tables={};semantic={}
    for (name,) in col.db.all("select name from sqlite_master where type='table' and name <> 'col' and name not glob 'sqlite_*' order by name"):
        rows=col.db.all('select * from "'+name.replace('"','""')+'"')
        tables[name]=sorted([[{'blob':v.hex()} if isinstance(v,bytes) else v for v in row] for row in rows],key=encoded)
        columns=[row[1] for row in col.db.all('pragma table_info("'+name.replace('"','""')+'")')]
        semantic[name]=[[v for column,v in zip(columns,row) if not (name=='config' and column in ('usn','mtime_secs'))] for row in tables[name]]
    return {'sha256':sha(encoded(tables)),'semantic_sha256':sha(encoded(semantic)),
            'table_hashes':{k:sha(encoded(v)) for k,v in tables.items()},'table_counts':{k:len(v) for k,v in tables.items()},
            'reviews':tables['revlog'],'cards':tables['cards'],'config_rows':tables['config'],
            'config_columns':[row[1] for row in col.db.all('pragma table_info(config)')]}


def run(api,fixture):
    import shutil
    import time
    from media import hashes
    from validation import encoded,require,same_value
    require(same_value(fixture,RECOVERY_FIXTURE),'unknown recovery fixture')
    discovery=discover(api)
    require(discovery['public_interfaces']['media'].get('force_resync')=="() -> 'None'",'public reset interface unavailable')
    results={};details={}
    def progress():
        pathlib.Path('/out/recovery-progress.json').write_bytes(encoded({'runtime':api.runtime(),'results':results,'details':details}))
    for history in fixture['histories']:
        with tempfile.TemporaryDirectory(prefix='recovery-reference-') as scratch:
            root=pathlib.Path(scratch);clients=[]
            detail={'fixture_setup':'controlled stopped whole-root rollback; public SDK traffic is recorded separately'}
            details[history]=detail;progress()
            api.PHASE='recovery-'+history+'-seed'
            try:
                a,model=api.collection(root,'authority',fixture['base']);clients.append(a)
                for note in fixture['base']['notes']+[fixture['base']['second_note']]: api.add_note(a,model,note)
                require(a.note_count()==2,'recovery fixture needs two notes')
                a.set_config(fixture['config_key'],fixture['config_value'])
                card=a.sched.getCard();require(card is not None,'seed review unavailable');a.sched.answerCard(card,fixture['seed_review_ease'])
                b,_=api.collection(root,'existing',fixture['base']);clients.append(b)
                names=fixture['media_names']
                for index,name in enumerate(names): a.media.write_data(name,fixture['snapshot_payloads'][index].encode())
                with api.reference_server(root/'server') as (port,user,password),api.recorder(port) as rec:
                    rec.capture_media=True
                    endpoint=f'http://127.0.0.1:{rec.server_port}/'
                    aa=a.sync_login(username=user,password=password,endpoint=endpoint)
                    ab=b.sync_login(username=user,password=password,endpoint=endpoint)
                    api.full(a,aa,True);api.full(b,ab,False)
                    api.media_sync(a,aa);api.media_sync(b,ab)
                    detail['snapshot_media_hashes']=hashes(b)
                    detail['snapshot_traffic']=list(rec.observations)
                shutil.copytree(root/'server',root/'backup')
                # Continue the original history two mutations beyond the snapshot.
                with api.reference_server(root/'server',(user,password)) as (port,_,_),api.recorder(port) as rec:
                    rec.capture_media=True;endpoint=f'http://127.0.0.1:{rec.server_port}/'
                    aa=a.sync_login(username=user,password=password,endpoint=endpoint)
                    old_auth=b.sync_login(username=user,password=password,endpoint=endpoint)
                    for index,name in enumerate(names):
                        time.sleep(1.05);path=pathlib.Path(a.media.dir())/name
                        path.write_bytes(fixture['abandoned_payloads'][index].encode());path.parent.touch()
                        api.media_sync(a,aa);api.media_sync(b,old_auth)
                    changes=[change for row in rec.observations if row['route']=='/msync/mediaChanges' for change in row['media_result']]
                    client_cursor=max(change[1] for change in changes)
                    detail['abandoned_media_hashes']=hashes(b)
                    detail['abandoned_traffic']=list(rec.observations)
                # The reference server is stopped before every copy. No layout knowledge is used.
                shutil.rmtree(root/'server');shutil.copytree(root/'backup',root/'server')
                replacement_password=password+'-renewed'
                with api.reference_server(root/'server',(user,replacement_password)) as (port,_,_),api.recorder(port) as rec:
                    rec.capture_media=True;rec.capture_normal=True;endpoint=f'http://127.0.0.1:{rec.server_port}/'
                    aa=a.sync_login(username=user,password=replacement_password,endpoint=endpoint)
                    api.full(a,aa,False);a.media.force_resync();api.media_sync(a,aa)
                    # New legitimate changes reuse then overtake the abandoned cursor.
                    for index in range(0 if history=='ahead' else 2 if history=='equal-reused' else 4):
                        time.sleep(1.05);path=pathlib.Path(a.media.dir())/names[0 if index==0 else 1]
                        path.write_bytes(fixture['current_payloads'][index].encode());path.parent.touch()
                        api.media_sync(a,aa)
                    server_usn=next(row['media_begin']['usn'] for row in reversed(rec.observations) if 'media_begin' in row)
                    # The latest upload can advance past its begin; a settled begin measures the final head.
                    api.media_sync(a,aa)
                    server_usn=next(row['media_begin']['usn'] for row in reversed(rec.observations) if 'media_begin' in row)
                    expected=logical(a);expected_media=hashes(a)
                    detail.update(client_cursor=client_cursor,restored_server_usn=server_usn,
                                  expected_collection=expected,expected_media_hashes=expected_media)
                    api.PHASE='recovery-'+history+'-offline-edit'
                    note=b.get_note(b.db.scalar('select id from notes order by id limit 1'))
                    note.fields[0]=fixture['offline_edit'];b.update_note(note)
                    card=b.sched.getCard();require(card is not None,'offline review unavailable');b.sched.answerCard(card,fixture['offline_review_ease'])
                    detail['offline_collection']=logical(b)
                    # Controlled equal-mod/different-scm fixture; not an SDK-emitted mutation.
                    a.sync_collection(aa,sync_media=False)
                    server_meta=next(row['meta'] for row in reversed(rec.observations) if 'meta' in row)
                    b.db.execute('update col set mod=?, scm=?',server_meta['mod'],server_meta['scm']+fixture['schema_delta_ms'])
                    b.close(downgrade=False);clients.remove(b)
                    b=api.Collection(str(root/'existing'/'collection.anki2'));clients.append(b)
                    detail['pre_recovery_stamps']=dict(zip(('mod','scm'),b.db.first('select mod,scm from col')))
                    old_auth.endpoint=endpoint
                    before=len(rec.observations)
                    rejected=False
                    try: b.sync_collection(old_auth,sync_media=False)
                    except (api.NetworkError,api.SyncError): rejected=True
                    denied=rec.observations[before:]
                    detail['old_credential_traffic']=denied
                    ab=b.sync_login(username=user,password=replacement_password,endpoint=endpoint)
                    before=len(rec.observations)
                    ordinary=b.sync_collection(ab,sync_media=False)
                    detail['ordinary_collection_required']=ordinary.required
                    detail['ordinary_collection_traffic']=rec.observations[before:]
                    # Observe ordinary media behavior before exercising the explicit repair.
                    before=len(rec.observations)
                    api.media_sync(b,ab)
                    detail['ordinary_media_traffic']=rec.observations[before:]
                    detail['ordinary_media_hashes']=hashes(b)
                    api.PHASE='recovery-'+history+'-explicit-download-reset'
                    before=len(rec.observations);api.full(b,ab,False)
                    full_rows=rec.observations[before:]
                    b.media.force_resync();before=len(rec.observations);api.media_sync(b,ab)
                    reset_rows=rec.observations[before:]
                    detail['explicit_full_traffic']=full_rows;detail['reset_media_traffic']=reset_rows
                    actual=logical(b);actual_media=hashes(b)
                    detail['recovered_collection']=actual;detail['recovered_media_hashes']=actual_media
                    c,_=api.collection(root,'fresh',fixture['base']);clients.append(c)
                    ac=c.sync_login(username=user,password=replacement_password,endpoint=endpoint)
                    api.full(c,ac,False);api.media_sync(c,ac)
                    fresh=logical(c);fresh_media=hashes(c)
                    detail['fresh_collection']=fresh;detail['fresh_media_hashes']=fresh_media
                    b.close(downgrade=False);clients.remove(b)
                    b=api.Collection(str(root/'existing'/'collection.anki2'));clients.append(b)
                    reopened_before_sync=logical(b)
                    b.sync_collection(ab,sync_media=False);api.media_sync(b,ab)
                    reopened_after_sync=logical(b)
                    detail['reopened_before_sync']=reopened_before_sync;detail['reopened_after_sync']=reopened_after_sync
                    before_config={row[0]:row for row in expected['config_rows']}
                    after_config={row[0]:row for row in reopened_after_sync['config_rows']}
                    changed_columns=[name for index,name in enumerate(expected['config_columns'])
                        if any(before_config[key][index]!=after_config[key][index] for key in before_config)]
                    detail['post_normal_config_changed_columns']=changed_columns
                    pre=detail['pre_recovery_stamps']
                    results[history]={
                        'cursor_precondition':client_cursor>server_usn if history=='ahead' else client_cursor==server_usn if history=='equal-reused' else client_cursor<server_usn,
                        'equal_mod_different_scm':pre['mod']==server_meta['mod'] and pre['scm']!=server_meta['scm'],
                        'old_key_rejected':rejected and bool(denied) and all(row['status'] in (401,403) for row in denied),
                        'renewed_credentials':True,
                        'explicit_download_count':sum(row['route']=='/sync/download' and row['status']==200 for row in full_rows),
                        'no_offline_upload':not any(row['route'] in ('/sync/upload','/sync/applyChanges','/sync/applyChunk') for row in detail['ordinary_collection_traffic']+full_rows),
                        'collection_hash_equal':actual['sha256']==expected['sha256'],
                        'review_schedule_equal':actual['reviews']==expected['reviews'] and actual['cards']==expected['cards'],
                        'media_hashes_equal':actual_media==expected_media,
                        'reset_listing_starts_at_zero':next((row['media_request']['lastUsn'] for row in reset_rows if row['route']=='/msync/mediaChanges'),None)==0,
                        'fresh_receiver_equal':fresh['sha256']==expected['sha256'] and fresh_media==expected_media,
                        'reopened_equal':reopened_before_sync['sha256']==expected['sha256'] and reopened_after_sync['semantic_sha256']==expected['semantic_sha256'] and hashes(b)==expected_media,
                        'ordinary_media_hashes_equal':detail['ordinary_media_hashes']==expected_media,
                        'offline_review_added':len(detail['offline_collection']['reviews'])==len(expected['reviews'])+1,
                        'config_value_bytes_equal':{key:row[3] for key,row in before_config.items()}=={key:row[3] for key,row in after_config.items()},
                        'config_changed_columns':changed_columns}
                    detail['traffic']=list(rec.observations)
                    require(not rec.errors,'recovery recording incomplete');progress()
            finally:
                for col in clients: col.close(downgrade=False)
    return {'probe_kind':'recovery','fixture':fixture,'runtime':api.runtime(),'public_api':discovery['public_interfaces'],
            'recovery_results':results,'recovery_details':details}
