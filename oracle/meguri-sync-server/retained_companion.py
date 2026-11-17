# SPDX-License-Identifier: AGPL-3.0-or-later
"""Closed retained8 Desktop companion; every output is noncredit."""
import copy
import pathlib
import re
import sqlite3
import struct
import sys

from candidate import read_credentials, retained_dependencies
from final_contract import RECIPE_SHA256, recipes
from retained_observer import action_input, observing, public_file
from validation import Artifacts, DIGEST, SOURCE_FIELDS, decode, encoded, require, same_value, sha

SELECTION_SHA256='3df220aac299fe6f7c253a3e32c3c340171fb6c856b078589354f869c08592d7'
OUTER_FIELDS=set(SOURCE_FIELDS)|{'executable_sha256','run_id','author_session','root','server_arguments','data_volume','owner_nonce'}
# Original public schema metadata ca55bdc950d506be75182f9ad2c036b7a534ed912f3ea49ea60ad5a373562db1.
TABLES=(
    ('col','id crt mod scm ver dty usn ls conf models decks dconf tags','id','mod scm usn ls'),
    ('notes','id guid mid mod usn tags flds sfld csum flags data','id','mod usn'),
    ('cards','id nid did ord mod usn type queue due ivl factor reps lapses left odue odid flags data','id','mod usn'),
    ('revlog','id cid usn ease ivl lastIvl factor time type','id','usn'),
    ('graves','oid type usn','oid type','usn'),
    ('deck_config','id name mtime_secs usn config','id','mtime_secs usn'),
    ('config','KEY usn mtime_secs val','KEY','mtime_secs usn'),
    ('tags','tag usn collapsed config','tag','usn'),
    ('fields','ntid ord name config','ntid ord',''),
    ('templates','ntid ord name mtime_secs usn config','ntid ord','mtime_secs usn'),
    ('notetypes','id name mtime_secs usn config','id','mtime_secs usn'),
    ('decks','id name mtime_secs usn common kind','id','mtime_secs usn'),
)
ACTIONS={
    'full-upload':('prepare_rich','upload_baseline','witness_upload'),
    'full-download':('prepare_right','upload_baseline','witness_download'),
    'normal-no-change':('prepare_rich','upload_baseline','settle_normal'),
    'normal-bidirectional':('prepare_rich','upload_baseline','settle_normal','receive_forward','prepare_reverse','send_reverse'),
    'media-bidirectional':('baseline_media','prepare_forward_media','send_forward_media','receive_reverse_media'),
    'manual-uncertain-recovery':('witness_committed',),
}
OFFLINE=frozenset(('prepare_rich','prepare_right','prepare_reverse','prepare_forward_media'))
NEW_AUTHORITY=frozenset(('prepare_rich','prepare_right'))
WITNESSES=frozenset(('witness_upload','witness_download','witness_committed'))
SEND=frozenset(('upload_baseline','send_reverse','send_forward_media'))
MEDIA_ONLY=frozenset(('baseline_media','prepare_forward_media','send_forward_media','receive_reverse_media'))


def actor_for(case,action):
    require(action in ACTIONS.get(case,()),'unknown retained companion action')
    return 'fresh_client' if action in WITNESSES else 'client_b'


def cell(kind,value):
    if kind == b'null':return {'null':True}
    if kind == b'integer':return {'integer':str(value)}
    if kind == b'real':return {'real_bits':struct.pack('>d',value).hex()}
    require(kind in {b'text',b'blob'} and isinstance(value,bytes),'unsupported retained SQLite cell')
    return {'text_hex' if kind == b'text' else 'blob_hex':value.hex()}


def validate_snapshot(value):
    require(isinstance(value,dict) and set(value) == {'format','tables','media_sha256'} and value['format'] == 'meguri-retained-snapshot-v1','invalid retained snapshot')
    require(isinstance(value['tables'],dict) and set(value['tables']) == {row[0] for row in TABLES},'incomplete retained snapshot tables')
    for name,columns,_,_ in TABLES:
        table=value['tables'][name];names=columns.split()
        require(isinstance(table,dict) and set(table) == {'columns','rows'} and table['columns'] == names and isinstance(table['rows'],list),'invalid retained snapshot columns')
        for row in table['rows']:
            require(isinstance(row,list) and len(row) == len(names),'invalid retained snapshot row')
            for item in row:
                require(isinstance(item,dict) and len(item) == 1,'invalid retained snapshot cell')
                kind,data=next(iter(item.items()))
                if kind == 'null':require(data is True,'invalid retained null')
                elif kind == 'integer':
                    require(type(data) is str and re.fullmatch(r'0|-?[1-9][0-9]*',data) and -(1<<63) <= int(data) < (1<<63),'invalid retained integer')
                elif kind == 'real_bits':require(type(data) is str and re.fullmatch('[0-9a-f]{16}',data),'invalid retained real bits')
                else:require(kind in {'text_hex','blob_hex'} and type(data) is str and re.fullmatch('(?:[0-9a-f]{2})*',data),'invalid retained byte cell')
    media=value['media_sha256']
    require(isinstance(media,dict) and all(type(name) is str and name and '/' not in name and '\\' not in name and type(digest) is str and DIGEST.fullmatch(digest) for name,digest in media.items()),'invalid retained whole media map')
    return value


def snapshot(database,media):
    require(database.is_file() and not database.is_symlink() and database.stat().st_nlink == 1,'invalid companion collection file')
    connection=sqlite3.connect(database.as_uri()+'?mode=ro',uri=True)
    connection.text_factory=bytes
    connection.create_collation('unicase',lambda a,b:(a.casefold()>b.casefold())-(a.casefold()<b.casefold()))
    tables={}
    try:
        connection.execute('pragma query_only=on')
        for name,columns,keys,_ in TABLES:
            names=columns.split();quoted=[f'"{column}"' for column in names]
            order=','.join(f'"{key}"'+(' collate unicase' if name == 'tags' else ' collate binary' if name == 'config' else '') for key in keys.split())
            sql='select '+','.join(part for column in quoted for part in (f'typeof({column})',column))+f' from "{name}" order by '+order
            rows=[[cell(row[i],row[i+1]) for i in range(0,len(row),2)] for row in connection.execute(sql)]
            tables[name]={'columns':names,'rows':rows}
    finally:connection.close()
    require(not media.is_symlink(),'linked companion media directory')
    hashes={}
    for path in sorted(media.iterdir()) if media.exists() else []:
        require(not path.is_symlink(),'linked companion media file')
        require(path.is_file() and path.stat().st_nlink == 1,'special companion media file')
        hashes[path.name]=sha(path.read_bytes())
    return validate_snapshot({'format':'meguri-retained-snapshot-v1','tables':tables,'media_sha256':hashes})


def supported(value):
    validate_snapshot(value);result=copy.deepcopy(value['tables'])
    for name,columns,_,stamps in TABLES:
        positions=[columns.split().index(column) for column in stamps.split()]
        for row in result[name]['rows']:
            for index in positions:row[index]={'protocol_stamp':True}
    return result


def collection_equal(actual,expected):return same_value(supported(actual),supported(expected))


def compare(actual,expected,media_only=False,witness=False):
    validate_snapshot(actual);validate_snapshot(expected)
    if media_only:return same_value(actual['media_sha256'],expected['media_sha256'])
    return collection_equal(actual,expected) and (not witness or same_value(actual['media_sha256'],expected['media_sha256']))


def inputs(root,out):
    require({path.name for path in root.iterdir()} == {'action.json','invocation.json','preflight.json','expectations.json','artifacts'},'invalid companion input layout')
    require(not root.is_symlink() and not (root/'artifacts').is_symlink(),'linked companion inputs')
    action=action_input(root);actor=actor_for(action['case'],action['action'])
    raw={name:public_file(root/name) for name in ('action.json','invocation.json','preflight.json','expectations.json')}
    invocation=decode(raw['invocation.json']);preflight=decode(raw['preflight.json']);expect=decode(raw['expectations.json'])
    require(isinstance(invocation,dict) and set(invocation) == OUTER_FIELDS and invocation['run_id'] == __import__('os').environ.get('MG_RUN_ID'),'invalid companion invocation identity')
    require(isinstance(expect,dict) and set(expect) == {'format','case','action','actor','recipe_sha256','selection_sha256','review_sha256','baseline_sha256','expected_sha256'},'invalid companion expectations shape')
    require(expect['format'] == 'meguri-retained-companion-input-v1' and all(expect[key] == action[key] for key in ('case','action')) and expect['actor'] == actor,'companion action identity mismatch')
    require(expect['recipe_sha256'] == RECIPE_SHA256 and expect['selection_sha256'] == SELECTION_SHA256,'companion committed inputs changed')
    source=Artifacts(root/'artifacts');store=Artifacts(out/'artifacts')
    dependencies=retained_dependencies(preflight,source,invocation,preflight=True)
    for key in ('recipe_sha256','selection_sha256','review_sha256'):
        require(type(expect[key]) is str and DIGEST.fullmatch(expect[key]),'invalid companion input hash');dependencies.add(expect[key])
    for key in ('baseline_sha256','expected_sha256'):
        digest=expect[key]
        require((digest is None and action['action'] in OFFLINE) or (type(digest) is str and DIGEST.fullmatch(digest)),'missing companion authority')
        if digest is not None:dependencies.add(digest)
    source.read(expect['review_sha256']).decode('utf-8')  # Opaque accepted review; MG owns applicability.
    source.read(RECIPE_SHA256)
    selection=source.json(SELECTION_SHA256)
    require(selection.get('fixture_recipes_sha256') == RECIPE_SHA256 and len(selection.get('cases',[])) == 8 and action['case'] in {case['variants'][0] for case in selection['cases']},'invalid retained selection')
    for path in (root/'artifacts').iterdir():
        require(DIGEST.fullmatch(path.name),'invalid companion CAS entry');store.put(source.read(path.name))
    for digest in dependencies:store.read(digest)
    hashes={name:store.put(data) for name,data in raw.items()}
    return action,actor,invocation,expect,store,hashes


def owner(state,case,invocation,expect,new=False):
    require(not state.is_symlink(),'linked companion state');state.mkdir(exist_ok=True)
    identity={'case':case,'run_id':invocation['run_id'],'source_commit':invocation['source_commit'],
        'recipe_sha256':expect['recipe_sha256'],'selection_sha256':expect['selection_sha256'],'review_sha256':expect['review_sha256']}
    path=state/'owner.json'
    if new:
        require(not list(state.iterdir()),'companion profile already exists')
        with path.open('xb') as stream:stream.write(encoded(identity))
    else:require(same_value(decode(public_file(path)),identity),'companion state belongs to another case or run')
    for path in state.rglob('*'):
        require(not path.is_symlink() and (path.is_dir() or (path.is_file() and path.stat().st_nlink == 1)),'linked or special companion profile')


def logical_ids(col):
    recipe=recipes()['rich_initial']
    model=col.models.by_name(recipe['note_type']);require(model is not None,'missing original companion type')
    decks=[row for row in col.decks.all() if row['name'] == recipe['deck']['name']]
    require(len(decks) == 1,'missing original companion deck');deck=decks[0]
    presets=[row for row in col.decks.all_config() if row['name'] == recipe['preset']['name']]
    require(len(presets) == 1,'missing original companion preset')
    prompts={row['fields'][0]:row['key'] for row in recipe['notes']};notes={}
    for ident in col.find_notes(''):
        note=col.get_note(ident)
        if note.fields[0] in prompts:
            key=prompts[note.fields[0]];require(key not in notes,'ambiguous original companion note');notes[key]=ident
    require({'keep','delete_reverse'} <= set(notes),'missing original reverse inputs')
    return {'model':model['id'],'deck':deck['id'],'preset':presets[0]['id'],'notes':notes}


def execute(root,out,state,credential_path):
    import adapter as api
    from anki.sync import SyncOutput
    from final_desktop import author, change, expected_media, set_media
    from normal import sync as ordinary
    action,actor,invocation,expect,store,hashes=inputs(root,out)
    case,name=action['case'],action['action'];client=api.runtime()['client']
    require(client['version'] == '26.09.4' and client['build'] == 'c06370ff' and client['runtime_sha256'] == 'bf0ccafe0f15bcba59a652db68c4cfdfaa8191756f8c547839c9d134dee12784','companion Desktop identity changed')
    libraries=list(pathlib.Path(sys.modules['anki.collection'].__file__).parent.glob('_rsbridge*.so'))
    require(len(libraries) == 1,'companion native runtime artifact unavailable')
    require(store.put(libraries[0].read_bytes()) == client['runtime_sha256'],'companion native runtime artifact changed')
    result={'format':'meguri-retained-companion-v1',**action,'actor':actor,'invocation_sha256':hashes['invocation.json'],
        'preflight_peer_sha256':hashes['preflight.json'],'expectations_sha256':hashes['expectations.json'],'client':client,
        'before_sha256':None,'after_sha256':None,'authority_sha256':None,'wire_sha256':None,
        'observed':{'operation_complete':False,'error_kind':None,'expected_state_equal':None},'case_credit':False}
    expected=store.json(expect['expected_sha256']) if expect['expected_sha256'] is not None else None
    if expected is not None:validate_snapshot(expected)
    baseline=store.json(expect['baseline_sha256']) if expect['baseline_sha256'] is not None else None
    if baseline is not None:validate_snapshot(baseline)
    if name not in OFFLINE:result['authority_sha256']=expect['expected_sha256']
    before=None;after=None;col=None;wire=None;closed=False;stage='preparation'
    try:
        fresh=name in NEW_AUTHORITY or name == 'baseline_media' or name in WITNESSES
        if name in WITNESSES:
            owner(state,case,invocation,expect,new=not (state/'owner.json').exists())
            profile=state/'fresh_client'/name;require(not profile.exists(),'companion witness profile reused');profile.mkdir(parents=True)
        else:
            owner(state,case,invocation,expect,new=fresh);profile=state/'client_b'
        database=profile/'collection.anki2'
        if name in NEW_AUTHORITY:
            col,_=author(api,state,'client_b',recipes(),side='right' if name == 'prepare_right' else None)
            wanted=recipes()['replacement']['server_media_utf8_exact'] if name == 'prepare_right' else recipes()['media_utf8_exact']['initial']
            set_media(col,wanted)
        else:
            if database.exists():
                stage='projection';before=snapshot(database,profile/'collection.media');result['before_sha256']=store.put_json(before)
            else:require(fresh,'companion persistent profile is absent');profile.mkdir(exist_ok=True)
            stage='preparation';col=api.Collection(str(database))
            if before is None:
                require(col.db.scalar('select count(*) from notes') == 0 and col.db.scalar('select count(*) from cards') == 0,'companion receiver is not empty')
                require(not any(path.is_file() for path in pathlib.Path(col.media.dir()).iterdir()),'companion media receiver is not empty')
                col.close(downgrade=False);col=None
                stage='projection';before=snapshot(database,profile/'collection.media');result['before_sha256']=store.put_json(before)
                stage='preparation';col=api.Collection(str(database))
            if name == 'prepare_reverse':
                require(before is not None and expected is not None and compare(before,expected),'reverse preparation lacks unchanged forward authority')
                change(col,logical_ids(col),recipes(),'reverse',with_media=False)
            elif name == 'prepare_forward_media':
                require(before is not None and baseline is not None and compare(before,baseline,media_only=True),'media preparation lacks initial authority')
                set_media(col,recipes()['media_utf8_exact']['forward'])
            else:
                if name in SEND:
                    ready=before is not None and expected is not None and compare(before,expected,media_only=name in MEDIA_ONLY)
                    if not ready:result['observed']['expected_state_equal']=False
                    require(ready,'companion send differs from frozen authority')
                stage='public-api';credentials=read_credentials(credential_path)
                with observing(case,name,actor,hashes['invocation.json']) as observed:
                    try:
                        auth=col.sync_login(username=credentials['username'],password=credentials['password'],endpoint='http://127.0.0.1:18080/')
                        if name == 'upload_baseline':api.full(col,auth,True);api.media_sync(col,auth)
                        elif name == 'settle_normal':
                            decision=col.sync_collection(auth,sync_media=False).required
                            if decision in {SyncOutput.FULL_DOWNLOAD,SyncOutput.FULL_SYNC}:api.full(col,auth,False)
                            else:require(decision == SyncOutput.NO_CHANGES,'baseline requests unexpected upload')
                            ordinary(col,auth)
                        elif name in {'receive_forward','send_reverse'}:ordinary(col,auth)
                        elif name in MEDIA_ONLY:api.media_sync(col,auth)
                        else:
                            require(name in WITNESSES,'unknown companion online action');api.full(col,auth,False);api.media_sync(col,auth)
                    finally:wire=observed
                require(not wire.failed,'companion wire observation incomplete')
        col.close(downgrade=False);col=None;closed=True
        stage='projection';after=snapshot(database,profile/'collection.media');result['after_sha256']=store.put_json(after)
        stage='comparison'
        if name in OFFLINE:
            result['authority_sha256']=result['after_sha256']
            if name == 'prepare_forward_media':require(after['media_sha256'] == expected_media(recipes()['media_utf8_exact']['forward']),'forward media preparation differs from recipe')
        else:
            result['authority_sha256']=expect['expected_sha256']
            equal=compare(after,expected,media_only=name in MEDIA_ONLY,witness=name in WITNESSES)
            if name in MEDIA_ONLY and before is not None:equal=equal and collection_equal(before,after)
            if name not in (MEDIA_ONLY|WITNESSES) and before is not None:equal=equal and same_value(before['media_sha256'],after['media_sha256'])
            result['observed']['expected_state_equal']=equal;require(equal,'companion observed state mismatch')
            require(wire is not None and wire.rows and all(row['disposition'] == 'forwarded' and row['upstream_status'] == 200 for row in wire.rows),'companion online completion incomplete')
        result['observed']['operation_complete']=True
    except BaseException as error:
        if stage == 'projection' or (isinstance(error,ValueError) and str(error) == 'companion wire observation incomplete'):kind='incomplete'
        elif name in OFFLINE:kind='preparation_refused'
        elif isinstance(error,ValueError) and str(error) in {'companion observed state mismatch','companion send differs from frozen authority'}:kind='state_mismatch'
        else:kind='public_api_failure' if stage == 'public-api' else 'incomplete'
        result['observed']['error_kind']=kind
    finally:
        if col is not None:
            try:col.close(downgrade=False);closed=True
            except BaseException:result['observed'].update(operation_complete=False,error_kind='incomplete')
        if closed and after is None:
            try:after=snapshot(database,profile/'collection.media');result['after_sha256']=store.put_json(after)
            except BaseException:pass
        if wire is not None:result['wire_sha256']=store.put_json(wire.wire())
        (out/'observations.json').write_bytes(encoded(result))
    return 0 if result['observed']['operation_complete'] else 1


def main(arguments):
    require(arguments == ['--input','/in','--credentials','/secret/credentials.json'],'fixed retained companion paths required')
    root,out,state=pathlib.Path('/in'),pathlib.Path('/out'),pathlib.Path('/state')
    require(not list(out.iterdir()),'companion output already populated')
    return execute(root,out,state,'/secret/credentials.json')


if __name__ == '__main__':
    import sys
    try:code=main(sys.argv[1:])
    except BaseException:code=1
    sys.exit(code)
