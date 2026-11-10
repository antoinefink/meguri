#!/usr/bin/env python3
"""Generated native-tool doubles only; no real signing or cloud evidence."""
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
import plistlib
import signal
import struct
from pathlib import Path
import ssl
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('initial_signing_test_subject', ROOT/'tools/scripts/cloud-agent-initial-signing.py')
m = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = m
spec.loader.exec_module(m)
TEAM = 'ABCD123456'
CERT = b'generated-certificate-fixture'
SHA = hashlib.sha1(CERT).hexdigest().upper()


def code_image_fixture(signature=b's'*16,payload=b'CODEDATA',cpu=0x100000c,subtype=0):
    header=struct.pack('<8I',0xfeedfacf,cpu,subtype,6,2,88,0,0)
    size=len(payload)+len(signature)
    segment=struct.pack('<II16sQQQQIIII',0x19,72,b'__LINKEDIT',4096,(size+4095)//4096*4096,120,size,7,1,0,0)
    command=struct.pack('<4I',0x1d,16,120+len(payload),len(signature))
    return header+segment+command+payload+signature


def generated_fat(first,second):
    start=48;first_offset=(start+15)//16*16;second_offset=(first_offset+len(first)+15)//16*16
    result=struct.pack('>II',0xcafebabe,2)
    result+=struct.pack('>5I',0x100000c,0,first_offset,len(first),4)
    result+=struct.pack('>5I',0x1000007,3,second_offset,len(second),4)
    return result+b'\0'*(first_offset-len(result))+first+b'\0'*(second_offset-first_offset-len(first))+second


class CodeFingerprintTests(unittest.TestCase):
    def test_signature_growth_and_native_replacement_preserve_only_original_code(self):
        original=code_image_fixture();allocated=code_image_fixture(signature=b'c'*128)
        self.assertEqual(m.code_fingerprint(original),m.code_fingerprint(allocated))
        self.assertNotEqual(m.code_fingerprint(original),m.code_fingerprint(code_image_fixture(signature=b'c'*128,payload=b'BAD_CODE')))
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary).resolve();code=root/'code';code.write_bytes(original)
            with m.v.authority(str(root)) as parent:
                before=m.code_fingerprint(m.code_bytes(parent,'code'))
                replacement=root/'replacement';replacement.write_bytes(allocated);replacement.replace(code)
                self.assertEqual(m.code_fingerprint(m.code_bytes(parent,'code')),before)
    def test_fat_allocation_offset_growth_keeps_both_native_slices(self):
        old=generated_fat(code_image_fixture(),code_image_fixture(cpu=0x1000007,subtype=3))
        new=generated_fat(code_image_fixture(signature=b'x'*128),code_image_fixture(signature=b'y'*128,cpu=0x1000007,subtype=3))
        self.assertEqual(m.code_fingerprint(old),m.code_fingerprint(new))
        changed=generated_fat(code_image_fixture(),code_image_fixture(payload=b'BAD_CODE',cpu=0x1000007,subtype=3))
        self.assertNotEqual(m.code_fingerprint(old),m.code_fingerprint(changed))
    def test_linkedit_virtual_size_only_change_never_normalizes_away(self):
        original=code_image_fixture();changed=bytearray(original)
        struct.pack_into('<Q',changed,64,8192)
        self.assertNotEqual(m.code_fingerprint(original),m.code_fingerprint(bytes(changed)))
        # Larger signatures that change virtual allocation remain refused until
        # actual native behavior proves a narrower permitted relationship.
        self.assertNotEqual(m.code_fingerprint(original),m.code_fingerprint(code_image_fixture(signature=b'x'*5000)))

    def test_unknown_truncated_overlapping_and_missing_signature_layout_refuse(self):
        bad=[b'unknown',code_image_fixture()[:-1]]
        malformed=bytearray(code_image_fixture());struct.pack_into('<I',malformed,104,0);bad.append(bytes(malformed))
        overlap=bytearray(generated_fat(code_image_fixture(),code_image_fixture(cpu=0x1000007,subtype=3)))
        struct.pack_into('>I',overlap,36,64);bad.append(bytes(overlap))
        for raw in bad:
            with self.assertRaises(m.v.Refused):m.code_fingerprint(raw)


class CodeDifferenceDiagnosticTests(unittest.TestCase):
    def test_exact_vmsize_and_payload_deltas_are_classified_without_bytes(self):
        before=code_image_fixture();after=bytearray(code_image_fixture(payload=b'BAD_CODE'))
        struct.pack_into('<Q',after,64,8192)
        result=m.code_difference(before,bytes(after));item=result['slices'][0]
        self.assertTrue(item['changed_fields']['linkedit_vmsize'])
        self.assertEqual(item['before']['fields']['linkedit_vmsize'],4096)
        self.assertEqual(item['after']['fields']['linkedit_vmsize'],8192)
        self.assertGreater(item['other_changed_byte_count'],0)
        self.assertEqual(result['before_sha256'],hashlib.sha256(before).hexdigest())
        self.assertNotIn('BAD_CODE',json.dumps(result))
        self.assertNotEqual(m.code_fingerprint(before),m.code_fingerprint(bytes(after)))

    def test_private_diagnostic_is_exclusive_and_retains_first_evidence(self):
        before=code_image_fixture();after=code_image_fixture(signature=b'x'*5000)
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary).resolve();target='Debug-iphonesimulator/Meguri.app/Meguri.debug.dylib'
            m.write_code_difference(str(root),target,before,after)
            path=root/'initial-code-difference.json';raw=path.read_bytes()
            self.assertEqual(path.stat().st_mode&0o777,0o600)
            self.assertEqual(json.loads(raw)['target'],target)
            m.write_code_difference(str(root),target,after,before)
            self.assertEqual(path.read_bytes(),raw)
            self.assertNotEqual(m.code_fingerprint(before),m.code_fingerprint(after))

    def test_fat_diagnostic_reports_both_exact_transaction_slices(self):
        before=generated_fat(code_image_fixture(),code_image_fixture(cpu=0x1000007,subtype=3))
        after=generated_fat(code_image_fixture(signature=b'x'*5000),code_image_fixture(cpu=0x1000007,subtype=3))
        result=m.code_difference(before,after)
        self.assertEqual(result['slice_counts'],[2,2])
        self.assertTrue(result['slices'][0]['changed_fields']['linkedit_vmsize'])
        self.assertFalse(result['slices'][1]['changed_fields']['linkedit_vmsize'])


def measured_allocation_fixture(cpu=0x1000007,subtype=3,signature_size=480):
    # Generated Mach-O containing the exact measured allocation dimensions.
    start,fileoff = (20992,16384) if cpu==0x1000007 else (56128,49152)
    filesize=start+signature_size-fileoff
    header=struct.pack('<8I',0xfeedfacf,cpu,subtype,6,2,88,0,0)
    segment=struct.pack('<II16sQQQQIIII',0x19,72,b'__LINKEDIT',fileoff,
                        (filesize+16383)//16384*16384,fileoff,filesize,7,1,0,0)
    command=struct.pack('<4I',0x1d,16,start,signature_size)
    return header+segment+command+b'\x55'*(start-120)+b's'*signature_size


def measured_fat_fixture(first,second,align=14):
    first_offset=(48+(1<<align)-1)//(1<<align)*(1<<align)
    second_offset=(first_offset+len(first)+(1<<align)-1)//(1<<align)*(1<<align)
    header=struct.pack('>II',0xcafebabe,2)
    header+=struct.pack('>5I',0x1000007,3,first_offset,len(first),align)
    header+=struct.pack('>5I',0x100000c,0,second_offset,len(second),align)
    return header+b'\0'*(first_offset-len(header))+first+b'\0'*(second_offset-first_offset-len(first))+second


class SignatureAllocationTests(unittest.TestCase):
    def cases(self):
        return [(measured_allocation_fixture(),measured_allocation_fixture(signature_size=18704)),
                (measured_allocation_fixture(cpu=0x100000c,subtype=0,signature_size=416),
                 measured_allocation_fixture(cpu=0x100000c,subtype=0,signature_size=18640))]

    def rejects(self,before,after):
        try:result=m.signature_allocation_matches(before,after)
        except m.v.Refused:return
        self.assertFalse(result)

    def test_both_measured_thin_slices_and_fat_reallocation(self):
        x86,arm=self.cases()
        for before,after in (x86,arm):
            self.assertNotEqual(m.code_fingerprint(before),m.code_fingerprint(after))
            self.assertTrue(m.signature_allocation_matches(before,after))
        before=measured_fat_fixture(x86[0],arm[0]);after=measured_fat_fixture(x86[1],arm[1])
        self.assertEqual(struct.unpack_from('>I',before,36)[0],49152)
        self.assertEqual(struct.unpack_from('>I',after,36)[0],65536)
        self.assertTrue(m.signature_allocation_matches(before,after))

    def test_existing_strict_fast_path_stays_exact(self):
        self.assertTrue(m.signature_allocation_matches(code_image_fixture(),code_image_fixture(signature=b'x'*128)))
        self.rejects(code_image_fixture(),code_image_fixture(signature=b'x'*5000))

    def test_arbitrary_old_new_virtual_sizes_no_growth_or_shrink_refuse(self):
        before,after=self.cases()[0]
        for old_vm,new_vm in ((32768,49152),(16384,49152),(8192,32768)):
            old,new=bytearray(before),bytearray(after)
            struct.pack_into('<Q',old,64,old_vm);struct.pack_into('<Q',new,64,new_vm)
            self.rejects(bytes(old),bytes(new))
        virtual_only=bytearray(before);struct.pack_into('<Q',virtual_only,64,32768)
        self.rejects(before,bytes(virtual_only));self.rejects(after,before)

    def test_signature_start_arch_segment_layout_and_code_mutations_refuse(self):
        before,after=self.cases()[0]
        mutations=[(112,'<I',20993),(4,'<I',0x100000c),(8,'<I',0),
                   (56,'<Q',16385),(72,'<Q',16385),(80,'<Q',23313),
                   (116,'<I',18703),(16,'<I',3),(24,'<I',1)]
        for position,fmt,value in mutations:
            changed=bytearray(after);struct.pack_into(fmt,changed,position,value)
            with self.subTest(position=position):self.rejects(before,bytes(changed))
        shifted=bytearray(after);struct.pack_into('<I',shifted,112,20993);struct.pack_into('<I',shifted,116,18703)
        m.code_fingerprint(bytes(shifted))
        self.rejects(before,bytes(shifted))
        changed=bytearray(after);changed[200]^=1;self.rejects(before,bytes(changed))
        self.rejects(before,after+b'\0')

    def test_one_valid_fat_slice_cannot_mask_bad_peer_or_alignment(self):
        x86,arm=self.cases();before=measured_fat_fixture(x86[0],arm[0])
        peer=bytearray(arm[1]);struct.pack_into('<Q',peer,64,49152)
        self.rejects(before,measured_fat_fixture(x86[1],bytes(peer)))
        peer=bytearray(arm[1]);peer[200]^=1
        self.rejects(before,measured_fat_fixture(x86[1],bytes(peer)))
        self.rejects(before,measured_fat_fixture(x86[1],arm[1],align=15))
        overlap=bytearray(measured_fat_fixture(x86[1],arm[1]));struct.pack_into('>I',overlap,36,16384)
        self.rejects(before,bytes(overlap))


class AppIdentityEntitlementsTests(unittest.TestCase):
    BUNDLE='com.antoinefink.meguri.dev'
    def grants(self):
        return {'get-task-allow':True,'com.apple.developer.icloud-container-identifiers':['iCloud.generated.dev'],
                'com.apple.developer.icloud-services':['CloudKit'],'aps-environment':'development',
                'keychain-access-groups':['generated.group'],'data':b'fixture'}
    def test_missing_one_or_both_fields_and_correct_alias_preserve_every_original_grant(self):
        for present in ((),('application-identifier',),('com.apple.developer.team-identifier',),
                        ('application-identifier','com.apple.developer.team-identifier'),
                        ('com.apple.application-identifier',)):
            original=self.grants()
            for key in present:original[key]=TEAM if key=='com.apple.developer.team-identifier' else TEAM+'.'+self.BUNDLE
            before=m.typed_entitlements(original);expected=m.app_identity_entitlements(original,TEAM,self.BUNDLE)
            self.assertEqual(m.typed_entitlements(original),before)
            self.assertEqual(expected['application-identifier'],TEAM+'.'+self.BUNDLE)
            self.assertEqual(expected['com.apple.developer.team-identifier'],TEAM)
            self.assertEqual(set(expected)-set(original),{'application-identifier','com.apple.developer.team-identifier'}-set(original))
            for key in original:self.assertEqual(m.typed_entitlements({key:expected[key]}),m.typed_entitlements({key:original[key]}))
    def test_wrong_alias_canonical_value_or_type_never_overwrites(self):
        for key in ('application-identifier','com.apple.application-identifier','com.apple.developer.team-identifier'):
            for value in (True,1,None,[],{},'', 'WRONG00000.wrong.app'):
                original=self.grants();original[key]=value
                with self.subTest(key=key,kind=type(value).__name__):
                    with self.assertRaises(m.v.Refused):m.app_identity_entitlements(original,TEAM,self.BUNDLE)
        for team,bundle in (('bad',self.BUNDLE),(TEAM,'wrong.app')):
            with self.assertRaises(m.v.Refused):m.app_identity_entitlements(self.grants(),team,bundle)
    def test_typed_final_comparison_detects_bool_integer_and_arbitrary_grant_changes(self):
        expected=m.app_identity_entitlements(self.grants(),TEAM,self.BUNDLE)
        for mutate in ('type','add','remove','value'):
            changed=copy.deepcopy(expected)
            if mutate=='type':changed['get-task-allow']=1
            elif mutate=='add':changed['extra']=True
            elif mutate=='remove':del changed['keychain-access-groups']
            else:changed['aps-environment']='production'
            self.assertNotEqual(m.typed_entitlements(changed),m.typed_entitlements(expected))
    def test_app_signing_private_input_is_pinned_and_nonapp_arguments_stay_unchanged(self):
        captured=[]
        def native(arguments):
            captured.append(arguments)
            if '--entitlements' in arguments:
                path=Path(arguments[arguments.index('--entitlements')+1]);self.assertEqual(path.stat().st_mode&0o777,0o600)
                self.assertEqual(plistlib.loads(path.read_bytes())['application-identifier'],TEAM+'.'+self.BUNDLE)
        with tempfile.TemporaryDirectory() as temporary,patch.object(m,'signing_command',side_effect=native):
            root=str(Path(temporary).resolve())
            expected=m.sign_initial_code(root,'app','fixture',(self.BUNDLE,self.grants()),(TEAM,self.BUNDLE))
            self.assertEqual(expected,m.typed_entitlements(m.app_identity_entitlements(self.grants(),TEAM,self.BUNDLE)))
            private=Path(captured[0][captured[0].index('--entitlements')+1]);self.assertFalse(private.exists())
            self.assertIsNone(m.sign_initial_code(root,'agent','fixture',('agent',None)))
            self.assertIn('--preserve-metadata=identifier,entitlements',captured[1]);self.assertNotIn('--entitlements',captured[1])
    def test_actual_metadata_identifier_mismatch_refuses_before_signing(self):
        with patch.object(m,'signing_command') as native:
            with self.assertRaises(m.v.Refused):m.sign_initial_code('unused','app','fixture',('wrong.app',self.grants()),(TEAM,self.BUNDLE))
            native.assert_not_called()
    def test_private_input_replacement_or_mutation_refuses_after_original_call(self):
        for replacement in (False,True):
            def native(arguments):
                path=Path(arguments[arguments.index('--entitlements')+1]);raw=path.read_bytes()
                if replacement:path.unlink()
                path.write_bytes(raw if replacement else b'changed')
            with tempfile.TemporaryDirectory() as temporary,patch.object(m,'signing_command',side_effect=native):
                with self.assertRaises(m.v.Refused):m.sign_initial_code(str(Path(temporary).resolve()),'app','fixture',(self.BUNDLE,self.grants()),(TEAM,self.BUNDLE))
    def test_cancel_keeps_input_alive_until_native_call_unwinds(self):
        captured=[]
        def native(arguments):
            path=Path(arguments[arguments.index('--entitlements')+1]);captured.append(path);self.assertTrue(path.is_file())
            raise m.stage.Cancelled()
        with tempfile.TemporaryDirectory() as temporary,patch.object(m,'signing_command',side_effect=native):
            with self.assertRaises(m.stage.Cancelled):m.sign_initial_code(str(Path(temporary).resolve()),'app','fixture',(self.BUNDLE,self.grants()),(TEAM,self.BUNDLE))
        self.assertFalse(captured[0].exists())


class SignerTests(unittest.TestCase):
    def fake(self, args, category):
        if args[1] == 'find-identity':
            out = ('  1) '+SHA+' "Apple Development: Generated (generated-person-id)"\n').encode()
        elif args[1] == 'find-certificate':
            out = ssl.DER_cert_to_PEM_cert(CERT).encode()
        elif '-subject' in args:
            out = ('subject=CN=Apple Development: Generated,OU='+TEAM+',O=Generated\n').encode()
        else:
            self.assertIn('-checkend', args)
            out = b''
        return subprocess.CompletedProcess(args, 0, out, b'')

    def test_existing_valid_private_key_identity_matches_actual_certificate_and_team(self):
        with patch.object(m.v, 'command', side_effect=self.fake):
            self.assertEqual(m.choose_identity(TEAM), (SHA, CERT))

    def test_multiple_or_missing_identities_refuse_without_certificate_selection(self):
        line = ('  1) '+SHA+' "Apple Development: Generated ('+TEAM+')"\n').encode()
        for output in (b'', line+line):
            with self.subTest(count=len(output)):
                with patch.object(m.v, 'command', return_value=subprocess.CompletedProcess([],0,output,b'')) as command:
                    with self.assertRaises(m.v.Refused): m.choose_identity(TEAM)
                    self.assertEqual(command.call_count,1)

    def test_wrong_certificate_team_refuses_even_if_identity_label_claims_expected_team(self):
        def wrong(args, category):
            if '-subject' in args:
                return subprocess.CompletedProcess(args,0,b'subject=CN=Apple Development: Generated,OU=OTHER12345\n',b'')
            return self.fake(args,category)
        with patch.object(m.v, 'command', side_effect=wrong):
            with self.assertRaises(m.v.Refused): m.choose_identity(TEAM)


class MutationTests(unittest.TestCase):
    def setUp(self):
        self.before=({'code':{'type':'file','mode':0o755,'sha256':'a','bytes':4},
                     'resource':{'type':'file','mode':0o600,'sha256':'b','bytes':4}},
                     {'code':(1,2,0o100755,1,4,5,6),'resource':(1,3,0o100600,1,4,5,6)})

    def changed(self):
        result=copy.deepcopy(self.before)
        result[0]['code']['sha256']='c'
        result[1]['code']=(1,2,0o100755,1,8,9,10)
        return result

    def test_only_owned_signature_and_same_inode_executable_mutation_is_permitted(self):
        final=self.changed()
        final[0]['_CodeSignature']={'type':'directory','mode':0o755}
        final[1]['_CodeSignature']=(1,7,0o40755)
        final[0]['_CodeSignature/CodeResources']={'type':'file','mode':0o644,'sha256':'d','bytes':1}
        final[1]['_CodeSignature/CodeResources']=(1,8,0o100644,1,1,2,3)
        m.compare_product_files(self.before,final,'code','_CodeSignature',final[1]['code'])

    def test_resource_mutation_new_file_and_signature_link_refuse(self):
        cases=[]
        resource=self.changed();resource[0]['resource']['sha256']='x';cases.append(resource)
        added=self.changed();added[0]['extra']={'type':'file'};cases.append(added)
        link=self.changed();link[0]['_CodeSignature']={'type':'symlink','target':'outside'};cases.append(link)
        for final in cases:
            with self.assertRaises(m.v.Refused):m.compare_product_files(self.before,final,'code','_CodeSignature',final[1]['code'])

    def test_native_allocate_replacement_needs_exact_fresh_no_follow_result(self):
        final=self.changed();final[1]['code']=(1,99,0o100755,1,8,9,10)
        m.compare_product_files(self.before,final,'code',None,final[1]['code'])
        with self.assertRaises(m.v.Refused):
            m.compare_product_files(self.before,final,'code',None,self.before[1]['code'])

    def test_existing_frozen_manifest_refuses_before_signer_or_products(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary).resolve();run=root/'build/mg/build-app/20261110T000000Z-123'
            run.mkdir(parents=True);(run/'agent-build.json').write_text('{}')
            with patch.object(m.v,'source',return_value='a'*40),patch.object(m,'choose_identity') as choose:
                with self.assertRaisesRegex(m.v.Refused,'initial-already-frozen'):
                    m.initial_sign(str(root),str(run),'a'*40,'unused','unused','unused','unused')
                choose.assert_not_called()


class NativeProductTests(unittest.TestCase):
    def test_native_widget_and_preview_are_exact_members_and_extra_code_refuses(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary).resolve();products=root/'build/products';products.mkdir(parents=True)
            app='Debug-iphonesimulator/Meguri.app'
            for relative,identifier,executable,kind in [
                (app,'com.antoinefink.meguri.dev','Meguri','APPL'),
                (app+'/PlugIns/MeguriSnapshotTests.xctest','generated.agent','MeguriSnapshotTests','BNDL'),
                (app+'/PlugIns/MeguriWidget.appex','com.antoinefink.meguri.dev.widget','MeguriWidget','XPC!')]:
                bundle=products/relative;bundle.mkdir(parents=True)
                (bundle/'Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier':identifier,'CFBundleExecutable':executable,
                    'CFBundlePackageType':kind,'CFBundleSupportedPlatforms':['iPhoneSimulator']}))
                (bundle/executable).write_bytes(code_image_fixture())
            for relative in ('Frameworks/libXCTestBundleInject.dylib','Meguri.debug.dylib','__preview.dylib'):
                p=products/app/relative;p.parent.mkdir(exist_ok=True);p.write_bytes(code_image_fixture())
            for name in ('MeguriWidget.debug.dylib','__preview.dylib'):
                (products/app/'PlugIns/MeguriWidget.appex'/name).write_bytes(code_image_fixture())
            a,_=m.v.bundle(str(products),app,'ios-sim')
            agent,_=m.v.bundle(str(products),app+'/PlugIns/MeguriSnapshotTests.xctest','ios-sim')
            entries=m.v.snapshot(str(root/'build'),str(products))[0]
            paths,bundles=m.native_plan(str(products),a,agent,entries)
            self.assertEqual(len(paths),8)
            self.assertLess(paths.index(str(products/app/'PlugIns/MeguriWidget.appex')),paths.index(str(products/app)))
            self.assertIn(str(products/app/'__preview.dylib'),paths)
            self.assertLess(paths.index(str(products/app/'PlugIns/MeguriWidget.appex/MeguriWidget.debug.dylib')),
                            paths.index(str(products/app/'PlugIns/MeguriWidget.appex')))
            self.assertIn(str(products/app/'PlugIns/MeguriWidget.appex/__preview.dylib'),paths)
            (products/app/'unexpected.dylib').write_bytes(b'generated unknown code')
            with self.assertRaises(m.v.Refused):
                m.native_plan(str(products),a,agent,m.v.snapshot(str(root/'build'),str(products))[0])


class CancellationTests(unittest.TestCase):
    def test_cancellation_during_launch_joins_original_before_authority_closes(self):
        events=[]
        class Child:
            pid=987654321
            returncode=None
            stdin=None
            def wait(self,timeout=None):
                events.append('original-joined');self.returncode=-15
        child=Child()
        def launch(*args,**kwargs):
            self.assertTrue(kwargs['start_new_session'])
            self.assertNotIn('CODESIGN_ALLOCATE',kwargs['env'])
            self.assertNotIn('DEVELOPER_DIR',kwargs['env'])
            events.append('launch')
            signal.raise_signal(signal.SIGTERM)
            return child
        @contextlib.contextmanager
        def authority():
            events.append('authority-held')
            try:yield
            finally:events.append('authority-closed')
        with patch.object(m.stage.subprocess,'Popen',side_effect=launch), \
                patch.object(m.stage.os,'killpg',side_effect=lambda *args:events.append('original-signal')), \
                patch.object(m.stage.time,'sleep'):
            with self.assertRaises(m.stage.Cancelled):
                with authority():m.signing_command(['/generated/codesign','--sign','generated'])
        self.assertEqual(events[0],'authority-held')
        self.assertEqual(events[-2:],['original-joined','authority-closed'])
        self.assertEqual(events.count('launch'),1)


    def test_timeout_joins_original_before_authority_closes_without_second_launch(self):
        events=[]
        class Child:
            pid=987654321
            returncode=None
            stdin=None
            def communicate(self,data,timeout):
                events.append('original-timeout')
                raise subprocess.TimeoutExpired(['generated-native'],timeout)
            def wait(self,timeout=None):
                events.append('original-joined');self.returncode=-15
        child=Child()
        @contextlib.contextmanager
        def authority():
            events.append('authority-held')
            try:yield
            finally:events.append('authority-closed')
        # Advance the generated clock across the unchanged sixty-second bound.
        with patch.object(m.stage.subprocess,'Popen',return_value=child) as launch, \
                patch.object(m.stage.os,'killpg',side_effect=lambda *args:events.append('original-signal')), \
                patch.object(m.stage.time,'sleep'),patch.object(m.stage.time,'monotonic',side_effect=[0,0,61]):
            with self.assertRaises(m.stage.Refused):
                with authority():m.signing_command(['/generated/codesign','--sign','generated'])
        self.assertEqual(launch.call_count,1)
        self.assertEqual(events[-2:],['original-joined','authority-closed'])


class MetadataTests(unittest.TestCase):
    def test_absent_entitlements_and_present_empty_dictionary_are_distinct(self):
        for raw,expected in ((b'',None),(b'<?xml version="1.0"?><plist version="1.0"><dict/></plist>',{})):
            def fake(args,category):
                return subprocess.CompletedProcess(args,0,raw if '--entitlements' in args else b'',b'Identifier=generated\n')
            with patch.object(m.v,'command',side_effect=fake):
                self.assertEqual(m.signing_metadata('/generated'),('generated',expected))

    def test_failure_output_never_contains_native_or_private_exception_text(self):
        argv=['initial','--root','/generated','--run-directory','/generated','--source-commit','generated',
              '--products','/generated','--xctestrun','/generated','--settings','/generated','--widget-settings','/generated']
        output=io.StringIO()
        with patch.object(sys,'argv',argv),patch.object(m,'initial_sign',side_effect=m.v.Refused('generated-private-value')),contextlib.redirect_stderr(output):
            self.assertEqual(m.main(),1)
        self.assertNotIn('generated-private',output.getvalue())


GROUP = 'group.com.antoinefink.meguri.dev'


def grant_fixture(role='Meguri'):
    value = {'com.apple.security.application-groups': [GROUP]}
    if role == 'Meguri':
        value.update({'com.apple.developer.icloud-container-identifiers': ['iCloud.generated.dev'],
                      'com.apple.developer.icloud-services': ['CloudKit'],
                      'com.apple.developer.icloud-container-environment': 'Development',
                      'aps-environment': 'development'})
    return value


def role_settings(root, role):
    widget = role == 'MeguriWidget'
    return {'TARGET_NAME': role, 'PRODUCT_NAME': role, 'CONFIGURATION': 'Debug',
            'PLATFORM_NAME': 'iphonesimulator', 'MEGURI_SIGNING': 'team', 'DEVELOPMENT_TEAM': TEAM,
            'CODE_SIGN_IDENTITY': 'Apple Development', 'ENABLE_TESTABILITY': 'YES', 'MEGURI_CLOUD_AGENT_INJECTION': 'YES',
            'MEGURI_CORE_FEATURES': 'ankiweb,test-hooks', 'SWIFT_ACTIVE_COMPILATION_CONDITIONS': 'DEBUG MEGURI_TEST_HOOKS',
            'MEGURI_APP_GROUP': GROUP, 'MEGURI_CLOUD_CONTAINER': 'iCloud.generated.dev',
            'MEGURI_CLOUD_CONFIGURATION': 'development', 'MEGURI_CLOUD_ENVIRONMENT': 'development',
            'MEGURI_CLOUD_CONTAINER_development': 'iCloud.generated.dev', 'MEGURI_CLOUD_CONTAINER_production': 'iCloud.generated.production',
            'INFOPLIST_KEY_MeguriCloudContainer': 'iCloud.generated.dev', 'INFOPLIST_KEY_MeguriCloudEnvironment': 'development',
            'MEGURI_CLOUD_SCHEMA_CONFIGURED_development': 'YES', 'MEGURI_CLOUD_SCHEMA_CONFIGURED_production': 'NO',
            'MEGURI_CLOUD_SCHEMA_CONFIGURED': 'YES', 'MEGURI_CLOUD_SCHEMA_BOOLEAN': '1',
            'MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT': 'Development', 'MEGURI_CLOUD_PUSH_ENVIRONMENT': 'development',
            'MEGURI_BUNDLE_ID_SUFFIX': '.dev', 'SRCROOT': str(root/'app'),
            'PRODUCT_BUNDLE_IDENTIFIER': 'com.antoinefink.meguri.dev' + ('.widget' if widget else ''),
            'CODE_SIGN_ENTITLEMENTS': 'MeguriWidget/MeguriWidget-iOS.entitlements' if widget else 'Meguri/Meguri-iOS.entitlements',
            'FULL_PRODUCT_NAME': 'MeguriWidget.appex' if widget else 'Meguri.app',
            'TARGET_TEMP_DIR': str(root/'build/app/CloudAgentSimDerivedData/Build/Intermediates.noindex/Meguri.build/Debug-iphonesimulator'/ (role+'.build')),
            'BUILT_PRODUCTS_DIR': str(root/'build/app/CloudAgentSimDerivedData/Build/Products/Debug-iphonesimulator')}


def write_role_inputs(root, role):
    settings = role_settings(root, role)
    source = {'com.apple.security.application-groups': ['$(MEGURI_APP_GROUP)']}
    if role == 'Meguri':
        source.update({'com.apple.developer.icloud-container-identifiers': ['$(MEGURI_CLOUD_CONTAINER)'],
                       'com.apple.developer.icloud-services': ['CloudKit'],
                       'com.apple.developer.icloud-container-environment': '$(MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT)',
                       'aps-environment': '$(MEGURI_CLOUD_PUSH_ENVIRONMENT)'})
    paths = [root/'app'/settings['CODE_SIGN_ENTITLEMENTS'],
             Path(settings['TARGET_TEMP_DIR'])/(settings['FULL_PRODUCT_NAME']+'.xcent'),
             Path(settings['TARGET_TEMP_DIR'])/(settings['FULL_PRODUCT_NAME']+'-Simulated.xcent')]
    for path, value in zip(paths, (source, {}, grant_fixture(role))):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(plistlib.dumps(value))
    return settings, paths


class NativeRequestGrantTests(unittest.TestCase):
    def test_retained_file_owner_and_group_are_part_of_the_input_pin(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(); path = root/'input'; path.write_bytes(b'generated')
            original = path.stat(); actual_fstat = os.fstat
            class ChangedStat:
                def __init__(self, value, field): self.value, self.field = value, field
                def __getattr__(self, name):
                    value = getattr(self.value, name)
                    return value + 1 if name == self.field else value
            for field in ('st_uid', 'st_gid'):
                with contextlib.ExitStack() as held:
                    checks = []; m.retain_preparation_input(str(root), str(path), held, checks)
                    def observed(fd):
                        value = actual_fstat(fd)
                        return ChangedStat(value, field) if value.st_dev == original.st_dev and value.st_ino == original.st_ino else value
                    with patch.object(m.os, 'fstat', side_effect=observed):
                        with self.assertRaises(m.v.Refused): checks[0]()

    def test_each_role_adds_only_its_declared_grants_and_preserves_original_typed_map(self):
        for role in ('Meguri', 'MeguriWidget'):
            normal = {'existing-data': b'generated', 'existing-boolean': True, 'existing-integer': 1}
            before = m.typed_entitlements(normal)
            requested = grant_fixture(role)
            settings = role_settings(Path('/generated'), role)
            merged = m.merge_native_request(normal, requested, requested, role, settings)
            self.assertEqual(m.typed_entitlements(normal), before)
            for key, value in normal.items():
                self.assertEqual(m.typed_entitlements({key: merged[key]}), m.typed_entitlements({key: value}))
            self.assertEqual(set(merged)-set(normal), set(requested) | ({'get-task-allow'} if role == 'Meguri' else set()))
            if role == 'MeguriWidget': self.assertNotIn('get-task-allow', merged)

    def test_extra_missing_conflicting_or_wrong_typed_native_inputs_refuse(self):
        requested = grant_fixture()
        cases = []
        for key, value in (('extra', True), ('get-task-allow', True), ('aps-environment', 'production'),
                           ('com.apple.security.application-groups', []), ('com.apple.developer.icloud-services', True)):
            simulated = copy.deepcopy(requested); simulated[key] = value
            cases.append(({}, simulated))
        missing = copy.deepcopy(requested); del missing['aps-environment']; cases.append(({}, missing))
        for key, value in (('aps-environment', 'production'), ('com.apple.security.application-groups', []),
                           ('get-task-allow', 1), ('get-task-allow', False),
                           ('com.apple.security.get-task-allow', False), ('com.apple.security.get-task-allow', 1)):
            cases.append(({key: value}, requested))
        for normal, simulated in cases:
            with self.subTest(normal=normal, simulated=simulated):
                with self.assertRaises(m.v.Refused):
                    m.merge_native_request(normal, simulated, requested, 'Meguri', role_settings(Path('/generated'), 'Meguri'))
        for group in (None, [], ['wrong'], [GROUP, GROUP], [True]):
            changed = copy.deepcopy(requested); changed['com.apple.security.application-groups'] = group
            with self.assertRaises(m.v.Refused):
                m.merge_native_request({}, changed, changed, 'Meguri', role_settings(Path('/generated'), 'Meguri'))

    def test_widget_refuses_app_source_grants_or_debugger_inheritance(self):
        widget = grant_fixture('MeguriWidget'); settings = role_settings(Path('/generated'), 'MeguriWidget')
        for normal in (grant_fixture(), {'get-task-allow': True}, {'com.apple.security.get-task-allow': True}):
            with self.assertRaises(m.v.Refused): m.merge_native_request(normal, widget, widget, 'MeguriWidget', settings)
        with self.assertRaises(m.v.Refused): m.merge_native_request({}, grant_fixture(), grant_fixture(), 'MeguriWidget', settings)

    def test_explicit_debug_testability_and_hooks_intent_is_required(self):
        requested = grant_fixture()
        for key, value in (('CONFIGURATION', 'Profile'), ('CONFIGURATION', 'Release'), ('ENABLE_TESTABILITY', 'NO'),
                           ('MEGURI_SIGNING', 'adhoc'), ('MEGURI_CLOUD_AGENT_INJECTION', 'NO'), ('MEGURI_CORE_FEATURES', 'ankiweb'),
                           ('SWIFT_ACTIVE_COMPILATION_CONDITIONS', 'DEBUG'), ('MEGURI_APP_GROUP', '')):
            settings = role_settings(Path('/generated'), 'Meguri'); settings[key] = value
            with self.assertRaises(m.v.Refused): m.merge_native_request({}, requested, requested, 'Meguri', settings)

    def test_named_target_parser_refuses_missing_duplicate_wrong_role_config_and_shape(self):
        settings = role_settings(Path('/generated'), 'MeguriWidget')
        row = {'target': 'MeguriWidget', 'buildSettings': settings}
        self.assertEqual(m.resolved_targets(json.dumps([row]).encode(), ('MeguriWidget',)), {'MeguriWidget': settings})
        cases = [b'[{"target":"MeguriWidget","target":"Meguri"}]', [], [row, row], [{'target': 'Meguri', 'buildSettings': settings}], [True], {}, b'bad-json']
        for key, value in (('TARGET_NAME', 'Meguri'), ('CONFIGURATION', 'Release'), ('PLATFORM_NAME', 'iphoneos'),
                           ('DEVELOPMENT_TEAM', None), ('CODE_SIGN_IDENTITY', '-')):
            changed = copy.deepcopy(row); changed['buildSettings'][key] = value; cases.append([changed])
        for rows in cases:
            with self.assertRaises(m.v.Refused): m.resolved_targets(rows if type(rows) is bytes else json.dumps(rows).encode(), ('MeguriWidget',))

    def test_fixed_source_native_paths_and_role_descriptors_are_pinned(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(); settings, paths = write_role_inputs(root, 'Meguri')
            products = str(root/'build/app/CloudAgentSimDerivedData/Build/Products')
            for key, value in (('TARGET_NAME', 'MeguriWidget'), ('CODE_SIGN_ENTITLEMENTS', '../other'),
                               ('SRCROOT', '/elsewhere'), ('TARGET_TEMP_DIR', '/elsewhere'), ('BUILT_PRODUCTS_DIR', '/elsewhere')):
                changed = dict(settings); changed[key] = value
                with contextlib.ExitStack() as held, self.assertRaises(m.v.Refused):
                    m.prepare_role_grants(str(root), products, 'Meguri', changed, held, [])
            for index in range(3):
                original = paths[index].read_bytes()
                for substitution in ('mutation', 'replacement', 'symlink'):
                    with contextlib.ExitStack() as held:
                        checks = []; m.prepare_role_grants(str(root), products, 'Meguri', settings, held, checks)
                        if substitution == 'mutation': paths[index].write_bytes(b'changed')
                        else:
                            paths[index].unlink()
                            if substitution == 'replacement': paths[index].write_bytes(original)
                            else: paths[index].symlink_to(paths[(index+1)%3])
                        with self.assertRaises((m.v.Refused, OSError)): [check() for check in checks]
                    paths[index].unlink(); paths[index].write_bytes(original)
            source = plistlib.loads(paths[0].read_bytes()); source['unapproved'] = True
            paths[0].write_bytes(plistlib.dumps(source))
            with contextlib.ExitStack() as held, self.assertRaises(m.v.Refused):
                m.prepare_role_grants(str(root), products, 'Meguri', settings, held, [])


class InitialFreezeLifetimeTests(unittest.TestCase):
    def test_first_freeze_publishes_once_with_inputs_alive_and_refuses_a_second_call(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(); directory = root/'build/mg/build-app/20261110T000000Z-123'; directory.mkdir(parents=True)
            input_path = directory/'input'; input_path.write_bytes(b'generated-bound-input')
            products = root/'build/products'; products.mkdir(); (products/'generated').write_bytes(b'product')
            events = []
            def freeze(*args):
                events.append('freeze'); checks[0]()
                return {'products': {'entries': m.v.snapshot(str(root/'build'), str(products))[0]}}
            with contextlib.ExitStack() as held:
                checks = []; m.retain_preparation_input(str(root), str(input_path), held, checks)
                with patch.object(m.v, 'source', return_value='a'*40), patch.object(m.v, 'freeze_build', side_effect=freeze) as native:
                    m.first_freeze(str(root), str(directory), 'a'*40, str(products), str(products/'test.xctestrun'), checks)
                    self.assertEqual(native.call_count, 1)
                    reference = json.loads((directory/'agent-build-reference.json').read_bytes())
                    self.assertEqual(reference, m.v.file_reference(str(root/'build'), str(directory/'agent-build.json')))
                    with self.assertRaisesRegex(m.v.Refused, 'initial-already-frozen'):
                        m.first_freeze(str(root), str(directory), 'a'*40, str(products), str(products/'test.xctestrun'), checks)
                    self.assertEqual(native.call_count, 1)
            self.assertEqual(events, ['freeze'])

    def test_input_substitution_during_freeze_or_original_failure_never_publishes_reference(self):
        for failure in ('mutation', 'replacement', 'signing-failure', 'cancel'):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve(); directory = root/'build/mg/build-app/20261110T000000Z-123'; directory.mkdir(parents=True)
                products = root/'build/products'; products.mkdir(); (products/'generated').write_bytes(b'product')
                input_path = directory/'input'; input_path.write_bytes(b'bound')
                def freeze(*args):
                    if failure == 'cancel': raise m.stage.Cancelled()
                    if failure == 'signing-failure': raise m.v.Refused('build-signature')
                    if failure == 'replacement': input_path.unlink()
                    input_path.write_bytes(b'bound' if failure == 'replacement' else b'changed')
                    return {'products': {'entries': m.v.snapshot(str(root/'build'), str(products))[0]}}
                with contextlib.ExitStack() as held:
                    checks = []; m.retain_preparation_input(str(root), str(input_path), held, checks)
                    with patch.object(m.v, 'source', return_value='a'*40), patch.object(m.v, 'freeze_build', side_effect=freeze) as native:
                        with self.assertRaises((m.v.Refused, m.stage.Cancelled)):
                            m.first_freeze(str(root), str(directory), 'a'*40, str(products), str(products/'test.xctestrun'), checks)
                        self.assertEqual(native.call_count, 1)
                self.assertFalse((directory/'agent-build-reference.json').exists())

    def test_existing_manifest_or_reference_symlink_never_overwrites(self):
        for name in ('agent-build.json', 'agent-build-reference.json'):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve(); directory = root/'build/mg/build-app/20261110T000000Z-123'; directory.mkdir(parents=True)
                outside = root/'outside'; outside.write_bytes(b'keep'); (directory/name).symlink_to(outside)
                with patch.object(m.v, 'source', return_value='a'*40), patch.object(m.v, 'publish_frozen_build') as publish:
                    with self.assertRaises(m.v.Refused): m.first_freeze(str(root), str(directory), 'a'*40, 'unused', 'unused', [])
                    publish.assert_not_called()
                self.assertEqual(outside.read_bytes(), b'keep')


class InitialTransactionLifetimeTests(unittest.TestCase):
    def test_role_inputs_stay_bound_through_joined_signing_and_first_freeze(self):
        for mode in ('success', 'signing-failure', 'cancel', 'source-substitution', 'normal-substitution', 'simulated-substitution', 'settings-substitution'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve(); run = root/'build/mg/build-app/20261110T000000Z-123'; run.mkdir(parents=True)
                products = root/'build/app/CloudAgentSimDerivedData/Build/Products'; products.mkdir(parents=True)
                app_relative = 'Debug-iphonesimulator/Meguri.app'; app_path = products/app_relative
                agent_relative = app_relative+'/PlugIns/MeguriSnapshotTests.xctest'
                widget_relative = app_relative+'/PlugIns/MeguriWidget.appex'
                metadata = {}
                for relative, identifier, executable, kind in (
                        (app_relative, 'com.antoinefink.meguri.dev', 'Meguri', 'APPL'),
                        (agent_relative, 'generated.agent', 'MeguriSnapshotTests', 'BNDL'),
                        (widget_relative, 'com.antoinefink.meguri.dev.widget', 'MeguriWidget', 'XPC!')):
                    path = products/relative; path.mkdir(parents=True)
                    info = {'CFBundleIdentifier': identifier, 'CFBundleExecutable': executable,
                            'CFBundlePackageType': kind, 'CFBundleSupportedPlatforms': ['iPhoneSimulator'],
                            'CFBundleVersion': 'generated', 'MeguriCloudContainer': 'iCloud.generated.dev',
                            'MeguriCloudEnvironment': 'development'}
                    (path/'Info.plist').write_bytes(plistlib.dumps(info)); (path/executable).write_bytes(code_image_fixture())
                    metadata[str(path)] = (identifier, {})
                injection = app_path/'Frameworks/libXCTestBundleInject.dylib'; injection.parent.mkdir(); injection.write_bytes(code_image_fixture())
                metadata[str(injection)] = ('generated.library', None)
                app_settings, app_inputs = write_role_inputs(root, 'Meguri')
                widget_settings, widget_inputs = write_role_inputs(root, 'MeguriWidget')
                agent_settings = dict(app_settings); agent_settings.update(TARGET_NAME='MeguriSnapshotTests', PRODUCT_BUNDLE_IDENTIFIER='generated.agent')
                settings_path = run/'agent-settings.json'; widget_settings_path = run/'agent-widget-settings.json'
                settings_path.write_text(json.dumps([{'target': 'Meguri', 'buildSettings': app_settings},
                                                    {'target': 'MeguriSnapshotTests', 'buildSettings': agent_settings}]))
                widget_settings_path.write_text(json.dumps([{'target': 'MeguriWidget', 'buildSettings': widget_settings}]))
                xctestrun = products/'generated.xctestrun'; xctestrun.write_bytes(b'generated')
                agent, _ = m.v.bundle(str(products), agent_relative, 'ios-sim')
                events = []; live = []
                def native(arguments):
                    events.append('sign'); path = arguments[-1]
                    if '--entitlements' in arguments:
                        private = Path(arguments[arguments.index('--entitlements')+1]); live.append(private)
                        self.assertTrue(private.is_file())
                        metadata[path] = (metadata[path][0], plistlib.loads(private.read_bytes()))
                    if mode == 'signing-failure': raise m.stage.Refused()
                    if mode == 'cancel': raise m.stage.Cancelled()
                    substitution = {'source-substitution': app_inputs[0], 'normal-substitution': app_inputs[1],
                                    'simulated-substitution': app_inputs[2], 'settings-substitution': settings_path}.get(mode)
                    if substitution is not None:
                        original = substitution.read_bytes(); substitution.unlink(); substitution.write_bytes(original)
                def freeze(*args):
                    events.append('freeze')
                    self.assertEqual(metadata[str(app_path)][1]['get-task-allow'], True)
                    self.assertEqual(metadata[str(products/widget_relative)][1], grant_fixture('MeguriWidget'))
                    # Inputs still refer to their original bytes when publication runs.
                    self.assertEqual(plistlib.loads(app_inputs[2].read_bytes()), grant_fixture())
                    return {'products': {'entries': m.v.snapshot(str(root/'build'), str(products))[0]}}
                read_original = m.v._first_sync_bytes
                def certificate_or_original(base, path):
                    if Path(base).name.startswith('meguri-initial-signed-') and path == str(Path(base)/'certificate0'):
                        return CERT
                    return read_original(base, path)
                with patch.object(m.v, 'source', return_value='a'*40), patch.object(m, 'choose_identity', return_value=(SHA, CERT)), \
                     patch.object(m.v, 'test_inputs', return_value=agent), patch.object(m, 'signing_metadata', side_effect=lambda path: copy.deepcopy(metadata[path])), \
                     patch.object(m, 'signing_command', side_effect=native), patch.object(m.v, 'command'), \
                     patch.object(m.v, '_first_sync_bytes', side_effect=certificate_or_original), patch.object(m.v, 'independent_product', return_value=TEAM), \
                     patch.object(m.v, 'independent_agent'), patch.object(m.v, 'freeze_build', side_effect=freeze) as freeze_call:
                    args = (str(root), str(run), 'a'*40, str(products), str(xctestrun), str(settings_path), str(widget_settings_path))
                    if mode == 'success':
                        m.initial_sign(*args)
                        self.assertEqual(freeze_call.call_count, 1); self.assertEqual(events[-1], 'freeze')
                        self.assertTrue((run/'agent-build-reference.json').is_file())
                    else:
                        with self.assertRaises((m.v.Refused, m.stage.Refused, m.stage.Cancelled)): m.initial_sign(*args)
                        freeze_call.assert_not_called()
                        self.assertFalse((run/'agent-build.json').exists()); self.assertFalse((run/'agent-build-reference.json').exists())
                self.assertTrue(all(not path.exists() for path in live))


if __name__=='__main__':unittest.main()
