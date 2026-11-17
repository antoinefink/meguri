# SPDX-License-Identifier: AGPL-3.0-or-later
"""A finite execute/record boundary; recording never starts a client."""
import pathlib
from candidate import retained_dependencies, verify_package
from validation import Artifacts, SOURCE_FIELDS, decode, encoded, require, same_value


def execute(result,out,invocation,preflight,input_store,preflight_bytes):
    require(isinstance(preflight_bytes,bytes) and same_value(decode(preflight_bytes),preflight),'final preflight bytes differ from parsed input')
    require(preflight.get('kind') == 'final_package_preflight','non-credit final preflight required')
    require(not (out/'candidate-observations.json').exists() and not (out/'execute-admission.json').exists() and not (out/'evidence.json').exists(),'final execute output already populated')
    store=Artifacts(out/'artifacts')
    for digest in retained_dependencies(preflight,input_store,invocation,preflight=True):store.put(input_store.read(digest))
    require(result.get('phase') == invocation.get('child_phase') and same_value(result.get('runtime',{}).get('server'),preflight['server']),'executed child phase/peer mismatch')
    from final_contract import NEUTRAL, RECIPE_SHA256
    require(store.put((NEUTRAL/'final-fixture-recipes.json').read_bytes()) == RECIPE_SHA256,'preparation recipe changed')
    rules={name:__import__('hashlib').sha256(pathlib.Path(__file__).with_name(name).read_bytes()).hexdigest()
           for name in ('adapter.py','normal.py','media.py','final_desktop.py','final_checks.py','final_contract.py')}
    result['preparation_code_sha256']=rules
    observations=store.put_json(result);preflight_hash=store.put(preflight_bytes);invocation_hash=store.put_json(invocation)
    admission={'format':'meguri-sync-final-execute-v1','child_phase':invocation['child_phase'],'invocation_sha256':invocation_hash,
               'preflight_peer_sha256':preflight_hash,'observations_sha256':observations}
    (out/'candidate-observations.json').write_bytes(store.read(observations))
    (out/'execute-admission.json').write_bytes(encoded(admission))
    store.put_json(admission)
    # This is intentionally no compatibility summary, evidence or credit.
    return True


def record_inputs(out,invocation,package,input_store):
    require(package.get('kind') == 'final_package','complete final package required after execution')
    store=Artifacts(out/'artifacts');admission=store.json(invocation['execute_admission_sha256'])
    require(set(admission) == {'format','child_phase','invocation_sha256','preflight_peer_sha256','observations_sha256'},'invalid final admission shape')
    require(admission.get('format') == 'meguri-sync-final-execute-v1' and admission.get('child_phase') == invocation.get('child_phase'),'wrong captured execute admission')
    executed=store.json(admission['invocation_sha256'])
    require(executed.get('child_stage') == 'execute' and invocation.get('child_stage') == 'record','execute/record stage mismatch')
    expected={key:value for key,value in invocation.items() if key not in {'execute_admission_sha256','child_stage'}}
    require(same_value({key:value for key,value in executed.items() if key != 'child_stage'},expected),'execute/record invocation changed')
    preflight=store.json(admission['preflight_peer_sha256'])
    verify_package(preflight,store,executed,preflight=True)
    require(same_value({key:value for key,value in preflight.items() if key not in {'kind','execution_binding_sha256'}},
                       {key:value for key,value in package.items() if key not in {'kind','execution_binding_sha256'}}),'final package differs from admitted execution')
    before=store.json(preflight['execution_binding_sha256']);after=input_store.json(package['execution_binding_sha256'])
    require(same_value(before,{key:value for key,value in after.items() if key != 'container_inspection_after_sha256'}),'final execution identity changed after client run')
    verify_package(package,input_store,invocation)
    result=store.json(admission['observations_sha256'])
    require(result.get('phase') == invocation['child_phase'] and same_value(result['runtime']['server'],package['server']),'captured result identity mismatch')
    raw=out/'candidate-observations.json'
    require(raw.is_file() and not raw.is_symlink() and raw.stat().st_nlink == 1,'linked retained raw observations')
    require(raw.read_bytes() == store.read(admission['observations_sha256']),'retained raw observations changed')
    return result,store


def verify_admission(invocation,package,store,result):
    admission=store.json(invocation['execute_admission_sha256'])
    require(set(admission) == {'format','child_phase','invocation_sha256','preflight_peer_sha256','observations_sha256'},'invalid final admission shape')
    require(admission.get('format') == 'meguri-sync-final-execute-v1' and admission.get('child_phase') == invocation.get('child_phase'),'final admission phase mismatch')
    executed=store.json(admission['invocation_sha256'])
    require(executed.get('child_stage') == 'execute' and invocation.get('child_stage') == 'record','captured stage mismatch')
    require(same_value({key:value for key,value in executed.items() if key != 'child_stage'},
                       {key:value for key,value in invocation.items() if key not in {'execute_admission_sha256','child_stage'}}),'captured invocation differs')
    preflight=store.json(admission['preflight_peer_sha256']);verify_package(preflight,store,executed,preflight=True)
    require(same_value({key:value for key,value in preflight.items() if key not in {'kind','execution_binding_sha256'}},
                       {key:value for key,value in package.items() if key not in {'kind','execution_binding_sha256'}}),'captured package differs')
    before=store.json(preflight['execution_binding_sha256']);after=store.json(package['execution_binding_sha256'])
    require(same_value(before,{key:value for key,value in after.items() if key != 'container_inspection_after_sha256'}),'captured running identity differs')
    require(same_value(store.json(admission['observations_sha256']),result),'final retained observations differ from admitted execution')
    return admission
