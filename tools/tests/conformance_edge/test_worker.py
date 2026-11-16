"""Factory launcher lifecycle with a local daemon model, without Docker/oracle."""
import json
import os
import pathlib
import select
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_control import ControlEdges

ROOT = pathlib.Path(__file__).resolve().parents[3]
OWNER = 'a' * 32

# Only the daemon boundary is modeled: the production launcher, owner admission,
# edge checks, inherited pipes and independent launcher processes stay real.
DOCKER = r'''#!/usr/bin/env python3
import fcntl,json,os,pathlib,sys
root=pathlib.Path(os.environ['WORKER_MODEL'])
a=sys.argv[1:]
with (root/'lock').open('a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 s=json.loads((root/'daemon.json').read_text())
 s['calls'].append(a)
 out=''
 if a[0]=='compose':
  name=a[a.index('--name')+1]
  if any(c['Name']=='/'+name for c in s['containers'].values()): sys.exit(1)
  labels=dict(a[i+1].split('=',1) for i,x in enumerate(a) if x=='--label')
  token=labels['meguri.mg.worker']; cid=token*2
  s['containers'][cid]={'Id':cid,'Name':'/'+name,'Image':'sha256:'+'1'*64,
   'State':{'Running':True},'HostConfig':{'AutoRemove':False},
   'Config':{'Labels':labels,'Hostname':token[:12]},
   'NetworkSettings':{'Networks':{'test-project_oracle-internal':{
    'IPAddress':'172.20.0.'+str(10+len(s['containers'])),'Aliases':[name]}}}}
 elif a[0]=='inspect' and '--format' in a:
  assert a[-1]==s['service']['Id']
  out=json.dumps(s['service'])
 elif a[:2]==['network','inspect']:
  out=json.dumps(s['network'])
 elif a[0]=='inspect':
  found=[c for cid,c in s['containers'].items() if cid==a[1] or c['Name']=='/'+a[1]]
  if not found: sys.exit(1)
  out=json.dumps(found)
 elif a[0]=='ps' or a[:2]==['network','ls']:
  filters=[a[i+1] for i,x in enumerate(a) if x=='--filter']
  def matches(cid,c):
   for f in filters:
    if f.startswith('label='):
     k,v=f[6:].split('=',1)
     if c['Config']['Labels'].get(k)!=v: return False
    elif f.startswith('id=') and cid!=f[3:]: return False
   return True
  if 'label=com.docker.compose.service=sync-faults' in filters:
   out=s['service']['Id']
  else:
   out='\n'.join(cid for cid,c in s['containers'].items() if matches(cid,c)) if a[0]=='ps' else ''
 elif a[0]=='run':
  assert a[a.index('--network')+1]=='container:'+s['service']['Id']
  assert a[-3]=='127.0.0.1:8474'
  out=json.dumps({'status':200,'body':json.dumps(s['proxies'])})
 elif a[0]=='rm':
  for cid in a[2:]: s['containers'].pop(cid,None)
 elif a[0]!='exec': raise AssertionError(a)
 (root/'daemon.json').write_text(json.dumps(s))
if a[0]=='exec':
 assert a[1]=='-i' and a[-2:]==['/opt/oracle/entrypoint.sh','worker']
 # Byte-transparent stand-in, including a distinct stderr stream.
 os.write(2,b'driver stderr\n')
 for line in sys.stdin.buffer:
  sys.stdout.buffer.write(line); sys.stdout.buffer.flush()
 sys.exit(int(os.environ.get('WORKER_EXIT','0')))
print(out)
'''


class WorkerLifecycle(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        helpers = self.root / 'tools/mg.d'
        helpers.mkdir(parents=True)
        for name in ('conformance-worker.py', 'conformance-admission.py', 'conformance-edge.py'):
            shutil.copy2(ROOT / 'tools/mg.d' / name, helpers)
        binary = self.root / 'bin'
        binary.mkdir()
        docker = binary / 'docker'
        docker.write_text(DOCKER)
        docker.chmod(0o700)
        self.env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ['PATH'],
                        WORKER_MODEL=str(self.root))
        control = ControlEdges()
        control.temporary.cleanup()
        control.service['Labels'].update({
            'com.docker.compose.project.config_files': str(self.root / 'docker/compose.yml'),
            'com.docker.compose.project.working_dir': str(self.root / 'docker')})
        network = {k: v for k, v in control.network.items() if k != 'Containers'}
        network['Binding'] = control.network['Containers'][control.service['Id']]
        self.write({'containers': {}, 'proxies': {}, 'calls': [],
                    'service': control.service, 'network': network})
        self.children = []
        self.addCleanup(self.stop)

    def read(self):
        return json.loads((self.root / 'daemon.json').read_text())

    def write(self, state):
        (self.root / 'daemon.json').write_text(json.dumps(state))

    def stop(self):
        for child in self.children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)
            for pipe in (child.stdin, child.stdout, child.stderr):
                if pipe and not pipe.closed:
                    pipe.close()

    def start(self, index, exit_code=0):
        token = f'{index:032x}'
        command = [sys.executable, '-I', '-B', str(self.root / 'tools/mg.d/conformance-worker.py'),
                   '--root', str(self.root), '--project', 'test-project', '--run', 'test-run',
                   '--owner', OWNER, '--worker', token, '--handoff', str(self.root / 'handoff'),
                   '--faults', 'sync-faults:8474', '--tz', 'UTC' if index % 2 else 'Europe/Paris']
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 env=dict(self.env, WORKER_EXIT=str(exit_code)))
        self.children.append(child)
        return token, child

    def hello(self, child):
        payload = b'{"cmd":"hello", "opaque":"\\u0000"}\n'
        child.stdin.write(payload); child.stdin.flush()
        self.assertTrue(select.select([child.stdout], [], [], 5)[0], 'driver pipe stalled')
        self.assertEqual(child.stdout.readline(), payload)

    def finish(self, child):
        child.stdin.close()
        result = child.wait(timeout=5)
        self.assertEqual(child.stderr.read(), b'driver stderr\n')
        return result

    def test_successful_sequential_factory_retirement_bounds_held_namespaces(self):
        # More than three sequential timezone lifetimes, with a second driver
        # alive throughout. Baseline execvp leaves all seven holders behind.
        peer, active = self.start(99)
        self.hello(active)
        retired = []
        for index in range(1, 7):
            token, child = self.start(index)
            self.hello(child)
            self.assertEqual(self.finish(child), 0)
            retired.append(token * 2)
            self.hello(active)
        state = self.read()
        self.assertEqual(set(state['containers']), {peer * 2})
        self.assertTrue(all(cid not in state['containers'] for cid in retired))
        self.assertFalse((self.root / 'build/mg/conformance-routes' / OWNER / 'closing').exists())
        self.assertEqual(self.finish(active), 0)
        self.assertEqual(self.read()['containers'], {})

    def test_nonzero_and_cancelled_driver_preserve_holder(self):
        for index, code in ((1, 7), (2, -1)):
            token, child = self.start(index, max(0, code))
            self.hello(child)
            if code < 0:
                child.terminate()
                child.wait(timeout=5)
            else:
                self.assertEqual(self.finish(child), code)
            self.assertIn(token * 2, self.read()['containers'])
        self.assertFalse(any(c[0]=='rm' for c in self.read()['calls']))

    def test_retired_factory_identity_cannot_recreate_namespace(self):
        token, child = self.start(1)
        self.hello(child)
        self.assertEqual(self.finish(child), 0)
        self.assertNotIn(token * 2, self.read()['containers'])
        before = self.read()['calls']
        _, late = self.start(1)
        out, err = late.communicate(timeout=5)
        self.assertNotEqual(late.returncode, 0)
        self.assertEqual(out, b'')
        self.assertEqual(err, b'conformance worker unavailable\n')
        self.assertEqual(self.read()['calls'], before)

    def test_success_with_active_proxy_preserves_namespace_and_foreign_resources(self):
        token, child = self.start(1)
        self.hello(child)
        foreign, other = self.start(2)
        self.hello(other)
        state = self.read()
        state['proxies']['foreign-proxy'] = {'name':'foreign-proxy', 'listen':'0.0.0.0:30001',
            'upstream':state['containers'][token * 2]['NetworkSettings']['Networks'][
                'test-project_oracle-internal']['IPAddress'] + ':27701'}
        self.write(state)
        child.stdin.close(); child.wait(timeout=5)
        after = self.read()
        self.assertIn(token * 2, after['containers'])
        self.assertEqual(after['containers'][foreign * 2], state['containers'][foreign * 2])
        self.assertEqual(after['proxies'], state['proxies'])
        self.assertFalse(any(c[0]=='rm' for c in after['calls']))
        self.hello(other)
