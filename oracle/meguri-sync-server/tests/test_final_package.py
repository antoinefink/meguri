# SPDX-License-Identifier: AGPL-3.0-or-later
"""Synthetic artifact graphs; these tests grant no client or runtime credit."""
import copy
import io
import pathlib
import tarfile
import tempfile
import unittest

from candidate import ENDPOINT, retained_dependencies, verify_package
from validation import Artifacts


class FinalPackage(unittest.TestCase):
    def setUp(self):
        temp=tempfile.TemporaryDirectory();self.addCleanup(temp.cleanup)
        self.store=Artifacts(pathlib.Path(temp.name)/'artifacts')
        self.exe=self.store.put(b'validator executable');self.lock=self.store.put(b'validator lock')
        archive=io.BytesIO()
        with tarfile.open(fileobj=archive,mode='w') as target:
            row=tarfile.TarInfo('server/Cargo.lock');row.size=len(b'validator lock')
            target.addfile(row,io.BytesIO(b'validator lock'))
        source=self.store.put(archive.getvalue())
        covered=self.store.put(b'validator covered source');notices=self.store.put(b'validator notices')
        asset=self.store.put(b'validator asset')
        assets=self.store.put_json([{'path':'/index.html','sha256':asset,'bytes':len(b'validator asset')}])
        inputs=self.store.put_json({'source_commit':'0'*40,'profile':'release','features':[],
            'source_archive_sha256':source,'covered_source_sha256':covered,'rust_notices_sha256':notices,
            'inputs':[{'path':'server/Cargo.lock','sha256':self.lock,'bytes':len(b'validator lock')}]})
        self.inventory={name:{'sha256':digest,'bytes':len(self.store.read(digest))} for name,digest in [
            ('usr/local/bin/meguri-sync-server',self.exe),('usr/share/meguri-sync/package-inputs.json',inputs),
            ('usr/share/meguri-sync/assets.json',assets),('usr/share/meguri-sync/covered-source.tar',covered),
            ('usr/share/meguri-sync/rust-notices.json',notices)]}
        self.server={'kind':'meguri_sync','version':'0.1.0','build':'validator','source_commit':'0'*40,
            'source_sha256':source,'lock_sha256':self.lock,'executable_sha256':self.exe}
        self.image={'Id':'sha256:'+'3'*64,'Config':{'User':'10001:10001','Entrypoint':['/usr/local/bin/meguri-sync-server'],
            'Labels':{'org.opencontainers.image.revision':'0'*40}}}
        self.package={'image_id':self.image['Id'],'inputs_sha256':inputs,'runtime_inventory_sha256':self.store.put_json(self.inventory),
            'image_inspection_sha256':self.store.put_json([self.image]),'assets_sha256':assets}
        self.invocation={**self.server,'run_id':'validator-execution','root':'/canonical-checkout','owner_nonce':'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            'server_arguments':['serve','/data/owner','https://candidate.invalid','0.0.0.0:8080','--trusted-peer','127.0.0.1'],
            'data_volume':'validator-owned-volume'}
        result_dir='/canonical-checkout/build/mg/sync-server-package/validator-build'
        self.built={'job':'sync-server-package','run_id':'validator-build','args':[],'root':'/canonical-checkout',
            'status':'pass','exit_code':0,'dry_run':False,'commit':{'sha':'0'*40,'dirty':False},
            'artifacts':[{'kind':kind,'path':result_dir+suffix} for kind,suffix in [('package-inputs','/context/package-inputs.json'),
                ('package-source','/source.tar'),('package-product','/product'),('package-image','/image.json')]],
            'details':{'package':{key:self.package[key] for key in ('image_id','inputs_sha256','runtime_inventory_sha256')}}}
        self.built['details']['package'].update(source_commit='0'*40,profile='release',features=[])
        args=self.invocation['server_arguments']
        self.inspect={'Id':'4'*64,'Image':self.image['Id'],'Created':'synthetic validator creation identity',
            'Config':{'User':'10001:10001','Entrypoint':['/usr/local/bin/meguri-sync-server'],'Cmd':args,
                'Labels':{'meguri.mg.run-id':'validator-execution','meguri.sync.owner':'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'}},
            'Path':'/usr/local/bin/meguri-sync-server','Args':args,
            'HostConfig':{'ReadonlyRootfs':True,'PortBindings':{},'Privileged':False},'State':{'Running':True,'StartedAt':'synthetic validator actual start identity'},
            'Mounts':[{'Destination':'/data','Type':'volume','RW':True,'Name':'validator-owned-volume'}]}
        self.running={'format':'meguri-sync-candidate-execution-v1','endpoint':ENDPOINT,'server':self.server,
            'run_id':'validator-execution','container_id':'4'*64,'labels':{'meguri.mg.run-id':'validator-execution'},
            'observed_executable_sha256':self.exe,'package':self.package,'server_process':{'pid':7,'executable_sha256':self.exe},
            'container_inspection_before_sha256':self.store.put_json([self.inspect]),'container_inspection_after_sha256':self.store.put_json([self.inspect])}
        self.peer={'format':'meguri-sync-package-v1','kind':'final_package','server':self.server,'source_dirty':False,
            'profile':'release','features':[],'executable_artifact_sha256':self.exe,'lock_artifact_sha256':self.lock,
            'mg_build_receipt_sha256':self.store.put_json(self.built),'build_invocation':{'job':'sync-server-package','run_id':'validator-build','arguments':[]},
            'execution_binding_sha256':self.store.put_json(self.running),'package':self.package}

    def refresh(self):
        self.peer['execution_binding_sha256']=self.store.put_json(self.running)
        self.peer['mg_build_receipt_sha256']=self.store.put_json(self.built)

    def check(self):
        self.refresh();return verify_package(self.peer,self.store,self.invocation)

    def test_complete_release_graph_and_non_pid1_server_are_accepted(self):
        self.assertEqual(self.check()['kind'],'final_package')
        closure=retained_dependencies(self.peer,self.store,self.invocation)
        self.assertIn(self.server['source_sha256'],closure)
        self.assertIn(self.package['assets_sha256'],closure)
        self.assertIn(self.running['container_inspection_after_sha256'],closure)
        target=Artifacts(self.store.root.parent/'export')
        for digest in closure:target.put(self.store.read(digest))
        self.assertEqual(verify_package(self.peer,target,self.invocation),self.peer)

    def test_intermediate_label_cannot_accept_final_fields(self):
        self.peer['kind']='intermediate_build'
        with self.assertRaises(ValueError):self.check()

    def test_wrong_producer_and_noncanonical_root_are_rejected(self):
        for key,value in [('job','sync-server-candidate-build'),('root','/another-checkout')]:
            with self.subTest(key=key):
                before=self.built[key];self.built[key]=value
                with self.assertRaises(ValueError):self.check()
                self.built[key]=before

    def test_actual_process_pid_and_executable_are_mandatory(self):
        for process in [{'pid':True,'executable_sha256':self.exe},{'pid':0,'executable_sha256':self.exe},
                        {'pid':1,'executable_sha256':'0'*64},{'pid':2147483648,'executable_sha256':self.exe}]:
            with self.subTest(process=process):
                self.running['server_process']=process
                with self.assertRaises(ValueError):self.check()

    def test_missing_transitive_asset_is_rejected(self):
        assets=self.store.json(self.package['assets_sha256'])
        (self.store.root/assets[0]['sha256']).unlink()
        with self.assertRaises(ValueError):self.check()

    def test_tampered_source_archive_bytes_are_rejected(self):
        (self.store.root/self.server['source_sha256']).write_bytes(b'changed source')
        with self.assertRaises(ValueError):self.check()

    def test_wrong_running_image_or_writeable_root_or_mount_is_rejected(self):
        for path,value in [('Image','sha256:'+'8'*64),('HostConfig',{'ReadonlyRootfs':False,'PortBindings':{}}),('Mounts',[])]:
            with self.subTest(path=path):
                changed=copy.deepcopy(self.inspect);changed[path]=value
                self.running['container_inspection_after_sha256']=self.store.put_json([changed])
                with self.assertRaises(ValueError):self.check()

    def test_before_only_preflight_never_grants_final_receipt_acceptance(self):
        self.peer['kind']='final_package_preflight'
        self.running.pop('container_inspection_after_sha256');self.refresh()
        self.assertEqual(verify_package(self.peer,self.store,self.invocation,preflight=True)['kind'],'final_package_preflight')
        with self.assertRaises(ValueError):verify_package(self.peer,self.store,self.invocation)
        self.running['container_inspection_after_sha256']=self.running['container_inspection_before_sha256'];self.refresh()
        with self.assertRaises(ValueError):verify_package(self.peer,self.store,self.invocation,preflight=True)

    def test_execute_record_keeps_original_preflight_and_raw_observations_immutable(self):
        import json
        from unittest.mock import patch
        from candidate import load_inputs
        from final_handshake import execute, record_inputs
        from validation import decode, encoded, sha
        original_peer=copy.deepcopy(self.peer)
        preflight=copy.deepcopy(self.peer);preflight['kind']='final_package_preflight'
        before=copy.deepcopy(self.running);before.pop('container_inspection_after_sha256')
        preflight['execution_binding_sha256']=self.store.put_json(before)
        executed={**self.invocation,'child_phase':'final-fresh-fresh','child_stage':'execute','selection':['synthetic-validator-only']}
        out=self.store.root.parent/'out';out.mkdir()
        result={'phase':'final-fresh-fresh','runtime':{'server':self.server},'synthetic_validator_only':True}
        preflight_bytes=(json.dumps(preflight,indent=2,separators=(', ', ': '))+'\n').encode()
        self.assertEqual(decode(preflight_bytes),preflight)
        self.assertNotEqual(preflight_bytes,encoded(preflight))
        (self.store.root.parent/'preflight.json').write_bytes(preflight_bytes)
        with patch('candidate.read_credentials',return_value={}):
            loaded,inputs,_,loaded_bytes=load_inputs(self.store.root.parent,'unused-synthetic-credential-path',executed,preflight=True)
        self.assertEqual(loaded_bytes,preflight_bytes)
        with self.assertRaisesRegex(ValueError,'preflight bytes differ'):
            execute(result,out,executed,loaded,inputs,encoded({**preflight,'profile':'debug'}))
        self.assertTrue(execute(result,out,executed,loaded,inputs,loaded_bytes))
        admitted=decode((out/'execute-admission.json').read_bytes())
        self.assertEqual(admitted['preflight_peer_sha256'],sha(preflight_bytes))
        captured=Artifacts(out/'artifacts')
        self.assertEqual(captured.read(admitted['preflight_peer_sha256']),preflight_bytes)
        admission=sha((out/'execute-admission.json').read_bytes())
        recording={**executed,'child_stage':'record','execute_admission_sha256':admission}
        retained,recorded_store=record_inputs(out,recording,original_peer,self.store)
        self.assertTrue(retained['synthetic_validator_only'])
        self.assertEqual(recorded_store.read(admitted['preflight_peer_sha256']),preflight_bytes)
        self.assertEqual((out/'candidate-observations.json').read_bytes(),captured.read(admitted['observations_sha256']))
        self.assertFalse((out/'evidence.json').exists())
        self.assertFalse((out/'summary.json').exists())
        (out/'candidate-observations.json').write_bytes(b'changed observation bytes')
        with self.assertRaises(ValueError):record_inputs(out,recording,original_peer,self.store)

    def test_final_assembly_rejects_reused_container_nonce_or_volume(self):
        from final_assemble import distinct_owned_segment
        for duplicate in ('container_id','owner_nonce','data_volume'):
            with self.subTest(duplicate=duplicate):
                seen={};distinct_owned_segment(seen,self.invocation,self.peer,self.store)
                other_invocation=copy.deepcopy(self.invocation);other_invocation.update(owner_nonce='b'*32,data_volume='second-owned-volume')
                other_running=copy.deepcopy(self.running);other_running['container_id']='6'*64
                if duplicate == 'container_id':other_running['container_id']=self.running['container_id']
                if duplicate == 'owner_nonce':other_invocation['owner_nonce']=self.invocation['owner_nonce']
                if duplicate == 'data_volume':other_invocation['data_volume']=self.invocation['data_volume']
                inspection=copy.deepcopy(self.inspect);inspection['Id']=other_running['container_id'];inspection['Created']='second actual synthetic creation'
                inspection['Config']['Labels']['meguri.sync.owner']=other_invocation['owner_nonce']
                other_running['container_inspection_before_sha256']=self.store.put_json([inspection])
                other_peer=copy.deepcopy(self.peer);other_peer['execution_binding_sha256']=self.store.put_json(other_running)
                with self.assertRaisesRegex(ValueError,'reused owned'):
                    distinct_owned_segment(seen,other_invocation,other_peer,self.store)
