# SPDX-License-Identifier: AGPL-3.0-or-later
"""Bounded known-surface discovery only; no collection is opened or operated."""
import hashlib
import importlib.metadata
import inspect
import json
import os
import pathlib
import stat
import unittest


NAME_CAP=256
CANDIDATE_CAP=32
RECORD_CAP=4096
TOTAL_CAP=262144
IDENTITY_STRING_CAP=128
OUTPUT=pathlib.Path('/out/mobile-sdk-surface-quarantined.json')


def packed(value):
    return json.dumps(value,ensure_ascii=True,allow_nan=False,separators=(',',':'),sort_keys=True).encode('ascii')


class MobileSDKPublicSurface(unittest.TestCase):
    def test_current_pin_bounded_known_public_collection_surfaces(self):
        # Imports are confined to already named public surfaces and the build
        # metadata used by the existing adapter. No module walk or API call.
        result={'status':'incomplete','scope':'known-collection-surfaces-only','collection_operations':0,
                'mobile_credit':False,'surfaces':{},'candidates':[],
                'stage':'initial','initial_identity':{}}
        failure=None
        def retain_identity(slot,value):
            actual_type=type(value)
            known_types=((str,'str'),(bytes,'bytes'),(bytearray,'bytearray'),(bool,'bool'),
                         (int,'int'),(float,'float'),(list,'list'),(tuple,'tuple'),(dict,'dict'),(type(None),'NoneType'))
            entry={'type':next((name for known,name in known_types if actual_type is known),'other'),
                   'status':'unsupported-type','length_status':'not-read-unsupported-type'}
            result['initial_identity'][slot]=entry
            if any(actual_type is known for known in (str,bytes,bytearray,list,tuple,dict)):
                entry['length']=len(value)
                entry['length_status']='characters' if actual_type is str else 'bytes' if actual_type is bytes or actual_type is bytearray else 'items'
            elif any(actual_type is known for known in (bool,int,float,type(None))):entry['length_status']='not-applicable'
            if actual_type is not str:raise AssertionError('unsupported public identity type')
            if len(value) > IDENTITY_STRING_CAP:
                entry['status']='oversize-string';raise AssertionError('public identity string cap exceeded')
            try:data=value.encode('utf-8')
            except UnicodeError:
                entry['status']='invalid-unicode';raise AssertionError('public identity is not valid Unicode') from None
            entry['utf8_bytes']=len(data)
            if len(data) > IDENTITY_STRING_CAP:
                entry['status']='oversize-string';raise AssertionError('public identity string cap exceeded')
            entry['status']='complete-string';entry['value']=value
            self.assertLessEqual(len(packed(entry)),RECORD_CAP,'identity diagnostic record cap exceeded')
            self.assertLessEqual(len(packed(result)),TOTAL_CAP,'total discovery cap exceeded')
        try:
            result['stage']='import_collection'
            import anki.collection as module
            result['stage']='import_build_metadata'
            from anki import buildinfo
            result['stage']='read_distribution_version'
            distribution_version=importlib.metadata.version('anki')
            result['stage']='retain_distribution_version'
            retain_identity('distribution_version',distribution_version)
            result['stage']='assert_distribution_version'
            self.assertEqual(distribution_version,'26.09.4')
            result['stage']='read_build_version'
            build_version=buildinfo.version
            result['stage']='retain_build_version'
            retain_identity('build_version',build_version)
            result['stage']='assert_build_version'
            self.assertIn(build_version,('26.9.4','26.09.4'))
            result['stage']='read_build_hash'
            build_hash=buildinfo.buildhash
            result['stage']='retain_build_hash'
            retain_identity('build_hash',build_hash)
            result['stage']='assert_build_hash'
            self.assertTrue(type(build_hash) is str and build_hash)
            result['stage']='retain_client_identity'
            result['client']={'kind':'desktop','version':'26.09.4','build':build_hash}
            # These are fixed MG-owned installation metadata, not SDK sources.
            result['stage']='read_lock'
            lock=pathlib.Path('/opt/reference/requirements.lock').read_bytes()
            result['stage']='retain_lock_digest'
            result['lock_sha256']=hashlib.sha256(lock).hexdigest()
            result['stage']='read_install_metadata'
            installed=json.loads(pathlib.Path('/opt/reference/install-report.json').read_bytes())
            result['stage']='select_anki_wheel'
            wheels=[item for item in installed['install'] if item['metadata']['name'].lower() == 'anki']
            result['stage']='assert_wheel_count'
            self.assertEqual(len(wheels),1)
            result['stage']='read_wheel_digest'
            wheel=wheels[0]['download_info']['archive_info']['hashes']['sha256']
            result['stage']='assert_wheel_digest'
            self.assertIn(wheel,('52d319123a06d831fbc47cccfba087d75e00dc2e7eb4176c19a2901c525aad9e',
                                '67950105a13fe9894fe424eb799afe01b2a1762f402bf877aaa5b3cb879ec8cc'))
            result['stage']='retain_wheel_digest';result['wheel_sha256']=wheel
            candidates=[]
            for label,surface in (('anki.collection',module),('anki.collection.Collection',module.Collection)):
                result['stage']='list_public_surface_names'
                public=[name for name in dir(surface) if not name.startswith('_')]
                result['stage']='assert_surface_name_cap'
                self.assertLessEqual(len(public),NAME_CAP,'public surface name cap exceeded')
                result['stage']='list_public_surface_callables'
                names=[]
                for name in public:
                    value=getattr(surface,name)
                    if callable(value):
                        names.append(name)
                        if any(term in name.lower() for term in ('import','export','package','collection')):
                            candidates.append((label,name,value))
                result['stage']='retain_public_surface';result['surfaces'][label]={'public_name_count':len(public),'public_callable_names':names}
            result['stage']='assert_candidate_count_cap'
            self.assertLessEqual(len(candidates),CANDIDATE_CAP,'public candidate cap exceeded')
            for label,name,value in candidates:
                entry={'surface':label,'name':name}
                result['stage']='read_candidate_signature'
                try:entry['signature']=str(inspect.signature(value,follow_wrapped=False,eval_str=False))
                except (TypeError,ValueError):entry['signature_status']='unavailable-on-known-callable'
                result['stage']='read_candidate_documentation';documentation=inspect.getdoc(value)
                entry['documentation']=documentation if documentation is not None else ''
                result['stage']='assert_candidate_record_cap'
                self.assertLessEqual(len(packed(entry)),RECORD_CAP,'candidate signature/documentation cap exceeded')
                trial={**result,'candidates':result['candidates']+[entry]}
                result['stage']='assert_candidate_total_cap'
                self.assertLessEqual(len(packed(trial)),TOTAL_CAP,'total discovery cap exceeded')
                result['candidates'].append(entry)
            result['stage']='retain_known_export_fact'
            result['known_export_callable_listed']='export_collection_package' in result['surfaces']['anki.collection.Collection']['public_callable_names']
            result['status']='complete-known-surfaces'
            result['universal_api_absence_claim']=False
            result['stage']='assert_final_total_cap'
            self.assertLessEqual(len(packed(result)),TOTAL_CAP,'total discovery cap exceeded')
            result['stage']='complete'
        except Exception as error:
            failure=error
            # Keep no exception text, frames, source paths, or partial claim of
            # complete discovery. Bounded preceding records remain quarantine.
            result['status']='failed';result['failure_kind']=type(error).__name__
        directory=OUTPUT.parent.lstat()
        self.assertTrue(stat.S_ISDIR(directory.st_mode),'fixed output must be a plain owned directory')
        data=packed(result)
        if len(data) > TOTAL_CAP:
            # Explicit failed discovery, not a truncated complete surface.
            failure=AssertionError('total discovery cap exceeded')
            data=packed({'status':'failed','scope':'known-collection-surfaces-only','failure_kind':'retention-cap',
                         'mobile_credit':False,'collection_operations':0,'complete_surface_retained':False,
                         'stage':result['stage'],'initial_identity':result['initial_identity']})
        pending=OUTPUT.with_suffix('.pending')
        self.assertFalse(OUTPUT.exists() or OUTPUT.is_symlink(),'surface artifact already exists')
        with pending.open('xb') as stream:
            stream.write(data);stream.flush();os.fsync(stream.fileno())
        os.replace(pending,OUTPUT)
        if failure is not None:raise AssertionError('bounded known-surface qualification failed; diagnostics remain quarantined') from None
