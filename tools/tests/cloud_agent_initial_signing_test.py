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
                    m.initial_sign(str(root),str(run),'a'*40,'unused','unused','unused')
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
              '--products','/generated','--xctestrun','/generated','--settings','/generated']
        output=io.StringIO()
        with patch.object(sys,'argv',argv),patch.object(m,'initial_sign',side_effect=m.v.Refused('generated-private-value')),contextlib.redirect_stderr(output):
            self.assertEqual(m.main(),1)
        self.assertNotIn('generated-private',output.getvalue())


if __name__=='__main__':unittest.main()
