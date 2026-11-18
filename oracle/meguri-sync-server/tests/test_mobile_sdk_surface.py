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
OUTPUT=pathlib.Path('/out/mobile-sdk-surface-quarantined.json')


def packed(value):
    return json.dumps(value,ensure_ascii=True,allow_nan=False,separators=(',',':'),sort_keys=True).encode('ascii')


class MobileSDKPublicSurface(unittest.TestCase):
    def test_current_pin_bounded_known_public_collection_surfaces(self):
        # Imports are confined to already named public surfaces and the build
        # metadata used by the existing adapter. No module walk or API call.
        result={'status':'incomplete','scope':'known-collection-surfaces-only','collection_operations':0,
                'mobile_credit':False,'surfaces':{},'candidates':[]}
        failure=None
        try:
            import anki.collection as module
            from anki import buildinfo
            self.assertEqual(importlib.metadata.version('anki'),'26.9.4')
            self.assertIn(buildinfo.version,('26.9.4','26.09.4'))
            self.assertTrue(type(buildinfo.buildhash) is str and buildinfo.buildhash)
            result['client']={'kind':'desktop','version':'26.09.4','build':buildinfo.buildhash}
            # These are fixed MG-owned installation metadata, not SDK sources.
            lock=pathlib.Path('/opt/reference/requirements.lock').read_bytes()
            result['lock_sha256']=hashlib.sha256(lock).hexdigest()
            installed=json.loads(pathlib.Path('/opt/reference/install-report.json').read_bytes())
            wheels=[item for item in installed['install'] if item['metadata']['name'].lower() == 'anki']
            self.assertEqual(len(wheels),1)
            wheel=wheels[0]['download_info']['archive_info']['hashes']['sha256']
            self.assertIn(wheel,('52d319123a06d831fbc47cccfba087d75e00dc2e7eb4176c19a2901c525aad9e',
                                '67950105a13fe9894fe424eb799afe01b2a1762f402bf877aaa5b3cb879ec8cc'))
            result['wheel_sha256']=wheel
            candidates=[]
            for label,surface in (('anki.collection',module),('anki.collection.Collection',module.Collection)):
                public=[name for name in dir(surface) if not name.startswith('_')]
                self.assertLessEqual(len(public),NAME_CAP,'public surface name cap exceeded')
                names=[]
                for name in public:
                    value=getattr(surface,name)
                    if callable(value):
                        names.append(name)
                        if any(term in name.lower() for term in ('import','export','package','collection')):
                            candidates.append((label,name,value))
                result['surfaces'][label]={'public_name_count':len(public),'public_callable_names':names}
            self.assertLessEqual(len(candidates),CANDIDATE_CAP,'public candidate cap exceeded')
            for label,name,value in candidates:
                entry={'surface':label,'name':name}
                try:entry['signature']=str(inspect.signature(value,follow_wrapped=False,eval_str=False))
                except (TypeError,ValueError):entry['signature_status']='unavailable-on-known-callable'
                documentation=inspect.getdoc(value)
                entry['documentation']=documentation if documentation is not None else ''
                self.assertLessEqual(len(packed(entry)),RECORD_CAP,'candidate signature/documentation cap exceeded')
                trial={**result,'candidates':result['candidates']+[entry]}
                self.assertLessEqual(len(packed(trial)),TOTAL_CAP,'total discovery cap exceeded')
                result['candidates'].append(entry)
            result['known_export_callable_listed']='export_collection_package' in result['surfaces']['anki.collection.Collection']['public_callable_names']
            result['status']='complete-known-surfaces'
            result['universal_api_absence_claim']=False
            self.assertLessEqual(len(packed(result)),TOTAL_CAP,'total discovery cap exceeded')
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
                         'mobile_credit':False,'collection_operations':0,'complete_surface_retained':False})
        pending=OUTPUT.with_suffix('.pending')
        self.assertFalse(OUTPUT.exists() or OUTPUT.is_symlink(),'surface artifact already exists')
        with pending.open('xb') as stream:
            stream.write(data);stream.flush();os.fsync(stream.fileno())
        os.replace(pending,OUTPUT)
        if failure is not None:raise AssertionError('bounded known-surface qualification failed; diagnostics remain quarantined') from None
