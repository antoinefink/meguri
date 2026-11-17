# SPDX-License-Identifier: MIT
"""Finite public report and real fixed-assessment boundary, invoked by MG selftest."""
import copy
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
CONTROLS = [
    'docker/sync-web-compose.yml', 'docker/sync-web.Dockerfile',
    'docker/sync-web.Dockerfile.dockerignore', 'server/Dockerfile',
    'server/Dockerfile.dockerignore', 'server/build.rs', 'server/compose.yaml',
    'server/src/httpapi/mod.rs', 'server/src/webassets/mod.rs',
    'server/tools/package-context.py', 'server/tools/package-inventory.py',
    'tools/mg.d/meguri-sync.sh', 'tools/mg.d/sync-package.sh', 'tools/mg.d/sync-web.sh',
]
GENERATED = {'.test-build', 'dist', 'node_modules', 'playwright-report', 'test-results'}


def copy_baked_scripts(repo, target):
    # This fixture only models the current starts-exclude-all allowlist's flat
    # scripts COPY, not Docker itself. A rebuilt-image check remains required.
    rules = (repo / 'docker/sync-web.Dockerfile.dockerignore').read_text().splitlines()
    if rules[0] != '**' or any(line and not line.startswith('!') for line in rules[1:]):
        raise AssertionError('Revisit the finite Docker context fixture for changed rules')
    target.mkdir()
    for source in (repo / 'server/web/scripts').iterdir():
        relative = source.relative_to(repo).as_posix()
        if any(fnmatch.fnmatchcase(relative, rule[1:]) for rule in rules[1:] if rule):
            shutil.copyfile(source, target / source.name)


def recorded_report():
    fix = {'name': 'vite', 'version': '6.2.7', 'isSemVerMajor': True}
    def propagated(name, direct, via, effects, affected):
        return dict(name=name, severity='high', isDirect=direct, via=via,
                    effects=effects, range=affected, nodes=['node_modules/' + name],
                    fixAvailable=copy.deepcopy(fix))
    direct = dict(source=1247710, name='picomatch', dependency='picomatch',
                  title='picomatch vulnerable to ReDoS through single-branch repeated extglobs',
                  url='https://github.com/advisories/GHSA-fc5q-39xc-vjpf', severity='high',
                  cwe=['CWE-1333'], cvss=dict(score=7.5, vectorString='CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H'),
                  range='<=4.0.7')
    return dict(auditReportVersion=2, vulnerabilities={
        'fdir': propagated('fdir', False, ['picomatch'], ['tinyglobby'], '>=5.2.1'),
        'picomatch': propagated('picomatch', False, [direct], ['fdir', 'tinyglobby', 'vite'], '*'),
        'tinyglobby': propagated('tinyglobby', False, ['fdir', 'picomatch'], ['vite'], '*'),
        'vite': propagated('vite', True, ['picomatch', 'tinyglobby'], [], '>=6.3.0-beta.0'),
    }, metadata=dict(vulnerabilities=dict(info=0, low=0, moderate=0, high=4, critical=0, total=4),
                     dependencies=dict(prod=4, dev=110, optional=72, peer=0, peerOptional=0, total=113)))


class Applicability(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.repo = self.root / 'repo'
        self.web = self.repo / 'server/web'
        shutil.copytree(REPO / 'server/web', self.web,
                        ignore=lambda p, names: GENERATED.intersection(names) if Path(p) == REPO / 'server/web' else [])
        for name in CONTROLS:
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, path)
        self.baked = self.root / 'baked'
        copy_baked_scripts(self.repo, self.baked)
        self.helper = self.baked / 'audit-applicability.py'
        self.controls = self.root / 'controls'
        self.assertEqual(self.stage().returncode, 0)
        self.work = self.root / 'work'
        shutil.copytree(self.web, self.work)
        self.out = self.root / 'out'
        self.out.mkdir()
        self.raw = self.out / 'npm-audit.json'
        self.exit = self.out / 'npm-audit-exit.txt'
        self.stderr = self.out / 'npm-audit-stderr.txt'
        self.set_report(recorded_report(), 1)

    def stage(self):
        return subprocess.run([sys.executable, str(self.repo / 'server/web/scripts/audit-applicability.py'),
                               'stage-controls', str(self.repo), str(self.controls)], capture_output=True)

    def set_report(self, report, status):
        self.raw.write_bytes(('  ' + json.dumps(report, indent=2) + '\n\n').encode())
        self.exit.write_text(str(status) + '\n')
        self.stderr.write_bytes(b'')
        (self.out / 'npm-audit-tools.json').write_text(json.dumps(dict(node='v26.13.1', npm='11.20.0')))

    def decide(self, accepted=False):
        before = self.raw.read_bytes()
        result = subprocess.run([sys.executable, str(self.helper), 'assess', str(self.web),
                                 str(self.work), str(self.controls), str(self.out)], capture_output=True)
        self.assertEqual(result.returncode == 0, accepted, result.stderr.decode())
        self.assertEqual(self.raw.read_bytes(), before)
        decision = json.loads((self.out / 'npm-audit-decision.json').read_text())
        self.assertEqual(decision['raw_report_sha256'], hashlib.sha256(before).hexdigest())
        self.assertEqual(decision['raw_exit'], int(self.exit.read_text()))
        self.assertEqual(decision['disposition'] != 'blocked', accepted)
        try:
            report = json.loads(before)
            json.dumps(report, allow_nan=False)
        except (ValueError, UnicodeError):
            report = None
        if isinstance(report, dict) and isinstance(report.get('metadata'), dict):
            self.assertEqual(decision['vulnerability_counts'], report['metadata'].get('vulnerabilities'))
        return decision

    def test_exact_four_highs_are_assessed_with_raw_bytes_and_all_identities(self):
        decision = self.decide(True)
        self.assertEqual(decision['disposition'], 'assessed-inapplicable')
        self.assertEqual(decision['vulnerability_counts']['high'], 4)
        for field in ['web_input_sha256', 'copied_input_sha256', 'build_controls_sha256',
                      'locked_graph_sha256', 'lock_bytes_sha256', 'policy_sha256']:
            self.assertTrue(decision[field])

    def test_clean_report_still_requires_exact_boundary(self):
        report = recorded_report()
        report['vulnerabilities'] = {}
        report['metadata']['vulnerabilities'] = dict.fromkeys(report['metadata']['vulnerabilities'], 0)
        self.set_report(report, 0)
        self.assertEqual(self.decide(True)['disposition'], 'raw-clean')
        (self.web / 'src/api.ts').write_text('changed source')
        self.decide()

    def test_changed_added_removed_nested_linked_and_special_web_inputs(self):
        for target in [self.web, self.work]:
            path = target / 'src/api.ts'
            original = path.read_bytes()
            for change in ['edit', 'remove', 'link', 'fifo']:
                with self.subTest(target=target.name, change=change):
                    path.unlink()
                    if change == 'edit': path.write_bytes(original + b'\n')
                    if change == 'link': path.symlink_to(target / 'src/app.tsx')
                    if change == 'fifo':
                        import os
                        os.mkfifo(path)
                    self.decide()
                    if path.exists() or path.is_symlink(): path.unlink()
                    path.write_bytes(original)
            nested = target / 'src/nested'
            nested.mkdir()
            self.decide()
            (nested / 'input.ts').write_text('extra')
            self.decide()
            shutil.rmtree(nested)
            nested = target / 'src/node_modules'
            nested.mkdir()
            (nested / 'input.ts').write_text('nested generated names remain inputs')
            self.decide()
            shutil.rmtree(nested)
            extra = target / 'extra'
            extra.write_text('extra')
            self.decide()
            extra.unlink()
            # Only real generated directories at the root may be omitted.
            generated = target / 'node_modules'
            generated.mkdir()
            (generated / 'ignored').write_text('generated')
            self.decide(True)
            shutil.rmtree(generated)
            generated.symlink_to(target / 'src', target_is_directory=True)
            self.decide()
            generated.unlink()
            generated.write_text('not a generated directory')
            self.decide()
            generated.unlink()

    def test_every_actual_host_and_staged_control_is_bound(self):
        for name in CONTROLS:
            for root in [self.repo, self.controls]:
                with self.subTest(root=root.name, name=name):
                    path = root / name
                    before = path.read_bytes()
                    path.write_bytes(before + b'\n')
                    if root == self.repo:
                        shutil.rmtree(self.controls)
                        self.assertNotEqual(self.stage().returncode, 0)
                    else:
                        self.decide()
                    path.write_bytes(before)
                    if not self.controls.exists():
                        self.assertEqual(self.stage().returncode, 0)
        host_path = self.repo / CONTROLS[0]
        host_before = host_path.read_bytes()
        for linked in [False, True]:
            host_path.unlink()
            if linked:
                host_path.symlink_to(self.repo / CONTROLS[1])
            shutil.rmtree(self.controls)
            self.assertNotEqual(self.stage().returncode, 0)
            if host_path.is_symlink(): host_path.unlink()
            host_path.write_bytes(host_before)
            self.assertEqual(self.stage().returncode, 0)
        path = self.controls / CONTROLS[0]
        before = path.read_bytes()
        path.unlink()
        self.decide()
        path.symlink_to(self.repo / CONTROLS[0])
        self.decide()
        path.unlink()
        path.write_bytes(before)
        (self.controls / 'unexpected').write_text('extra')
        self.decide()

    def test_policy_and_baked_executable_alignment(self):
        for name in ['audit-applicability.py', 'audit-assessment.json', 'run.sh', 'copy-source.mjs']:
            path = self.web / 'scripts' / name
            before = path.read_bytes()
            path.write_bytes(before + b'\n')
            self.decide()
            path.write_bytes(before)
            baked = self.baked / name
            baked_before = baked.read_bytes()
            baked.write_bytes(baked_before + b'\n')
            self.decide()
            baked.write_bytes(baked_before)

    def test_lock_versions_integrities_dev_flags_and_new_graph_are_bound(self):
        path = self.work / 'package-lock.json'
        before = path.read_bytes()
        lock = json.loads(before)
        for field, value in [('version', '4.0.8'), ('integrity', 'sha512-changed'), ('dev', False)]:
            changed = copy.deepcopy(lock)
            changed['packages']['node_modules/picomatch'][field] = value
            path.write_text(json.dumps(changed))
            self.decide()
        changed = copy.deepcopy(lock)
        changed['packages']['node_modules/new'] = dict(version='1.0.0', dev=True)
        path.write_text(json.dumps(changed))
        self.decide()
        changed = copy.deepcopy(lock)
        changed['packages']['node_modules/vite']['dependencies']['tinyglobby'] = '0.2.16'
        path.write_text(json.dumps(changed))
        self.decide()
        path.write_bytes(before)

    def test_every_report_leaf_extra_missing_unknown_and_contradictory_field_blocks(self):
        report = recorded_report()
        def leaves(value, path=()):
            if isinstance(value, dict):
                for key, child in value.items(): yield from leaves(child, path + (key,))
            elif isinstance(value, list) and value:
                for index, child in enumerate(value): yield from leaves(child, path + (index,))
            else: yield path
        for path in leaves(report):
            with self.subTest(path=path):
                changed = copy.deepcopy(report)
                parent = changed
                for key in path[:-1]: parent = parent[key]
                value = parent[path[-1]]
                parent[path[-1]] = not value if isinstance(value, bool) else 'unexpected'
                self.set_report(changed, 1)
                self.decide()
        for changed in [{}, {'error': {'code': 'ENETUNREACH'}}, dict(report, error='network'),
                        dict(report, vulnerabilities={}), dict(report, extra=True)]:
            self.set_report(changed, 1)
            self.decide()
        changed = copy.deepcopy(report)
        changed['vulnerabilities']['unknown'] = copy.deepcopy(changed['vulnerabilities']['vite'])
        self.set_report(changed, 1)
        self.decide()
        changed = copy.deepcopy(report)
        del changed['vulnerabilities']['fdir']['effects']
        self.set_report(changed, 1)
        self.decide()
        for field in ['isDirect', 'fixAvailable']:
            changed = copy.deepcopy(report)
            if field == 'fixAvailable':
                changed['vulnerabilities']['vite'][field]['isSemVerMajor'] = 1
            else:
                changed['vulnerabilities']['vite'][field] = 1
            self.set_report(changed, 1)
            self.decide()
        changed = copy.deepcopy(report)
        changed['metadata']['vulnerabilities']['info'] = False
        self.set_report(changed, 1)
        self.decide()

    def test_raw_exit_tool_errors_invalid_and_duplicate_json_fail(self):
        for status in [0, 2, 127, 137]:
            self.set_report(recorded_report(), status)
            decision = self.decide()
            for field in ['web_input_sha256', 'copied_input_sha256', 'build_controls_sha256',
                          'locked_graph_sha256', 'lock_bytes_sha256', 'policy_sha256']:
                self.assertTrue(decision[field])
        self.set_report(recorded_report(), 1)
        versions = self.out / 'npm-audit-tools.json'
        for tools in [dict(node='v26.13.0', npm='11.20.0'), dict(node='v26.13.1', npm='11.20.1'), {}]:
            versions.write_text(json.dumps(tools))
            self.decide()
        self.set_report(recorded_report(), 1)
        self.stderr.write_text('npm error network unavailable\n')
        self.decide()
        self.stderr.write_bytes(b'')
        for raw in [b'', b'{', b'null', b'[]', b'{"auditReportVersion":2,"auditReportVersion":2}',
                    b'{"value":NaN}', b'{"metadata":{"vulnerabilities":{"high":1e999}}}', b'\xff']:
            self.raw.write_bytes(raw)
            self.decide()
        clean = recorded_report()
        clean['vulnerabilities'] = {}
        clean['metadata']['vulnerabilities'] = dict.fromkeys(clean['metadata']['vulnerabilities'], 0)
        self.set_report(clean, 1)
        self.decide()
        self.raw.unlink()
        result = subprocess.run([sys.executable, str(self.helper), 'assess', str(self.web),
                                 str(self.work), str(self.controls), str(self.out)], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        decision = json.loads((self.out / 'npm-audit-decision.json').read_text())
        self.assertEqual(decision['disposition'], 'blocked')
        self.assertIsNone(decision['raw_report_sha256'])


# Only external npm results and side-effect-only notice/metadata/Chromium calls
# are finite fixtures. The shell, copier, assessment and MG owner below are real.
# This does not prove Docker image contents, online audits or package acceptance.
EXTERNAL_COMMAND = r'''#!/usr/bin/python3
import os
from pathlib import Path
import sys

root = Path(os.environ['FRONTEND_CASE'])
args = sys.argv[1:]
def event(name):
    with (root / 'events').open('a') as log:
        log.write(name + '\n')
def reject():
    sys.exit('Unexpected finite frontend command: ' + repr(sys.argv))

if Path(sys.argv[0]).name == 'npm':
    if args == ['--version']:
        print('11.20.0')
    elif args == ['audit', '--fetch-retries=0', '--json']:
        event('audit')
        sys.stdout.buffer.write((root / 'stdout').read_bytes())
        sys.stderr.buffer.write((root / 'stderr').read_bytes())
        sys.exit(int((root / 'exit').read_text()))
    elif args == ['install', '--package-lock-only', '--ignore-scripts', '--save-exact',
                  '--no-audit', '--no-fund', '--engine-strict', '--fetch-retries=0']:
        event('resolve')
        if (root / 'resolved-lock').exists():
            Path('package-lock.json').write_bytes((root / 'resolved-lock').read_bytes())
    elif args in [
        ['ci', '--ignore-scripts', '--no-audit', '--no-fund', '--engine-strict', '--fetch-retries=0'],
        ['ci', '--offline', '--ignore-scripts', '--no-audit', '--no-fund', '--engine-strict', '--fetch-retries=0'],
    ]:
        event('ci')
        browsers = Path('node_modules/playwright-core/browsers.json')
        browsers.parent.mkdir(parents=True)
        browsers.write_text('{"browsers":[]}\n')
    elif args == ['ls', '--all', '--json']:
        event('ls')
        print('{}')
    else:
        reject()
elif Path(sys.argv[0]).name == 'node':
    if args == ['--version'] or args == [
        '-e', 'if (process.version !== "v26.13.1" || process.release.lts !== "Lithium") process.exit(1)'
    ]:
        os.execv(os.environ['FRONTEND_REAL_NODE'], ['node'] + args)
    elif args == [str(root / 'repo/server/web/scripts/copy-source.mjs'),
                  str(root / 'repo/server/web'), str(root / 'work')]:
        event('copy')
        os.execv(os.environ['FRONTEND_REAL_NODE'], ['node'] + args)
    elif args == ['scripts/notices.mjs', str(root / 'work'), str(root / 'out')]:
        event('notices')
        (root / 'out/runtime-notices.txt').write_text('Finite public notices fixture\n')
    elif args == ['scripts/metadata.mjs', str(root / 'work/package-lock.json'),
                  str(root / 'out/npm-metadata.json')]:
        event('metadata')
        (root / 'out/npm-metadata.json').write_text('{}\n')
    elif args == ['node_modules/playwright/cli.js', 'install', 'chromium']:
        event('chromium')
    else:
        reject()
else:
    reject()
'''


MG_OPERATION = r'''
set -euo pipefail
mg_job() { :; }
source "$FRONTEND_CASE/repo/tools/mg.d/sync-web.sh"
MG_ROOT="$FRONTEND_CASE/repo"
MG_RUN_DIR="$FRONTEND_CASE/out"
MG_RUN_ID=finite-frontend-operation
mg_compose_project() { printf '%s\n' finite-frontend; }
docker() {
  [ "$1 $2 $3" = 'image inspect --format' ] || return 97
  printf 'sha256:%064d\n' 1
}
mg_artifact() { printf '%s\n' "$2" >> "$MG_RUN_DIR/artifacts"; }
mg_failure() { printf '%s\n' "$1" >> "$MG_RUN_DIR/failures"; }
mg_status() { printf '%s\n' "$1" > "$MG_RUN_DIR/status"; }
mg_compose() {
  local operation_exit=0
  touch "$FRONTEND_CASE/compose-called"
  # /audit-controls was created exclusively by this dev-container selftest.
  # Substitute only Compose's process launch; add no mounts or containers.
  cp -R "$MG_RUN_DIR/audit-controls/." /audit-controls/
  bash "$FRONTEND_CASE/baked/run.sh" "$FRONTEND_MODE" || operation_exit=$?
  printf '%s\n' "$operation_exit" > "$MG_RUN_DIR/operation-exit"
  return "$operation_exit"
}
mg_sync_web__run sync-web-resolver "$FRONTEND_MODE"
[ "$(cat "$MG_RUN_DIR/status")" = pass ]
'''


class RunOperations(unittest.TestCase):
    def setUp(self):
        # run.sh's fixed container paths are real directories. Refuse existing
        # state, including dangling links; never adopt or delete it. Bats runs
        # this file serially inside the existing disposable MG dev container.
        self.assertEqual(os.environ.get('MG_CONTAINER'), 'dev')
        for name in ['/audit-controls', '/tmp/npm-cache', '/tmp/xdg-cache']:
            path = Path(name)
            path.mkdir()  # Atomic absence proof; no exist_ok and no fallback.
            identity = path.lstat()
            self.addCleanup(self.remove_owned, path, identity)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.sequence = 0

    def remove_owned(self, path, identity):
        current = path.lstat()
        self.assertEqual((current.st_dev, current.st_ino, current.st_mode),
                         (identity.st_dev, identity.st_ino, identity.st_mode))
        shutil.rmtree(path)
        self.assertFalse(os.path.lexists(path))

    def operation(self, mode, scenario='accepted'):
        self.sequence += 1
        root = self.root / str(self.sequence)
        repo = root / 'repo'
        web = repo / 'server/web'
        shutil.copytree(REPO / 'server/web', web,
                        ignore=lambda p, names: GENERATED.intersection(names) if Path(p) == REPO / 'server/web' else [])
        for name in CONTROLS:
            destination = repo / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, destination)
        copy_baked_scripts(repo, root / 'baked')
        out, deps = root / 'out', root / 'deps'
        out.mkdir()
        (deps / 'cache').mkdir(parents=True)
        if mode in ['audit', 'browser-install']:
            for name in ['package.json', 'package-lock.json']:
                shutil.copyfile(web / name, deps / name)
            (deps / 'runtime-notices.txt').write_text('Previously admitted fixture notices\n')
        if scenario == 'installed-lock-mismatch':
            with (deps / 'package-lock.json').open('ab') as lock:
                lock.write(b'\n')
        before_source = {name: (web / name).read_bytes() for name in ['package.json', 'package-lock.json']}
        before_deps = {p.name: p.read_bytes() for p in deps.iterdir() if p.is_file()}
        report, status, stderr = recorded_report(), 1, b''
        if scenario == 'unknown-advisory':
            report['vulnerabilities']['picomatch']['via'][0]['source'] = 1247711
        elif scenario == 'tool-error':
            report = {'error': {'code': 'ENETUNREACH', 'summary': 'Finite offline error'}}
        elif scenario == 'nonzero':
            status = 73
        elif scenario == 'stderr':
            stderr = b'npm error finite diagnostic\n\n'
        raw = ('  ' + json.dumps(report, indent=2) + '\n\n').encode()
        (root / 'stdout').write_bytes(raw)
        (root / 'stderr').write_bytes(stderr)
        (root / 'exit').write_text(str(status))
        if scenario == 'changed-resolve':
            (root / 'resolved-lock').write_bytes(before_source['package-lock.json'] + b'\n')
        if scenario == 'host-control-mutation':
            with (repo / 'docker/sync-web.Dockerfile').open('ab') as control:
                control.write(b'\n')
        if scenario == 'generated-roots':
            for name in GENERATED:
                generated = web / name
                generated.mkdir()
                (generated / 'omitted-sentinel').write_text('generated fixture')
                (generated / 'omitted-link').symlink_to(web / 'src')
        if scenario == 'root-link':
            (web / 'node_modules').symlink_to(web / 'src')
        binary = root / 'bin'
        binary.mkdir()
        for name in ['npm', 'node']:
            command = binary / name
            command.write_text(EXTERNAL_COMMAND)
            command.chmod(0o755)
        env = dict(os.environ, FRONTEND_CASE=str(root), FRONTEND_MODE=mode,
                   FRONTEND_REAL_NODE=shutil.which('node'), PATH=str(binary) + os.pathsep + os.environ['PATH'],
                   MEGURI_WEB_SOURCE=str(web), MEGURI_WEB_WORK=str(root / 'work'),
                   MEGURI_WEB_OUTPUT=str(out), MEGURI_WEB_DEPS=str(deps), PYTHONDONTWRITEBYTECODE='1')
        result = subprocess.run(['bash', '-c', MG_OPERATION], env=env, capture_output=True)
        accepted = scenario in ['accepted', 'generated-roots']
        self.assertEqual(result.returncode == 0, accepted, result.stderr.decode())
        self.assertEqual((out / 'status').read_text(), 'pass\n' if accepted else 'fail\n')
        self.assertEqual({name: (web / name).read_bytes() for name in before_source}, before_source)
        events = (root / 'events').read_text().splitlines() if (root / 'events').exists() else []
        preaudit_failure = scenario in ['installed-lock-mismatch', 'host-control-mutation', 'root-link']
        if preaudit_failure:
            self.assertNotIn('audit', events)
            self.assertFalse((out / 'npm-audit.json').exists())
            self.assertFalse((out / 'npm-audit-decision.json').exists())
            if scenario == 'installed-lock-mismatch':
                self.assertEqual(events, ['copy'])
        else:
            self.assertEqual(events.count('copy'), 1)
            self.assertEqual(events.count('audit'), 1)
            self.assertEqual((out / 'npm-audit.json').read_bytes(), raw)
            self.assertEqual((out / 'npm-audit-stderr.txt').read_bytes(), stderr)
            self.assertEqual((out / 'npm-audit-exit.txt').read_bytes(), (str(status) + '\n').encode())
            self.assertEqual(json.loads((out / 'npm-audit-tools.json').read_text()),
                             dict(node='v26.13.1', npm='11.20.0'))
            decision = json.loads((out / 'npm-audit-decision.json').read_text())
            self.assertEqual(decision['raw_exit'], status)
            self.assertEqual(decision['raw_report_sha256'], hashlib.sha256(raw).hexdigest())
            self.assertEqual(decision['vulnerability_counts'], report.get('metadata', {}).get('vulnerabilities'))
            self.assertEqual(decision['disposition'], 'assessed-inapplicable' if accepted else 'blocked')
            self.assertIn(str(out / 'npm-audit-decision.json'), (out / 'artifacts').read_text().splitlines())
        if accepted:
            self.assertEqual((out / 'operation-exit').read_text(), '0\n')
            self.assertEqual(decision['vulnerability_counts']['high'], 4)
            self.assertFalse((out / 'failures').exists())
            if mode in ['install', 'resolve']:
                for name, data in before_source.items():
                    self.assertEqual((deps / name).read_bytes(), data)
                self.assertEqual((deps / 'runtime-notices.txt').read_bytes(), (out / 'runtime-notices.txt').read_bytes())
            if mode != 'browser-install':
                self.assertIn('metadata', events)
                self.assertLess(events.index('audit'), events.index('metadata'))
                self.assertTrue((out / 'npm-metadata.json').is_file())
            else:
                self.assertIn('chromium', events)
                self.assertLess(events.index('audit'), events.index('notices'))
                self.assertLess(events.index('audit'), events.index('chromium'))
                self.assertTrue((out / 'browsers.json').is_file())
        else:
            self.assertNotIn('metadata', events)
            self.assertNotIn('chromium', events)
            self.assertFalse((out / 'npm-metadata.json').exists())
            self.assertFalse((out / 'browsers.json').exists())
            self.assertEqual({p.name: p.read_bytes() for p in deps.iterdir() if p.is_file()}, before_deps)
            failures = (out / 'failures').read_text().splitlines()
            self.assertIn('sync-web-execution', failures)
            if not preaudit_failure:
                self.assertNotEqual((out / 'operation-exit').read_text(), '0\n')
                self.assertNotIn('sync-web-audit-decision', failures)
                self.assertIn(str(out), (out / 'artifacts').read_text().splitlines())
                if mode == 'browser-install':
                    self.assertNotIn('notices', events)
        if mode == 'resolve':
            self.assertIn('resolve', events)
            expected = (root / 'resolved-lock').read_bytes() if scenario == 'changed-resolve' else before_source['package-lock.json']
            self.assertEqual((out / 'package-lock.json').read_bytes(), expected)
        if scenario == 'generated-roots':
            for name in GENERATED:
                self.assertFalse((root / 'work' / name / 'omitted-sentinel').exists())
                self.assertFalse(os.path.lexists(root / 'work' / name / 'omitted-link'))
            self.assertEqual((root / 'work/src/api.ts').read_bytes(), (web / 'src/api.ts').read_bytes())
        if scenario == 'host-control-mutation':
            self.assertFalse((root / 'compose-called').exists())
            self.assertEqual(events, [])
        return root

    def test_real_operations_capture_reject_publish_and_propagate(self):
        for mode in ['resolve', 'install', 'audit', 'browser-install']:
            with self.subTest(mode=mode, scenario='accepted'):
                self.operation(mode)
        # Three real audit call sites: shared install/resolve, audit, browser.
        for mode in ['install', 'audit', 'browser-install']:
            for scenario in ['unknown-advisory', 'tool-error', 'nonzero', 'stderr']:
                with self.subTest(mode=mode, scenario=scenario):
                    self.operation(mode, scenario)
        for mode, scenario in [('resolve', 'changed-resolve'), ('audit', 'installed-lock-mismatch'),
                               ('install', 'host-control-mutation'), ('install', 'generated-roots'),
                               ('install', 'root-link')]:
            with self.subTest(mode=mode, scenario=scenario):
                root = self.operation(mode, scenario)
                if scenario == 'root-link':
                    # The real caller rejects before copying. Also exercise the
                    # real copier's own link rejection, without the node fixture.
                    copied = subprocess.run([shutil.which('node'), str(root / 'repo/server/web/scripts/copy-source.mjs'),
                                             str(root / 'repo/server/web'), str(root / 'link-copy')], capture_output=True)
                    self.assertNotEqual(copied.returncode, 0)
                    self.assertIn(b'Linked build input', copied.stderr)


if __name__ == '__main__':
    unittest.main()
