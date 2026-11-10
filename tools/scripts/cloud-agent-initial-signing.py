#!/usr/bin/env python3
"""Initial simulator CloudAgent signing, before the producer's first freeze."""
import argparse
import contextlib
import hashlib
import importlib.util
import json
import os
import plistlib
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
    previous = os.umask(0o022)
    try:
        stage.command(args, timeout=60, env=env)
    finally:
        os.umask(previous)


def prepare_install_modes(boundary, products, app_path, original):
    """Use native simulator bundle permissions inside the generated app only."""
    entries, pins = original
    expected = dict(entries)
    with v.authority(os.path.join(products, app_path)):
        for path, entry in entries.items():
            if not path.startswith(app_path + '/'):
                continue
            mode = entry.get('mode')
            standard = {0o600: 0o644, 0o700: 0o755}.get(mode)
            if standard is None:
                continue
            directory = entry['type'] == 'directory'
            identity = v.directory_identity if directory else v.identity
            parent, name = os.path.split(os.path.join(products, path))
            flags = os.O_RDONLY | os.O_NOFOLLOW | (os.O_DIRECTORY if directory else os.O_NONBLOCK)
            with v.authority(parent) as fd:
                child = os.open(name, flags, dir_fd=fd)
                try:
                    opened = os.fstat(child)
                    need(opened.st_uid == os.getuid() and (directory or opened.st_nlink == 1) and
                         identity(opened) == pins[path] ==
                         identity(os.stat(name, dir_fd=fd, follow_symlinks=False)), 'initial-install-modes')
                    os.fchmod(child, standard)
                    need(identity(os.fstat(child)) == identity(os.stat(name, dir_fd=fd, follow_symlinks=False)),
                         'initial-install-modes')
                finally:
                    os.close(child)
            expected[path] = {**entry, 'mode': standard}
    if expected == entries:
        return original
    after = v.snapshot(boundary, products)
    need(after[0] == expected and all(
        after[1][path][:2] == pin[:2] if expected.get(path) != entries.get(path) else after[1][path] == pin
        for path, pin in pins.items()), 'initial-install-modes')
    return after


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


def code_difference(before, after):
    """Classify exact transaction inputs without recording their code bytes."""
    def slices(raw):
        if raw[:4] == b'\xcf\xfa\xed\xfe':
            return [(0, len(raw), raw)]
        formats = {b'\xca\xfe\xba\xbe': ('>', False), b'\xbe\xba\xfe\xca': ('<', False),
                   b'\xca\xfe\xba\xbf': ('>', True), b'\xbf\xba\xfe\xca': ('<', True)}
        endian, wide = formats[raw[:4]]
        count = struct.unpack_from(endian + 'I', raw, 4)[0]
        result = []
        for index in range(count):
            values = struct.unpack_from(endian + ('IIQQII' if wide else 'IIIII'), raw,
                                        8 + index * (32 if wide else 20))
            offset, size = values[2:4]
            result.append((offset, size, raw[offset:offset + size]))
        return result

    def describe(value):
        offset, size, raw = value
        head = struct.unpack_from('<8I', raw)
        cursor, fields, values = 32, {}, {}
        for _ in range(head[4]):
            command, length = struct.unpack_from('<II', raw, cursor)
            if command == 0x1d:
                fields['signature_offset'] = (cursor + 8, 4)
                fields['signature_size'] = (cursor + 12, 4)
            elif command == 0x19 and raw[cursor + 8:cursor + 24].rstrip(b'\0') == b'__LINKEDIT':
                for name, displacement in (('linkedit_vmaddr', 24), ('linkedit_vmsize', 32),
                                           ('linkedit_fileoff', 40), ('linkedit_filesize', 48)):
                    fields[name] = (cursor + displacement, 8)
            cursor += length
        for name, (position, width) in fields.items():
            values[name] = int.from_bytes(raw[position:position + width], 'little')
        return {'cpu': head[1], 'subtype': head[2], 'fat_offset': offset,
                'slice_bytes': size, 'fields': values}, fields, raw

    # The caller has already applied the unchanged strict parser to both inputs.
    old, new = [describe(value) for value in slices(before)], [describe(value) for value in slices(after)]
    report = {'before_sha256': hashlib.sha256(before).hexdigest(),
              'after_sha256': hashlib.sha256(after).hexdigest(),
              'slice_counts': [len(old), len(new)], 'slices': []}
    for left, right in zip(old, new):
        a, af, ar = left
        b, bf, br = right
        limit = min(a['fields']['signature_offset'], b['fields']['signature_offset'])
        covered = set()
        changed = {}
        for name, (position, width) in af.items():
            changed[name] = a['fields'][name] != b['fields'].get(name)
            if bf.get(name) == (position, width):
                covered.update(range(position, position + width))
        count, first = 0, []
        for index in range(limit):
            if index not in covered and ar[index] != br[index]:
                count += 1
                if len(first) < 32:
                    first.append(index)
        report['slices'].append({'before': a, 'after': b, 'changed_fields': changed,
                                 'other_changed_byte_count': count,
                                 'other_changed_offsets_first32': first})
    return report


def signature_allocation_matches(before, after):
    """Accept only the measured pairwise 16KiB signature-allocation growth."""
    original = code_fingerprint(before)
    if code_fingerprint(after) == original:
        return True
    if before[:4] != after[:4]:
        return False
    difference = code_difference(before, after)
    if difference['slice_counts'][0] != difference['slice_counts'][1]:
        return False
    adjusted, changed = bytearray(after), False
    for item in difference['slices']:
        old, new = item['before'], item['after']
        a, b = old['fields'], new['fields']
        if ((old['cpu'], old['subtype']) != (new['cpu'], new['subtype']) or
                item['other_changed_byte_count'] != 0 or
                any(a[name] != b.get(name) for name in
                    ('signature_offset', 'linkedit_fileoff', 'linkedit_vmaddr'))):
            return False
        if a['linkedit_vmsize'] == b['linkedit_vmsize']:
            continue
        signature_growth = b['signature_size'] - a['signature_size']
        if not (signature_growth > 0 and
                b['linkedit_filesize'] - a['linkedit_filesize'] == signature_growth and
                b['linkedit_vmsize'] > a['linkedit_vmsize'] and
                all(fields['linkedit_vmsize'] == ((fields['linkedit_filesize'] + 16383) // 16384) * 16384
                    for fields in (a, b))):
            return False
        # Both inputs already passed the unchanged strict Mach-O/fat parser.
        # Restore only this proved field in an in-memory comparison copy.
        base = new['fat_offset']
        cursor = base + 32
        ncmds = struct.unpack_from('<I', after, base + 16)[0]
        for _ in range(ncmds):
            command, size = struct.unpack_from('<II', after, cursor)
            if command == 0x19 and after[cursor + 8:cursor + 24].rstrip(b'\0') == b'__LINKEDIT':
                struct.pack_into('<Q', adjusted, cursor + 32, a['linkedit_vmsize'])
                changed = True
                break
            cursor += size
    return changed and code_fingerprint(bytes(adjusted)) == original


def write_code_difference(directory, target, before, after):
    """Private diagnostic only. Its success never changes the refusing guard."""
    try:
        diagnostic = {'scope': 'INITIAL-SIGNING-EXACT-TRANSACTION-DIAGNOSTIC',
                      'target': v.relative(target), **code_difference(before, after)}
        with v.authority(directory) as parent:
            fd = os.open('initial-code-difference.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o600, dir_fd=parent)
            with os.fdopen(fd, 'w') as stream:
                json.dump(diagnostic, stream, sort_keys=True)
                stream.write('\n')
    except (v.Refused, OSError, ValueError, TypeError, KeyError, IndexError, struct.error):
        # Diagnostics cannot replace or suppress initial-code-changed.
        pass


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


def typed_entitlements(value):
    return plistlib.dumps(value, fmt=plistlib.FMT_BINARY, sort_keys=True)


def app_identity_entitlements(original, team, bundle):
    need(type(original) is dict and isinstance(team, str) and re.fullmatch(r'[A-Z0-9]{10}', team)
         and bundle == 'com.antoinefink.meguri.dev', 'initial-entitlements')
    expected = dict(original)
    identity = team + '.' + bundle
    for key, value in (('com.apple.developer.team-identifier', team),
                       ('application-identifier', identity), ('com.apple.application-identifier', identity)):
        need(key not in original or type(original[key]) is str and original[key] == value,
             'initial-entitlements')
    expected.setdefault('com.apple.developer.team-identifier', team)
    expected.setdefault('application-identifier', identity)
    return expected


def retain_preparation_input(root, path, held, checks):
    v.contained(root, path)
    witness = {}
    parent = held.enter_context(v.authority(os.path.dirname(path), witness))
    name = os.path.basename(path)
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    held.callback(os.close, fd)
    info = os.fstat(fd)
    need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == os.getuid() and
         not info.st_mode & 0o022 and 0 < info.st_size <= 16 * 1024 * 1024, 'initial-entitlements')
    raw = os.pread(fd, info.st_size + 1, 0)
    pin = (v.identity(info), info.st_uid, info.st_gid)
    def check():
        witness['check']()
        current = os.fstat(fd)
        named = os.stat(name, dir_fd=parent, follow_symlinks=False)
        need((v.identity(current), current.st_uid, current.st_gid) == pin ==
             (v.identity(named), named.st_uid, named.st_gid)
             and os.pread(fd, info.st_size + 1, 0) == raw and len(raw) == info.st_size, 'initial-entitlements')
    check()
    checks.append(check)
    return raw


def expand_role_entitlements(value, settings):
    if type(value) is dict:
        return {key: expand_role_entitlements(item, settings) for key, item in value.items()}
    if type(value) is list:
        return [expand_role_entitlements(item, settings) for item in value]
    if type(value) is not str:
        return value
    def replace(match):
        name = match.group(1) or match.group(2)
        need(name in settings and type(settings[name]) is str, 'initial-entitlements')
        return settings[name]
    result = re.sub(r'\$\(([^)]+)\)|\$\{([^}]+)\}', replace, value)
    need('$(' not in result and '${' not in result, 'initial-entitlements')
    return result


def merge_native_request(normal, simulated, requested, role, settings):
    need(type(normal) is dict and type(simulated) is dict and type(requested) is dict and
         role in ('Meguri', 'MeguriWidget'), 'initial-entitlements')
    cloud = {'com.apple.developer.icloud-container-identifiers', 'com.apple.developer.icloud-services',
             'com.apple.developer.icloud-container-environment', 'aps-environment'}
    keys = {'com.apple.security.application-groups'} | (cloud if role == 'Meguri' else set())
    native_requests = dict(simulated)
    if 'application-identifier' in native_requests:
        # Xcode's measured simulator input also carries its role identity
        # (MG22050). Validate that one assertion; never treat it as a grant.
        bundle = 'com.antoinefink.meguri.dev' + ('.widget' if role == 'MeguriWidget' else '')
        team = settings.get('DEVELOPMENT_TEAM')
        native_identity = native_requests.pop('application-identifier')
        need(type(team) is str and re.fullmatch(r'[A-Z0-9]{10}', team) and
             settings.get('PRODUCT_BUNDLE_IDENTIFIER') == bundle and
             type(native_identity) is str and native_identity == team + '.' + bundle,
             'initial-entitlements')
    need(set(requested) == keys and typed_entitlements(native_requests) == typed_entitlements(requested),
         'initial-entitlements')
    groups = requested.get('com.apple.security.application-groups')
    need(type(groups) is list and groups == ['group.com.antoinefink.meguri.dev'] and
         all(type(group) is str for group in groups) and settings.get('MEGURI_APP_GROUP') == groups[0],
         'initial-entitlements')
    expected = dict(normal)
    for key, value in requested.items():
        need(key not in normal or typed_entitlements({key: normal[key]}) == typed_entitlements({key: value}),
             'initial-entitlements')
        expected.setdefault(key, value)
    if role == 'Meguri':
        need(settings.get('CONFIGURATION') == 'Debug' and settings.get('ENABLE_TESTABILITY') == 'YES' and
             settings.get('MEGURI_CLOUD_AGENT_INJECTION') == 'YES' and
             settings.get('MEGURI_SIGNING') == 'team' and settings.get('MEGURI_CORE_FEATURES') == 'ankiweb,test-hooks' and
             {'DEBUG', 'MEGURI_TEST_HOOKS'} <= set(settings.get('SWIFT_ACTIVE_COMPILATION_CONDITIONS', '').split()),
             'initial-settings')
        need('get-task-allow' not in expected or expected['get-task-allow'] is True, 'initial-entitlements')
        need('com.apple.security.get-task-allow' not in expected or
             expected['com.apple.security.get-task-allow'] is True, 'initial-entitlements')
        expected.setdefault('get-task-allow', True)
    else:
        need(not any(v.protected(key) for key in expected) and
             not any(key in expected for key in ('get-task-allow', 'com.apple.security.get-task-allow')),
             'initial-entitlements')
    return expected


def prepare_role_grants(root, products, role, settings, held, checks):
    widget = role == 'MeguriWidget'
    bundle = 'com.antoinefink.meguri.dev' + ('.widget' if widget else '')
    source = ('MeguriWidget/MeguriWidget-iOS.entitlements' if widget else 'Meguri/Meguri-iOS.entitlements')
    product = 'MeguriWidget.appex' if widget else 'Meguri.app'
    temp = os.path.join(root, 'build/app/CloudAgentSimDerivedData/Build/Intermediates.noindex/Meguri.build',
                        'Debug-iphonesimulator', role + '.build')
    need(role in ('Meguri', 'MeguriWidget') and settings.get('TARGET_NAME') == role and
         settings.get('PRODUCT_NAME') == role and settings.get('PRODUCT_BUNDLE_IDENTIFIER') == bundle and
         settings.get('CONFIGURATION') == 'Debug' and settings.get('PLATFORM_NAME') == 'iphonesimulator' and
         settings.get('MEGURI_SIGNING') == 'team' and settings.get('CODE_SIGN_IDENTITY') == 'Apple Development' and
         settings.get('SRCROOT') == os.path.join(root, 'app') and settings.get('CODE_SIGN_ENTITLEMENTS') == source and
         settings.get('FULL_PRODUCT_NAME') == product and settings.get('TARGET_TEMP_DIR') == temp and
         settings.get('BUILT_PRODUCTS_DIR') == os.path.join(products, 'Debug-iphonesimulator') and
         settings.get('MEGURI_BUNDLE_ID_SUFFIX') == '.dev',
         'initial-settings')
    original_source = v.plist(retain_preparation_input(root, os.path.join(root, 'app', source), held, checks), 'initial-entitlements')
    expected_source = {'com.apple.security.application-groups': ['$(MEGURI_APP_GROUP)']}
    if not widget:
        expected_source.update({
            'com.apple.developer.icloud-container-identifiers': ['$(MEGURI_CLOUD_CONTAINER)'],
            'com.apple.developer.icloud-services': ['CloudKit'],
            'com.apple.developer.icloud-container-environment': '$(MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT)',
            'aps-environment': '$(MEGURI_CLOUD_PUSH_ENVIRONMENT)'})
    need(typed_entitlements(original_source) == typed_entitlements(expected_source), 'initial-entitlements')
    requested = expand_role_entitlements(original_source, settings)
    if not widget:
        try:
            v.check_cloud(settings, 'Debug', 'ios', 'team')
        except (ValueError, TypeError, KeyError):
            raise v.Refused('initial-settings') from None
    normal = v.plist(retain_preparation_input(root, os.path.join(temp, product + '.xcent'), held, checks), 'initial-entitlements')
    simulated = v.plist(retain_preparation_input(root, os.path.join(temp, product + '-Simulated.xcent'), held, checks), 'initial-entitlements')
    return normal, merge_native_request(normal, simulated, requested, role, settings)


def resolved_targets(raw, targets, exact=False):
    def pairs(items):
        need(len({key for key, _ in items}) == len(items), 'initial-settings')
        return dict(items)
    try:
        rows = json.loads(raw, object_pairs_hook=pairs)
    except (ValueError, UnicodeError):
        raise v.Refused('initial-settings') from None
    need(type(rows) is list and all(type(row) is dict for row in rows) and
         (not exact or len(rows) == len(targets) and {row.get('target') for row in rows} == set(targets)),
         'initial-settings')
    result = {}
    for target in targets:
        matches = [row.get('buildSettings') for row in rows if row.get('target') == target]
        need(len(matches) == 1 and type(matches[0]) is dict, 'initial-settings')
        value = matches[0]
        need(value.get('TARGET_NAME') == target and value.get('CONFIGURATION') == 'Debug' and
             value.get('PLATFORM_NAME') == 'iphonesimulator' and value.get('MEGURI_SIGNING') == 'team' and
             value.get('CODE_SIGN_IDENTITY') == 'Apple Development' and
             type(value.get('DEVELOPMENT_TEAM')) is str and
             re.fullmatch(r'[A-Z0-9]{10}', value['DEVELOPMENT_TEAM']), 'initial-settings')
        result[target] = value
    return result


def first_freeze(root, directory, sha, products, xctestrun, checks):
    # All source/native/settings descriptors remain alive through publication.
    def check():
        for verify in checks:
            verify()
        need(v.source(root) == sha, 'source-changed')
    check()
    need(not any(os.path.lexists(os.path.join(directory, name)) for name in
                 ('agent-build.json', 'agent-build-reference.json')), 'initial-already-frozen')
    reference = v.publish_frozen_build(root, products, xctestrun, 'ios-sim', os.path.basename(directory),
                                       sha, os.path.join(directory, 'agent-build.json'))
    check()
    raw = (json.dumps(reference, sort_keys=True) + '\n').encode()
    with v.authority(directory) as parent:
        fd = os.open('agent-build-reference.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                     0o600, dir_fd=parent)
        with os.fdopen(fd, 'wb') as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
    check()
    need(v.file_reference(os.path.join(root, 'build'), os.path.join(directory, 'agent-build.json')) == reference,
         'initial-product-changed')


@contextlib.contextmanager
def private_entitlements_input(directory, value):
    raw = plistlib.dumps(value, fmt=plistlib.FMT_XML, sort_keys=True)
    need(0 < len(raw) <= 16 * 1024 * 1024, 'initial-entitlements')
    run_witness = {}
    with v.authority(directory, run_witness), tempfile.TemporaryDirectory(
            prefix='initial-app-entitlements-', dir=directory) as temporary:
        run_witness['check']()
        private_witness = {}
        with v.authority(temporary, private_witness) as parent:
            info = os.fstat(parent)
            need(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700, 'initial-entitlements')
            fd = os.open('entitlements.plist', os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o600, dir_fd=parent)
            try:
                os.fchmod(fd, 0o600)
                remaining = memoryview(raw)
                while remaining:
                    count = os.write(fd, remaining)
                    need(count > 0, 'initial-entitlements')
                    remaining = remaining[count:]
                os.fsync(fd)
                pin = v.identity(os.fstat(fd))
                def check():
                    run_witness['check']()
                    private_witness['check']()
                    info = os.fstat(fd)
                    need(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and
                         stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1 and
                         v.identity(info) == pin == v.identity(os.stat(
                             'entitlements.plist', dir_fd=parent, follow_symlinks=False)) and
                         os.pread(fd, len(raw) + 1, 0) == raw, 'initial-entitlements')
                check()
                try:
                    yield os.path.join(temporary, 'entitlements.plist'), check
                finally:
                    check()
            finally:
                os.close(fd)


def sign_initial_code(directory, path, signer, metadata, app_identity=None, role_grants=None):
    identifier, entitlements = metadata
    arguments = ['/usr/bin/codesign', '--force', '--sign', signer, '--timestamp=none']
    if app_identity is None and role_grants is None:
        signing_command(arguments + ['--preserve-metadata=identifier,entitlements', path])
        return None
    expected = entitlements if role_grants is None else role_grants
    if app_identity is not None:
        team, bundle = app_identity
        need(identifier == bundle, 'initial-metadata')
        expected = app_identity_entitlements(expected, team, bundle)
    expected_raw = typed_entitlements(expected)
    with private_entitlements_input(directory, expected) as (input_path, check):
        check()
        signing_command(arguments + ['--preserve-metadata=identifier', '--entitlements', input_path, path])
        check()
    return expected_raw


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


def initial_sign(root, directory, sha, products, xctestrun, settings_path, widget_settings_path):
    need(v.source(root) == sha, 'source-changed')
    run = os.path.basename(directory)
    need(v.MG_RUN_ID.fullmatch(run) and directory == os.path.join(root, 'build/mg/build-app', run), 'initial-run')
    need(not any(os.path.lexists(os.path.join(directory, x)) for x in ('agent-build.json', 'agent-build-reference.json')),
         'initial-already-frozen')
    need(products == os.path.join(root, 'build/app/CloudAgentSimDerivedData/Build/Products'), 'initial-products')
    v.contained(products, xctestrun)
    with contextlib.ExitStack() as held:
        checks = []
        need(settings_path == os.path.join(directory, 'agent-settings.json') and
             widget_settings_path == os.path.join(directory, 'agent-widget-settings.json'), 'initial-settings')
        target_settings = resolved_targets(retain_preparation_input(root, settings_path, held, checks),
                                            ('Meguri', 'MeguriSnapshotTests'))
        widget_settings = resolved_targets(retain_preparation_input(root, widget_settings_path, held, checks),
                                            ('MeguriWidget',), exact=True)['MeguriWidget']
        team = target_settings['Meguri']['DEVELOPMENT_TEAM']
        need(target_settings['MeguriSnapshotTests']['DEVELOPMENT_TEAM'] == team == widget_settings['DEVELOPMENT_TEAM'],
             'initial-settings')
        app, info = v.bundle(products, 'Debug-iphonesimulator/Meguri.app', 'ios-sim')
        need(app['bundle_id'] == 'com.antoinefink.meguri.dev', 'initial-settings')
        agent = v.test_inputs(products, xctestrun, app, 'ios-sim')
        app_path = os.path.join(products, app['path'])
        agent_path = os.path.join(products, agent['path'])
        boundary = os.path.join(root, 'build')
        before = prepare_install_modes(boundary, products, app['path'], v.snapshot(boundary, products))
        paths, bundles = native_plan(products, app, agent, before[0])
        need(all(target_settings[target].get('PRODUCT_BUNDLE_IDENTIFIER') == item['bundle_id']
                 for target, item in (('Meguri', app), ('MeguriSnapshotTests', agent))), 'initial-settings')
        role_grants = {}
        for role, settings in (('Meguri', target_settings['Meguri']), ('MeguriWidget', widget_settings)):
            normal, expected = prepare_role_grants(root, products, role, settings, held, checks)
            role_grants[app_path if role == 'Meguri' else app_path + '/PlugIns/MeguriWidget.appex'] = (normal, expected)
        need(target_settings['Meguri'].get('MEGURI_APP_GROUP') == widget_settings.get('MEGURI_APP_GROUP') ==
             'group.com.antoinefink.meguri.dev', 'initial-entitlements')
        signer, certificate = choose_identity(team)
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
            for check in checks:
                check()
            need(v.source(root) == sha and v.snapshot(os.path.join(root, 'build'), products) == before,
                 'initial-product-changed')
            identifier, entitlements = signing_metadata(path)
            item = bundles.get(path)
            need(item is None or identifier == item['bundle_id'], 'initial-metadata')
            executable = os.path.relpath(path, products) if item is None else item['path'] + '/' + item['executable']
            signature_root = None if item is None else item['path'] + '/_CodeSignature'
            parent=executable_parents[path];name=os.path.basename(executable)
            original_bytes=code_bytes(parent,name)
            original_code=code_fingerprint(original_bytes)
            # choose_identity already bound this team to the selected DER/private key;
            # only the exact resolved and metadata-validated app gets identity composition.
            composed = role_grants.get(path)
            if composed is not None:
                need(typed_entitlements(entitlements) == typed_entitlements(composed[0]), 'initial-entitlements')
            expected_entitlements = sign_initial_code(directory, path, signer, (identifier, entitlements),
                (team, app['bundle_id']) if path == app_path else None,
                None if composed is None else composed[1])
            for check in checks:
                check()
            for witness in witnesses:
                witness['check']()
            native_result = v.identity(os.stat(os.path.basename(executable), dir_fd=executable_parents[path], follow_symlinks=False))
            after = v.snapshot(os.path.join(root, 'build'), products)
            compare_product_files(before, after, executable, signature_root, native_result)
            final_bytes=code_bytes(parent,name)
            final_code=code_fingerprint(final_bytes)
            same_code = final_code == original_code or signature_allocation_matches(original_bytes, final_bytes)
            if not same_code:
                write_code_difference(directory, os.path.relpath(path, products), original_bytes, final_bytes)
            need(same_code,'initial-code-changed')
            final_metadata = signing_metadata(path)
            if composed is not None:
                need(final_metadata[0] == identifier and typed_entitlements(final_metadata[1]) ==
                     expected_entitlements, 'initial-entitlements-changed')
            else:
                need(final_metadata == (identifier, entitlements), 'initial-entitlements-changed')
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
        need(v.snapshot(os.path.join(root, 'build'), products) == before and v.source(root) == sha,
             'initial-product-changed')
        first_freeze(root, directory, sha, products, xctestrun,
                     checks + [witness['check'] for witness in witnesses])


def main():
    for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(number, lambda *_: (_ for _ in ()).throw(stage.Cancelled()))
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'run-directory', 'source-commit', 'products', 'xctestrun', 'settings', 'widget-settings'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    try:
        initial_sign(args.root, args.run_directory, args.source_commit, args.products, args.xctestrun, args.settings, args.widget_settings)
        return 0
    except (v.Refused, v.Invalid, stage.Refused, stage.Cancelled, OSError, ValueError, TypeError, KeyError, AttributeError) as error:
        allowed = {'source-changed', 'source-dirty', 'initial-run', 'initial-already-frozen', 'initial-products',
            'initial-settings', 'initial-widget', 'initial-code', 'initial-code-changed', 'initial-unknown-code', 'initial-signer', 'initial-signer-ambiguous',
            'initial-certificate', 'initial-metadata', 'initial-entitlements', 'initial-product-changed',
            'initial-executable-replaced', 'initial-signing', 'initial-entitlements-changed', 'initial-signature',
            'initial-install-modes',
            'product-signature', 'product-cloud-grants', 'product-debug', 'product-entitlements',
            'agent-signature', 'agent-platform', 'input-changed'}
        code = error.args[0] if isinstance(error, v.Refused) and len(error.args) == 1 and type(error.args[0]) is str and error.args[0] in allowed else 'guarded-input'
        print('Initial simulator development signing refused: ' + code + '; no signed build admission', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
