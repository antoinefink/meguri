#!/usr/bin/env python3
"""Internal app-only Mac CloudAgent profile binding; stdout is private input."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import re
import sys
import subprocess

_spec=importlib.util.spec_from_file_location('cloud_mac_profile_verify',Path(__file__).with_name('cloud-agent-verify.py'))
v=importlib.util.module_from_spec(_spec);sys.modules[_spec.name]=v;_spec.loader.exec_module(v)
need=v.need


def expand(value,settings):
    if isinstance(value,dict):return {key:expand(item,settings) for key,item in value.items()}
    if isinstance(value,list):return [expand(item,settings) for item in value]
    if not isinstance(value,str):return value
    def replacement(match):
        key=match.group(1) or match.group(2)
        need(key in settings and isinstance(settings[key],str),'profile-settings')
        return settings[key]
    result=re.sub(r'\$\(([^)]+)\)|\$\{([^}]+)\}',replacement,value)
    need('$(' not in result and '${' not in result,'profile-settings')
    return result


def certificate_team(certificate,team):
    # Display labels may end in a person identifier. Team authority is in DER.
    def inspect(arguments):
        try:
            value=subprocess.run(['/usr/bin/openssl','x509','-inform','DER',*arguments],
                input=certificate,capture_output=True,timeout=60)
        except (OSError,subprocess.SubprocessError):
            raise v.Refused('profile-signer') from None
        need(value.returncode==0 and len(value.stdout)+len(value.stderr)<=1024*1024,'profile-signer')
        return value.stdout
    subject=inspect(['-noout','-subject','-nameopt','RFC2253']).decode().strip().replace('subject=','',1).strip()
    inspect(['-noout','-checkend','0'])
    return (re.findall(r'(?:^|,)OU=([A-Z0-9]{10})(?=,|$)',subject)==[team] and
            re.search(r'(?:^|,)CN=Apple Development:',subject) is not None)


def private_identity(profile,team):
    raw=v.command(['/usr/bin/security','find-identity','-v','-p','codesigning'],'profile-signer').stdout.decode()
    hashes={sha.upper() for sha,_ in re.findall(r'^\s*\d+\)\s+([A-Fa-f0-9]{40})\s+"(Apple Development:[^"\n]+)"\s*$',raw,re.M)}
    certificates=profile.get('DeveloperCertificates',[])
    need(isinstance(certificates,list) and all(isinstance(c,bytes) for c in certificates),'profile-signer')
    choices=[c for c in certificates if hashlib.sha1(c).hexdigest().upper() in hashes and certificate_team(c,team)]
    need(len(choices)==1,'profile-signer')
    return choices[0]


def decode_profile(raw):
    # Decode exactly the bytes admitted above, without reopening a mutable path.
    need(type(raw) is bytes and len(raw)<=16*1024*1024,'profile-input')
    try:
        decoded=subprocess.run(['/usr/bin/security','cms','-D'],input=raw,
            capture_output=True,timeout=60)
    except (OSError,subprocess.SubprocessError):
        raise v.Refused('profile-input') from None
    need(decoded.returncode==0 and len(decoded.stdout)+len(decoded.stderr)<=16*1024*1024,'profile-input')
    return v.plist(decoded.stdout,'profile-input')


def resolve(root,path):
    sha=v.source(root)
    path=os.path.abspath(path)
    raw=v._first_sync_bytes(os.path.dirname(path),path)
    profile=decode_profile(raw)
    uuid=profile.get('UUID')
    need(isinstance(uuid,str) and re.fullmatch(r'[0-9A-Fa-f-]{36}',uuid),'profile-input')
    cached=[];cache_paths=[]
    for cache in (Path.home()/'Library/Developer/Xcode/UserData/Provisioning Profiles',Path.home()/'Library/MobileDevice/Provisioning Profiles'):
        for suffix in ('.provisionprofile','.mobileprovision'):
            candidate=cache/(uuid+suffix)
            if candidate.exists():
                cached.append(v._first_sync_bytes(str(cache),str(candidate)));cache_paths.append((str(cache),str(candidate)))
    need(cached and all(value==raw for value in cached),'profile-cache')
    # Query each named target alone. Other native queries may repeat a target.
    derived=os.path.join(root,'build/app/CloudAgentMacDerivedData/Build')
    command=['/usr/bin/xcodebuild','-showBuildSettings','-json','-project','app/Meguri.xcodeproj',
             '-configuration','Debug','-sdk','macosx','SYMROOT='+derived+'/Products',
             'OBJROOT='+derived+'/Intermediates.noindex','MEGURI_SIGNING=team']
    overrides=['MEGURI_MAC_AGENT_PROFILE_STYLE=Manual','MEGURI_MAC_AGENT_PROFILE_SPECIFIER='+uuid]
    settings=[];baseline=[]
    roles=(('Meguri','com.antoinefink.meguri.dev'),('MeguriWidget','com.antoinefink.meguri.dev.widget'),
           ('MeguriSnapshotTests','com.antoinefink.meguri.dev.snapshottests'))
    for role,identifier in roles:
        for output,extra in ((settings,overrides),(baseline,[])):
            values=json.loads(v.command(command+['-target',role]+extra,'profile-settings').stdout)
            need(isinstance(values,list) and len(values)==1 and isinstance(values[0],dict)
                 and values[0].get('target')==role and isinstance(values[0].get('buildSettings'),dict),'profile-target-scope')
            value=values[0]['buildSettings']
            need(value.get('TARGET_NAME')==role and value.get('PRODUCT_BUNDLE_IDENTIFIER')==identifier
                 and value.get('PLATFORM_NAME')=='macosx' and value.get('CONFIGURATION')=='Debug'
                 and value.get('MEGURI_SIGNING')=='team','profile-target-scope')
            output.extend(values)
    apps=[x['buildSettings'] for x in settings if x.get('target')=='Meguri']
    need(len(apps)==1,'profile-settings');app=apps[0]
    need(app.get('CONFIGURATION')=='Debug' and app.get('PLATFORM_NAME')=='macosx'
         and app.get('MEGURI_SIGNING')=='team' and app.get('CODE_SIGN_STYLE')=='Manual'
         and app.get('PROVISIONING_PROFILE_SPECIFIER')==uuid
         and app.get('PRODUCT_BUNDLE_IDENTIFIER')=='com.antoinefink.meguri.dev','profile-settings')
    for target in ('MeguriWidget','MeguriSnapshotTests'):
        previous=[x['buildSettings'] for x in baseline if x.get('target')==target]
        current=[x['buildSettings'] for x in settings if x.get('target')==target]
        need(len(previous)==len(current)==1,'profile-target-scope')
        for key in ('CODE_SIGN_STYLE','PROVISIONING_PROFILE_SPECIFIER'):
            need((key in previous[0],previous[0].get(key))==(key in current[0],current[0].get(key)),'profile-target-scope')
    team=app.get('DEVELOPMENT_TEAM');need(isinstance(team,str) and re.fullmatch(r'[A-Z0-9]{10}',team),'profile-settings')
    ent_path=app.get('CODE_SIGN_ENTITLEMENTS');need(isinstance(ent_path,str) and '$' not in ent_path,'profile-settings')
    ent_path=os.path.normpath(os.path.join(root,'app',ent_path))
    v.contained(os.path.join(root,'app'),ent_path)
    ent_raw=v._first_sync_bytes(os.path.join(root,'app'),ent_path)
    substitutions=dict(app)
    for prefix in ('AppIdentifierPrefix','TeamIdentifierPrefix'):
        need(not substitutions.get(prefix) or substitutions[prefix]==team+'.','profile-settings')
        substitutions[prefix]=team+'.'
    ent=expand(v.plist(ent_raw,'profile-entitlements'),substitutions)
    v.check_cloud(app,'Debug','mac','team',entitlements=ent)
    # These are the native development identity facts checked by profile_check,
    # not writes to an entitlement source, profile or built product.
    ent['com.apple.developer.team-identifier']=team
    ent['com.apple.application-identifier']=team+'.com.antoinefink.meguri.dev'
    hardware=json.loads(v.command(['/usr/sbin/system_profiler','SPHardwareDataType','-json'],'profile-host').stdout)['SPHardwareDataType'][0]
    host=hardware.get('provisioning_UDID')
    need(isinstance(host,str) and host and not re.search(r'[\s\x00]',host),'profile-host')
    v.profile_check(profile,ent,private_identity(profile,team),host,'mac')
    need(all(v._first_sync_bytes(base,candidate)==raw for base,candidate in cache_paths) and
         v._first_sync_bytes(os.path.dirname(path),path)==raw and
         v._first_sync_bytes(os.path.join(root,'app'),ent_path)==ent_raw and v.source(root)==sha,'profile-changed')
    return {'file':path,'uuid':uuid,'sha256':v.digest(raw),'bytes':len(raw),'source_commit':sha}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',required=True);p.add_argument('--file',required=True);p.add_argument('--check',action='store_true')
    a=p.parse_args()
    try:
        value=resolve(a.root,a.file)
        if a.check:
            expected=json.load(sys.stdin);need(value==expected,'profile-changed')
        else:print(json.dumps(value,sort_keys=True))
        return 0
    except Exception as error:
        allowed={'source-dirty','source-changed','profile-input','profile-cache','profile-settings','profile-target-scope',
          'profile-entitlements','profile-host','profile-signer','profile-team','profile-identity','profile-device',
          'profile-expired','profile-development','profile-grants','profile-signer-platform','profile-changed'}
        code=error.args[0] if isinstance(error,v.Refused) and len(error.args)==1 and type(error.args[0]) is str and error.args[0] in allowed else 'guarded-input'
        print('App-only Mac CloudAgent profile validation refused: '+code,file=sys.stderr)
        return 1


if __name__=='__main__':sys.exit(main())
