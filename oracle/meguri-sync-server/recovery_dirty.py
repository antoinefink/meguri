# SPDX-License-Identifier: AGPL-3.0-or-later
"""One original dirty-media fixture; public SDK calls only, no runtime source inspection."""
import pathlib
import shutil
import tempfile
import time

from media import hashes
from recovery import RECOVERY_FIXTURE, logical
from validation import encoded, require, same_value, sha

DIRTY_FIXTURE = {
    'format':'meguri-sync-dirty-recovery-v1','base':RECOVERY_FIXTURE,
    'cursor_scenario':'equal','added_name':'recovery-offline-only.bin',
    'added_payload':'Recovery original offline addition',
    'replacement_payload':'Recovery original offline replacement',
    'actions':['add-local-only-file','replace-first-existing-file','delete-second-existing-file'],
    'limits':['One reference SDK fixture; no candidate, GUI, Mobile or universal conflict-rule claim.',
              'Offline disk changes are real unsynchronized file operations, not injected media responses.',
              'The untouched closed old profile is preserved before the in-place attempt.']}


def dirty_selection():
    observed={'equal_cursor':True,'dirty_action_count':3,'old_key_rejected':True,
              'renewed_credentials':True,'collection_snapshot_equal':True,'card_review_rows_equal':True,
              'full_download_count':1,'reset_starts_zero':True,'media_snapshot_equal':False,
              'server_witness_media_equal':False,'inplace_exact_snapshot_plus_added_file':True,
              'witness_exact_snapshot_plus_added_file':True,'preserved_profile_bytes_unchanged':True}
    clean={'collection_snapshot_equal':True,'card_review_rows_equal':True,'media_snapshot_equal':True,
           'independent_receiver_snapshot_equal':True,'reopened_snapshot_equal':True,
           'media_upload_count':0,'preserved_profile_bytes_unchanged':True,'server_restored_from_protected_snapshot':True}
    cases=[]
    for variant,expected in [('dirty-inplace-observed',observed),('dirty-clean-receiver',clean)]:
        cases.append({'id':'P-M07:desktop.reference.recovery-'+variant,'family':'P-M07','classification':'current_probe',
          'spec_ids':['sync/server-media#M5','sync/server-lifecycle#L6'],'variants':[variant],'client_kind':'desktop',
          'expected':[{'name':key,'value':value} for key,value in expected.items()],
          'actions':[{'actor':'fault_driver','operation':'prepare_synthetic','description':'Preserve a stopped reference snapshot and a closed dirty profile after one offline media addition, replacement and deletion, plus offline collection editing and review.'},
                     {'actor':'client_b','operation':'client_sync','description':'Observe the in-place reset outcome without calling a media-snapshot mismatch a recovered result.'},
                     {'actor':'fault_driver','operation':'prepare_synthetic','description':'After the diagnostic attempt, reestablish the protected server snapshot before a separate clean-receiver procedure.'},
                     {'actor':'fresh_client','operation':'client_sync','description':'Leave the preserved dirty profile offline; use renewed credentials and fresh collection/media state to download and verify the chosen snapshot.'}]})
    return {'format':'meguri-sync-selection-v1','lane_kind':'reference',
      'scope':'Two facts from one dirty-media fixture: in-place recovery fails exact media equality; a separate clean receiver is compared exactly with the protected chosen snapshot.',
      'cases':cases}


def profile_manifest(directory):
    entries={}
    for path in sorted(directory.rglob('*')):
        require(not path.is_symlink(),'synthetic profile unexpectedly contains a link')
        if path.is_file(): entries[str(path.relative_to(directory))]={'bytes':path.stat().st_size,'sha256':sha(path.read_bytes())}
    return entries


def run(api,fixture):
    require(same_value(fixture,DIRTY_FIXTURE),'unknown dirty recovery fixture')
    base=fixture['base'];details={};results={}
    def progress():
        pathlib.Path('/out/recovery-dirty-progress.json').write_bytes(encoded({'runtime':api.runtime(),'details':details,'results':results}))
    with tempfile.TemporaryDirectory(prefix='dirty-recovery-reference-') as scratch:
        root=pathlib.Path(scratch);clients=[]
        api.PHASE='dirty-recovery-seed'
        try:
            a,model=api.collection(root,'authority',base['base']);clients.append(a)
            for note in base['base']['notes']+[base['base']['second_note']]:api.add_note(a,model,note)
            a.set_config(base['config_key'],base['config_value'])
            card=a.sched.getCard();require(card is not None,'seed review unavailable');a.sched.answerCard(card,base['seed_review_ease'])
            b,_=api.collection(root,'existing',base['base']);clients.append(b)
            names=base['media_names']
            for index,name in enumerate(names):a.media.write_data(name,base['snapshot_payloads'][index].encode())
            with api.reference_server(root/'server') as (port,user,password),api.recorder(port) as rec:
                rec.capture_media=True;endpoint=f'http://127.0.0.1:{rec.server_port}/'
                aa=a.sync_login(username=user,password=password,endpoint=endpoint)
                old_auth=b.sync_login(username=user,password=password,endpoint=endpoint)
                api.full(a,aa,True);api.full(b,old_auth,False)
                api.media_sync(a,aa);api.media_sync(b,old_auth)
                expected=logical(b);expected_media=hashes(b)
                details['seed_traffic']=list(rec.observations)
                client_cursor=max(change[1] for row in rec.observations if row['route']=='/msync/mediaChanges' for change in row['media_result'])
            shutil.copytree(root/'server',root/'snapshot')
            api.PHASE='dirty-recovery-offline-actions'
            time.sleep(1.05)
            media_dir=pathlib.Path(b.media.dir())
            (media_dir/fixture['added_name']).write_bytes(fixture['added_payload'].encode())
            (media_dir/names[0]).write_bytes(fixture['replacement_payload'].encode())
            (media_dir/names[1]).unlink();media_dir.touch()
            note=b.get_note(b.db.scalar('select id from notes order by id limit 1'))
            note.fields[0]=base['offline_edit'];b.update_note(note)
            card=b.sched.getCard();require(card is not None,'offline review unavailable');b.sched.answerCard(card,base['offline_review_ease'])
            dirty_collection=logical(b);dirty_media=hashes(b)
            b.close(downgrade=False);clients.remove(b)
            shutil.copytree(root/'existing',root/'preserved-existing')
            preserved_manifest=profile_manifest(root/'preserved-existing')
            b=api.Collection(str(root/'existing'/'collection.anki2'));clients.append(b)
            details.update(expected_collection=expected,expected_media_hashes=expected_media,dirty_collection=dirty_collection,
                           dirty_media_hashes=dirty_media,client_cursor=client_cursor,preserved_profile_manifest=preserved_manifest)
            progress()
            # Only A contacts the post-backup server. B's three media changes stay offline.
            with api.reference_server(root/'server',(user,password)) as (port,_,_),api.recorder(port) as rec:
                endpoint=f'http://127.0.0.1:{rec.server_port}/'
                aa=a.sync_login(username=user,password=password,endpoint=endpoint)
                for index,name in enumerate(names):
                    time.sleep(1.05);path=pathlib.Path(a.media.dir())/name
                    path.write_bytes(base['abandoned_payloads'][index].encode());path.parent.touch();api.media_sync(a,aa)
                details['abandoned_server_traffic']=list(rec.observations)
            shutil.rmtree(root/'server');shutil.copytree(root/'snapshot',root/'server')
            replacement_password=password+'-renewed'
            api.PHASE='dirty-recovery-inplace-attempt'
            with api.reference_server(root/'server',(user,replacement_password)) as (port,_,_),api.recorder(port) as rec:
                rec.capture_media=True;endpoint=f'http://127.0.0.1:{rec.server_port}/'
                old_auth.endpoint=endpoint;before=len(rec.observations);rejected=False
                try:b.sync_collection(old_auth,sync_media=False)
                except (api.NetworkError,api.SyncError):rejected=True
                denied=rec.observations[before:]
                ab=b.sync_login(username=user,password=replacement_password,endpoint=endpoint)
                before=len(rec.observations);api.full(b,ab,False);full_rows=rec.observations[before:]
                collection_after=logical(b)
                b.media.force_resync();before=len(rec.observations);api.media_sync(b,ab);reset_rows=rec.observations[before:]
                media_after=hashes(b)
                c,_=api.collection(root,'witness',base['base']);clients.append(c)
                ac=c.sync_login(username=user,password=replacement_password,endpoint=endpoint)
                api.full(c,ac,False);api.media_sync(c,ac)
                witness_collection=logical(c);witness_media=hashes(c)
                server_usn=next(row['media_begin']['usn'] for row in reset_rows if 'media_begin' in row)
                details.update(restored_server_usn=server_usn,old_credential_traffic=denied,full_traffic=full_rows,
                               reset_media_traffic=reset_rows,inplace_collection=collection_after,inplace_media_hashes=media_after,
                               witness_collection=witness_collection,witness_media_hashes=witness_media)
                results['dirty-inplace-observed']={'equal_cursor':client_cursor==server_usn,
                    'dirty_action_count':int(fixture['added_name'] in dirty_media)+int(dirty_media.get(names[0])!=expected_media[names[0]])+int(names[1] not in dirty_media),
                    'old_key_rejected':rejected and bool(denied) and all(row['status']==403 for row in denied),
                    'renewed_credentials':True,'collection_snapshot_equal':collection_after['sha256']==expected['sha256'],
                    'card_review_rows_equal':collection_after['cards']==expected['cards'] and collection_after['reviews']==expected['reviews'],
                    'full_download_count':sum(row['route']=='/sync/download' and row['status']==200 for row in full_rows),
                    'reset_starts_zero':next((row['media_request']['lastUsn'] for row in reset_rows if row['route']=='/msync/mediaChanges'),None)==0,
                    'media_snapshot_equal':media_after==expected_media,'server_witness_media_equal':witness_media==expected_media,
                    'inplace_exact_snapshot_plus_added_file':media_after=={**expected_media,fixture['added_name']:sha(fixture['added_payload'].encode())},
                    'witness_exact_snapshot_plus_added_file':witness_media=={**expected_media,fixture['added_name']:sha(fixture['added_payload'].encode())},
                    'preserved_profile_bytes_unchanged':profile_manifest(root/'preserved-existing')==preserved_manifest}
                require(not rec.errors,'dirty recovery recording incomplete');progress()
            # The diagnostic attempt may change the server. Reestablish the protected
            # chosen snapshot before testing the independent clean-receiver procedure.
            shutil.rmtree(root/'server');shutil.copytree(root/'snapshot',root/'server')
            restored_manifest=profile_manifest(root/'server')
            snapshot_manifest=profile_manifest(root/'snapshot')
            api.PHASE='dirty-recovery-clean-receiver'
            with api.reference_server(root/'server',(user,replacement_password)) as (port,_,_),api.recorder(port) as rec:
                rec.capture_media=True;endpoint=f'http://127.0.0.1:{rec.server_port}/'
                clean,_=api.collection(root,'clean-receiver',base['base']);clients.append(clean)
                auth=clean.sync_login(username=user,password=replacement_password,endpoint=endpoint)
                api.full(clean,auth,False);api.media_sync(clean,auth)
                clean_collection=logical(clean);clean_media=hashes(clean)
                clean.close(downgrade=False);clients.remove(clean)
                clean=api.Collection(str(root/'clean-receiver'/'collection.anki2'));clients.append(clean)
                reopened_collection=logical(clean);api.media_sync(clean,auth);reopened_media=hashes(clean)
                independent,_=api.collection(root,'clean-witness',base['base']);clients.append(independent)
                auth2=independent.sync_login(username=user,password=replacement_password,endpoint=endpoint)
                api.full(independent,auth2,False);api.media_sync(independent,auth2)
                independent_collection=logical(independent);independent_media=hashes(independent)
                details.update(clean_collection=clean_collection,clean_media_hashes=clean_media,
                               clean_reopened_collection=reopened_collection,clean_reopened_media_hashes=reopened_media,
                               independent_collection=independent_collection,independent_media_hashes=independent_media,
                               clean_traffic=list(rec.observations),protected_snapshot_manifest=snapshot_manifest,
                               clean_server_manifest_before_serve=restored_manifest)
                results['dirty-clean-receiver']={
                    'collection_snapshot_equal':clean_collection['sha256']==expected['sha256'],
                    'card_review_rows_equal':clean_collection['cards']==expected['cards'] and clean_collection['reviews']==expected['reviews'],
                    'media_snapshot_equal':clean_media==expected_media,
                    'independent_receiver_snapshot_equal':independent_collection['sha256']==expected['sha256'] and independent_media==expected_media,
                    'reopened_snapshot_equal':reopened_collection['sha256']==expected['sha256'] and reopened_media==expected_media,
                    'media_upload_count':sum(row['route']=='/msync/uploadChanges' for row in rec.observations),
                    'preserved_profile_bytes_unchanged':profile_manifest(root/'preserved-existing')==preserved_manifest,
                    'server_restored_from_protected_snapshot':restored_manifest==snapshot_manifest}
                require(not rec.errors,'clean recovery recording incomplete');progress()
        finally:
            for col in clients:col.close(downgrade=False)
    return {'probe_kind':'recovery-dirty','fixture':fixture,'runtime':api.runtime(),'dirty_results':results,'dirty_details':details}
