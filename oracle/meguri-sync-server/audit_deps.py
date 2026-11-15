# SPDX-License-Identifier: AGPL-3.0-or-later
"""Review the exact test-only lock against public package metadata."""
import json
import pathlib
import re
import sys
import urllib.parse
import urllib.request

LICENSES = {'anki':'AGPL-3.0-or-later', 'attrs':'MIT', 'certifi':'MPL-2.0',
            'charset-normalizer':'MIT', 'decorator':'BSD-2-Clause', 'distro':'Apache-2.0',
            'idna':'BSD-3-Clause', 'jsonschema':'MIT', 'jsonschema-specifications':'MIT',
            'markdown':'BSD-3-Clause', 'orjson':'MPL-2.0 AND (Apache-2.0 OR MIT)', 'protobuf':'BSD-3-Clause',
            'pysocks':'BSD-3-Clause', 'referencing':'MIT', 'requests':'Apache-2.0', 'rpds-py':'MIT',
            'typing-extensions':'PSF-2.0', 'urllib3':'MIT', 'zstandard':'BSD-3-Clause'}
LICENSE_METADATA_ALIASES = {'distro':{'Apache License, Version 2.0'}, 'protobuf':{'3-Clause BSD License'}, 'pysocks':{'BSD'}}


def license_matches(name, reported):
    return name in LICENSES and (reported == LICENSES[name] or reported in LICENSE_METADATA_ALIASES.get(name, set()))


def main():
    pins = {}
    current = None
    for line in pathlib.Path('/lock/requirements.lock').read_text().splitlines():
        match = re.match(r'^([A-Za-z0-9_.-]+)==([^\s]+)', line)
        if match:
            current = match[1].lower().replace('_','-')
            if current in pins: raise ValueError('duplicate package pin')
            pins[current] = {'version':match[2], 'hashes':[]}
        digest = re.search(r'--hash=sha256:([a-f0-9]{64})', line)
        if digest:
            if current is None: raise ValueError('hash without package')
            pins[current]['hashes'].append(digest[1])
    if set(pins) != set(LICENSES): raise ValueError('unreviewed test dependency graph')
    packages = []
    clean = True
    for name, pin in pins.items():
        url = 'https://pypi.org/pypi/' + urllib.parse.quote(name, safe='') + '/' + urllib.parse.quote(pin['version'], safe='') + '/json'
        with urllib.request.urlopen(url, timeout=20) as response:
            metadata = json.load(response)
        published = {entry['digests']['sha256'] for entry in metadata['urls'] if not entry['yanked']}
        hashes_match = bool(pin['hashes']) and set(pin['hashes']) <= published
        advisories = metadata.get('vulnerabilities', [])
        reported_license = metadata['info'].get('license_expression') or (metadata['info'].get('license') or '').split('\n')[0][:160]
        approved_license = license_matches(name, reported_license)
        packages.append({'name':name, 'version':pin['version'], 'artifact_hashes_match':hashes_match,
                         'selected_license':LICENSES[name], 'reported_license':reported_license, 'license_matches_review':approved_license,
                         'reported_advisory_ids':[entry.get('id') for entry in advisories], 'metadata_source':url})
        clean = clean and hashes_match and not advisories and approved_license
    report = {'format':'meguri-sync-reference-deps-v1','test_only':True,'shipped_in_production':False,
              'packages':packages,'passed':clean,'limit':'Public package metadata and reported advisories are not an exhaustive security audit.'}
    pathlib.Path('/out/dependencies.json').write_text(json.dumps(report,indent=2)+'\n')
    pathlib.Path('/out/summary.json').write_text(json.dumps({'total':len(packages),'passed':sum(p['artifact_hashes_match'] and not p['reported_advisory_ids'] and p['license_matches_review'] for p in packages),
                                                          'failed':sum(not p['artifact_hashes_match'] or bool(p['reported_advisory_ids']) or not p['license_matches_review'] for p in packages)}))
    return 0 if clean else 1


if __name__ == '__main__':
    try: sys.exit(main())
    except Exception as error:
        print('test dependency audit failure: '+type(error).__name__,file=sys.stderr)
        sys.exit(1)
