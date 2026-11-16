# SPDX-License-Identifier: AGPL-3.0-or-later
"""Finite valid synthetic protocol matrix, with separate actual SDK receivers."""
from __future__ import annotations
import json
import contextlib
import copy
import pathlib
import secrets
import sqlite3
import tempfile
from normal import exact_numbers
from validation import require, same_value, encoded

MATRIX_FIXTURE={'format':'meguri-sync-normal-matrix-v1','base':{'notetype':'Cedar matrix recall','fields':['Prompt','Reply'],'template':{'name':'Cedar face','question':'{{Prompt}}','answer':'{{Reply}}'},'css':''},'note_count':5,'fixed_mod':2000000000,'cursor':1,'limits':['Desktop 26.09.4 only. No whole-family, candidate NORMAL, Mobile or final acceptance.','Auxiliary valid normal protocol fixtures, not SDK-emitted traffic. Actual SDK receivers reported separately.','Structural full-sync selection unobserved. Only selected ASCII name collision facts.']}

def definitions():
 return [('P-N06','card-pending-matrix',{'older_kept':True,'equal_kept':True,'newer_incoming':True,'nonpending_older_incoming':True,'unknown_top_level_dropped':True,'known_edit_retains_known_data':True}),
 ('P-N04','model-mod-matrix',{'older_kept':True,'equal_incoming':True,'newer_incoming':True,'response_before_merge':True}),
 ('P-N04','preset-mod-matrix',{'older_kept':True,'equal_incoming':True,'newer_incoming':True,'response_before_merge':True}),
 ('P-N04','ascii-name-collisions',{'deck_repeated_plus':True,'model_repeated_plus':True,'tag_spelling_state_preserved':True}),
 ('P-N05','config-optional',{'omitted_preserved':True,'null_preserved':True}),
 ('P-N05','config-replacement-outgoing',{'old_key_removed':True,'replacement_received':True,'outgoing_config':True,'outgoing_crt':True}),
 ('P-N03','grave-then-occurrence',{'card_present':True,'note_present':True,'deck_present':True,'graves_retained':True})]

def matrix_selection():
 return {'format':'meguri-sync-selection-v1','lane_kind':'reference','scope':'Finite Desktop matrix; auxiliary protocol and actual SDK receivers. No broad compatibility credit.','cases':[{'id':f'{family}:desktop.reference.matrix-{variant}','family':family,'classification':'current_probe','spec_ids':['sync/server-collection#C4' if family=='P-N06' else 'sync/server-collection#C3'],'variants':[variant],'client_kind':'desktop','expected':[{'name':k,'value':v} for k,v in values.items()],'actions':[{'actor':'client_a','operation':'prepare_synthetic','description':'Create fresh authored Cedar note type, templates, CSS and rows.'},{'actor':'fault_driver','operation':'native_request','description':'Send bounded auxiliary valid protocol traffic with retained exact bytes.'},{'actor':'client_b','operation':'client_sync','description':'Inspect committed state with actual Desktop SDK receiver.'}]} for family,variant,values in definitions()]}

SERVERS={}
PROGRESS={}
def persist():
 pathlib.Path('/out/matrix-progress.json').write_bytes(encoded(exact_numbers(PROGRESS)))
def raw_state(root):return SERVERS[root]()

def database_state(db):
 db.row_factory=sqlite3.Row
 available={row['name'] for row in db.execute("select name from sqlite_master where type='table'")}
 output={table:[{k:v for k,v in dict(row).items() if not isinstance(v,bytes)} for row in db.execute('select * from '+table)] for table in ('col','notetypes','decks','deck_config','config','cards','notes','graves','tags') if table in available}
 if 'config' in available:
  output['config']=[{k:({'json':json.loads(bytes(v)),'hex':bytes(v).hex()} if isinstance(v,bytes) else v) for k,v in dict(row).items()} for row in db.execute('select * from config')]
 output['storage_tables']=sorted(available);col=output['col'][0]
 for table,key in [('notetypes','models'),('decks','decks'),('deck_config','dconf')]:
  if table not in output:output[table]=list(json.loads(col[key]).values())
 if 'config' not in output:output['config']=[{'key':key,'value':value} for key,value in json.loads(col['conf']).items()]
 if 'tags' not in output:output['tags']=json.loads(col['tags'])
 return output

def closed_state(root):
 paths=list((root/'server').rglob('*.anki2'));require(len(paths)==1,'one closed reference database required')
 with sqlite3.connect('file:'+str(paths[0])+'?mode=ro',uri=True) as db:
  db.create_collation('unicase',lambda left,right:(left.casefold()>right.casefold())-(left.casefold()<right.casefold()))
  return database_state(db)

def by_id(rows,ident):return next(row for row in rows if row['id']==ident)

@contextlib.contextmanager
def environment(api,root):
 root.mkdir()
 with api.reference_server(root/'server') as (port,user,password),api.recorder(port) as rec:
  rec.capture_normal=True;rec.capture_matrix=True;clients=[]
  try:
   a,model=api.collection(root,'a',MATRIX_FIXTURE['base']);clients.append(a)
   for i in range(MATRIX_FIXTURE['note_count']):api.add_note(a,model,[f'Cedar prompt {i}',f'Cedar reply {i}'])
   if root.name=='ascii-name-collisions':
    note=a.get_note(a.find_notes('')[0]);note.tags=['CedarTag'];a.update_note(note)
    a.db.execute('update tags set usn=0,collapsed=0 where tag=?','CedarTag')
   a.decks.add_config('Cedar matrix preset')
   b,_=api.collection(root,'b',MATRIX_FIXTURE['base']);clients.append(b)
   endpoint=f'http://127.0.0.1:{rec.server_port}/'
   aa=a.sync_login(username=user,password=password,endpoint=endpoint);ab=b.sync_login(username=user,password=password,endpoint=endpoint)
   api.full(a,aa,True);api.full(b,ab,False);a.sync_collection(aa,sync_media=False)
   traffic=[]
   PROGRESS.setdefault('traffic',{})[root.name]=traffic
   def call(method,body,session=''):
    response=api.native_request(port,aa,method,body,session,retain_input=True)
    traffic.append({'origin':'auxiliary-valid-normal-protocol','method':method,'session':session,'request':copy.deepcopy(body),'response':response})
    persist()
    require(response['status']==200,'valid matrix request refused');return response['value']
   def meta():return call('meta',{'v':11,'cv':'meguri-cedar-matrix'})
   def begin(cursor=0,lnewer=False):
    s=secrets.token_hex(8);call('start',{'minUsn':cursor,'lnewer':lnewer,'graves':None},s);return s
   def changes(s,value=None):return call('applyChanges',{'changes':value if value is not None else {'models':[],'decks':[[],[]],'tags':[]}},s)
   def end(s,chunk=None):
    for _ in range(3):
     if call('chunk',{'_pad':None},s)['done']:break
    else:raise ValueError('small matrix chunk bound exceeded')
    call('applyChunk',{'chunk':chunk or {'done':True}},s);return call('finish',{'_pad':None},s)
   def receive():
    api.full(b,ab,False)
    return {'models':[b.models.get(x.id) for x in b.models.all_names_and_ids()],'decks':b.decks.all(),'presets':b.decks.all_config(),'cards':b.db.all('select * from cards order by id'),'notes':b.db.all('select id,mod,usn,flds from notes order by id'),'graves':b.db.all('select * from graves order by oid,type'),'config_stamps':b.db.all('select key,usn,mtime_secs from config order by key')}
   def served():
    before=len(rec.observations);api.full(b,ab,False)
    rows=[row['download_stamp']['matrix_state'] for row in rec.observations[before:] if 'download_stamp' in row]
    require(len(rows)==1,'one raw served matrix state required')
    PROGRESS.setdefault('served',{}).setdefault(root.name,[]).append(rows[0]);persist();return rows[0]
   SERVERS[root]=served
   yield a,b,model,rec,traffic,meta,begin,changes,end,receive,call
   require(not rec.errors,'matrix recorder incomplete')
  finally:
   SERVERS.pop(root,None)
   for col in clients:col.close(downgrade=False)

def run(api,fixture):
 require(same_value(fixture,MATRIX_FIXTURE),'unknown matrix fixture');identity=api.runtime()
 require(identity['client']['build']=='c06370ff' and identity['client']['runtime_sha256']=='bf0ccafe0f15bcba59a652db68c4cfdfaa8191756f8c547839c9d134dee12784','matrix runtime pin mismatch')
 require(identity['lock_sha256']=='56e1bef86def9f5f230d12317e3510f2176eb079928f3a75f86afb0dcc408ec8','matrix lock mismatch')
 results={};details={};inputs={}
 PROGRESS.clear();PROGRESS.update(runtime=identity,fixture=fixture,normal_results=results,matrix_details=details);persist()
 with tempfile.TemporaryDirectory(prefix='cedar-matrix-') as scratch:
  root=pathlib.Path(scratch)
  variant='card-pending-matrix';api.PHASE='matrix-'+variant;path=root/variant
  with environment(api,path) as (a,b,model,rec,traffic,meta,begin,changes,end,receive,call):
   originals=a.db.all('select * from cards order by id');samples=[];outcomes=[]
   for index,(label,seed_usn,delta) in enumerate([('older',1,-1),('equal',1,0),('newer',1,1),('nonpending_older',0,-1)]):
    seed=copy.deepcopy(originals[index]);seed[4]=2000000000;seed[5]=seed_usn;seed[8]=17
    s=begin(2);changes(s);end(s,{'cards':[seed],'done':True})
    prepared=None
    if label=='nonpending_older':
     prepared=by_id(raw_state(path)['cards'],seed[0])
     require((prepared['mod'],prepared['usn'],prepared['due'])==(2000000000,0,17),'nonpending seed was not stored as prepared')
    incoming=copy.deepcopy(seed);incoming[4]+=delta;incoming[5]=meta()['usn'];incoming[8]=23
    s=begin(1);changes(s);end(s,{'cards':[incoming],'done':True})
    after=by_id(raw_state(path)['cards'],seed[0]);outcomes.append(after)
    samples.append({'label':label,'seed_tuple':seed,'incoming_tuple':incoming,'cursor':1,'raw_served_before_sdk_normalization':after})
    if prepared is not None:samples[-1]['raw_seed_before_incoming_round']=prepared
   extra=copy.deepcopy(originals[4]);extra[4]=2000000000;extra[5]=meta()['usn'];extra[17]='{"pos":5,"zz":7}'
   s=begin();changes(s);end(s,{'cards':[extra],'done':True});unknown=by_id(raw_state(path)['cards'],extra[0])
   unknown_input=copy.deepcopy(extra)
   extra[4]+=1;extra[5]=meta()['usn'];extra[8]=29
   s=begin();changes(s);end(s,{'cards':[extra],'done':True});known=by_id(raw_state(path)['cards'],extra[0])
   samples.append({'label':'unknown-top-level-data-then-known-edit','incoming_unknown_tuple':unknown_input,'raw_after_unknown':unknown,'raw_after_known_edit':known})
   receiver=receive()
   results[variant]=dict(zip(('older_kept','equal_kept','newer_incoming','nonpending_older_incoming'),[outcomes[0]['due']==17 and outcomes[0]['mod']==2000000000 and outcomes[0]['usn']==1,outcomes[1]['due']==17 and outcomes[1]['mod']==2000000000 and outcomes[1]['usn']==1,outcomes[2]['due']==23 and outcomes[2]['mod']==2000000001 and outcomes[2]['usn']==samples[2]['incoming_tuple'][5],samples[3]['raw_seed_before_incoming_round']['mod']==2000000000 and samples[3]['raw_seed_before_incoming_round']['usn']==0 and samples[3]['raw_seed_before_incoming_round']['due']==17 and outcomes[3]['due']==23 and outcomes[3]['mod']==1999999999 and outcomes[3]['usn']==incoming[5]]))
   results[variant].update(unknown_top_level_dropped=unknown['data']=='{"pos":5}',known_edit_retains_known_data=known['data']=='{"pos":5}' and known['due']==29)
   details[variant]={'samples':samples,'receiver':receiver,'traffic':traffic,'sdk_traffic':rec.observations};inputs[variant]=traffic
  details[variant]['server_closed_state']=closed_state(path)
  additional(api,root,results,details,inputs)
 persist()
 return exact_numbers({'probe_kind':'normal-matrix','runtime':identity,'fixture':fixture,'normal_results':results,'matrix_details':details,'case_inputs':inputs})

def additional(api,root,results,details,inputs):
 for kind in ('model','preset'):
  variant=kind+'-mod-matrix';api.PHASE='matrix-'+variant;path=root/variant
  with environment(api,path) as (a,b,model,rec,traffic,meta,begin,changes,end,receive,call):
   original=copy.deepcopy(a.models.get(model['id']) if kind=='model' else next(config for config in a.decks.all_config() if config['name']=='Cedar matrix preset'))
   original['name']='Cedar stored '+kind;original['mod']=MATRIX_FIXTURE['fixed_mod'];S=original['mod'];ident=original['id'];outcomes=[];responses=[];samples=[]
   def body(obj):return {'models':[obj] if kind=='model' else [],'decks':[[],[obj] if kind=='preset' else []],'tags':[]}
   s=begin();changes(s,body(original));end(s)
   for label,delta in [('older',-1),('equal',0),('newer',1)]:
    incoming=copy.deepcopy(original);incoming.update(name='Cedar '+label+' '+kind,mod=S+delta,usn=meta()['usn'])
    if kind=='model':incoming['css']='.card { color: #456789; }'
    else:incoming['new']['perDay']=31+delta
    s=begin();response=changes(s,body(incoming));end(s)
    raw=raw_state(path);stored=by_id(raw['notetypes' if kind=='model' else 'deck_config'],ident)
    prior=next(o for o in (response['models'] if kind=='model' else response['decks'][1]) if o['id']==ident)
    receiver=receive();seen=next(o for o in receiver['models' if kind=='model' else 'presets'] if o['id']==ident)
    outcomes.append(seen['name']);responses.append(prior['name'])
    samples.append({'label':label,'S':S,'incoming':incoming,'response_prior':prior,'raw_served':stored,'receiver':seen})
   results[variant]={'older_kept':outcomes[0]==original['name'],'equal_incoming':outcomes[1]=='Cedar equal '+kind,'newer_incoming':outcomes[2]=='Cedar newer '+kind,'response_before_merge':responses==[original['name'],original['name'],'Cedar equal '+kind]}
   details[variant]={'samples':samples,'traffic':traffic,'sdk_traffic':rec.observations};inputs[variant]=traffic
  details[variant]['server_closed_state']=closed_state(path)
 variant='ascii-name-collisions';api.PHASE='matrix-'+variant;path=root/variant
 with environment(api,path) as (a,b,model,rec,traffic,meta,begin,changes,end,receive,call):
  for index,name in enumerate(('Alpha','alpha+','alpha')):
   deck=copy.deepcopy(a.decks.get(1));deck.update(id=810001+index,name=name,mod=2000000000,usn=meta()['usn'])
   nt=copy.deepcopy(model);nt.update(id=820001+index,name=name,mod=2000000000,usn=meta()['usn'])
   s=begin();changes(s,{'models':[nt],'decks':[[deck],[]],'tags':[]});end(s)
  before_tag=raw_state(path);s=begin();changes(s,{'models':[],'decks':[[],[]],'tags':['cedartag']});end(s)
  raw=raw_state(path);receiver=receive()
  before_tag_row=next(row for row in before_tag['tags'] if row['tag']=='CedarTag')
  after_tag_row=next(row for row in raw['tags'] if row['tag']=='CedarTag')
  results[variant]={'tag_spelling_state_preserved':before_tag_row==after_tag_row and not any(row['tag']=='cedartag' for row in raw['tags']),'deck_repeated_plus':by_id(raw['decks'],810003)['name']=='alpha++','model_repeated_plus':by_id(raw['notetypes'],820003)['name']=='alpha++'}
  details[variant]={'tag_before':before_tag_row,'tag_after':after_tag_row,'raw_served':raw,'receiver':receiver,'traffic':traffic,'sdk_traffic':rec.observations};inputs[variant]=traffic
 details[variant]['server_closed_state']=closed_state(path)
 for variant in ('config-optional','config-replacement-outgoing'):
  api.PHASE='matrix-'+variant;path=root/variant
  with environment(api,path) as (a,b,model,rec,traffic,meta,begin,changes,end,receive,call):
   def cb(conf):return {'models':[],'decks':[[],[]],'tags':[],**conf}
   seed={'cedar_old':{'payload':'old'},'cedar_keep':7};creation=a.db.scalar('select crt from col')
   s=begin(lnewer=True);changes(s,cb({'conf':seed,'crt':creation}));end(s);samples=[]
   if variant=='config-optional':
    for label,optional in [('omitted',{}),('null',{'conf':None,'crt':None})]:
     before=raw_state(path);s=begin(lnewer=True);response=changes(s,cb(optional));end(s);after=raw_state(path)
     samples.append({'form':label,'response':response,'before':before,'after':after})
    results[variant]={sample['form']+'_preserved':sample['before']['config']==sample['after']['config'] and sample['before']['col'][0]['crt']==sample['after']['col'][0]['crt'] for sample in samples}
   else:
    replacement={'cedar_keep':9,'cedar_new':['cedar',4]};s=begin(lnewer=True);changes(s,cb({'conf':replacement,'crt':creation+1}));end(s)
    after=raw_state(path);s=begin(lnewer=False);response=changes(s);end(s);keys={row.get('key',row.get('KEY')) for row in after['config']}
    results[variant]={'old_key_removed':'cedar_old' not in keys,'replacement_received':keys==set(replacement),'outgoing_config':same_value(response.get('conf'),replacement),'outgoing_crt':response.get('crt')==creation+1}
    samples=[{'raw_served':after,'outgoing':response}]
   receiver=receive();details[variant]={'samples':samples,'receiver':receiver,'traffic':traffic,'sdk_traffic':rec.observations};inputs[variant]=traffic
  details[variant]['server_closed_state']=closed_state(path)
 variant='grave-then-occurrence';api.PHASE='matrix-'+variant;path=root/variant
 with environment(api,path) as (a,b,model,rec,traffic,meta,begin,changes,end,receive,call):
  card=a.db.all('select * from cards order by id')[0]
  note=a.db.all('select id,guid,mid,mod,usn,tags,flds from notes order by id')[1]+['','',0,'']
  deck=copy.deepcopy(a.decks.get(1));deck.update(id=830001,name='Cedar grave deck',mod=2000000000,usn=meta()['usn'])
  card[2]=deck['id']
  s=begin();changes(s,{'models':[],'decks':[[deck],[]],'tags':[]});end(s,{'cards':[card],'done':True})
  current=meta()['usn'];s=begin(current)
  call('applyGraves',{'chunk':{'cards':[],'notes':[],'decks':[deck['id']]}},s);changes(s);end(s)
  deleted=raw_state(path)
  # Restore this valid non-default deck and card before the same-round experiment.
  deck['mod']+=1;deck['usn']=meta()['usn'];card[4]=2000000000;card[5]=deck['usn']
  s=begin();changes(s,{'models':[],'decks':[[deck],[]],'tags':[]});end(s,{'cards':[card],'done':True})
  current=meta()['usn'];s=begin(current);grave={'cards':[card[0]],'notes':[note[0]],'decks':[deck['id']]}
  call('applyGraves',{'chunk':grave},s)
  deck['usn']=current;deck['mod']+=1;changes(s,{'models':[],'decks':[[deck],[]],'tags':[]})
  card[4]=2000000001;card[5]=current;note[3]=2000000001;note[4]=current
  end(s,{'cards':[card],'notes':[note],'done':True});raw=raw_state(path);receiver=receive();graves={(row['oid'],row['type']) for row in raw['graves']}
  results[variant]={'card_present':any(row['id']==card[0] for row in raw['cards']),'note_present':any(row['id']==note[0] for row in raw['notes']),'deck_present':any(row['id']==deck['id'] for row in raw['decks']),'graves_retained':{(card[0],0),(note[0],1),(deck['id'],2)}<=graves}
  details[variant]={'raw_after_deck_grave_only':deleted,'raw_served':raw,'receiver':receiver,'traffic':traffic,'sdk_traffic':rec.observations};inputs[variant]=traffic


 details[variant]['server_closed_state']=closed_state(path)
