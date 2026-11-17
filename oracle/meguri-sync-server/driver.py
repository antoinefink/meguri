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
from normal import NORMAL_FIXTURE, normal_selection
from normal_matrix import MATRIX_FIXTURE, matrix_selection
from media import MEDIA_FIXTURE, media_selection
from recovery import RECOVERY_FIXTURE, recovery_selection
from recovery_dirty import DIRTY_FIXTURE, dirty_selection


def adapter(request):
    process = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name("adapter.py"))],
                             input=encoded(request), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                             timeout=600 if str(request.get('phase','')).startswith('final-') or request['mode'] == 'edges' or request.get('phase') == 'media' or request['mode'] == 'media' else 120)
    if process.returncode != 0:
        failure = decode(process.stdout).get('failure', {}) if process.stdout else {}
        print('SDK phase failure: ' + str(failure.get('phase', 'unknown')) + ' (' + str(failure.get('kind', 'worker-exit')) + ')', file=sys.stderr)
        if failure.get('driver_frames'): print('Driver frames: '+','.join(failure['driver_frames']),file=sys.stderr)
        if failure.get('recording_errors'):
            print('Recording error kinds: ' + ','.join(failure['recording_errors']), file=sys.stderr)
        raise ValueError('reference SDK worker failed')
    try:
        return decode(process.stdout)
    except ValueError as error:
        if str(error) in {"wide integer requires an exact decimal string","duplicate JSON member","nonfinite number"}:
            print("SDK result boundary: " + str(error),file=sys.stderr)
        else:
            print("SDK result boundary: invalid JSON",file=sys.stderr)
        raise


class PrivateResult(unittest.TextTestResult):
    def _exc_info_to_string(self, error, test):
        return "quarantined test failure: " + error[0].__name__


def main():
    if len(sys.argv) > 1 and sys.argv[1] == 'retained-companion':
        child=subprocess.run([sys.executable,str(pathlib.Path(__file__).with_name('retained_companion.py')),*sys.argv[2:]],
                             stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        return child.returncode
    if len(sys.argv) > 1 and sys.argv[1] == 'retained-observer':
        from retained_observer import main as observe
        return observe(sys.argv[2:])
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["selftest", "identity", "recovery-discover", "recovery", "recovery-dirty", "smoke", "edges", "normal", "normal-sdk", "normal-matrix", "media", "candidate", "final-assemble", "catalog", "validate", "seal"])
    parser.add_argument('--stage',choices=['normal','execute','record'],default='normal')
    parser.add_argument('--request')
    parser.add_argument('--input')
    parser.add_argument('--credentials')
    from final_contract import SEGMENTS
    parser.add_argument('--phase',choices=['full','normal','media','complete']+['final-'+segment for segment in SEGMENTS],default='full')
    args = parser.parse_args()
    out = pathlib.Path('/out')
    if args.mode != 'catalog':
        neutral = pathlib.Path('/opt/reference/neutral')
        for name, expected in [('catalog.json',catalog()),('selection.json',selection()),('fixture.json',FIXTURE),
                               ('edge-selection.json',edge_selection()),('edge-fixture.json',EDGE_FIXTURE),
                               ('recovery-selection.json',recovery_selection()),('recovery-fixture.json',RECOVERY_FIXTURE),
                               ('recovery-dirty-selection.json',dirty_selection()),('recovery-dirty-fixture.json',DIRTY_FIXTURE),
                               ('candidate-media-selection.json',candidate_selection('media')),('media-selection.json',media_selection()),('media-fixture.json',MEDIA_FIXTURE),
                               ('candidate-full-selection.json',candidate_selection('full')),('candidate-normal-selection.json',candidate_selection('normal')),('candidate-complete-selection.json',candidate_selection('complete'))]:
            require(same_value(decode((neutral / name).read_bytes()), expected), 'generated neutral input is stale')
    if args.mode == "selftest":
        suite = unittest.defaultTestLoader.discover(str(pathlib.Path(__file__).parent / "tests"))
        result = unittest.TextTestRunner(verbosity=1, resultclass=PrivateResult).run(suite)
        (out / 'summary.json').write_bytes(encoded({"total": result.testsRun, "passed": result.testsRun - len(result.failures) - len(result.errors), "failed": len(result.failures) + len(result.errors)}))
        return 0 if result.wasSuccessful() and result.testsRun else 1
    if args.mode == 'catalog':
        value = catalog()
        (out / 'catalog.json').write_bytes(encoded(value))
        (out / 'recovery-selection.json').write_bytes(encoded(recovery_selection()))
        (out / 'recovery-fixture.json').write_bytes(encoded(RECOVERY_FIXTURE))
        (out / 'recovery-dirty-fixture.json').write_bytes(encoded(DIRTY_FIXTURE))
        (out / 'recovery-dirty-selection.json').write_bytes(encoded(dirty_selection()))
        (out / 'selection.json').write_bytes(encoded(selection()))
        (out / 'edge-selection.json').write_bytes(encoded(edge_selection()))
        (out / 'matrix-selection.json').write_bytes(encoded(matrix_selection()))
        (out / 'matrix-fixture.json').write_bytes(encoded(MATRIX_FIXTURE))
        (out / 'normal-selection.json').write_bytes(encoded(normal_selection()))
        (out / 'normal-fixture.json').write_bytes(encoded(NORMAL_FIXTURE))
        (out / 'edge-fixture.json').write_bytes(encoded(EDGE_FIXTURE))
        (out / 'candidate-full-selection.json').write_bytes(encoded(candidate_selection('full')))
        (out / 'candidate-normal-selection.json').write_bytes(encoded(candidate_selection('normal')))
        (out / 'candidate-complete-selection.json').write_bytes(encoded(candidate_selection('complete')))
        (out / 'candidate-media-selection.json').write_bytes(encoded(candidate_selection('media')))
        (out / 'media-selection.json').write_bytes(encoded(media_selection()))
        (out / 'media-fixture.json').write_bytes(encoded(MEDIA_FIXTURE))
        (out / 'fixture.json').write_bytes(encoded(FIXTURE))
        (out / 'summary.json').write_bytes(encoded({"total": 0, "passed": 0, "failed": 0, 'catalog_cases': value['required_case_count']}))
        return 0
    if args.mode == 'recovery-dirty':
        value = adapter({'mode':'recovery-dirty','fixture':DIRTY_FIXTURE})
        (out / 'recovery-dirty-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value,out,invocation) else 1
    if args.mode == 'recovery':
        value = adapter({'mode':'recovery','fixture':RECOVERY_FIXTURE})
        (out / 'recovery-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value,out,invocation) else 1
    if args.mode == 'recovery-discover':
        value = adapter({'mode':'recovery-discover'})
        (out / 'recovery-api.json').write_bytes(encoded(value))
        (out / 'summary.json').write_bytes(encoded({'total':1,'passed':1,'failed':0,'scope':'Public interface discovery only; no recovery acceptance.'}))
        return 0
    if args.mode == 'identity':
        value = adapter({"mode": "identity"})
        (out / 'runtime.json').write_bytes(encoded(value))
        return 0
    if args.mode == 'normal-matrix':
        value = adapter({'mode':'normal-matrix','fixture':MATRIX_FIXTURE})
        (out / 'matrix-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value,out,invocation) else 1
    if args.mode == 'media':
        value = adapter({'mode':'media','fixture':MEDIA_FIXTURE})
        (out / 'media-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value,out,invocation) else 1
    if args.mode in {'normal','normal-sdk'}:
        value = adapter({'mode':args.mode,'fixture':NORMAL_FIXTURE})
        (out / 'normal-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value,out,invocation) else 1
    if args.mode == 'edges':
        value = adapter({'mode':'edges','fixture':EDGE_FIXTURE})
        (out / 'edge-observations.json').write_bytes(encoded(value))
        invocation = decode((out / 'invocation.json').read_bytes())
        require(value['runtime']['lock_sha256'] == invocation['lock_sha256'], 'runtime lock receipt mismatch')
        return 0 if record(value,out,invocation) else 1
    if args.mode == 'candidate':
        from candidate import load_inputs, verify_package
        from validation import Artifacts
        require(args.input == '/in' and args.credentials == '/secret/credentials.json','candidate inputs require the fixed separately mounted paths')
        invocation=decode((out/'invocation.json').read_bytes());final=args.phase.startswith('final-')
        require(args.stage in {'execute','record'} if final else args.stage == 'normal','candidate stage/phase mismatch')
        contract=candidate_selection(args.phase)
        if final:
            require(invocation.get('child_phase') == args.phase and invocation.get('child_stage') == args.stage,'final child phase/stage invocation mismatch')
            invocation['selection']=[case['id'] for case in contract['cases']]
            (out/'invocation.json').write_bytes(encoded(invocation))
        if args.stage == 'record':
            from final_handshake import record_inputs
            inputs=Artifacts(pathlib.Path('/in/artifacts'));package=decode(pathlib.Path('/in/peer.json').read_bytes())
            value,_=record_inputs(out,invocation,package,inputs)
            return 0 if record(value,out,invocation,package,inputs) else 1
        if args.stage == 'execute':
            require(set(path.name for path in out.iterdir()) == {'invocation.json'} and 'execute_admission_sha256' not in invocation,'final execute requires initially empty owned output')
        package,inputs,credentials,package_bytes=load_inputs('/in',args.credentials,invocation,preflight=args.stage == 'execute')
        target={**credentials,'server':package['server']}
        phase=args.phase.removeprefix('final-') if final else args.phase
        extra={'case':contract['cases'][0]} if final and phase not in {'full','normal','media'} else {}
        value=adapter({'mode':'candidate','fixture':MEDIA_FIXTURE if phase == 'media' else NORMAL_FIXTURE if phase == 'normal' else FIXTURE,
                       'target':target,'phase':args.phase,'full_only':phase == 'full',**extra})
        if args.stage == 'execute':
            from final_handshake import execute
            return 0 if execute(value,out,invocation,package,inputs,package_bytes) else 1
        (out/'candidate-observations.json').write_bytes(encoded(value))
        return 0 if record(value,out,invocation,package,inputs) else 1
    if args.mode == 'final-assemble':
        from final_assemble import assemble
        require(args.input == '/in','final assembly requires fixed input mount')
        return 0 if assemble(pathlib.Path('/in/segments'),out,decode((out/'invocation.json').read_bytes()),adapter({'mode':'identity'})) else 1
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
