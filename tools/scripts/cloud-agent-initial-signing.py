#!/usr/bin/env python3
"""Initial simulator CloudAgent signing, before the producer's first freeze."""
import argparse
import contextlib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import ssl
import stat
import struct
import sys
import tempfile

_spec = importlib.util.spec_from_file_location('cloud_initial_verify', Path(__file__).with_name('cloud-agent-verify.py'))
v = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = v
_spec.loader.exec_module(v)
need = v.need
_stage_spec = importlib.util.spec_from_file_location('cloud_initial_native', Path(__file__).with_name('vm-xcode-stage.py'))
stage = importlib.util.module_from_spec(_stage_spec)
_stage_spec.loader.exec_module(stage)


def signing_command(args):
    # The existing supervisor defers launch cancellation and joins the exact
    # original process group before this call can release product authority.
    env=dict(os.environ)
    for key in ('CODESIGN_ALLOCATE','DEVELOPER_DIR','SDKROOT','TOOLCHAINS'):
        env.pop(key,None)
    stage.command(args, timeout=60, env=env)


def code_bytes(parent,name):
    fd=os.open(name,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK,dir_fd=parent)
    try:
        info=os.fstat(fd);pin=v.identity(info)
        need(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and 0<info.st_size<=512*1024*1024,'initial-code')
        pieces=[];count=0
        while count<info.st_size:
            raw=os.read(fd,min(1024*1024,info.st_size-count))
            need(raw,'initial-code');pieces.append(raw);count+=len(raw)
        need(v.identity(os.fstat(fd))==pin and
             v.identity(os.stat(name,dir_fd=parent,follow_symlinks=False))==pin,'initial-code')
        return b''.join(pieces)
    finally:os.close(fd)


def code_fingerprint(raw):
    # Apple mach-o/loader.h and fat.h: only 64-bit native simulator slices.
    # Retain all code/data and load commands except trailing signature sizes.
    # Preserve exact LINKEDIT virtual size until native changes are measured.
    def thin(data):
        need(len(data)>=32 and data[:4]==b'\xcf\xfa\xed\xfe','initial-code')
        _,cpu,subtype,_,ncmds,sizeofcmds,_,_=struct.unpack_from('<8I',data)
        need(0<ncmds<=1024 and 0<sizeofcmds<=1024*1024 and 32+sizeofcmds<=len(data),'initial-code')
        prefix=bytearray(data);offset=32;signature=None;linkedit=None
        for _ in range(ncmds):
            need(offset+8<=32+sizeofcmds,'initial-code')
            command,size=struct.unpack_from('<II',data,offset)
            need(size>=8 and size%8==0 and offset+size<=32+sizeofcmds,'initial-code')
            if command==0x1d:  # LC_CODE_SIGNATURE / linkedit_data_command
                need(signature is None and size==16,'initial-code')
                start,length=struct.unpack_from('<II',data,offset+8)
                need(start>=32+sizeofcmds and length>0 and start+length==len(data),'initial-code')
                signature=start;prefix[offset+12:offset+16]=b'\0'*4
            elif command==0x19 and data[offset+8:offset+24].rstrip(b'\0')==b'__LINKEDIT':
                need(linkedit is None and size>=72,'initial-code')
                fileoff,filesize=struct.unpack_from('<QQ',data,offset+40)
                need(fileoff+filesize==len(data),'initial-code')
                linkedit=fileoff
                prefix[offset+48:offset+56]=b'\0'*8
            offset+=size
        need(offset==32+sizeofcmds and signature is not None and linkedit is not None
             and linkedit<=signature,'initial-code')
        return cpu,subtype,hashlib.sha256(prefix[:signature]).hexdigest()
    if raw[:4]==b'\xcf\xfa\xed\xfe':return (thin(raw),)
    formats={b'\xca\xfe\xba\xbe':('>',False),b'\xbe\xba\xfe\xca':('<',False),
             b'\xca\xfe\xba\xbf':('>',True),b'\xbf\xba\xfe\xca':('<',True)}
    need(len(raw)>=8 and raw[:4] in formats,'initial-code')
    endian,wide=formats[raw[:4]];count=struct.unpack_from(endian+'I',raw,4)[0]
    size=32 if wide else 20;end=8+count*size
    need(0<count<=8 and end<=len(raw),'initial-code')
    slices=[];last=end
    for n in range(count):
        values=struct.unpack_from(endian+('IIQQII' if wide else 'IIIII'),raw,8+n*size)
        cpu,subtype,offset,length,align=values[:5]
        need((not wide or values[5]==0) and align<=30 and offset%(1<<align)==0
             and length>0 and offset>=end and offset+length<=len(raw),'initial-code')
        slices.append((offset,length,cpu,subtype,align))
    result=[]
    for offset,length,cpu,subtype,align in sorted(slices):
        need(offset>=last and not any(raw[last:offset]),'initial-code')
        value=thin(raw[offset:offset+length]);need(value[:2]==(cpu,subtype),'initial-code')
        result.append((*value,align));last=offset+length
    need(not any(raw[last:]) and len({x[:2] for x in result})==count,'initial-code')
    return tuple(sorted(result))


def native_plan(products, app, agent, entries):
    app_path = os.path.join(products, app['path'])
    agent_path = os.path.join(products, agent['path'])
    widget, info = v.bundle(products, app['path'] + '/PlugIns/MeguriWidget.appex', 'ios-sim')
    need(widget['bundle_id'] == 'com.antoinefink.meguri.dev.widget' and widget['executable']=='MeguriWidget'
         and info.get('CFBundlePackageType') == 'XPC!'
         and info.get('CFBundleSupportedPlatforms') == ['iPhoneSimulator'], 'initial-widget')
    widget_path = os.path.join(products, widget['path'])
    paths = [app_path + '/Frameworks/libXCTestBundleInject.dylib']
    for name in ('Meguri.debug.dylib', '__preview.dylib'):
        path = app_path + '/' + name
        if os.path.lexists(path):
            paths.append(path)
    # These are the exact additional outputs of the native widget target.
    for name in ('MeguriWidget.debug.dylib', '__preview.dylib'):
        path = widget_path + '/' + name
        if os.path.lexists(path):
            paths.append(path)
    paths += [widget_path, agent_path, app_path]
    bundles = {app_path: app, agent_path: agent, widget_path: widget}
    known = {os.path.relpath(path, products) for path in paths}
    for path in paths:
        entry = entries.get(os.path.relpath(path, products))
        need(entry is not None and entry['type'] == ('directory' if path in bundles else 'file'), 'initial-unknown-code')
    for path in entries:
        if path.startswith(app['path'] + '/') and path.endswith(('.dylib', '.framework', '.appex', '.app', '.xctest')):
            need(path in known, 'initial-unknown-code')
    return paths, bundles


def choose_identity(team):
    """Use actual certificate team authority, not the display label suffix."""
    raw = v.command(['/usr/bin/security', 'find-identity', '-v', '-p', 'codesigning'], 'initial-signer').stdout.decode()
    matches = re.findall(r'^\s*\d+\)\s+([A-Fa-f0-9]{40})\s+"(Apple Development:[^"\n]+)"\s*$', raw, re.M)
    hashes = {sha.upper() for sha, _ in matches}
    need(hashes and len(hashes) == len(matches), 'initial-signer-ambiguous')
    pem = v.command(['/usr/bin/security', 'find-certificate', '-a', '-p'], 'initial-certificate').stdout.decode()
    certificates = {ssl.PEM_cert_to_DER_cert(block) for block in
                    re.findall(r'-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----', pem, re.S)}
    selected = [(hashlib.sha1(cert).hexdigest().upper(), cert) for cert in certificates
                if hashlib.sha1(cert).hexdigest().upper() in hashes]
    choices = []
    with tempfile.TemporaryDirectory(prefix='meguri-initial-cert-') as temporary:
        for number, (sha, certificate) in enumerate(selected):
            path = os.path.join(os.path.realpath(temporary), 'certificate' + str(number))
            with open(path, 'xb') as stream:
                stream.write(certificate)
            subject = v.command(['/usr/bin/openssl', 'x509', '-inform', 'DER', '-in', path,
                                 '-noout', '-subject', '-nameopt', 'RFC2253'], 'initial-certificate').stdout.decode().strip()
            subject = subject.replace('subject=', '', 1).strip()
            if (re.findall(r'(?:^|,)OU=([A-Z0-9]{10})(?=,|$)', subject) == [team] and
                    re.search(r'(?:^|,)CN=Apple Development:', subject)):
                v.command(['/usr/bin/openssl', 'x509', '-inform', 'DER', '-in', path,
                           '-checkend', '0', '-noout'], 'initial-certificate')
                choices.append((sha, certificate))
    need(len(choices) == 1, 'initial-signer-ambiguous')
    return choices[0]


def signing_metadata(path):
    lines = v.command(['/usr/bin/codesign', '-d', '--verbose=4', path], 'initial-metadata').stderr.decode().splitlines()
    identifiers = [x.split('=', 1)[1] for x in lines if x.startswith('Identifier=')]
    need(len(identifiers) == 1 and identifiers[0], 'initial-metadata')
    raw = v.command(['/usr/bin/codesign', '-d', '--entitlements', '-', '--xml', path], 'initial-entitlements').stdout
    return identifiers[0], None if not raw else v.plist(raw, 'initial-entitlements')


def compare_product_files(before, after, executable, signature_root, native_result):
    entries, pins = before
    final, new_pins = after
    for path in set(entries) | set(final):
        mutable = path == executable or signature_root is not None and (path == signature_root or path.startswith(signature_root + '/'))
        if not mutable:
            need(entries.get(path) == final.get(path) and pins.get(path) == new_pins.get(path), 'initial-product-changed')
        elif path == executable:
            # Only the just-joined native signing transaction may replace this
            # one known executable. Bind its fresh no-follow result through the
            # retained parent; all other file identities remain unchanged.
            need(path in entries and path in final and entries[path]['type'] == final[path]['type'] == 'file'
                 and entries[path]['mode'] == final[path]['mode'] and pins[path][0] == new_pins[path][0]
                 and new_pins[path][3] == 1 and new_pins[path] == native_result,
                 'initial-executable-replaced')
        else:
            need(path not in final or final[path]['type'] in ('file', 'directory'), 'initial-product-changed')


def initial_sign(root, directory, sha, products, xctestrun, settings_path):
    need(v.source(root) == sha, 'source-changed')
    run = os.path.basename(directory)
    need(v.MG_RUN_ID.fullmatch(run) and directory == os.path.join(root, 'build/mg/build-app', run), 'initial-run')
    need(not any(os.path.lexists(os.path.join(directory, x)) for x in ('agent-build.json', 'agent-build-reference.json')),
         'initial-already-frozen')
    need(products == os.path.join(root, 'build/app/CloudAgentSimDerivedData/Build/Products'), 'initial-products')
    v.contained(products, xctestrun)
    settings_pin = v.binary(os.path.join(root, 'build'), settings_path)
    settings = v.json_input(os.path.join(root, 'build'), settings_path)
    target_settings = {}
    for target in ('Meguri', 'MeguriSnapshotTests'):
        rows = [x['buildSettings'] for x in settings if x.get('target') == target]
        need(len(rows) == 1, 'initial-settings')
        value = rows[0]
        need(value.get('CONFIGURATION') == 'Debug' and value.get('PLATFORM_NAME') == 'iphonesimulator'
             and value.get('MEGURI_SIGNING') == 'team' and value.get('CODE_SIGN_IDENTITY') == 'Apple Development'
             and isinstance(value.get('DEVELOPMENT_TEAM'), str)
             and re.fullmatch(r'[A-Z0-9]{10}', value['DEVELOPMENT_TEAM']), 'initial-settings')
        target_settings[target] = value
    team = target_settings['Meguri']['DEVELOPMENT_TEAM']
    need(target_settings['MeguriSnapshotTests']['DEVELOPMENT_TEAM'] == team, 'initial-settings')
    app, info = v.bundle(products, 'Debug-iphonesimulator/Meguri.app', 'ios-sim')
    need(app['bundle_id'] == 'com.antoinefink.meguri.dev', 'initial-settings')
    agent = v.test_inputs(products, xctestrun, app, 'ios-sim')
    need(all(target_settings[target].get('PRODUCT_BUNDLE_IDENTIFIER') == item['bundle_id']
             for target, item in (('Meguri', app), ('MeguriSnapshotTests', agent))), 'initial-settings')
    app_path = os.path.join(products, app['path'])
    agent_path = os.path.join(products, agent['path'])
    before = v.snapshot(os.path.join(root, 'build'), products)
    paths, bundles = native_plan(products, app, agent, before[0])
    signer, certificate = choose_identity(team)
    with contextlib.ExitStack() as held:
        witnesses, executable_parents = [], {}
        for path in paths:
            witness = {}
            held.enter_context(v.authority(path if path in bundles else os.path.dirname(path), witness))
            witnesses.append(witness)
            item = bundles.get(path)
            executable_path = path if item is None else os.path.join(products, item['path'], item['executable'])
            executable_parents[path] = held.enter_context(v.authority(os.path.dirname(executable_path)))
        for path in paths:
            for witness in witnesses:
                witness['check']()
            need(v.source(root) == sha and v.snapshot(os.path.join(root, 'build'), products) == before
                 and v.binary(os.path.join(root, 'build'), settings_path) == settings_pin, 'initial-product-changed')
            identifier, entitlements = signing_metadata(path)
            item = bundles.get(path)
            need(item is None or identifier == item['bundle_id'], 'initial-metadata')
            executable = os.path.relpath(path, products) if item is None else item['path'] + '/' + item['executable']
            signature_root = None if item is None else item['path'] + '/_CodeSignature'
            parent=executable_parents[path];name=os.path.basename(executable)
            original_code=code_fingerprint(code_bytes(parent,name))
            signing_command(['/usr/bin/codesign', '--force', '--sign', signer, '--timestamp=none',
                             '--preserve-metadata=identifier,entitlements', path])
            for witness in witnesses:
                witness['check']()
            native_result = v.identity(os.stat(os.path.basename(executable), dir_fd=executable_parents[path], follow_symlinks=False))
            after = v.snapshot(os.path.join(root, 'build'), products)
            compare_product_files(before, after, executable, signature_root, native_result)
            need(code_fingerprint(code_bytes(parent,name))==original_code,'initial-code-changed')
            need(signing_metadata(path) == (identifier, entitlements), 'initial-entitlements-changed')
            v.command(['/usr/bin/codesign', '--verify', '--deep', '--strict', path], 'initial-signature')
            with tempfile.TemporaryDirectory(prefix='meguri-initial-signed-') as temporary:
                temporary = os.path.realpath(temporary)
                prefix = temporary + '/certificate'
                v.command(['/usr/bin/codesign', '-d', '--extract-certificates=' + prefix, path], 'initial-signature')
                need(v._first_sync_bytes(temporary, prefix + '0') == certificate, 'initial-signature')
            before = after
        expected = {'device_kind': 'iphone', 'executable_sha256': v.binary(products, app_path + '/' + app['executable'])[0]['sha256'],
                    'app_build': info['CFBundleVersion'], 'container': info['MeguriCloudContainer'], 'environment': info['MeguriCloudEnvironment']}
        need(v.independent_product(root, app_path, expected, target_settings['Meguri']) == team, 'initial-signature')
        v.independent_agent(root, {'platform': 'ios-sim', 'products': {'path': products}, 'agent': v.bundle(products, agent['path'], 'ios-sim')[0]}, team)
        need(v.snapshot(os.path.join(root, 'build'), products) == before and v.source(root) == sha
             and v.binary(os.path.join(root, 'build'), settings_path) == settings_pin, 'initial-product-changed')


def main():
    for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(number, lambda *_: (_ for _ in ()).throw(stage.Cancelled()))
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'run-directory', 'source-commit', 'products', 'xctestrun', 'settings'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    try:
        initial_sign(args.root, args.run_directory, args.source_commit, args.products, args.xctestrun, args.settings)
        return 0
    except (v.Refused, v.Invalid, stage.Refused, stage.Cancelled, OSError, ValueError, TypeError, KeyError, AttributeError) as error:
        allowed = {'source-changed', 'source-dirty', 'initial-run', 'initial-already-frozen', 'initial-products',
            'initial-settings', 'initial-widget', 'initial-code', 'initial-code-changed', 'initial-unknown-code', 'initial-signer', 'initial-signer-ambiguous',
            'initial-certificate', 'initial-metadata', 'initial-entitlements', 'initial-product-changed',
            'initial-executable-replaced', 'initial-signing', 'initial-entitlements-changed', 'initial-signature',
            'product-signature', 'product-cloud-grants', 'product-debug', 'product-entitlements',
            'agent-signature', 'agent-platform', 'input-changed'}
        code = error.args[0] if isinstance(error, v.Refused) and len(error.args) == 1 and type(error.args[0]) is str and error.args[0] in allowed else 'guarded-input'
        print('Initial simulator development signing refused: ' + code + '; no frozen proof created', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
