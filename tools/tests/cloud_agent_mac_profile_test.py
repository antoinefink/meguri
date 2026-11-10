#!/usr/bin/env python3
"""Generated profile/native-command fixtures only; no setup or signing evidence."""
import copy,hashlib,importlib.util,json,os,plistlib,subprocess,sys,unittest
from pathlib import Path
from unittest.mock import patch
import cloud_agent_test as fixtures
spec=importlib.util.spec_from_file_location('mac_profile_subject',Path(__file__).resolve().parents[1]/'scripts/cloud-agent-mac-profile.py')
m=importlib.util.module_from_spec(spec);sys.modules[spec.name]=m;spec.loader.exec_module(m)
UUID='00000000-0000-0000-0000-000000000001'

class ProfileTests(unittest.TestCase):
 def setUp(self):
  self.f=fixtures.AdmissionTests();self.f.setUp();self.addCleanup(self.f.doCleanups)
  self.root=self.f.root;self.home=self.root/'generated-home';self.home.mkdir()
  self.profile=copy.deepcopy(self.f.profile);self.profile['UUID']=UUID
  self.input=self.home/'approved.provisionprofile';self.input.write_bytes(b'generated-approved-profile')
  cache=self.home/'Library/Developer/Xcode/UserData/Provisioning Profiles';cache.mkdir(parents=True)
  self.cache=cache/(UUID+'.provisionprofile');self.cache.write_bytes(self.input.read_bytes())
  ent=self.root/'app/request.entitlements';ent.parent.mkdir(parents=True,exist_ok=True);ent.write_bytes(plistlib.dumps(self.f.ent))
  self.certificate_team=fixtures.TEAM;self.expired_certificate=False;self.other_certificate_cn=False;self.two_identities=False
  self.ent=ent;self.changed_other=False;self.target_shape=None;self.unexpected_role=None
  self.settings=fixtures.settings();self.settings.update(CONFIGURATION='Debug',PLATFORM_NAME='macosx',MEGURI_SIGNING='team',
   DEVELOPMENT_TEAM=fixtures.TEAM,CODE_SIGN_ENTITLEMENTS='request.entitlements')
  for item in (patch.object(m.Path,'home',return_value=self.home),patch.object(m.v,'source',return_value=fixtures.SHA),
               patch.object(m.v,'command',side_effect=self.command),
               patch.object(m.subprocess,'run',side_effect=self.decode)):
   item.start();self.addCleanup(item.stop)
 def decode(self,args,*,input,capture_output,timeout):
  if args[0]=='/usr/bin/openssl':
   self.assertEqual(args[1:4],['x509','-inform','DER']);self.assertIn(input,self.profile['DeveloperCertificates'])
   self.assertTrue(capture_output);self.assertEqual(timeout,60)
   if '-subject' in args:
    cn='Generated Other' if self.other_certificate_cn else 'Apple Development: Generated'
    out=('subject=CN='+cn+',OU='+self.certificate_team+',O=Generated').encode()
    return subprocess.CompletedProcess(args,0,out,b'')
   self.assertEqual(args[-3:],['-noout','-checkend','0'])
   return subprocess.CompletedProcess(args,1 if self.expired_certificate else 0,b'',b'')
  self.assertEqual(args,['/usr/bin/security','cms','-D'])
  self.assertEqual(input,b'generated-approved-profile')
  self.assertTrue(capture_output);self.assertEqual(timeout,60)
  return subprocess.CompletedProcess(args,0,plistlib.dumps(self.profile),b'')
 def command(self,args,category):
  if args[0]=='/usr/bin/xcodebuild':
   self.assertIn('-showBuildSettings',args);self.assertNotIn('-alltargets',args)
   self.assertEqual(args[args.index('-configuration')+1],'Debug')
   self.assertEqual(args[args.index('-sdk')+1],'macosx')
   self.assertIn('SYMROOT='+str(self.root/'build/app/CloudAgentMacDerivedData/Build/Products'),args)
   self.assertIn('OBJROOT='+str(self.root/'build/app/CloudAgentMacDerivedData/Build/Intermediates.noindex'),args)
   for excluded in ('-scheme','-testPlan','-derivedDataPath','build-for-testing'):self.assertNotIn(excluded,args)
   role=args[args.index('-target')+1]
   self.assertIn(role,('Meguri','MeguriWidget','MeguriSnapshotTests'))
   custom=any(x.startswith('MEGURI_MAC_AGENT_PROFILE_SPECIFIER=') for x in args)
   app=dict(self.settings,TARGET_NAME='Meguri',CODE_SIGN_STYLE='Manual' if custom else 'Automatic',PROVISIONING_PROFILE_SPECIFIER=UUID if custom else '')
   other={'CODE_SIGN_STYLE':'Manual' if custom and self.changed_other else 'Automatic','CONFIGURATION':'Debug',
    'PLATFORM_NAME':'macosx','MEGURI_SIGNING':'team','TARGET_NAME':role,
    'PRODUCT_BUNDLE_IDENTIFIER':'com.antoinefink.meguri.dev.'+('widget' if role=='MeguriWidget' else 'snapshottests')}
   targets=[{'target':role,'buildSettings':app if role=='Meguri' else other}]
   if self.unexpected_role==role:targets.append({'target':'MeguriUITests','buildSettings':{}})
   if self.target_shape is not None:
    target,phase,duplicate=self.target_shape
    if custom==phase:
     selected=[row for row in targets if row['target']==target]
     targets=targets+selected if duplicate else [row for row in targets if row['target']!=target]
   out=json.dumps(targets).encode()
  elif args[0]=='/usr/sbin/system_profiler':out=json.dumps({'SPHardwareDataType':[{'provisioning_UDID':fixtures.FIXTURE_VM_ID}]}).encode()
  elif 'find-identity' in args:
   certs=self.profile['DeveloperCertificates'] if self.two_identities else self.profile['DeveloperCertificates'][:1]
   out=''.join(str(n)+') '+hashlib.sha1(cert).hexdigest()+' "Apple Development: Generated (generated-person-id)"\n'
    for n,cert in enumerate(certs,1)).encode()
  else:out=plistlib.dumps(self.profile)
  return subprocess.CompletedProcess(args,0,out,b'')
 def test_exact_installed_profile_and_current_requested_grants_resolve_private_binding(self):
  value=m.resolve(str(self.root),str(self.input))
  self.assertEqual(value,{'file':str(self.input),'uuid':UUID,'sha256':m.v.digest(self.input.read_bytes()),'bytes':len(self.input.read_bytes()),'source_commit':fixtures.SHA})
 def test_changed_cache_wrong_host_grants_and_other_target_style_refuse(self):
  self.cache.write_bytes(b'other')
  with self.assertRaises(m.v.Refused):m.resolve(str(self.root),str(self.input))
  self.cache.write_bytes(self.input.read_bytes())
  self.profile['ProvisionedDevices']=['generated-other-device']
  with self.assertRaises(m.v.Refused):m.resolve(str(self.root),str(self.input))
  self.profile=copy.deepcopy(self.f.profile);self.profile['UUID']=UUID
  self.profile['Entitlements'].pop('com.apple.developer.icloud-services')
  with self.assertRaises(m.v.Refused):m.resolve(str(self.root),str(self.input))
  self.profile=copy.deepcopy(self.f.profile);self.profile['UUID']=UUID;self.changed_other=True
  with self.assertRaises(m.v.Refused):m.resolve(str(self.root),str(self.input))
 def test_named_queries_reject_missing_or_duplicate_each_named_role_in_either_phase(self):
  for target in ('Meguri','MeguriWidget','MeguriSnapshotTests'):
   for custom in (False,True):
    for duplicate in (False,True):
     self.target_shape=(target,custom,duplicate)
     with self.subTest(target=target,custom=custom,duplicate=duplicate):
      with self.assertRaisesRegex(m.v.Refused,'profile-target-scope'):
       m.resolve(str(self.root),str(self.input))
 def test_named_queries_reject_unexpected_role_in_each_query(self):
  for role in ('Meguri','MeguriWidget','MeguriSnapshotTests'):
   self.unexpected_role=role
   with self.assertRaisesRegex(m.v.Refused,'profile-target-scope'):
    m.resolve(str(self.root),str(self.input))
 def test_named_target_metadata_mismatch_refuses_even_with_one_row(self):
  original=self.command
  for key,bad in [('TARGET_NAME','GeneratedOther'),('PRODUCT_BUNDLE_IDENTIFIER','generated.other'),
                  ('PLATFORM_NAME','iphonesimulator'),('CONFIGURATION','Release'),('MEGURI_SIGNING','adhoc')]:
   def changed(args,category):
    result=original(args,category)
    if args[0]=='/usr/bin/xcodebuild':
     rows=json.loads(result.stdout);rows[0]['buildSettings'][key]=bad
     return subprocess.CompletedProcess(args,0,json.dumps(rows).encode(),b'')
    return result
   with patch.object(m.v,'command',side_effect=changed):
    with self.assertRaisesRegex(m.v.Refused,'profile-target-scope'):m.resolve(str(self.root),str(self.input))
 def test_transient_original_path_substitution_cannot_change_decoded_authority(self):
  admitted=self.input.read_bytes()
  def substitute(args,*,input,capture_output,timeout):
   if args[0]!='/usr/bin/security':return self.decode(args,input=input,capture_output=capture_output,timeout=timeout)
   self.assertEqual(args,['/usr/bin/security','cms','-D'])
   self.assertEqual(input,admitted)
   # A pathname decoder would inspect this different profile. The stdin
   # decoder consumes only admitted bytes even during transient substitution.
   self.input.write_bytes(b'generated-substituted-profile')
   try:
    result=subprocess.CompletedProcess(args,0,plistlib.dumps(self.profile),b'')
   finally:self.input.write_bytes(admitted)
   return result
  with patch.object(m.subprocess,'run',side_effect=substitute) as native:
   value=m.resolve(str(self.root),str(self.input))
  self.assertEqual(sum(call.args[0][0]=='/usr/bin/security' for call in native.call_args_list),1)
  self.assertEqual(value['sha256'],m.v.digest(admitted))
  self.assertEqual(value['uuid'],UUID)
 def test_persistent_original_path_substitution_still_refuses(self):
  def substitute(args,**kwargs):
   if args[0]!='/usr/bin/security':return self.decode(args,**kwargs)
   result=self.decode(args,**kwargs)
   self.input.write_bytes(b'generated-substituted-profile')
   return result
  with patch.object(m.subprocess,'run',side_effect=substitute):
   with self.assertRaisesRegex(m.v.Refused,'profile-changed'):
    m.resolve(str(self.root),str(self.input))
 def test_native_decode_refusal_timeout_and_oversize_output_are_bounded(self):
  for result in (subprocess.CompletedProcess([],1,b'',b'generated-native-detail'),
                 subprocess.CompletedProcess([],0,b'x'*(16*1024*1024+1),b'')):
   with patch.object(m.subprocess,'run',return_value=result):
    with self.assertRaisesRegex(m.v.Refused,'profile-input'):m.decode_profile(b'generated-approved-profile')
  with patch.object(m.subprocess,'run',side_effect=subprocess.TimeoutExpired([],60)):
   with self.assertRaisesRegex(m.v.Refused,'profile-input'):m.decode_profile(b'generated-approved-profile')
 def test_certificate_der_team_development_cn_expiry_and_ambiguity_remain_strict(self):
  for field,value in [('certificate_team','OTHER12345'),('other_certificate_cn',True),('expired_certificate',True)]:
   original=getattr(self,field);setattr(self,field,value)
   with self.subTest(field=field):
    with self.assertRaisesRegex(m.v.Refused,'profile-signer'):m.resolve(str(self.root),str(self.input))
   setattr(self,field,original)
  self.profile['DeveloperCertificates'].append(b'generated-second-private-key-certificate')
  self.two_identities=True
  with self.assertRaisesRegex(m.v.Refused,'profile-signer'):m.resolve(str(self.root),str(self.input))
 def test_unresolved_macro_refuses(self):
  with self.assertRaises(m.v.Refused):m.expand({'key':'$(UNSET)'},{})

if __name__=='__main__':unittest.main()
