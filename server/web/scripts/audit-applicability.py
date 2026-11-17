# SPDX-License-Identifier: MIT
"""One fixed frontend build assessment; no caller-selected policy or advisory."""
import hashlib
import json
from pathlib import Path
import shutil
import stat
import sys

SCRIPTS = Path(__file__).absolute().parent
POLICY = SCRIPTS / 'audit-assessment.json'
TRUST_ROOT = {'scripts/audit-applicability.py', 'scripts/audit-assessment.json'}
GENERATED = {'.test-build', 'dist', 'node_modules', 'playwright-report', 'test-results'}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode()


def parse(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('Duplicate JSON field')
            result[key] = value
        return result
    def invalid_constant(value):
        raise ValueError('Invalid JSON constant: ' + value)
    value = json.loads(data.decode('utf-8'), object_pairs_hook=unique, parse_constant=invalid_constant)
    canonical(value)  # Reject numeric overflow to infinity as well as named constants.
    return value


def regular_bytes(path):
    # Check parents as well: a narrow control mount must never follow a link.
    for parent in reversed(path.parents):
        if not stat.S_ISDIR(parent.lstat().st_mode):
            raise ValueError('Linked or non-directory input parent: ' + str(parent))
    if not stat.S_ISREG(path.lstat().st_mode):
        raise ValueError('Linked or nonregular input: ' + str(path))
    return path.read_bytes()


def tree(root, omit_generated=False):
    directories, files = [], {}
    def visit(path, relative):
        mode = path.lstat().st_mode
        if stat.S_ISDIR(mode):
            directories.append(relative)
            for child in sorted(path.iterdir()):
                if not relative and omit_generated and child.name in GENERATED and stat.S_ISDIR(child.lstat().st_mode):
                    continue
                visit(child, (relative + '/' if relative else '') + child.name)
        elif stat.S_ISREG(mode):
            data = regular_bytes(path)
            files[relative] = dict(bytes=len(data), sha256=digest(data))
        else:
            raise ValueError('Linked or nonregular input: ' + str(path))
    # Reject a linked root or any linked ancestor before walking.
    for parent in reversed(root.parents):
        if not stat.S_ISDIR(parent.lstat().st_mode):
            raise ValueError('Linked input parent: ' + str(parent))
    if not stat.S_ISDIR(root.lstat().st_mode):
        raise ValueError('Non-directory input root')
    visit(root, '')
    return dict(directories=directories, files=files)


def policy_identity():
    return {name: digest(regular_bytes(SCRIPTS / Path(name).name)) for name in sorted(TRUST_ROOT)}


def web_identity(root, policy):
    actual = tree(root, True)
    reviewed = dict(directories=actual['directories'], files=dict(actual['files']))
    # Enumerate these exact two files, bind their bytes to the executing image,
    # and exclude only their content fingerprints from the self-description.
    for name in TRUST_ROOT:
        if regular_bytes(root / name) != regular_bytes(SCRIPTS / Path(name).name):
            raise ValueError('Source/image policy mismatch: ' + name)
        reviewed['files'][name] = {'trust_root': True}
    if canonical(reviewed) != canonical(policy['web_inputs']):
        raise ValueError('Complete copied web input set differs from assessment')
    return actual


def control_identity(root, policy, staged=False):
    files = {}
    for name in policy['build_controls']['files']:
        data = regular_bytes(root / name)
        files[name] = dict(bytes=len(data), sha256=digest(data))
    actual = dict(directories=policy['build_controls']['directories'], files=files)
    if staged:
        actual = tree(root)
    if canonical(actual) != canonical(policy['build_controls']):
        raise ValueError('Build/packaging controls differ from assessment')
    return actual


def check_inputs(web, controls, policy):
    source = web_identity(web, policy)
    # run.sh and every other baked script must come from this exact input set.
    baked = tree(SCRIPTS)
    expected_scripts = tree(web / 'scripts')
    if canonical(baked) != canonical(expected_scripts):
        raise ValueError('Source/image executable scripts differ; rebuild sync-web-image')
    actual_controls = control_identity(controls, policy, True)
    lock = regular_bytes(web / 'package-lock.json')
    if digest(lock) != policy['lock_bytes_sha256']:
        raise ValueError('Locked dependency bytes differ')
    locked_graph = digest(canonical(parse(lock)))
    if locked_graph != policy['locked_graph_sha256']:
        raise ValueError('Exact locked graph differs')
    return dict(web_input_sha256=digest(canonical(source)),
                build_controls_sha256=digest(canonical(actual_controls)),
                lock_bytes_sha256=digest(lock), locked_graph_sha256=locked_graph,
                policy_sha256=policy_identity())


def assess(web, work, controls, output, policy):
    result = dict(schema_version=1, assessment=policy['assessment'], disposition='blocked',
                  raw_report_sha256=None, raw_exit=None, vulnerability_counts=None,
                  web_input_sha256=None, copied_input_sha256=None,
                  build_controls_sha256=None, lock_bytes_sha256=None,
                  locked_graph_sha256=None, policy_sha256=policy_identity())
    try:
        raw = regular_bytes(output / 'npm-audit.json')
        result['raw_report_sha256'] = digest(raw)
        exit_bytes = regular_bytes(output / 'npm-audit-exit.txt')
        if not exit_bytes.strip().isdigit():
            raise ValueError('Invalid raw npm audit exit encoding')
        result['raw_exit'] = int(exit_bytes)
        report, parse_error = None, None
        try:
            report = parse(raw)
            if isinstance(report, dict) and isinstance(report.get('metadata'), dict):
                result['vulnerability_counts'] = report['metadata'].get('vulnerabilities')
        except (ValueError, RecursionError) as error:
            parse_error = str(error)
        # Preserve valid boundary identities even for malformed reports or tool exits.
        result.update(check_inputs(web, controls, policy))
        copied = web_identity(work, policy)
        result['copied_input_sha256'] = digest(canonical(copied))
        if result['copied_input_sha256'] != result['web_input_sha256']:
            raise ValueError('Executed copied inputs differ from mounted source')
        tools = parse(regular_bytes(output / 'npm-audit-tools.json'))
        result['tools'] = tools
        if canonical(tools) != canonical(policy['tools']):
            raise ValueError('Unexpected Node/npm tool versions')
        if parse_error is not None:
            raise ValueError(parse_error)
        if exit_bytes not in (b'0\n', b'1\n'):
            raise ValueError('Unexpected raw npm audit exit')
        if regular_bytes(output / 'npm-audit-stderr.txt'):
            raise ValueError('npm audit reported diagnostic output')
        clean = dict(auditReportVersion=2, vulnerabilities={}, metadata=dict(
            vulnerabilities=dict(info=0, low=0, moderate=0, high=0, critical=0, total=0),
            dependencies=policy['recorded_report']['metadata']['dependencies']))
        if result['raw_exit'] == 0 and canonical(report) == canonical(clean):
            result['disposition'] = 'raw-clean'
        elif result['raw_exit'] == 1 and canonical(report) == canonical(policy['recorded_report']):
            result['disposition'] = 'assessed-inapplicable'
        else:
            raise ValueError('Report is outside the exact assessed advisory or clean result')
    except (OSError, ValueError, TypeError, KeyError, RecursionError) as error:
        result['reason'] = str(error)
    (output / 'npm-audit-decision.json').write_bytes(canonical(result) + b'\n')
    if result['disposition'] == 'blocked':
        raise ValueError(result['reason'])


def main():
    policy = parse(regular_bytes(POLICY))
    command, *args = sys.argv[1:]
    # Internal fixed MG seams, not policy selection or approval-file interfaces.
    if command == 'stage-controls' and len(args) == 2:
        root, target = map(Path, args)
        controls = control_identity(root, policy)
        # Check everything before publishing any staged control input.
        for name in controls['files']:
            destination = target / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(root / name, destination)
        control_identity(target, policy, True)
        print(canonical(dict(build_controls_sha256=digest(canonical(controls)), policy_sha256=policy_identity())).decode())
    elif command == 'check-inputs' and len(args) == 2:
        check_inputs(*map(Path, args), policy)
    elif command == 'assess' and len(args) == 4:
        assess(*map(Path, args), policy)
    else:
        raise ValueError('Invalid internal applicability invocation')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, TypeError, KeyError, RecursionError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
