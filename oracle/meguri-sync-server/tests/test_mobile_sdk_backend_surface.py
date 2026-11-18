# SPDX-License-Identifier: AGPL-3.0-or-later
"""Metadata only for the measured public backend alias; never construct it."""
import hashlib
import importlib.metadata
import inspect
import json
import os
import pathlib
import stat
import unittest


NAME_CAP=512
RECORD_CAP=4096
CANDIDATE_CAP=16
TOTAL_CAP=262144
IDENTITY_STRING_CAP=128
LOCK_SHA256='56e1bef86def9f5f230d12317e3510f2176eb079928f3a75f86afb0dcc408ec8'
OUTPUT=pathlib.Path('/out/mobile-sdk-backend-surface-quarantined.json')


def packed(value):
    return json.dumps(value,ensure_ascii=True,allow_nan=False,separators=(',',':'),sort_keys=True).encode('ascii')


class MobileSDKBackendSurface(unittest.TestCase):
    def test_current_pin_public_backend_alias_metadata_only(self):
        result={'status':'incomplete','scope':'anki.collection.RustBackend-public-metadata-only',
                'backend_operations':0,'collection_operations':0,'mobile_credit':False,
                'stage':'initial','initial_identity':{},'public_names':[],'records':[]}
        failure=None
        def identity(slot,value):
            actual_type=type(value)
            types=((str,'str'),(bytes,'bytes'),(bytearray,'bytearray'),(bool,'bool'),(int,'int'),
                   (float,'float'),(list,'list'),(tuple,'tuple'),(dict,'dict'),(type(None),'NoneType'))
            entry={'type':next((name for known,name in types if actual_type is known),'other'),
                   'status':'unsupported-type','length_status':'not-read-unsupported-type'}
            result['initial_identity'][slot]=entry
            if any(actual_type is known for known in (str,bytes,bytearray,list,tuple,dict)):
                entry['length']=len(value)
                entry['length_status']='characters' if actual_type is str else 'bytes' if actual_type is bytes or actual_type is bytearray else 'items'
            elif any(actual_type is known for known in (bool,int,float,type(None))):entry['length_status']='not-applicable'
            if actual_type is not str:raise AssertionError('unsupported public identity type')
            if len(value) > IDENTITY_STRING_CAP:
                entry['status']='oversize-string';raise AssertionError('identity diagnostic cap exceeded')
            try:data=value.encode('utf-8')
            except UnicodeError:
                entry['status']='invalid-unicode';raise AssertionError('invalid identity Unicode') from None
            entry['utf8_bytes']=len(data)
            if len(data) > IDENTITY_STRING_CAP:
                entry['status']='oversize-string';raise AssertionError('identity diagnostic cap exceeded')
            entry['status']='complete-string';entry['value']=value
            self.assertLessEqual(len(packed(entry)),RECORD_CAP,'identity diagnostic record cap exceeded')
            self.assertLessEqual(len(packed(result)),TOTAL_CAP,'total backend discovery cap exceeded')
        def retain_metadata(kind,name,value):
            self.assertLess(len(result['records']),CANDIDATE_CAP,'backend metadata record cap exceeded')
            entry={'kind':kind,'name':name,'callable':callable(value)}
            if entry['callable']:
                result['stage']='read_public_signature'
                try:entry['signature']=str(inspect.signature(value,follow_wrapped=False,eval_str=False))
                except (TypeError,ValueError):entry['signature_status']='unavailable-on-known-callable'
                result['stage']='read_public_documentation';documentation=inspect.getdoc(value)
                entry['documentation']=documentation if documentation is not None else ''
            else:entry['metadata_status']='enumerated-noncallable'
            result['stage']='assert_metadata_record_cap'
            self.assertLessEqual(len(packed(entry)),RECORD_CAP,'backend metadata record byte cap exceeded')
            trial={**result,'records':result['records']+[entry]}
            result['stage']='assert_metadata_total_cap'
            self.assertLessEqual(len(packed(trial)),TOTAL_CAP,'total backend discovery cap exceeded')
            result['records'].append(entry)
        try:
            result['stage']='import_collection'
            import anki.collection as module
            result['stage']='import_build_metadata'
            from anki import buildinfo
            result['stage']='read_distribution_version';distribution_version=importlib.metadata.version('anki')
            result['stage']='retain_distribution_version';identity('distribution_version',distribution_version)
            result['stage']='assert_distribution_version';self.assertEqual(distribution_version,'26.09.4')
            result['stage']='read_build_version';build_version=buildinfo.version
            result['stage']='retain_build_version';identity('build_version',build_version)
            result['stage']='assert_build_version';self.assertEqual(build_version,'26.09.4')
            result['stage']='read_build_hash';build_hash=buildinfo.buildhash
            result['stage']='retain_build_hash';identity('build_hash',build_hash)
            result['stage']='assert_build_hash';self.assertEqual(build_hash,'c06370ff')
            result['client']={'kind':'desktop','version':'26.09.4','build':build_hash}
            result['stage']='read_lock';lock=pathlib.Path('/opt/reference/requirements.lock').read_bytes()
            result['lock_sha256']=hashlib.sha256(lock).hexdigest()
            result['stage']='assert_lock_identity';self.assertEqual(result['lock_sha256'],LOCK_SHA256)
            result['stage']='read_install_metadata';installed=json.loads(pathlib.Path('/opt/reference/install-report.json').read_bytes())
            result['stage']='select_anki_wheel';wheels=[item for item in installed['install'] if item['metadata']['name'].lower() == 'anki']
            result['stage']='assert_wheel_count';self.assertEqual(len(wheels),1)
            result['stage']='read_wheel_digest';wheel=wheels[0]['download_info']['archive_info']['hashes']['sha256']
            result['stage']='assert_wheel_digest'
            self.assertIn(wheel,('52d319123a06d831fbc47cccfba087d75e00dc2e7eb4176c19a2901c525aad9e',
                                '67950105a13fe9894fe424eb799afe01b2a1762f402bf877aaa5b3cb879ec8cc'))
            result['wheel_sha256']=wheel
            result['stage']='read_measured_public_alias';surface=module.RustBackend
            result['stage']='assert_alias_class';self.assertTrue(inspect.isclass(surface),'measured alias is not a class')
            result['stage']='list_public_alias_names';names=[name for name in dir(surface) if not name.startswith('_')]
            result['stage']='assert_public_name_cap';self.assertLessEqual(len(names),NAME_CAP,'public backend name cap exceeded')
            result['public_names']=names;result['public_name_count']=len(names)
            result['stage']='assert_public_name_total_cap';self.assertLessEqual(len(packed(result)),TOTAL_CAP,'total backend discovery cap exceeded')
            # Only members actually returned above may be selected or read.
            selected=[name for name in names if 'collection' in name.lower() and 'package' in name.lower()]
            result['stage']='assert_selected_record_count'
            # The constructor shares the same record budget, not an extra slot.
            self.assertLessEqual(1+len(selected),CANDIDATE_CAP,'backend metadata record cap exceeded')
            result['selected_member_count']=len(selected)
            result['stage']='capture_public_constructor';retain_metadata('constructor','RustBackend',surface)
            for name in selected:
                result['stage']='read_enumerated_public_member';member=getattr(surface,name)
                retain_metadata('enumerated-member',name,member)
            result['universal_api_absence_claim']=False
            result['stage']='assert_final_total_cap';self.assertLessEqual(len(packed(result)),TOTAL_CAP,'total backend discovery cap exceeded')
            result['stage']='complete';result['status']='complete-public-alias-metadata'
        except Exception as error:
            failure=error
            result['status']='failed';result['failure_kind']=type(error).__name__
        directory=OUTPUT.parent.lstat()
        self.assertTrue(stat.S_ISDIR(directory.st_mode),'fixed backend output must be a plain owned directory')
        data=packed(result)
        if len(data) > TOTAL_CAP:
            failure=AssertionError('total backend discovery cap exceeded')
            data=packed({'status':'failed','scope':'anki.collection.RustBackend-public-metadata-only',
                         'failure_kind':'retention-cap','stage':result['stage'],'initial_identity':result['initial_identity'],
                         'backend_operations':0,'collection_operations':0,'mobile_credit':False,'complete_surface_retained':False})
        self.assertLessEqual(len(data),TOTAL_CAP,'bounded backend failure retention cap exceeded')
        pending=OUTPUT.with_suffix('.pending')
        self.assertFalse(OUTPUT.exists() or OUTPUT.is_symlink(),'backend surface artifact already exists')
        with pending.open('xb') as stream:
            stream.write(data);stream.flush();os.fsync(stream.fileno())
        os.replace(pending,OUTPUT)
        if failure is not None:raise AssertionError('public backend metadata discovery failed; diagnostics remain quarantined') from None
