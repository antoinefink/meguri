# SPDX-License-Identifier: AGPL-3.0-or-later
"""Finite media experiments; every network request is emitted by the public SDK."""
import pathlib
import struct
import tempfile
import time

from synthetic import FIXTURE
from validation import require, same_value, sha


MEDIA_FIXTURE = {
    'format':'meguri-sync-media-v1', 'base':FIXTURE,
    'image_name':'aster-original.svg', 'audio_name':'aster-original.wav',
    'batch_count':26, 'listing_count':1005,
    'payload_prefix':'Meguri original synthetic media payload ',
    'limits':['Actual Desktop SDK only; no crafted requests, Mobile, restart or concurrency credit.',
              'One job-owned account retains mutation history before the batching and listing checks.']}


def definitions():
    return [
        ('P-M07','sdk-bidirectional-mutations', {'forward_add_hashes_equal':True,'reverse_add_hashes_equal':True,
          'forward_replace_hashes_equal':True,'reverse_replace_hashes_equal':True,
          'forward_deleted':True,'reverse_deleted':True,'all_media_statuses_success':True}),
        ('P-M07','sdk-no-change', {'client_a_routes':['/msync/begin'],'client_b_routes':['/msync/begin'],
          'retransmission_count':0,'all_media_statuses_success':True}),
        ('P-M05','sdk-26-downloads', {'download_request_counts':[25,1],'requested_file_count':26,
          'received_file_count':26,'receiver_hashes_equal':True,'all_media_statuses_success':True}),
        ('P-M05','sdk-1005-listing', {'page_lengths':[1000,5,0],'listed_unique_names':1005,
          'ordered_unique_usns':True,'listing_cursor_requests':[0,1006,1011], 'last_change_usn':1011,
          'received_file_count':1005,'fresh_receiver_hashes_equal':True,'existing_receiver_hashes_equal':True,
          'all_media_statuses_success':True}),
    ]


def media_selection():
    cases=[]
    for family,variant,values in definitions():
        cases.append({'id':f'{family}:desktop.reference.media-{variant}', 'family':family,
          'classification':'current_probe', 'spec_ids':['sync/server-media#M3' if family=='P-M05' else 'sync/server-media#M1'],
          'variants':[variant], 'client_kind':'desktop',
          'expected':[{'name':key,'value':value} for key,value in values.items()],
          'actions':[{'actor':'client_a','operation':'prepare_synthetic',
                      'description':'Create original collections, SVG image, WAV audio and generated payloads on one job-owned account.'},
                     {'actor':'client_a','operation':'client_sync',
                      'description':'Run public Desktop media sync calls and retain successful public completion, request counts and cursor observations.'},
                     {'actor':'client_b','operation':'client_sync',
                      'description':'Exchange additions, replacements and deletions in both directions; verify every received byte hash.'},
                     {'actor':'fresh_client','operation':'client_sync',
                      'description':'After the mutation and 26-file checks, receive the final 1005 current files with a fresh SDK cursor.'}]})
    return {'format':'meguri-sync-selection-v1','lane_kind':'reference',
            'scope':'Four finite actual Desktop media groups with retained account history; no whole-family or Mobile credit.', 'cases':cases}


def image(revision):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#{revision:06x}"/></svg>'.encode()


def audio(revision):
    samples = bytes([128 + revision,128 - revision] * 8)
    return b'RIFF'+struct.pack('<I',36+len(samples))+b'WAVEfmt '+struct.pack('<IHHIIHH',16,1,1,8000,8000,1,8)+b'data'+struct.pack('<I',len(samples))+samples


def hashes(col):
    return {entry.name:sha(entry.read_bytes()) for entry in pathlib.Path(col.media.dir()).iterdir() if entry.is_file()}


def success(rows):
    media=[row for row in rows if row['route'].startswith('/msync/')]
    return bool(media) and all(row['status']==200 and row.get('media_error_empty',True) for row in media)


def run(api, fixture, target=None):
    require(same_value(fixture,MEDIA_FIXTURE),'unknown media fixture')
    identity=api.runtime()
    if target is not None: identity['server']=target['server']
    results={}; details={}
    with tempfile.TemporaryDirectory(prefix='media-reference-') as scratch:
        root=pathlib.Path(scratch)
        api.PHASE='media-sdk-setup'
        with api.probe_server(root/'server',target) as (port,user,password), api.recorder(port) as record:
            record.capture_media=True
            clients=[]
            completions=[]
            try:
                a,model=api.collection(root,'a',fixture['base']); clients.append(a)
                api.add_note(a,model,fixture['base']['notes'][0])
                b,_=api.collection(root,'b',fixture['base']); clients.append(b)
                trace=getattr(api,'FINAL_LEGACY',None)
                if trace is not None:trace.prepare(a);trace.authoritative(a,'client_a')
                endpoint=f'http://127.0.0.1:{record.server_port}/'
                aa=trace.call(a,'client_a','login',record,lambda:a.sync_login(username=user,password=password,endpoint=endpoint),comparison='unchanged',auxiliary=True) if trace is not None else a.sync_login(username=user,password=password,endpoint=endpoint)
                ab=trace.call(b,'client_b','login',record,lambda:b.sync_login(username=user,password=password,endpoint=endpoint),comparison='unchanged',auxiliary=True) if trace is not None else b.sync_login(username=user,password=password,endpoint=endpoint)
                if trace is not None:
                    trace.call(a,'client_a','setup-upload',record,lambda:api.full(a,aa,True),auxiliary=True)
                    trace.call(b,'client_b','setup-download',record,lambda:api.full(b,ab,False),auxiliary=True)
                else:api.full(a,aa,True);api.full(b,ab,False)

                def sync(col,auth,actor):
                    before=len(record.observations)
                    if trace is not None:trace.call(col,actor,'media',record,lambda:api.media_sync(col,auth,timeout=120),comparison='media')
                    else:api.media_sync(col,auth,timeout=120)
                    rows=record.observations[before:]
                    require(success(rows),'media SDK traffic failed')
                    completions.append({'actor':actor,'active':False,'public_status_returned':True})
                    return rows

                def exchange(sender,receiver,auth_sender,auth_receiver,actor_sender,actor_receiver,name,data):
                    time.sleep(1.05)
                    path=pathlib.Path(sender.media.dir())/name
                    if data is None: path.unlink()
                    elif path.exists():
                        path.write_bytes(data)
                        # Registration observes the media directory's timestamp too.
                        path.parent.touch()
                    else: require(sender.media.write_data(name,data)==name,'media addition changed the requested name')
                    if trace is not None:trace.authoritative(sender,actor_sender)
                    sync(sender,auth_sender,actor_sender); sync(receiver,auth_receiver,actor_receiver)
                    received=pathlib.Path(receiver.media.dir())/name
                    return not received.exists() if data is None else received.read_bytes()==data and sha(received.read_bytes())==sha(data)

                if trace is not None:trace.use('sdk-bidirectional-mutations')
                api.PHASE='media-sdk-bidirectional-mutations'
                start=len(record.observations)
                name_image,name_audio=fixture['image_name'],fixture['audio_name']
                values={}
                values['forward_add_hashes_equal']=exchange(a,b,aa,ab,'client_a','client_b',name_image,image(1))
                values['reverse_add_hashes_equal']=exchange(b,a,ab,aa,'client_b','client_a',name_audio,audio(1))
                values['forward_replace_hashes_equal']=exchange(a,b,aa,ab,'client_a','client_b',name_audio,audio(2))
                values['reverse_replace_hashes_equal']=exchange(b,a,ab,aa,'client_b','client_a',name_image,image(2))
                values['forward_deleted']=exchange(a,b,aa,ab,'client_a','client_b',name_image,None)
                values['reverse_deleted']=exchange(b,a,ab,aa,'client_b','client_a',name_audio,None)
                rows=record.observations[start:]
                values['all_media_statuses_success']=success(rows)
                results['sdk-bidirectional-mutations']=values
                details['sdk-bidirectional-mutations']={'traffic':rows,'final_hashes':{'client_a':hashes(a),'client_b':hashes(b)}}

                if trace is not None:trace.use('sdk-no-change');trace.authoritative(a,'client_a')
                api.PHASE='media-sdk-no-change'
                ar=sync(a,aa,'client_a'); br=sync(b,ab,'client_b')
                results['sdk-no-change']={'client_a_routes':[r['route'] for r in ar], 'client_b_routes':[r['route'] for r in br],
                  'retransmission_count':sum(r['route'] in {'/msync/uploadChanges','/msync/downloadFiles'} for r in ar+br),
                  'all_media_statuses_success':success(ar+br)}
                details['sdk-no-change']={'traffic':ar+br}

                # Reuse both deleted names so listing has exactly 1005 current states,
                # while its observed USNs honestly include all six earlier mutations.
                if trace is not None:trace.use('sdk-26-downloads')
                api.PHASE='media-sdk-26-downloads'
                time.sleep(1.05)
                names=[name_image,name_audio]+[f'aster-payload-{index:04}.bin' for index in range(2,fixture['listing_count'])]
                expected={}
                for index,name in enumerate(names):
                    data=image(3) if index==0 else audio(3) if index==1 else (fixture['payload_prefix']+str(index)).encode()
                    expected[name]=sha(data)
                    if index<fixture['batch_count']: a.media.write_data(name,data)
                if trace is not None:trace.authoritative(a,'client_a')
                uploaded=sync(a,aa,'client_a'); received=sync(b,ab,'client_b')
                downloads=[r['media_request']['files'] for r in received if r['route']=='/msync/downloadFiles']
                results['sdk-26-downloads']={'download_request_counts':[len(v) for v in downloads],
                  'requested_file_count':len({name for batch in downloads for name in batch}), 'received_file_count':len(hashes(b)),
                  'receiver_hashes_equal':hashes(b)=={name:expected[name] for name in names[:fixture['batch_count']]},
                  'all_media_statuses_success':success(uploaded+received)}
                details['sdk-26-downloads']={'traffic':uploaded+received,'receiver_hashes':hashes(b)}

                if trace is not None:trace.use('sdk-1005-listing')
                api.PHASE='media-sdk-1005-listing'
                time.sleep(1.05)
                for index in range(fixture['batch_count'],fixture['listing_count']):
                    a.media.write_data(names[index],(fixture['payload_prefix']+str(index)).encode())
                if trace is not None:trace.all_cases();trace.authoritative(a,'client_a');trace.use('sdk-1005-listing')
                uploaded=sync(a,aa,'client_a')
                c,_=api.collection(root,'fresh',fixture['base']); clients.append(c)
                if trace is not None:trace.all_cases()
                ac=trace.call(c,'fresh_client','login',record,lambda:c.sync_login(username=user,password=password,endpoint=endpoint),comparison='unchanged') if trace is not None else c.sync_login(username=user,password=password,endpoint=endpoint)
                if trace is not None:trace.call(c,'fresh_client','full-download',record,lambda:api.full(c,ac,False))
                else:api.full(c,ac,False)
                if trace is not None:trace.all_cases()
                received=sync(c,ac,'fresh_client')
                if trace is not None:trace.use('sdk-1005-listing')
                existing=sync(b,ab,'client_b')
                listing=[r for r in received if r['route']=='/msync/mediaChanges']
                changes=[change for row in listing for change in row['media_result']]
                usns=[change[1] for change in changes]
                results['sdk-1005-listing']={'page_lengths':[len(row['media_result']) for row in listing],
                  'listed_unique_names':len({change[0] for change in changes}),
                  'ordered_unique_usns':usns==sorted(set(usns)),
                  'listing_cursor_requests':[row['media_request']['lastUsn'] for row in listing],
                  'last_change_usn':usns[-1], 'received_file_count':len(hashes(c)),
                  'fresh_receiver_hashes_equal':hashes(c)==expected, 'existing_receiver_hashes_equal':hashes(b)==expected,
                  'all_media_statuses_success':success(uploaded+received+existing)}
                details['sdk-1005-listing']={'traffic':uploaded+received+existing,
                  'expected_hashes':expected,'fresh_receiver_hashes':hashes(c),'existing_receiver_hashes':hashes(b)}
                require(not record.errors,'media recording incomplete')
            finally:
                for col in clients: col.close(downgrade=False)
    return {'probe_kind':'candidate' if target is not None else 'media','phase':'media',
            'fixture':fixture,'runtime':identity,'media_results':results,'media_details':details,'sdk_completions':completions}
