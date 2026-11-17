# SPDX-License-Identifier: AGPL-3.0-or-later
"""Assemble nine fixed successful segments without rerunning any client."""
import copy
import pathlib
from evidence import candidate_selection
from final_contract import SEGMENTS
from validation import Artifacts, SOURCE_FIELDS, binding, decode, encoded, require, same_value, validate


def assemble(root,out,invocation,runtime):
    contract=candidate_selection('final');store=Artifacts(out/'artifacts');cases=[];peer_identity=None;owned={}
    require(set(path.name for path in root.iterdir()) == {'final-'+segment for segment in SEGMENTS},'missing or unexpected final segment directory')
    schema=decode(pathlib.Path('/opt/reference/schema.json').read_bytes())
    for segment in SEGMENTS:
        phase='final-'+segment;source=root/phase
        require(source.is_dir() and not source.is_symlink(),'linked final segment directory')
        require(all((source/name).is_file() and not (source/name).is_symlink() and (source/name).stat().st_nlink == 1 for name in ('selection.json','invocation.json','evidence.json','summary.json')),'missing or linked final segment input')
        selected=decode((source/'selection.json').read_bytes());execution=decode((source/'invocation.json').read_bytes())
        manifest=decode((source/'evidence.json').read_bytes());old_store=Artifacts(source/'artifacts')
        require(same_value(selected,candidate_selection(phase)),'final segment selection changed')
        require(execution.get('child_phase') == phase and all(same_value(execution.get(key),invocation.get(key)) for key in SOURCE_FIELDS+('run_id','author_session')),'final segment outer identity mismatch')
        validate(manifest,selected,schema,old_store,execution,runtime)
        require(all(case['status'] == 'matched' for case in manifest['cases']),'final segment is not successful new execution')
        first=manifest['cases'][0]
        first_package=old_store.json(next(row['sha256'] for row in first['artifacts'] if row['role'] == 'package'))
        distinct_owned_segment(owned,execution,first_package,old_store)
        for artifact in old_store.root.iterdir():store.put(old_store.read(artifact.name))
        for case in manifest['cases']:
            package=store.json(next(row['sha256'] for row in case['artifacts'] if row['role'] == 'package'))
            require(package.get('kind') == 'final_package','intermediate result cannot enter final selection')
            identity={key:value for key,value in package.items() if key != 'execution_binding_sha256'}
            if peer_identity is None:peer_identity=identity
            require(same_value(identity,peer_identity),'final segments use different package/source peers')
            cases.append(copy.deepcopy(case))
    require([case['id'] for case in cases] == [case['id'] for case in contract['cases']] and len(cases) == 31,'final exact31 union mismatch')
    invocation={**invocation,'selection':[case['id'] for case in cases]}
    manifest={key:copy.deepcopy(invocation[key]) for key in SOURCE_FIELDS}
    manifest.update(format='meguri-sync-server-v1',baseline_release='26.09.3',document_kind='evidence',
        executable_sha256=peer_identity['server']['executable_sha256'],expected_case_count=31,selected_case_count=31,completed_case_count=31,cases=cases)
    for case in cases:
        for artifact in case['artifacts']:
            if artifact['role'] == 'mg_receipt':
                receipt=store.json(artifact['sha256'])
                require(same_value(receipt['binding'],binding(manifest,case)),'aggregate changed case binding')
                receipt.update(selection=invocation['selection'],completed_ids=invocation['selection'])
                artifact['sha256']=store.put_json(receipt);artifact['bytes']=len(store.read(artifact['sha256']))
    summary=validate(manifest,contract,schema,store,invocation,runtime)
    for name,value in [('selection.json',contract),('invocation.json',invocation),('evidence.json',manifest)]:
        (out/name).write_bytes(encoded(value))
    (out/'summary.json').write_bytes(encoded({'total':31,'passed':31,'failed':0,'expected_case_count':31,'selected_case_count':31,
        'completed_case_count':31,'selection_credit':summary['credit'],'broad_catalog_credit':False}))
    return summary['credit']


def distinct_owned_segment(seen,invocation,package,store):
    """The reviewed MG factory owns fresh init; reject resource reuse here."""
    running=store.json(package['execution_binding_sha256'])
    before=store.json(running['container_inspection_before_sha256'])[0]
    identity={'container_id':running['container_id'],'owner_nonce':invocation['owner_nonce'],
              'data_volume':invocation['data_volume']}
    require(before.get('Created') and before.get('Id') == identity['container_id'],'segment creation identity missing')
    require(before.get('Config',{}).get('Labels',{}).get('meguri.sync.owner') == identity['owner_nonce'],'segment creation nonce differs')
    for field,value in identity.items():
        require(value not in seen.setdefault(field,set()),'final segments reused owned '+field)
    for field,value in identity.items():seen[field].add(value)
    return {**identity,'created':before['Created']}
