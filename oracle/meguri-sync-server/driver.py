# SPDX-License-Identifier: AGPL-3.0-or-later
"""Current-release test boundary. Reference dependencies never leave this image."""
import argparse
import pathlib
import json
import subprocess
import sys
import unittest

from catalog import catalog
from synthetic import FIXTURE, EDGE_FIXTURE
from validation import decode, encoded, require, same_value
from evidence import record, selection, edge_selection, candidate_selection
from request import check_request


def adapter(request):
    process = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name("adapter.py"))],
                             input=encoded(request), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                             timeout=600 if request['mode'] == 'edges' else 120)
    if process.returncode != 0:
        failure = decode(process.stdout).get('failure', {}) if process.stdout else {}
        print('SDK phase failure: ' + str(failure.get('phase', 'unknown')) + ' (' + str(failure.get('kind', 'worker-exit')) + ')', file=sys.stderr)
        if failure.get('recording_errors'):
            print('Recording error kinds: ' + ','.join(failure['recording_errors']), file=sys.stderr)
        raise ValueError('reference SDK worker failed')
    return decode(process.stdout)


class PrivateResult(unittest.TextTestResult):
    def _exc_info_to_string(self, error, test):
        return "quarantined test failure: " + error[0].__name__


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["selftest", "identity", "smoke", "edges", "candidate", "catalog", "validate", "seal"])
    parser.add_argument('--request')
    parser.add_argument('--input')
    parser.add_argument('--credentials')
    parser.add_argument('--phase',choices=['full','complete'],default='full')
    args = parser.parse_args()
    out = pathlib.Path('/out')
    if args.mode != 'catalog':
        neutral = pathlib.Path('/opt/reference/neutral')
        for name, expected in [('catalog.json',catalog()),('selection.json',selection()),('fixture.json',FIXTURE),
                               ('edge-selection.json',edge_selection()),('edge-fixture.json',EDGE_FIXTURE),
                               ('candidate-full-selection.json',candidate_selection('full')),('candidate-complete-selection.json',candidate_selection('complete'))]:
            require(same_value(decode((neutral / name).read_bytes()), expected), 'generated neutral input is stale')
    if args.mode == "selftest":
        suite = unittest.defaultTestLoader.discover(str(pathlib.Path(__file__).parent / "tests"))
        result = unittest.TextTestRunner(verbosity=1, resultclass=PrivateResult).run(suite)
        (out / 'summary.json').write_bytes(encoded({"total": result.testsRun, "passed": result.testsRun - len(result.failures) - len(result.errors), "failed": len(result.failures) + len(result.errors)}))
        return 0 if result.wasSuccessful() and result.testsRun else 1
    if args.mode == 'catalog':
        value = catalog()
        (out / 'catalog.json').write_bytes(encoded(value))
        (out / 'selection.json').write_bytes(encoded(selection()))
        (out / 'edge-selection.json').write_bytes(encoded(edge_selection()))
        (out / 'edge-fixture.json').write_bytes(encoded(EDGE_FIXTURE))
        (out / 'candidate-full-selection.json').write_bytes(encoded(candidate_selection('full')))
        (out / 'candidate-complete-selection.json').write_bytes(encoded(candidate_selection('complete')))
        (out / 'fixture.json').write_bytes(encoded(FIXTURE))
        (out / 'summary.json').write_bytes(encoded({"total": 0, "passed": 0, "failed": 0, 'catalog_cases': value['required_case_count']}))
        return 0
    if args.mode == 'identity':
        value = adapter({"mode": "identity"})
        (out / 'runtime.json').write_bytes(encoded(value))
        return 0
    if args.mode == 'edges':
        value = adapter({'mode':'edges','fixture':EDGE_FIXTURE})
        (out / 'edge-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value,out,invocation) else 1
    if args.mode == 'candidate':
        from candidate import load_inputs
        require(args.input == '/in' and args.credentials == '/secret/credentials.json','candidate inputs require the fixed separately mounted paths')
        invocation = decode((out / 'invocation.json').read_bytes())
        package,inputs,credentials = load_inputs('/in',args.credentials,invocation)
        target = {**credentials,'server':package['server']}
        value = adapter({'mode':'candidate','fixture':FIXTURE,'target':target,'full_only':args.phase == 'full'})
        (out / 'candidate-observations.json').write_bytes(encoded(value))
        return 0 if record(value,out,invocation,package,inputs) else 1
    if args.mode == 'smoke':
        request = check_request(decode(pathlib.Path(args.request).read_bytes()) if args.request else
                                {'format': 'meguri-sync-request-v1', 'operation': 'smoke', 'client': 'desktop', 'peer': 'reference'})
        if request['client'] == 'mobile':
            (out / 'unavailable.json').write_bytes(encoded({'target': 'mobile', 'reason': 'Actual AnkiMobile 26.10 device/build execution receipt is unavailable.'}))
            raise ValueError('actual device required')
        if request['peer'] == 'candidate':
            raise ValueError('candidate requires the verified peer and separate credential input')
        value = adapter({"mode": "smoke", "fixture": FIXTURE})
        (out / 'smoke-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value, out, invocation) else 1
    if args.mode in {'validate', 'seal'}:
        from evidence import validate_existing
        require(args.input, 'retained evidence directory required')
        return 0 if validate_existing(pathlib.Path(args.input), out, decode((out / 'invocation.json').read_bytes()), adapter({'mode':'identity'}), args.mode == 'seal') else 1
    raise RuntimeError("operation not implemented")


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print("reference harness failure: " + type(error).__name__, file=sys.stderr)
        sys.exit(1)
