# SPDX-License-Identifier: AGPL-3.0-or-later
"""Six fixed final Desktop experiments on synthetic job-owned profiles."""
import contextlib
import copy
import pathlib
import tempfile
import time

from anki.scheduler_pb2 import CardAnswer
from anki.sync import SyncOutput
from final_contract import recipes, RECIPE_SHA256
from normal import exact_numbers, sync as normal_sync
from validation import decode, encoded, require, same_value, sha

SEGMENTS=('migration','replacement-upload','replacement-download','metadata-only','fresh-fresh','rich')
NORMALIZATION=['models/decks/presets: top-level mod and usn only','notes/cards: mod and usn columns only',
               'revlog/graves: usn column only','configuration: complete key set and exact JSON values/bytes; only row usn and mtime_secs stamps excluded']


def objects(col):
    def clean(row):return {key:copy.deepcopy(value) for key,value in row.items() if key not in {'mod','usn'}}
    return {'models':sorted([clean(col.models.get(row.id)) for row in col.models.all_names_and_ids()],key=lambda row:row['id']),
            'decks':sorted([clean(row) for row in col.decks.all()],key=lambda row:row['id']),
            'presets':sorted([clean(row) for row in col.decks.all_config()],key=lambda row:row['id']),
            'tags':sorted(col.tags.all()),
            'config':configuration(col), 'config_json_hex':configuration(col,raw=True)}


def configuration(col,raw=False):
    values={}
    for key,value in col.db.all('select key,val from config order by key'):
        data=value.encode() if isinstance(value,str) else bytes(value)
        values[key]=data.hex() if raw else exact_numbers(__import__('json').loads(data))
    return values


def projection(col):
    return exact_numbers({'objects':objects(col),
        'notes':col.db.all('select id,guid,mid,tags,flds,sfld,csum,flags,data from notes order by id'),
        'cards':col.db.all('select id,nid,did,ord,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data from cards order by id'),
        'reviews':col.db.all('select id,cid,ease,ivl,lastIvl,factor,time,type from revlog order by id'),
        'graves':col.db.all('select oid,type from graves order by oid,type')})


def media_map(col):
    from media import hashes
    return hashes(col)


def utf8_map(col):
    return {path.name:path.read_text() for path in pathlib.Path(col.media.dir()).iterdir() if path.is_file()}


def expected_media(value):return {name:sha(data.encode()) for name,data in value.items()}


def set_media(col,wanted):
    directory=pathlib.Path(col.media.dir());time.sleep(1.05)
    for path in directory.iterdir():
        if path.is_file() and path.name not in wanted:path.unlink()
    for name,value in wanted.items():
        path=directory/name
        if path.exists():path.write_bytes(value.encode())
        else:require(col.media.write_data(name,value.encode()) == name,'authored media filename changed')
    directory.touch()


def author(api,root,name,recipe,side=None,empty=False):
    initial=recipe['rich_initial'];fixture={'notetype':initial['note_type'],'fields':initial['fields_in_order'],'template':initial['template']}
    if side:fixture=copy.deepcopy(fixture);fixture['notetype']=recipe['replacement'][side+'_objects']['note_type']
    col,model=api.collection(root,name,fixture)
    model=col.models.get(model['id']);model['css']=initial['template']['css'];col.models.save(model)
    deck=initial['deck'];preset=initial['preset']
    did=col.decks.id(recipe['replacement'][side+'_objects']['deck'] if side else deck['name'])
    config=col.decks.add_config(recipe['replacement'][side+'_objects']['preset'] if side else preset['name'])
    config['new']['perDay']=preset['new_per_day'];config['rev']['perDay']=preset['reviews_per_day']
    config['maxTaken']=preset['maximum_answer_seconds'];config['autoplay']=preset['autoplay'];col.decks.update_config(config)
    saved=col.decks.get(did);saved['desc']=deck['description'];saved['collapsed']=deck['collapsed'];saved['conf']=config['id'];col.decks.save(saved)
    col.set_config('meguri_sync_fixture',recipe['replacement'][side+'_objects']['config']['meguri_sync_fixture'] if side else initial['config']['meguri_sync_fixture'])
    ids={}
    if not empty:
        notes=[{'key':side,'fields':recipe['replacement'][side+'_fields']}] if side else initial['notes']
        for item in notes:
            note=col.new_note(model);note.fields=item['fields'];note.tags=initial['tags'];col.add_note(note,did);ids[item['key']]=note.id
    col.decks.select(did)
    col.sched.today  # Initialize actual scheduler/day defaults before freezing authority.
    return col,{'model':model['id'],'deck':did,'preset':config['id'],'notes':ids}


def authored_values(col,ids):
    model=col.models.get(ids['model']);deck=col.decks.get(ids['deck']);preset=col.decks.get_config(ids['preset'])
    return {'note_type':model['name'],'fields_in_order':[row['name'] for row in model['flds']],
        'template':{'name':model['tmpls'][0]['name'],'question':model['tmpls'][0]['qfmt'],'answer':model['tmpls'][0]['afmt'],'css':model['css']},
        'deck':{'name':deck['name'],'description':deck['desc'],'collapsed':deck['collapsed']},
        'preset':{'name':preset['name'],'new_per_day':preset['new']['perDay'],'reviews_per_day':preset['rev']['perDay'],
                  'maximum_answer_seconds':preset['maxTaken'],'autoplay':preset['autoplay']},
        'config':{'meguri_sync_fixture':col.get_config('meguri_sync_fixture')},
        'notes':{key:list(col.get_note(ident).fields) for key,ident in ids['notes'].items() if col.db.scalar('select count(*) from notes where id=?',ident)}}


def expected_values(recipe,checkpoint):
    initial=copy.deepcopy(recipe['rich_initial']);notes={row['key']:row['fields'] for row in initial.pop('notes')};initial.pop('tags')
    for name in ('forward','reverse'):
        if checkpoint == 'initial':break
        delta=recipe['rich_'+name+'_changes']
        for key in ('question','answer','css'):initial['template'][key]=delta[key]
        initial['deck'].update(description=delta['deck_description'],collapsed=delta['deck_collapsed'])
        initial['preset'].update(delta['preset']);initial['config']=delta['config']
        if 'keep_answer' in delta:notes['keep'][1]=delta['keep_answer']
        if 'keep_context' in delta:notes['keep'][2]=delta['keep_context']
        del notes[delta['delete_note_key']]
        if checkpoint == name:break
    initial['notes']=notes
    return initial


def change(col,ids,recipe,direction):
    delta=recipe['rich_'+direction+'_changes'];time.sleep(1.05)
    model=col.models.get(ids['model']);model['tmpls'][0].update(qfmt=delta['question'],afmt=delta['answer']);model['css']=delta['css'];col.models.save(model)
    deck=col.decks.get(ids['deck']);deck.update(desc=delta['deck_description'],collapsed=delta['deck_collapsed']);col.decks.save(deck)
    preset=col.decks.get_config(ids['preset']);options=delta['preset'];preset['new']['perDay']=options['new_per_day'];preset['rev']['perDay']=options['reviews_per_day'];preset['maxTaken']=options['maximum_answer_seconds'];preset['autoplay']=options['autoplay'];col.decks.update_config(preset)
    col.set_config('meguri_sync_fixture',delta['config']['meguri_sync_fixture'])
    note=col.get_note(ids['notes']['keep'])
    if 'keep_answer' in delta:note.fields[1]=delta['keep_answer']
    if 'keep_context' in delta:note.fields[2]=delta['keep_context']
    col.update_note(note);col.remove_notes([ids['notes'][delta['delete_note_key']]])
    cid=col.db.scalar('select id from cards where nid=?',note.id);states=col._backend.get_scheduling_states(cid)
    button=recipe['review_actions'][direction]['button'];rating=CardAnswer.GOOD if button == 3 else CardAnswer.HARD
    col.sched.answer_card(CardAnswer(card_id=cid,current_state=states.current,new_state=states.good if button == 3 else states.hard,
        rating=rating,answered_at_millis=int(time.time()*1000),milliseconds_taken=0))
    set_media(col,recipe['media_utf8_exact'][direction])


class Run:
    def __init__(self,api,root,target,case):
        self.api,self.root,self.target,self.case=api,root,target,case
        self.clients=[];self.inputs=[];self.receipts=[];self.comparisons=[]

    def freeze(self,col,actor,action,checkpoint):
        value={'format':'meguri-sync-final-action-input-v1','case_id':self.case['id'],'action_index':action,'actor':actor,
            'checkpoint':checkpoint,'recipe_sha256':RECIPE_SHA256,'normalization':NORMALIZATION,
            'projection':projection(col),'media':media_map(col)}
        digest=sha(encoded(value));self.inputs.append({'sha256':digest,'value':decode(encoded(value))})
        return copy.deepcopy(value)

    def call(self,col,auth,actor,action,operation,record,upload=None):
        frozen=self.freeze(col,actor,action,operation);before=len(record.observations);start=time.monotonic()
        self.api.PHASE='final-'+operation+('-upload' if upload else '-download' if operation == 'full' else '')
        try:
            if operation == 'normal':result=normal_sync(col,auth,record,actor);decision=result.required
            elif operation == 'decision':result=col.sync_collection(auth,sync_media=False);decision=result.required
            elif operation == 'media':self.api.media_sync(col,auth);decision=None
            else:self.api.full(col,auth,upload);decision=None
        except BaseException as error:
            kind=('SyncError' if isinstance(error,self.api.SyncError) else 'NetworkError' if isinstance(error,self.api.NetworkError)
                  else 'TimeoutError' if isinstance(error,TimeoutError) else 'unexpected-error')
            self.api.FINAL_FAILURE={'case_id':self.case['id'],'action_index':action,'actor':actor,'operation':operation,
                'phase':self.api.PHASE,'kind':kind,'traffic':copy.deepcopy(record.observations[before:])}
            raise
        rows=copy.deepcopy(record.observations[before:])
        self.receipts.append({'sequence':len(self.receipts),'case_id':self.case['id'],'action_index':action,'actor':actor,'operation':operation,
            'input_sha256':sha(encoded(frozen)),'resources':resources(),'elapsed_ms':int((time.monotonic()-start)*1000),'decision':decision,'traffic':rows,
            'request_bytes':sum(row.get('request_bytes',0) for row in rows),'response_bytes':sum(row.get('response_bytes',0) for row in rows)})
        return decision

    def compare(self,col,ids,expected,checkpoint,actor,action):
        actual=projection(col);values=authored_values(col,ids)
        wanted_values=expected_values(recipes(),checkpoint)
        row={'actor':actor,'action_index':action,'checkpoint':checkpoint,'expected_projection_sha256':sha(encoded(expected['projection'])),
             'observed_projection':actual,'observed_projection_sha256':sha(encoded(actual)),
             'authored_values':values,'expected_authored_values':wanted_values,'media':media_map(col)}
        self.comparisons.append(row)
        return same_value(actual,expected['projection']) and same_value(values,wanted_values)

    def result(self,values,extra=None):
        require(all(any(receipt['action_index'] == index and receipt['actor'] == action['actor'] for receipt in self.receipts)
                    for index,action in enumerate(self.case['actions'])), 'final action/actor coverage missing')
        return {'values':values,'inputs':self.inputs,'receipts':self.receipts,'comparisons':self.comparisons,**(extra or {})}


@contextlib.contextmanager
def environment(api,root,target,case,ordinary_blank=False):
    root.mkdir();run=Run(api,root,target,case)
    with api.recorder(8080) as record:
        record.capture_normal=True;record.capture_media=True;record.sdk_completions=[]
        endpoint=f'http://127.0.0.1:{record.server_port}/'
        def client(name,side=None,empty=False):
            if ordinary_blank or (name in {'b','fresh'} and side is None and not empty):
                directory=root/name;directory.mkdir();col=api.Collection(str(directory/'collection.anki2'));ids=None
            else:col,ids=author(api,root,name,recipes(),side,empty)
            run.clients.append(col)
            auth=col.sync_login(username=target['username'],password=target['password'],endpoint=endpoint)
            return col,ids,auth
        try:
            yield run,record,client
            require(not record.errors,'final Desktop recording incomplete')
        finally:
            for col in run.clients:col.close(downgrade=False)


def rich(api,root,target,case):
    recipe=recipes()
    with environment(api,root,target,case) as (run,record,client):
        a,ids,aa=client('a');b,_,ab=client('b');c,_,ac=client('fresh')
        set_media(a,recipe['media_utf8_exact']['initial'])
        initial=run.freeze(a,'client_a',0,'initial-authority')
        require(same_value(authored_values(a,ids),expected_values(recipe,'initial')), 'initial author differs from original recipe')
        run.call(a,aa,'client_a',0,'full',record,True);run.call(a,aa,'client_a',0,'media',record)
        run.call(b,ab,'client_b',1,'full',record,False);run.call(b,ab,'client_b',1,'media',record)
        full_equal=run.compare(b,ids,initial,'initial','client_b',1)
        run.call(a,aa,'client_a',2,'normal',record)
        settled=projection(a)
        require(same_value(settled,initial['projection']),'settlement changed original authoritative state')
        change(a,ids,recipe,'forward');forward=run.freeze(a,'client_a',2,'forward-authority')
        require_declared_mutation(initial['projection'],forward['projection'],ids,recipe,'forward')
        require(same_value(authored_values(a,ids),expected_values(recipe,'forward')), 'forward author differs from original recipe')
        run.call(a,aa,'client_a',2,'normal',record);run.call(a,aa,'client_a',2,'media',record)
        run.call(b,ab,'client_b',3,'normal',record);run.call(b,ab,'client_b',3,'media',record)
        forward_equal=run.compare(b,ids,forward,'forward','client_b',3)
        require(same_value(projection(b),forward['projection']),'reverse author did not retain independent forward authority')
        change(b,ids,recipe,'reverse');reverse=run.freeze(b,'client_b',3,'reverse-authority')
        require_declared_mutation(forward['projection'],reverse['projection'],ids,recipe,'reverse')
        require(same_value(authored_values(b,ids),expected_values(recipe,'reverse')), 'reverse author differs from original recipe')
        run.call(b,ab,'client_b',3,'normal',record);run.call(b,ab,'client_b',3,'media',record)
        run.call(a,aa,'client_a',4,'normal',record);run.call(a,aa,'client_a',4,'media',record)
        reverse_equal=run.compare(a,ids,reverse,'reverse','client_a',4)
        run.call(c,ac,'fresh_client',5,'full',record,False);run.call(c,ac,'fresh_client',5,'media',record)
        fresh_equal=run.compare(c,ids,reverse,'reverse','fresh_client',5)
        deleted=[ids['notes'][name] for name in ('delete_forward','delete_reverse')]
        return run.result({'full_supported_values_match_recipe':full_equal,'forward_supported_values_match_recipe':forward_equal,
            'reverse_supported_values_match_recipe':reverse_equal,'fresh_supported_values_match_recipe':fresh_equal,
            'history_scheduling_equal':same_value(projection(c),reverse['projection']) and len(reverse['projection']['reviews']) == 2,
            'deleted_content_absent':all(not c.db.scalar('select count(*) from notes where id=?',ident) and not c.db.scalar('select count(*) from cards where nid=?',ident) for ident in deleted),
            'final_media_matches_recipe':media_map(a) == media_map(b) == media_map(c) == expected_media(recipe['media_utf8_exact']['reverse'])},
            {'logical_ids':ids,'frozen_authorities':[initial,forward,reverse],'final_media_maps':[media_map(a),media_map(b),media_map(c)],'post_settlement_projection':settled})


def metadata_only(api,root,target,case):
    recipe=recipes()
    with environment(api,root,target,case) as (run,record,client):
        a,ids,aa=client('a',empty=True);c,_,ac=client('fresh')
        set_media(a,recipe['metadata_only']['media_utf8_exact']);expected=run.freeze(a,'client_a',0,'metadata-authority')
        decision=run.call(a,aa,'client_a',0,'decision',record)
        require(decision in {SyncOutput.FULL_SYNC,SyncOutput.FULL_UPLOAD,SyncOutput.FULL_DOWNLOAD}, 'no-card metadata initial decision unavailable')
        run.call(a,aa,'client_a',0,'full',record,True);run.call(a,aa,'client_a',0,'media',record)
        run.call(c,ac,'fresh_client',1,'full',record,False);run.call(c,ac,'fresh_client',1,'media',record)
        actual=projection(c);run.comparisons.append({'expected_projection_sha256':sha(encoded(expected['projection'])),'observed_projection':actual,'media':media_map(c)})
        return run.result({'cards':len(actual['cards']),'notes':len(actual['notes']),
            'objects_config_equal':same_value(actual['objects'],expected['projection']['objects']),
            'media_utf8_exact':utf8_map(c),'valid_client_state':a.db.scalar('pragma integrity_check') == c.db.scalar('pragma integrity_check') == 'ok'},
            {'logical_ids':ids,'frozen_authorities':[expected],'initial_decision':decision,'media_utf8_exact':utf8_map(c),'integrity':[a.db.scalar('pragma integrity_check'),c.db.scalar('pragma integrity_check')]})


def fresh_fresh(api,root,target,case):
    with environment(api,root,target,case,ordinary_blank=True) as (run,record,client):
        a,_,aa=client('a');decision=run.call(a,aa,'client_a',0,'decision',record)
        upload=decision == SyncOutput.FULL_UPLOAD
        transferable=decision in {SyncOutput.FULL_UPLOAD,SyncOutput.FULL_DOWNLOAD,SyncOutput.FULL_SYNC}
        if transferable:run.call(a,aa,'client_a',0,'full',record,upload)
        elif decision != SyncOutput.NO_CHANGES:raise ValueError('unknown fresh/fresh SDK direction')
        follow=run.call(a,aa,'client_a',0,'normal',record)
        return run.result({'decision_observed':decision in {SyncOutput.FULL_UPLOAD,SyncOutput.FULL_DOWNLOAD,SyncOutput.FULL_SYNC,SyncOutput.NO_CHANGES},
            'explicit_authority_matches_decision':transferable or decision == SyncOutput.NO_CHANGES,
            'blank_state_valid':a.db.scalar('select count(*) from cards') == a.db.scalar('select count(*) from notes') == 0 and a.db.scalar('pragma integrity_check') == 'ok',
            'ordinary_completed':follow == SyncOutput.NO_CHANGES},{'initial_decision':decision,'explicit_authority':'upload' if upload else 'download' if transferable else 'none','blank_final':{'notes':a.db.scalar('select count(*) from notes'),'cards':a.db.scalar('select count(*) from cards'),'integrity':a.db.scalar('pragma integrity_check')}})


def replacement(api,root,target,case,upload):
    recipe=recipes();replacement=recipe['replacement']
    with environment(api,root,target,case) as (run,record,client):
        b,right_ids,ab=client('b','right');set_media(b,replacement['server_media_utf8_exact'])
        right=run.freeze(b,'client_b',0,'right-authority')
        run.call(b,ab,'client_b',0,'full',record,True);run.call(b,ab,'client_b',0,'media',record)
        time.sleep(1.05);a,left_ids,aa=client('a','left');set_media(a,replacement['local_media_utf8_exact'])
        left=run.freeze(a,'client_a',1,'left-authority');decision=run.call(a,aa,'client_a',1,'decision',record)
        require(decision == SyncOutput.FULL_SYNC,'divergent populated collections did not require explicit authority')
        chosen=left if upload else right;chosen_ids=left_ids if upload else right_ids
        run.call(a,aa,'client_a',1,'full',record,upload)
        actor='client_b' if upload else 'fresh_client'
        receiver,receiver_auth=empty_receiver(api,root,'collection-observer',run,target,record)
        run.call(receiver,receiver_auth,actor,2,'full',record,False)
        first_media=empty_media_observation(run,receiver,receiver_auth,actor,2,record)
        after_collection=utf8_map(receiver);actual=projection(receiver)
        require_media_preserved(first_media,replacement['expected_server_media_after_collection_only'])
        run.comparisons.append({'actor':actor,'action_index':2,'expected_projection_sha256':sha(encoded(chosen['projection'])),'observed_projection':actual,'media':media_map(receiver)})
        run.call(a,aa,'client_a',3,'media',record)
        receiver,receiver_auth=empty_receiver(api,root,'final-observer',run,target,record)
        run.call(receiver,receiver_auth,actor,4,'full',record,False)
        final_media=empty_media_observation(run,receiver,receiver_auth,actor,4,record)
        require_media_preserved(final_media,replacement['expected_server_media_after_ordinary_reconcile'])
        final_projection=projection(receiver)
        run.comparisons.append({'actor':actor,'action_index':4,'expected_projection_sha256':sha(encoded(chosen['projection'])),'observed_projection':final_projection,'media':media_map(receiver)})
        observed_fields=[value.split('\x1f') for value in receiver.db.list('select flds from notes')]
        values={'full_required_before_authority':decision == SyncOutput.FULL_SYNC,'authority':'upload' if upload else 'download',
            'chosen_fields':observed_fields[0] if len(observed_fields) == 1 else observed_fields,
            'displaced_collection_content_absent':same_value(actual,chosen['projection']) and replacement['right_fields' if upload else 'left_fields'] not in observed_fields,
            'history_scheduling_equal':same_value(actual,chosen['projection']) and same_value(final_projection,chosen['projection']),
            'server_media_after_collection_only':after_collection,'server_media_after_reconcile':utf8_map(receiver)}
        if not upload:values['candidate_not_overwritten']=same_value(actual,right['projection']) and same_value(final_projection,right['projection'])
        return run.result(values,{'frozen_authorities':[right,left],'authority_ids':[right_ids,left_ids],'logical_ids':chosen_ids,'initial_decision':decision,'media_maps':{'after_collection':after_collection,'after_reconcile':utf8_map(receiver)},'media_observations':[first_media,final_media]})


def migration(api,root,target,case):
    recipe=recipes();source={}
    with environment(api,root,target,case) as (run,record,client):
        a,ids,aa=client('a');b,_,ab=client('b');c,_,ac=client('fresh')
        set_media(a,recipe['media_utf8_exact']['initial']);initial=run.freeze(a,'client_a',0,'source-initial')
        require(same_value(authored_values(a,ids),expected_values(recipe,'initial')),'migration author differs from original recipe')
        with api.reference_server(root/'source-service',(target['username'],target['password']),fixed_port=8081,evidence=source) as (port,user,password):
            require(port == 8081,'migration source port changed')
            with api.recorder(8081) as source_record:
                source_record.capture_normal=True;source_record.capture_media=True;source_record.sdk_completions=[]
                auth=a.sync_login(username=user,password=password,endpoint=f'http://127.0.0.1:{source_record.server_port}/')
                run.call(a,auth,'client_a',0,'full',source_record,True);run.call(a,auth,'client_a',0,'media',source_record)
                run.call(a,auth,'client_a',0,'normal',source_record)
                witness_dir=root/'source-witness';witness_dir.mkdir();witness=api.Collection(str(witness_dir/'collection.anki2'));run.clients.append(witness)
                witness_auth=witness.sync_login(username=user,password=password,endpoint=f'http://127.0.0.1:{source_record.server_port}/')
                api.full(witness,witness_auth,False);api.media_sync(witness,witness_auth)
                require(not source_record.errors,'migration source recording incomplete')
                source_equal=same_value(projection(witness),initial['projection']) and media_map(witness) == expected_media(recipe['media_utf8_exact']['initial'])
                authority=run.freeze(a,'client_a',0,'pre-switch-source-authority')
                source['witness_projection']=projection(witness);source['source_sync_completed']=source_equal
                source['preparation_witness']={'actor':'fresh_client','scope':'source-only preparation witness; never candidate credit','traffic':copy.deepcopy(source_record.observations)}
                source['runtime']=api.runtime();source['endpoint']='http://127.0.0.1:8081/'
                source['authoritative_projection_sha256']=sha(encoded(authority['projection']));source['media']=media_map(witness)
            aa=a.sync_login(username=target['username'],password=target['password'],endpoint=f'http://127.0.0.1:{record.server_port}/')
            decision=run.call(a,aa,'client_a',1,'decision',record)
            require(decision in {SyncOutput.FULL_UPLOAD,SyncOutput.FULL_SYNC},'migration candidate did not require upload authority')
            run.call(a,aa,'client_a',1,'full',record,True);run.call(a,aa,'client_a',1,'media',record)
            run.call(b,ab,'client_b',2,'full',record,False);run.call(b,ab,'client_b',2,'media',record)
            b_equal=run.compare(b,ids,authority,'initial','client_b',2)
            run.call(c,ac,'fresh_client',3,'full',record,False);run.call(c,ac,'fresh_client',3,'media',record)
            c_equal=run.compare(c,ids,authority,'initial','fresh_client',3)
        require(source.get('cleanup_complete') is True,'owned migration source cleanup failed')
        return run.result({'source_sync_completed':source_equal,'distinct_owned_source':source.get('owned_pid',0) > 0 and source.get('endpoint') == 'http://127.0.0.1:8081/' and source.get('cleanup_complete') is True,
            'authority':'upload','history_scheduling_equal':b_equal and c_equal,
            'media_equal':media_map(b) == media_map(c) == expected_media(recipe['media_utf8_exact']['initial']),
            'supported_values_match_initial_recipe':b_equal and c_equal},
            {'source':source,'frozen_authorities':[initial,authority],'logical_ids':ids,'initial_decision':decision})


def run(api,segment,target,case):
    require(segment in SEGMENTS,'unknown final Desktop segment')
    identity=api.runtime();require(identity['client']['version'] == '26.09.4' and identity['client']['build'] == 'c06370ff','final Desktop pin mismatch')
    identity['server']=target['server']
    with tempfile.TemporaryDirectory(prefix='final-desktop-') as directory:
        root=pathlib.Path(directory)/segment;api.PHASE='final-'+segment
        if segment == 'rich':details=rich(api,root,target,case)
        elif segment == 'metadata-only':details=metadata_only(api,root,target,case)
        elif segment == 'fresh-fresh':details=fresh_fresh(api,root,target,case)
        elif segment == 'migration':details=migration(api,root,target,case)
        else:details=replacement(api,root,target,case,segment == 'replacement-upload')
    return {'probe_kind':'candidate','phase':'final-'+segment,'fixture':recipes(),'runtime':identity,
            'final_results':{case['variants'][0]:details['values']},'final_details':details}


def resources():
    import os
    import platform
    import resource
    limits={}
    for name in ('cpu.max','memory.max','memory.current'):
        path=pathlib.Path('/sys/fs/cgroup')/name
        if path.is_file():limits[name]=path.read_text().strip()
    usage=resource.getrusage(resource.RUSAGE_SELF)
    return {'system':platform.system(),'release':platform.release(),'machine':platform.machine(),
            'available_cpus':os.cpu_count(),'client_maxrss':usage.ru_maxrss,'client_user_seconds':usage.ru_utime,
            'client_system_seconds':usage.ru_stime,'owned_cgroup':limits}


def empty_receiver(api,root,name,run,target,record):
    directory=root/name;directory.mkdir();col=api.Collection(str(directory/'collection.anki2'));run.clients.append(col)
    require(media_map(col) == {},'replacement observer media is not empty')
    auth=col.sync_login(username=target['username'],password=target['password'],endpoint=f'http://127.0.0.1:{record.server_port}/')
    return col,auth


def empty_media_observation(run,col,auth,actor,action,record):
    initial=media_map(col);require(initial == {},'replacement observation reused local media')
    start=len(record.observations);run.call(col,auth,actor,action,'media',record)
    rows=record.observations[start:]
    cursors=[row['media_request']['lastUsn'] for row in rows if row['route'] == '/msync/mediaChanges']
    return {'actor':actor,'action_index':action,'initial_media':initial,'first_listing_cursor':cursors[0] if cursors else None,
            'observed_hashes':media_map(col),'traffic':copy.deepcopy(rows)}


def require_media_preserved(observation,expected):
    require(observation['initial_media'] == {} and observation['first_listing_cursor'] == 0,'replacement observation lacks independent empty media/cursor')
    require(observation['observed_hashes'] == expected_media(expected),'candidate media changed before reconciliation')


def require_declared_mutation(before,after,ids,recipe,direction):
    """Keep every untouched baseline value; admit only declared local SDK outputs."""
    wanted=copy.deepcopy(before);delta=recipe['rich_'+direction+'_changes']
    model=next(row for row in wanted['objects']['models'] if row['id'] == ids['model'])
    model['tmpls'][0].update(qfmt=delta['question'],afmt=delta['answer']);model['css']=delta['css']
    deck=next(row for row in wanted['objects']['decks'] if row['id'] == ids['deck'])
    deck.update(desc=delta['deck_description'],collapsed=delta['deck_collapsed'])
    preset=next(row for row in wanted['objects']['presets'] if row['id'] == ids['preset']);options=delta['preset']
    preset['new']['perDay']=options['new_per_day'];preset['rev']['perDay']=options['reviews_per_day']
    preset['maxTaken']=options['maximum_answer_seconds'];preset['autoplay']=options['autoplay']
    # Only the declared review's SDK-generated daily counters may change on the
    # selected deck and its parents. They remain compared to this frozen output.
    actual_decks={row['id']:row for row in after['objects']['decks']}
    for row in wanted['objects']['decks']:
        if row['name'] == deck['name'] or deck['name'].startswith(row['name']+'::'):
            for key in ('newToday','revToday','lrnToday','timeToday'):
                if key in actual_decks[row['id']]:row[key]=copy.deepcopy(actual_decks[row['id']][key])
    wanted['objects']['config']['meguri_sync_fixture']=copy.deepcopy(delta['config']['meguri_sync_fixture'])
    wanted['objects']['config_json_hex']['meguri_sync_fixture']=after['objects']['config_json_hex']['meguri_sync_fixture']
    require(same_value(after['objects']['config'].get('meguri_sync_fixture'),delta['config']['meguri_sync_fixture']),'declared config event differs from recipe')
    deleted=ids['notes'][delta['delete_note_key']];keep=ids['notes']['keep']
    keep_row=next(row for row in wanted['notes'] if row[0] == keep);fields=keep_row[4].split('\x1f')
    if 'keep_answer' in delta:fields[1]=delta['keep_answer']
    if 'keep_context' in delta:fields[2]=delta['keep_context']
    keep_row[4]='\x1f'.join(fields)
    wanted['notes']=[row for row in wanted['notes'] if row[0] != deleted]
    removed=[row[0] for row in wanted['cards'] if row[1] == deleted]
    wanted['cards']=[row for row in wanted['cards'] if row[1] != deleted]
    card=next(row for row in wanted['cards'] if row[1] == keep);actual_card=next(row for row in after['cards'] if row[0] == card[0])
    for index in list(range(4,14))+[15]:card[index]=copy.deepcopy(actual_card[index])
    old_reviews={row[0]:row for row in before['reviews']};new=[row for row in after['reviews'] if row[0] not in old_reviews]
    require(len(new) == 1 and new[0][1] == card[0] and new[0][2] == recipe['review_actions'][direction]['button'],'declared single review output missing or wrong')
    wanted['reviews']=sorted(copy.deepcopy(before['reviews'])+copy.deepcopy(new),key=lambda row:row[0])
    wanted['graves']=sorted(copy.deepcopy(before['graves'])+[[ident,0] for ident in removed]+[[deleted,1]],key=lambda row:(row[0],row[1]))
    require(same_value(wanted,after),'undeclared change escaped the original authoritative baseline')


class LegacyTrace:
    """Finite event map for unchanged final-full/normal/media selections."""
    def __init__(self,phase,contract):
        self.phase=phase;self.cases=contract['cases'];self.active=list(self.cases)
        self.inputs=[];self.events=[];self.baselines={};self.order=0;self.authority=None
        self.preparation={name:sha(pathlib.Path(__file__).with_name(name).read_bytes()) for name in
                          ('adapter.py','normal.py','media.py','final_desktop.py','final_checks.py','final_contract.py')}

    def use(self,*variants):
        self.active=[case for case in self.cases if case['variants'][0] in variants]
        require(bool(self.active) and len(self.active) == len(variants),'unknown fixed legacy event group')

    def all_cases(self):self.active=list(self.cases)

    def tick(self):
        number=self.order;self.order+=1;return number

    def bindings(self,actor,action=None):
        rows=[]
        for case in self.active:
            indices=([action] if action is not None else [i for i,row in enumerate(case['actions']) if i > 0 and row['actor'] == actor])
            require(len(indices) == 1 and case['actions'][indices[0]]['actor'] == actor,'legacy actor/action association ambiguous')
            rows.append({'case_id':case['id'],'action_index':indices[0],'actor':actor})
        return rows

    def freeze(self,col,actor,operation,bindings):
        value={'format':'meguri-sync-final-legacy-action-input-v1','phase':self.phase,'actor':actor,'operation':operation,
               'bindings':copy.deepcopy(bindings),'capture_sequence':self.tick(),'preparation_identity':self.preparation,
               'baselines':{row['case_id']:self.baselines.get(row['case_id']) for row in bindings},
               'normalization':NORMALIZATION,'projection':projection(col),'media':media_map(col)}
        digest=sha(encoded(value));self.inputs.append({'sha256':digest,'value':decode(encoded(value))});return digest,value

    def prepare(self,col):
        bindings=self.bindings('client_a',0);digest,value=self.freeze(col,'client_a','prepare',bindings)
        for row in bindings:
            require(row['case_id'] not in self.baselines,'legacy baseline recaptured from candidate')
            self.baselines[row['case_id']]=digest
        self.events.append({'sequence':len(self.events),'operation':'prepare','actor':'client_a','bindings':bindings,
            'input_sha256':digest,'capture_sequence':value['capture_sequence'],'submit_sequence':None,'completion_sequence':self.tick(),
            'baselines':{row['case_id']:digest for row in bindings},'preparation_identity':self.preparation})

    def authoritative(self,col,actor,action=None):
        bindings=self.bindings(actor,action);digest,value=self.freeze(col,actor,'local-authority',bindings)
        self.authority=(digest,value)

    def call(self,col,actor,operation,record,perform,action=None,comparison='state',auxiliary=False):
        bindings=[] if auxiliary else self.bindings(actor,action)
        digest,value=self.freeze(col,actor,operation,bindings)
        authoritative=self.authority if comparison in {'state','media'} else (digest,value)
        require(authoritative is not None,'legacy independent authority missing')
        authority_digest,authority=authoritative
        begin=len(record.observations);submit=self.tick();started=time.monotonic();result=perform();complete=self.tick()
        output={'projection':projection(col),'media':media_map(col)}
        if comparison == 'blank':output['integrity']=col.db.scalar('pragma integrity_check')
        rows=copy.deepcopy(record.observations[begin:])
        if comparison == 'state':matched=same_value(output['projection'],authority['projection'])
        elif comparison == 'media':matched=output['media'] == authority['media'] and same_value(output['projection'],value['projection'])
        elif comparison == 'blank':matched=not output['projection']['notes'] and not output['projection']['cards'] and col.db.scalar('pragma integrity_check') == 'ok'
        else:matched=same_value(output['projection'],value['projection']) and output['media'] == value['media']
        event={'sequence':len(self.events),'operation':operation,'actor':actor,'bindings':bindings,'input_sha256':digest,
            'authoritative_input_sha256':authority_digest,'capture_sequence':value['capture_sequence'],'submit_sequence':submit,
            'completion_sequence':complete,'baselines':{row['case_id']:self.baselines[row['case_id']] for row in bindings},
            'preparation_identity':self.preparation,'comparison':comparison,'output':output,'output_sha256':sha(encoded(output)),
            'traffic':rows,'matched':matched,'elapsed_ms':int((time.monotonic()-started)*1000),'resources':resources()}
        if operation == 'normal' or operation == 'decision':event['required']=result.required
        self.events.append(event)
        require(matched,'legacy output differs from unchanged authoritative input')
        return result

    def result(self):
        require(set(self.baselines) == {case['id'] for case in self.cases},'legacy initial baseline coverage missing')
        for case in self.cases:
            for index in range(len(case['actions'])):
                require(any(any(row['case_id'] == case['id'] and row['action_index'] == index for row in event['bindings']) for event in self.events),'legacy actual action missing')
        return {'phase':self.phase,'preparation_identity':self.preparation,'baselines':self.baselines,'events':self.events}
