# SPDX-License-Identifier: AGPL-3.0-or-later
"""Finite normal-round experiments on original, job-owned collections.

All low-level requests are marked auxiliary protocol traffic. They use supported
normal-round bodies and order, and are never described as SDK-emitted bytes.
"""
from __future__ import annotations
import copy
import pathlib
import secrets
import tempfile
import time

from anki.sync import SyncOutput

from synthetic import FIXTURE
from validation import require, same_value

NORMAL_FIXTURE = {'format':'meguri-sync-normal-v1','base':FIXTURE,
    'note_counts':[125,251], 'grave_count':251, 'fixed_mod':2000000000,
    'conf_key':'aster_normal', 'conf_value':{'source':'Aster','count':3},
    'tag':'AsterNormal','deck':'Aster normal deck','preset':'Aster normal preset',
    'limits':['Desktop SDK 26.09.4 only; no Mobile or historical-client coverage.',
              'Auxiliary tuple/object probes are explicit native protocol traffic, followed by actual SDK full receivers.',
              'No malformed input, session-policy, restart, expiry, lost acknowledgment or concurrency claims.',
              'Conflict matrices cover notes and one deck; card, note type and preset conflict matrices remain unobserved.',
              'No casefolded object collision, config replacement/optional-null, same-clock finish or failed-sanity claim.',
              'No claim that these finite cases close any complete P-family.']}


def definitions():
    return [
      ('P-N10','sdk-no-change',{'required':SyncOutput.NO_CHANGES,'routes':['/sync/meta'],'full_transfer_count':0,'request_body_bytes_positive':True}),
      ('P-N10','sdk-one-review',{'review_occurrences':1,'card_occurrences':1,'note_occurrences':0,'full_transfer_count':0,'receiver_equal':True}),
      ('P-N03','sdk-bidirectional-edits-deletes',{'forward_equal':True,'reverse_equal':True,'deleted_note_absent':True,'deleted_card_absent':True}),
      ('P-N05','sdk-object-config-tag-exchange',{'model_forward':True,'model_reverse':True,'deck_received':True,'preset_received':True,'tag_received':True,'config_received':True}),
      ('P-N02','sdk-250-boundary',{'sent_lengths':[250,0],'sent_done':[False,True],'received_lengths':[250,0],'received_done':[False,True],'unique_note_ids':125,'unique_card_ids':125,'receiver_equal':True}),
      ('P-N02','sdk-multichunk',{'sent_lengths':[250,250,2],'sent_done':[False,False,True],'received_lengths':[250,250,2],'received_done':[False,False,True],'unique_note_ids':251,'unique_card_ids':251,'receiver_equal':True}),
      ('P-N02','sdk-251-graves',{'sent_grave_lengths':[250,1],'sent_card_graves':251,'receiver_card_count':0,'receiver_note_count':251}),
      ('P-N02','inclusive-usn',{'below_selected':False,'equal_selected':True,'above_selected':True}),
      ('P-N04','object-conflicts',{'older_kept':True,'equal_incoming':True,'newer_incoming':True,'response_before_merge':True}),
      ('P-N06','row-conflicts',{'pending_older_kept':True,'pending_equal_kept':True,'pending_newer_replaces':True,'nonpending_older_replaces':True}),
      ('P-N07','ordered-duplicates',{'nonpending_last_mod':10,'pending_first_accepted_mod':30,'cross_chunk_last_mod':10,'review_first_ease':2,'review_count':1}),
      ('P-N08','sanity-finish-stamps',{'sanity_status':'ok','sanity_ignored_due_graves':True,'server_usn_delta':1,'server_mod_is_finish':True,'server_ls_is_finish':True,'schema_creation_preserved':True,'row_usn_is_round_usn':True}),
    ]


def reverse_definitions():
    return [
      ('P-N10','sdk-reverse-review',{'review_occurrences':1,'card_occurrences':1,'note_occurrences':0,'full_transfer_count':0,'receiver_equal':True}),
      ('P-N03','sdk-reverse-delete',{'deleted_note_absent':True,'deleted_card_absent':True,'full_transfer_count':0,'receiver_equal':True}),
      ('P-N05','sdk-reverse-object-config-tag',{'deck_received':True,'preset_received':True,'tag_received':True,'config_received':True,'full_transfer_count':0,'receiver_equal':True}),
    ]


def normal_selection(sdk_only=False):
    cases=[]
    for family,variant,values in definitions() + (reverse_definitions() if sdk_only else []):
        if sdk_only and not variant.startswith('sdk-'): continue
        sdk = variant.startswith('sdk-')
        reverse = variant.startswith('sdk-reverse-')
        cases.append({'id':f'{family}:desktop.reference.normal-{variant}','family':family,'classification':'current_probe',
          'spec_ids':['sync/server-collection#'+('C5' if family=='P-N08' else 'C4' if family in {'P-N06','P-N07'} else 'C3' if family in {'P-N03','P-N04','P-N05'} else 'C2')],
          'variants':[variant],'client_kind':'desktop','expected':[{'name':key,'value':value} for key,value in values.items()],
          'actions':[{'actor':'client_a','operation':'prepare_synthetic','description':'Create original job-owned collection with authored note type, templates and CSS.'},
            {'actor':('client_b' if reverse else 'client_a') if sdk else 'fault_driver','operation':'client_sync' if sdk else 'native_request',
             'description':'Run actual Desktop SDK normal exchanges and retain exact method and tuple counts.' if sdk else 'Send explicitly marked auxiliary supported normal-round protocol traffic over loopback; these bytes are not claimed as Desktop SDK output.'},
            {'actor':'client_a' if reverse else 'client_b','operation':'client_sync','description':'Use the actual Desktop SDK receiver to inspect committed state.'}]})
    return {'format':'meguri-sync-selection-v1','lane_kind':'reference','scope':'Finite Desktop NORMAL selection; auxiliary supported protocol fixtures are explicitly marked. No whole-family or Mobile credit.','cases':cases}


def exact_numbers(value):
    """Preserve every wide integer exactly at the neutral JSON boundary."""
    if type(value) is int and abs(value)>9007199254740991:
        return str(value)
    if isinstance(value,list):
        return [exact_numbers(item) for item in value]
    if isinstance(value,dict):
        return {key:exact_numbers(item) for key,item in value.items()}
    return value


def length(chunk):
    return sum(len(chunk.get(key,[])) for key in ('notes','cards','revlog'))


def run(api, fixture, target=None, sdk_only=False):
    require(same_value(fixture,NORMAL_FIXTURE),'unknown normal fixture')
    identity=api.runtime()
    require(identity['client']['build']=='c06370ff' and identity['client']['runtime_sha256']=='bf0ccafe0f15bcba59a652db68c4cfdfaa8191756f8c547839c9d134dee12784','normal runtime pin mismatch')
    require(identity['lock_sha256']=='56e1bef86def9f5f230d12317e3510f2176eb079928f3a75f86afb0dcc408ec8','normal runtime lock mismatch')
    if target is not None: identity['server'] = target['server']
    results={}; details={}
    with tempfile.TemporaryDirectory(prefix='normal-reference-') as scratch:
        root=pathlib.Path(scratch)
        api.PHASE='normal-sdk'
        sdk(api,root/'sdk',fixture,results,details,target,bidirectional=sdk_only)
        if not sdk_only:
            api.PHASE='normal-protocol'
            protocol(api,root/'protocol',fixture,results,details)
    return {'probe_kind':'candidate' if target is not None else 'normal-sdk' if sdk_only else 'normal','phase':'normal','runtime':identity,'fixture':fixture,'normal_results':results,'normal_details':details}


def environment(api, root, count=1, target=None):
    """A context manager with no shared profiles or ambient credentials."""
    import contextlib
    @contextlib.contextmanager
    def entered():
        root.mkdir()
        with api.probe_server(root/'server',target) as (port,user,password),api.recorder(port) as record:
            record.capture_normal=True
            record.sdk_completions=[]
            clients=[]
            try:
                a,model=api.collection(root,'a',FIXTURE);clients.append(a)
                for index in range(count): api.add_note(a,model,[f'Aster cue {index}',f'Aster response {index}'])
                b,_=api.collection(root,'b',FIXTURE);clients.append(b)
                endpoint=f'http://127.0.0.1:{record.server_port}/'
                aa=a.sync_login(username=user,password=password,endpoint=endpoint)
                ab=b.sync_login(username=user,password=password,endpoint=endpoint)
                api.full(a,aa,True);api.full(b,ab,False)
                # Bring A's cursor/stamps up to the post-download server state.
                sync(a,aa,record,'client_a')
                yield a,b,model,aa,ab,record,port,clients
                require(not record.errors,'normal recording incomplete')
            finally:
                for col in clients: col.close(downgrade=False)
    return entered()


def sync(col,auth,recorder=None,actor=None):
    result=col.sync_collection(auth,sync_media=False)
    if recorder is not None:
        recorder.sdk_completions.append({'actor':actor,'required':result.required})
    require(result.required == SyncOutput.NO_CHANGES,'normal SDK outcome requires full transfer')
    col.models=type(col.models)(col)
    return result


def sdk(api,root,f,results,details,target=None,bidirectional=False):
    root.mkdir()
    with environment(api,root/'exchange',3,target) as (a,b,model,aa,ab,rec,port,clients):
        # A settled no-change exchange measures actual wire payloads.
        sync(a,aa,rec,'client_a');before=len(rec.observations);settled=sync(a,aa,rec,'client_a')
        rows=rec.observations[before:]
        results['sdk-no-change']={'required':settled.required,'routes':[r['route'] for r in rows],'full_transfer_count':sum(r['route'] in {'/sync/upload','/sync/download'} for r in rows),'request_body_bytes_positive':all(r['request_bytes']>0 for r in rows)}
        details['no_change']=rows
        # A review through the public scheduler, then a real receiving client.
        api.PHASE='normal-sdk-review-get'
        time.sleep(1.05)
        card=a.sched.getCard(); require(card is not None,'synthetic review card unavailable')
        api.PHASE='normal-sdk-review-answer'
        a.sched.answerCard(card,3)
        before=len(rec.observations);sync(a,aa,rec,'client_a');review_rows=rec.observations[before:]
        sync(b,ab,rec,'client_b')
        chunks=[r['normal_request']['chunk'] for r in review_rows if r['route']=='/sync/applyChunk']
        results['sdk-one-review']={'review_occurrences':sum(len(c.get('revlog',[])) for c in chunks),'card_occurrences':sum(len(c.get('cards',[])) for c in chunks),'note_occurrences':sum(len(c.get('notes',[])) for c in chunks),'full_transfer_count':sum(r['route'] in {'/sync/upload','/sync/download'} for r in review_rows),'receiver_equal':api.snapshot(a)==api.snapshot(b)}
        details['one_review']=review_rows
        if bidirectional:
            api.PHASE='normal-sdk-reverse-review'
            time.sleep(1.05)
            card=b.sched.getCard();require(card is not None,'reverse synthetic review card unavailable')
            b.sched.answerCard(card,3)
            before=len(rec.observations);sync(b,ab,rec,'client_b');reverse_rows=rec.observations[before:]
            sync(a,aa,rec,'client_a')
            chunks=[r['normal_request']['chunk'] for r in reverse_rows if r['route']=='/sync/applyChunk']
            results['sdk-reverse-review']={'review_occurrences':sum(len(c.get('revlog',[])) for c in chunks),'card_occurrences':sum(len(c.get('cards',[])) for c in chunks),'note_occurrences':sum(len(c.get('notes',[])) for c in chunks),'full_transfer_count':sum(r['route'] in {'/sync/upload','/sync/download'} for r in reverse_rows),'receiver_equal':api.snapshot(a)==api.snapshot(b)}
            details['reverse_review']=reverse_rows
        # Both directions, plus ordinary note deletion (with its card grave).
        api.PHASE='normal-sdk-edit-delete'
        note_id=a.find_notes('')[0]
        time.sleep(1.05);n=a.get_note(note_id);n.fields[1]='Aster forward';a.update_note(n);sync(a,aa,rec,'client_a');sync(b,ab,rec,'client_b')
        forward=api.snapshot(a)==api.snapshot(b)
        time.sleep(1.05);n=b.get_note(note_id);n.fields[1]='Aster reverse';b.update_note(n);sync(b,ab,rec,'client_b');sync(a,aa,rec,'client_a')
        reverse=api.snapshot(a)==api.snapshot(b)
        delete_id=a.find_notes('')[-1];delete_card=a.db.scalar('select id from cards where nid=?',delete_id)
        time.sleep(1.05);a.remove_notes([delete_id]);sync(a,aa,rec,'client_a');sync(b,ab,rec,'client_b')
        results['sdk-bidirectional-edits-deletes']={'forward_equal':forward,'reverse_equal':reverse,'deleted_note_absent':b.db.scalar('select count(*) from notes where id=?',delete_id)==0,'deleted_card_absent':b.db.scalar('select count(*) from cards where id=?',delete_card)==0}
        # Author every changed object; schema shape and templates remain our own.
        time.sleep(1.05)
        api.PHASE='normal-sdk-objects'
        m=a.models.get(model['id']);m['css']='.card { color: #234567; }';a.models.save(m)
        api.PHASE='normal-sdk-deck-preset'
        did=a.decks.id(f['deck']);preset=a.decks.add_config(f['preset']);cid=preset['id']
        api.PHASE='normal-sdk-config-tag'
        a.set_config(f['conf_key'],f['conf_value'])
        n=a.get_note(note_id);n.tags=[f['tag']];a.update_note(n)
        sync(a,aa,rec,'client_a');sync(b,ab,rec,'client_b')
        api.PHASE='normal-sdk-object-receive'
        model_forward=b.models.get(model['id'])['css']=='.card { color: #234567; }'
        time.sleep(1.05);m=b.models.get(model['id']);m['css']='.card { color: #345678; }';b.models.save(m);sync(b,ab,rec,'client_b');sync(a,aa,rec,'client_a')
        results['sdk-object-config-tag-exchange']={'model_forward':model_forward,'model_reverse':a.models.get(model['id'])['css']=='.card { color: #345678; }','deck_received':b.decks.get(did)['name']==f['deck'],'preset_received':b.decks.get_config(cid)['name']==f['preset'],'tag_received':f['tag'] in b.tags.all(),'config_received':b.get_config(f['conf_key'])==f['conf_value']}
        if bidirectional:
            api.PHASE='normal-sdk-reverse-objects'
            time.sleep(1.05)
            reverse_deck=b.decks.id(f['deck']+' reverse')
            reverse_preset=b.decks.add_config(f['preset']+' reverse')
            reverse_config={'source':'Aster reverse','count':4}
            b.set_config(f['conf_key'],reverse_config)
            reverse_tag=f['tag']+'Reverse'
            n=b.get_note(note_id);n.tags=[reverse_tag];b.update_note(n)
            before=len(rec.observations);sync(b,ab,rec,'client_b');sync(a,aa,rec,'client_a');reverse_rows=rec.observations[before:]
            results['sdk-reverse-object-config-tag']={'deck_received':a.decks.get(reverse_deck)['name']==f['deck']+' reverse','preset_received':a.decks.get_config(reverse_preset['id'])['name']==f['preset']+' reverse','tag_received':reverse_tag in a.tags.all(),'config_received':a.get_config(f['conf_key'])==reverse_config,'full_transfer_count':sum(r['route'] in {'/sync/upload','/sync/download'} for r in reverse_rows),'receiver_equal':api.snapshot(a)==api.snapshot(b)}
            details['reverse_objects']=reverse_rows
            api.PHASE='normal-sdk-reverse-delete'
            delete_id=next(ident for ident in b.find_notes('') if ident!=note_id)
            delete_card=b.db.scalar('select id from cards where nid=?',delete_id)
            time.sleep(1.05);b.remove_notes([delete_id])
            before=len(rec.observations);sync(b,ab,rec,'client_b');sync(a,aa,rec,'client_a');reverse_rows=rec.observations[before:]
            results['sdk-reverse-delete']={'deleted_note_absent':a.db.scalar('select count(*) from notes where id=?',delete_id)==0,'deleted_card_absent':a.db.scalar('select count(*) from cards where id=?',delete_card)==0,'full_transfer_count':sum(r['route'] in {'/sync/upload','/sync/download'} for r in reverse_rows),'receiver_equal':api.snapshot(a)==api.snapshot(b)}
            details['reverse_delete']=reverse_rows
        details['exchange_traffic']=rec.observations
        details['exchange_sdk_completions']=rec.sdk_completions
    api.PHASE='normal-sdk-boundaries'
    for count in f['note_counts']:
        with environment(api,root/('boundary-'+str(count)),1,target) as (a,b,model,aa,ab,rec,port,clients):
            time.sleep(1.05)
            # New rows: exactly two tuples per note; initial fixture row is unchanged.
            for index in range(count): api.add_note(a,a.models.get(model['id']),[f'Boundary cue {index}',f'Boundary response {index}'])
            before=len(rec.observations);sync(a,aa,rec,'client_a');sent=rec.observations[before:]
            before=len(rec.observations);sync(b,ab,rec,'client_b');received=rec.observations[before:]
            sc=[r['normal_request']['chunk'] for r in sent if r['route']=='/sync/applyChunk']
            rc=[r['normal_response'] for r in received if r['route']=='/sync/chunk']
            variant='sdk-250-boundary' if count==125 else 'sdk-multichunk'
            results[variant]={'sent_lengths':[length(c) for c in sc],'sent_done':[c['done'] for c in sc],'received_lengths':[length(c) for c in rc],'received_done':[c['done'] for c in rc],'unique_note_ids':len({n[0] for c in rc for n in c.get('notes',[])}),'unique_card_ids':len({n[0] for c in rc for n in c.get('cards',[])}),'receiver_equal':api.snapshot(a)==api.snapshot(b)}
            details[variant]={'sender':sent,'receiver':received,'sdk_completions':rec.sdk_completions}
    api.PHASE='normal-sdk-graves'
    with environment(api,root/'graves',251,target) as (a,b,model,aa,ab,rec,port,clients):
        time.sleep(1.05)
        # Author an otherwise valid fixture with card-only graves and orphan notes.
        card_ids=a.find_cards('')
        a.db.executemany('insert into graves(usn,oid,type) values(-1,?,0)',[(ident,) for ident in card_ids])
        a.db.execute('delete from cards')
        a.db.execute('update col set mod=?',int(time.time()*1000)+1000)
        before=len(rec.observations);sync(a,aa,rec,'client_a');sent=rec.observations[before:];sync(b,ab,rec,'client_b')
        graves=[r['normal_request']['chunk'] for r in sent if r['route']=='/sync/applyGraves']
        results['sdk-251-graves']={'sent_grave_lengths':[sum(len(v) for v in c.values()) for c in graves],'sent_card_graves':sum(len(c['cards']) for c in graves),'receiver_card_count':b.db.scalar('select count(*) from cards'),'receiver_note_count':b.db.scalar('select count(*) from notes')}
        details['sdk-251-graves']=sent
        details['sdk-251-graves-completions']=rec.sdk_completions


def protocol(api,root,f,results,details):
    with environment(api,root,3) as (a,b,model,aa,ab,rec,port,clients):
        traffic=[]
        def call(method,body,session):
            response=api.native_request(port,aa,method,body,session)
            traffic.append({'origin':'auxiliary-supported-normal-protocol','method':method,'request':body,'response':response})
            require(response['status']==200,'supported normal protocol request failed')
            return response['value']
        def meta():return call('meta',{'v':11,'cv':'meguri-synthetic-normal'},'')
        def begin(cursor,lnewer=False):
            session=secrets.token_hex(8)
            return session,call('start',{'minUsn':cursor,'lnewer':lnewer,'graves':None},session)
        def changes(session,value=None):return call('applyChanges',{'changes':value or {'models':[],'decks':[[],[]],'tags':[]}},session)
        def chunks(session):
            output=[]
            for _ in range(20):
                chunk=call('chunk',{'_pad':None},session);output.append(chunk)
                if chunk['done']:return output
            raise ValueError('finite chunk bound exceeded')
        def apply(session,chunk):return call('applyChunk',{'chunk':chunk},session)
        def finish(session):return call('finish',{'_pad':None},session)
        def receive():
            before_download=len(rec.observations)
            api.full(b,ab,False)
            served=[entry['download_stamp'] for entry in rec.observations[before_download:] if 'download_stamp' in entry]
            require(len(served)==1,'one raw served collection stamp required')
            return served[0]
        u=meta()['usn'];base=f['fixed_mod']
        # Seed authored tuple state in one ordinary round, then read at the exact cursor.
        s,_=begin(u);changes(s);chunks(s)
        note_rows=[row+['','',0,''] for row in a.db.all('select id,guid,mid,mod,usn,tags,flds from notes order by id')]
        card_rows=a.db.all('select * from cards order by id');ids=[row[0] for row in note_rows]
        # Supplied outgoing USN is controlled as fixture data, around the cursor.
        for index,row in enumerate(note_rows):row[3]=base;row[4]=u-1+index
        apply(s,{'notes':note_rows,'done':True});finish(s)
        s,_=begin(u);changes(s);out=chunks(s);apply(s,{'done':True});finish(s)
        outgoing={row[0] for c in out for row in c.get('notes',[])}
        results['inclusive-usn']={'below_selected':ids[0] in outgoing,'equal_selected':ids[1] in outgoing,'above_selected':ids[2] in outgoing}
        # One authored deck, independently observed through Desktop receiver.
        receive();deck=copy.deepcopy(b.decks.get(1));deck['name']='Aster fixture deck';deck['mod']=base;deck['usn']=meta()['usn']
        s,_=begin(deck['usn']);changes(s,{'models':[],'decks':[[deck],[]],'tags':[]});chunks(s);apply(s,{'done':True});finish(s)
        outcomes=[];before_merge=[]
        for label,stamp in [('older',base-1),('equal',base),('newer',base+1)]:
            current=meta()['usn'];incoming=copy.deepcopy(deck);incoming.update(name='Aster '+label,mod=stamp,usn=current)
            s,_=begin(0);response=changes(s,{'models':[],'decks':[[incoming],[]],'tags':[]});chunks(s);apply(s,{'done':True});finish(s);receive()
            outcomes.append(b.decks.get(1)['name'])
            before_merge.append(next(d['name'] for d in response['decks'][0] if d['id']==1))
        results['object-conflicts']={'older_kept':outcomes[0]=='Aster fixture deck','equal_incoming':outcomes[1]=='Aster equal','newer_incoming':outcomes[2]=='Aster newer','response_before_merge':before_merge==['Aster fixture deck','Aster fixture deck','Aster equal']}
        # Each state is seeded in a prior committed round. No implementation internals.
        def write_note(seed_mod,seed_usn,incoming_mod,cursor):
            current=meta()['usn'];row=copy.deepcopy(note_rows[0]);row[3]=seed_mod;row[4]=seed_usn;row[6]='Aster cue\x1fAster seeded'
            s,_=begin(current);changes(s);chunks(s);apply(s,{'notes':[row],'done':True});finish(s)
            row[3]=incoming_mod;row[4]=meta()['usn'];row[6]='Aster cue\x1fAster incoming'
            s,_=begin(cursor);changes(s);chunks(s);apply(s,{'notes':[row],'done':True});finish(s);receive()
            return b.db.scalar('select flds from notes where id=?',row[0])
        # A high cursor is only used for fixture selection, not as a client claim.
        threshold=1
        values=[write_note(base,threshold,base-1,threshold),write_note(base,threshold,base,threshold),write_note(base,threshold,base+1,threshold),write_note(base,0,base-1,threshold)]
        results['row-conflicts']={'pending_older_kept':values[0].endswith('seeded'),'pending_equal_kept':values[1].endswith('seeded'),'pending_newer_replaces':values[2].endswith('incoming'),'nonpending_older_replaces':values[3].endswith('incoming')}
        # Duplicate tuples within/across normal chunks; first review occurrence wins.
        current=meta()['usn'];s,_=begin(current);changes(s);chunks(s)
        def nr(ident,mod,usn):
            row=copy.deepcopy(note_rows[0]);row[0]=ident;row[1]='aster'+str(ident);row[3]=mod;row[4]=usn;return row
        nid=700001;rid=700004;cid=card_rows[0][0]
        apply(s,{'notes':[nr(nid,30,0),nr(nid,10,0),nr(nid+1,30,current),nr(nid+1,10,0),nr(nid+2,30,0)],'revlog':[[rid,cid,current,2,10,0,2500,1000,1],[rid,cid,current,4,20,10,2600,2000,1]],'done':False})
        apply(s,{'notes':[nr(nid+2,10,0)],'done':True});finish(s);served_before=receive()
        results['ordered-duplicates']={'nonpending_last_mod':b.db.scalar('select mod from notes where id=?',nid),'pending_first_accepted_mod':b.db.scalar('select mod from notes where id=?',nid+1),'cross_chunk_last_mod':b.db.scalar('select mod from notes where id=?',nid+2),'review_first_ease':b.db.scalar('select ease from revlog where id=?',rid),'review_count':b.db.scalar('select count(*) from revlog where id=?',rid)}
        # Sanity and durable stamp behavior, with a pre/post Desktop full receiver.
        before={key:served_before[key] for key in ('scm','crt')}
        current=meta()['usn'];s,_=begin(current);changes(s);chunks(s)
        row=nr(nid+3,base,current);apply(s,{'notes':[row],'done':True})
        counts=[[123,456,789],b.db.scalar('select count(*) from cards'),b.db.scalar('select count(*) from notes')+1,b.db.scalar('select count(*) from revlog'),999,b.db.scalar('select count(*) from notetypes'),b.db.scalar('select count(*) from decks'),b.db.scalar('select count(*) from deck_config')]
        sanity=call('sanityCheck2',{'client':counts},s);stamp=finish(s);after_meta=meta();served_after=receive()
        results['sanity-finish-stamps']={'sanity_status':sanity['status'],'sanity_ignored_due_graves':sanity.get('c',[None])[0]==[0,0,0] and sanity['c'][4]==0,'server_usn_delta':after_meta['usn']-current,'server_mod_is_finish':after_meta['mod']==stamp,'server_ls_is_finish':served_after['ls']==stamp,'schema_creation_preserved':all(served_after[key]==before[key] for key in ('scm','crt')),'row_usn_is_round_usn':b.db.scalar('select usn from notes where id=?',row[0])==current}
        details['finish_served_stamps']={'before':served_before,'after':served_after,'finish':stamp}
        details['auxiliary_protocol']=traffic
