# SPDX-License-Identifier: AGPL-3.0-or-later
"""Exact local release evidence. This module never opens a service or source tree."""
import io
import pathlib
import re
import tarfile

from validation import DIGEST, decode, require, same_value, sha

PACKAGE_KEYS = {'image_id','inputs_sha256','runtime_inventory_sha256','image_inspection_sha256','assets_sha256'}
EXECUTION_KEYS = {'format','endpoint','server','run_id','container_id','labels','observed_executable_sha256',
                  'package','server_process','container_inspection_before_sha256','container_inspection_after_sha256'}
EXECUTABLE = '/usr/local/bin/meguri-sync-server'


def _one(value):
    require(isinstance(value,list) and len(value) == 1 and isinstance(value[0],dict), 'one actual Docker inspection required')
    return value[0]


def _entries(entries):
    require(isinstance(entries,list) and bool(entries), 'empty package input manifest')
    result = {}
    for row in entries:
        name = row.get('path')
        require(isinstance(name,str) and name and not name.startswith('/') and '..' not in pathlib.PurePosixPath(name).parts and name not in result, 'invalid or duplicate package input path')
        require(type(row.get('bytes')) is int and row['bytes'] >= 0 and DIGEST.fullmatch(row.get('sha256','')), 'invalid package input descriptor')
        result[name] = row
    return result


def package_dependencies(peer, store, invocation, built):
    """Return every retained digest after checking its transitive relationships."""
    package, server = peer['package'], peer['server']
    require(isinstance(package,dict) and set(package) == PACKAGE_KEYS, 'invalid final package shape')
    require(re.fullmatch('sha256:[0-9a-f]{64}',package.get('image_id','')), 'invalid immutable image identity')
    require(peer['profile'] == 'release' and same_value(peer['features'],[]), 'final package is not hook-free release')
    require(built.get('job') == 'sync-server-package' and same_value(built.get('args'),[]), 'wrong final package producer')
    root = built.get('root')
    require(isinstance(root,str) and root.startswith('/') and root == invocation.get('root'), 'final package checkout root mismatch')
    result_dir = root + '/build/mg/sync-server-package/' + built['run_id']
    artifacts = built.get('artifacts',[])
    for kind, suffix in [('package-inputs','/context/package-inputs.json'),('package-source','/source.tar'),('package-product','/product'),('package-image','/image.json')]:
        require(sum(row.get('kind') == kind and row.get('path') == result_dir+suffix for row in artifacts) == 1, 'noncanonical final package artifact location')
    details = built.get('details',{}).get('package',{})
    require(all(details.get(key) == package[key] for key in ('image_id','inputs_sha256','runtime_inventory_sha256')), 'package result identity mismatch')
    require(details.get('source_commit') == server['source_commit'] and details.get('profile') == 'release' and same_value(details.get('features'),[]), 'package result source or configuration mismatch')
    hashes = set()
    def read(digest, length=None):
        data = store.read(digest,length); hashes.add(digest); return data
    def document(digest): return decode(read(digest))
    inputs = document(package['inputs_sha256'])
    inventory = document(package['runtime_inventory_sha256'])
    image = _one(document(package['image_inspection_sha256']))
    assets = document(package['assets_sha256'])
    require(inputs.get('source_commit') == server['source_commit'] and inputs.get('profile') == 'release' and same_value(inputs.get('features'),[]), 'package inputs source/configuration mismatch')
    require(inputs.get('source_archive_sha256') == server['source_sha256'], 'package source archive identity mismatch')
    source = read(inputs['source_archive_sha256'])
    rows = _entries(inputs.get('inputs'))
    require(rows.get('server/Cargo.lock',{}).get('sha256') == server['lock_sha256'], 'frozen server lock identity mismatch')
    # Read only members declared by the frozen manifest; never extract archive paths.
    with tarfile.open(fileobj=io.BytesIO(source),mode='r:') as archive:
        members = archive.getmembers()
        require(len({member.name for member in members}) == len(members), 'duplicate package source member')
        require({member.name for member in members if member.isfile()} == set(rows), 'source archive input set mismatch')
        require(all(member.isfile() or member.isdir() for member in members), 'linked or special package source member')
        members = {member.name:member for member in members}
        for name, row in rows.items():
            member = members.get(name)
            require(member is not None and member.isfile() and member.size == row['bytes'], 'frozen source input missing or invalid')
            stream = archive.extractfile(member)
            require(stream is not None and sha(stream.read()) == row['sha256'], 'frozen source input differs from manifest')
    require(inventory.get('usr/local/bin/meguri-sync-server',{}).get('sha256') == server['executable_sha256'], 'runtime executable identity mismatch')
    require(inventory.get('usr/share/meguri-sync/package-inputs.json',{}).get('sha256') == package['inputs_sha256'], 'runtime package inputs mismatch')
    require(inventory.get('usr/share/meguri-sync/assets.json',{}).get('sha256') == package['assets_sha256'], 'runtime assets mismatch')
    require(inventory.get('usr/share/meguri-sync/covered-source.tar',{}).get('sha256') == inputs.get('covered_source_sha256'), 'covered source identity mismatch')
    require(inventory.get('usr/share/meguri-sync/rust-notices.json',{}).get('sha256') == inputs.get('rust_notices_sha256'), 'notice identity mismatch')
    require(isinstance(inventory,dict) and bool(inventory), 'empty runtime inventory')
    for name, entry in inventory.items():
        require(not name.startswith('/') and '..' not in pathlib.PurePosixPath(name).parts, 'invalid inventory path')
        if 'sha256' in entry: read(entry['sha256'],entry['bytes'])
    require(isinstance(assets,list) and assets and len({row['path'] for row in assets}) == len(assets), 'invalid asset inventory')
    for row in assets: read(row['sha256'],row['bytes'])
    require(image.get('Id') == package['image_id'], 'retained image inspection mismatch')
    config = image.get('Config',{})
    require(config.get('User') == '10001:10001' and config.get('Entrypoint') == [EXECUTABLE], 'release image user/entrypoint mismatch')
    require(config.get('Labels',{}).get('org.opencontainers.image.revision') == server['source_commit'], 'image revision mismatch')
    return hashes


def execution_dependencies(peer, store, invocation, running, preflight=False):
    keys=EXECUTION_KEYS - {'container_inspection_after_sha256'} if preflight else EXECUTION_KEYS
    require(set(running) == keys and same_value(running['package'],peer['package']), 'final execution binding shape/package mismatch')
    process = running['server_process']
    require(isinstance(process,dict) and set(process) == {'pid','executable_sha256'} and type(process['pid']) is int and 1 <= process['pid'] <= 2147483647, 'actual server process identity missing')
    require(process['executable_sha256'] == peer['executable_artifact_sha256'] == running['observed_executable_sha256'], 'actual process executable mismatch')
    hashes = set()
    inspections=[]
    for key in (('container_inspection_before_sha256',) if preflight else ('container_inspection_before_sha256','container_inspection_after_sha256')):
        digest=running[key]; current=_one(decode(store.read(digest))); hashes.add(digest)
        require(current.get('Id') == running['container_id'] and current.get('Image') == peer['package']['image_id'], 'owned container/image mismatch')
        config,host=current.get('Config',{}),current.get('HostConfig',{})
        labels=config.get('Labels',{})
        require(all(labels.get(key) == value for key,value in running['labels'].items()), 'container ownership labels mismatch')
        require(config.get('User') == '10001:10001' and config.get('Entrypoint') == [EXECUTABLE], 'running user/entrypoint mismatch')
        require(host.get('NetworkMode') != 'host','host network refused')
        require(host.get('ReadonlyRootfs') is True and host.get('PortBindings') in ({},None), 'running root or public ports mismatch')
        require(labels.get('meguri.sync.owner') == invocation.get('owner_nonce') and isinstance(invocation.get('owner_nonce'),str) and re.fullmatch('[0-9a-f]{32}',invocation['owner_nonce']), 'owned creation nonce missing or wrong')
        arguments=invocation.get('server_arguments')
        require(isinstance(arguments,list) and len(arguments) == 6 and arguments[0:2] == ['serve','/data/owner'] and arguments[3:5] == ['0.0.0.0:8080','--trusted-peer'], 'invalid fixed serve arguments')
        from urllib.parse import urlsplit
        origin=urlsplit(arguments[2])
        require(origin.scheme == 'https' and origin.hostname and origin.path in {'','/'} and not origin.username and not origin.password and not origin.query and not origin.fragment, 'invalid configured HTTPS origin')
        import ipaddress
        ipaddress.ip_address(arguments[5])
        require(same_value(config.get('Cmd'),arguments) and current.get('Path') == EXECUTABLE and same_value(current.get('Args'),arguments), 'running executable/serve arguments mismatch')
        mounts=current.get('Mounts',[])
        data=[mount for mount in mounts if mount.get('Destination') == '/data']
        require(len(data) == 1 and data[0].get('Type') == 'volume' and data[0].get('RW') is True and data[0].get('Name') == invocation.get('data_volume') and isinstance(invocation.get('data_volume'),str) and bool(invocation['data_volume']), 'owned private data mount mismatch')
        require(all(mount.get('Destination') == '/data' or (mount.get('Destination') == '/tmp' and mount.get('Type') == 'tmpfs' and mount.get('RW') is True) for mount in mounts), 'unexpected runtime mount')
        require(host.get('Privileged') in {False,None}, 'privileged runtime refused')
        require(current.get('State',{}).get('Running') is True, 'owned server is not running')
        inspections.append(current)
    require(bool(inspections[0].get('Created')), 'container creation identity missing')
    if not preflight:
        require(inspections[0].get('State',{}).get('StartedAt') and inspections[0]['State']['StartedAt'] == inspections[1].get('State',{}).get('StartedAt'),'owned container restarted during execution')
        require(same_value(inspections[0].get('Created'),inspections[1].get('Created')) and same_value(inspections[0].get('Mounts'),inspections[1].get('Mounts')), 'container ownership/mount changed during execution')
    return hashes
