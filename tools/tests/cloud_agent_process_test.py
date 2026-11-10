#!/usr/bin/env python3
"""Generated authority models and one Docker-only child; no VM or SDK work."""
import contextlib
import ctypes
import errno
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import unittest
from types import SimpleNamespace
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('group_subject',ROOT/'tools/scripts/cloud-agent-handoff.py')
h=importlib.util.module_from_spec(spec);sys.modules[spec.name]=h;spec.loader.exec_module(h)

class Model:
    def __init__(self):self.running=False;self.quiescent=True;self.code=23;self.calls=[];self.fault=False
    def terminal(self,pid):
        self.calls.append('terminal')
        if self.fault:raise h.v.Refused('setup-join-uncertain')
        return None if self.running else self.code
    def quiet(self,pid,terminal):self.calls.append('quiet');return terminal is not None and self.quiescent
    def owned(self,pid,terminal,start_stamp):self.calls.append('owned');return True,(1,2)

class ManagedTests(unittest.TestCase):
    def child(self,model=None):
        m=model or Model();c=h.SetupChild.__new__(h.SetupChild)
        c._setup_ready=True;c._setup_kernel=m;c._setup_lock=threading.RLock()
        c._setup_pid=c.pid=123;c._setup_joined=c._setup_reaped=c._setup_invalid=False;c._setup_start_stamp=None
        c.returncode=None;c.args=['generated'];c.stdin=c.stdout=c.stderr=None;c._sigint_wait_secs=0.25
        h.SetupChild.originals[c.pid]=c;self.addCleanup(lambda:h.SetupChild.originals.pop(c.pid,None))
        return c,m
    def guards(self):return patch.object(h,'setup_launch',return_value=contextlib.nullcontext())
    def test_terminal_alone_is_not_completion_and_quiet_reaps_once(self):
        child,model=self.child();model.quiescent=False
        with self.guards(),patch.object(h.os,'waitpid',return_value=(123,23<<8)) as reap,patch.object(h.os,'killpg') as kill:
            self.assertIsNone(child.poll());self.assertIsNone(child.returncode);reap.assert_not_called()
            model.quiescent=True;self.assertEqual(child.wait(timeout=180),23)
            count=len(model.calls);self.assertEqual(child.poll(),23);self.assertEqual(child.wait(timeout=0),23)
            child.join_original();self.assertEqual(len(model.calls),count);reap.assert_called_once_with(123,0);kill.assert_not_called()
    def test_echild_mismatch_and_lost_authority_never_become_success(self):
        for outcome in ('echild','wrong-pid','wrong-status','authority'):
            child,model=self.child();model.fault=outcome=='authority'
            result=(124,23<<8) if outcome=='wrong-pid' else (123,0)
            with self.guards(),patch.object(h.os,'waitpid',side_effect=ChildProcessError(errno.ECHILD,'generated') if outcome=='echild' else None,return_value=result) as reap:
                with self.assertRaises((h.v.Refused,OSError)):child.poll()
                self.assertIsNone(child.returncode);self.assertTrue(child._setup_invalid)
                calls=len(model.calls)
                with self.assertRaises(h.v.Refused):child._internal_poll(_deadstate=0)
                self.assertEqual(len(model.calls),calls)
                if outcome=='authority':reap.assert_not_called()
    def test_private_wait_paths_and_destructor_cannot_reap_early(self):
        child,model=self.child()
        with patch.object(h.os,'waitpid') as reap:
            with self.assertRaises(h.v.Refused):child._try_wait(os.WNOHANG)
            with self.assertRaises(h.v.Refused):child._handle_exitstatus(0)
            child.__del__();reap.assert_not_called();self.assertIsNone(child.returncode)
    def test_context_exit_uses_verified_managed_wait(self):
        child,_=self.child()
        with self.guards(),patch.object(h.os,'waitpid',return_value=(123,23<<8)) as reap:
            with child:pass
            self.assertTrue(child._setup_joined);reap.assert_called_once_with(123,0)

    def test_keyboard_interrupt_context_exit_cannot_use_an_early_reap(self):
        child,model=self.child();model.quiescent=False
        with patch.object(h.os,'waitpid') as reap,patch.object(h.time,'sleep'):
            child._sigint_wait_secs=0
            with self.assertRaises(KeyboardInterrupt):
                with child:raise KeyboardInterrupt()
            self.assertFalse(child._setup_reaped);self.assertIsNone(child.returncode);reap.assert_not_called()

    def test_cancel_during_final_reap_retains_joined_identity(self):
        child,_=self.child();handlers={number:signal.SIG_DFL for number in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP)}
        def handler(number,value):
            previous=handlers[number];handlers[number]=value;return previous
        def reap(pid,options):
            handlers[signal.SIGTERM](signal.SIGTERM,None)
            return pid,23<<8
        with patch.object(h.signal,'signal',side_effect=handler),patch.object(h.os,'waitpid',side_effect=reap) as waitpid,patch.object(h.os,'killpg') as kill:
            with self.assertRaisesRegex(h.v.Refused,'setup-cancelled'):child.poll()
            self.assertTrue(child._setup_joined);self.assertFalse(child._setup_invalid)
            self.assertEqual(child.returncode,23);h.setup_join(child)
            waitpid.assert_called_once_with(123,0);kill.assert_not_called()

    def test_cancel_during_nonreaping_observation_keeps_cleanup_authority(self):
        child,model=self.child()
        with patch.object(model,'terminal',side_effect=h.v.Refused('setup-cancelled')),patch.object(h.os,'waitpid') as reap:
            with self.assertRaisesRegex(h.v.Refused,'setup-cancelled'):child.poll()
            self.assertFalse(child._setup_invalid);self.assertFalse(child._setup_reaped)
            reap.assert_not_called()
        with self.guards(),patch.object(h.os,'waitpid',return_value=(123,23<<8)) as reap,patch.object(h.os,'killpg') as kill:
            h.setup_join(child)
            self.assertTrue(child._setup_joined);reap.assert_called_once_with(123,0);kill.assert_not_called()
    def test_existing_term_timeout_kill_and_final_wait_budgets(self):
        child,model=self.child();model.running=True;model.quiescent=False;signals=[];waits=[]
        def signal_group(pid,number):
            self.assertEqual(pid,123);self.assertFalse(child._setup_reaped);signals.append(number)
            if number==signal.SIGKILL:model.running=False;model.quiescent=True
        def wait(timeout=None):
            waits.append(timeout)
            if timeout==10:raise subprocess.TimeoutExpired('generated',10)
            return h.SetupChild.wait(child,timeout)
        with self.guards(),patch.object(child,'wait',side_effect=wait),patch.object(h.os,'killpg',side_effect=signal_group),patch.object(h.os,'waitpid',return_value=(123,23<<8)) as reap:
            child.join_original()
        self.assertEqual(signals,[signal.SIGTERM,signal.SIGKILL]);self.assertEqual(waits,[10,None]);reap.assert_called_once_with(123,0)
    def test_signal_eperm_requires_fresh_quiet_and_actual_reap(self):
        for quiet in (False,True):
            child,model=self.child();model.running=True;model.quiescent=False
            def signal_group(pid,number):
                model.running=False;model.quiescent=quiet;raise PermissionError(errno.EPERM,'generated-private')
            with self.guards(),patch.object(h.os,'killpg',side_effect=signal_group),patch.object(h.os,'waitpid',return_value=(123,23<<8)) as reap:
                if quiet:child.join_original();self.assertTrue(child._setup_joined);reap.assert_called_once()
                else:
                    with self.assertRaises(h.v.Refused):child.join_original()
                    reap.assert_not_called();self.assertIsNone(child.returncode)
                    self.assertTrue(child._setup_invalid)
                    with self.assertRaises(h.v.Refused):child._setup_signal(signal.SIGKILL)
                    reap.assert_not_called()

    def test_uncertain_join_cannot_become_a_queued_user_step(self):
        class UserSetup(h.v.Refused):pass
        primary=UserSetup('generated-owner-step');child,model=self.child();model.fault=True
        with patch.object(h.signal,'signal'),patch.object(h.os,'killpg') as kill,patch.object(h.os,'waitpid') as reap:
            try:
                raise primary
            except UserSetup:
                with self.assertRaises(h.v.Refused) as error:h.setup_join(child)
            self.assertIs(type(error.exception),h.v.Refused)
            self.assertEqual(error.exception.args,('setup-join-uncertain',))
            self.assertIs(error.exception.__cause__,primary)
            self.assertTrue(child._setup_invalid);self.assertFalse(child._setup_joined)
            kill.assert_not_called();reap.assert_not_called()

    def test_incomplete_owned_sample_stays_failed_without_signalling(self):
        child,model=self.child();model.running=True
        with patch.object(model,'owned',return_value=(False,None)),patch.object(h.os,'killpg') as kill,patch.object(h.os,'waitpid') as reap:
            with self.assertRaises(h.v.Refused):child._setup_signal(signal.SIGTERM)
            model.running=False;model.quiescent=True
            with self.assertRaises(h.v.Refused):child.join_original()
            self.assertTrue(child._setup_invalid);kill.assert_not_called();reap.assert_not_called()

    def test_kernel_failure_happens_before_any_child_is_spawned(self):
        with patch.object(h,'SetupKernel',side_effect=h.v.Refused('setup-join-uncertain')),patch.object(subprocess.Popen,'__init__') as spawn:
            with self.assertRaises(h.v.Refused):h.SetupChild(['generated'],start_new_session=True)
            spawn.assert_not_called()
    @unittest.skipUnless(sys.platform.startswith('linux') and hasattr(os,'waitid'),'Docker-only inherited communicate check')
    def test_inherited_communicate_waits_for_quiet_and_real_reap(self):
        events=[]
        class LinuxModel:
            def terminal(self,pid):
                value=os.waitid(os.P_PID,pid,os.WEXITED|os.WNOHANG|os.WNOWAIT)
                return None if value is None else value.si_status
            def quiet(self,pid,status):
                if status is not None:events.append('proved-quiet')
                return status is not None
            def owned(self,pid,status,start_stamp):return True,(1,2)
        with patch.object(h,'SetupKernel',return_value=LinuxModel()):
            child=h.SetupChild([sys.executable,'-c',"import sys; sys.stdin.buffer.read(); sys.stdout.buffer.write(b'out'); sys.stderr.buffer.write(b'err'); sys.exit(23)"],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
            try:
                self.assertEqual(child.communicate(b'generated',timeout=5),(b'out',b'err'))
                self.assertEqual(child.returncode,23);self.assertTrue(child._setup_joined);self.assertIn('proved-quiet',events)
            finally:h.setup_join(child)

    def test_timeout_preserves_original_without_reap(self):
        child,model=self.child();model.running=True
        with patch.object(h.os,'waitpid') as reap:
            with self.assertRaises(subprocess.TimeoutExpired) as error:child.wait(timeout=0)
            self.assertEqual(error.exception.timeout,0);reap.assert_not_called();self.assertIsNone(child.returncode)

class KernelTests(unittest.TestCase):
    def kernel(self):return h.SetupKernel.__new__(h.SetupKernel)
    def test_verified_darwin_layout(self):
        self.assertEqual(ctypes.sizeof(h.SetupSigInfo),104);self.assertEqual(ctypes.sizeof(h.SetupBSDInfo),136)
        self.assertEqual(h.SetupBSDInfo.pgid.offset,100);self.assertEqual(h.SetupSigInfo.status.offset,20)
    def test_waitid_zero_uid_is_not_identity_and_echild_refuses(self):
        kernel=self.kernel()
        def waitid(kind,pid,pointer,flags):
            self.assertEqual((kind,pid,flags),(1,123,37));x=pointer._obj;x.signo=signal.SIGCHLD;x.pid=pid;x.code=1;x.status=23;x.uid=0;return 0
        kernel.lib=SimpleNamespace(waitid=waitid);self.assertEqual(kernel.terminal(123),23)
        def lost(*args):ctypes.set_errno(errno.ECHILD);return -1
        kernel.lib.waitid=lost
        with self.assertRaises(h.v.Refused):kernel.terminal(123)
    def test_group_count_not_bytes_and_bad_or_incomplete_members_refuse(self):
        kernel=self.kernel()
        for members,count in (((123,),1),((123,123),2),((0,),1),((456,),1),((),0),((),h.SetupKernel.CAPACITY), ((),-1)):
            def listing(pid,values,size):
                self.assertEqual(pid,123);self.assertEqual(size,h.SetupKernel.CAPACITY*4)
                for n,value in enumerate(members):values[n]=value
                return count
            kernel.lib=SimpleNamespace(proc_listpgrppids=listing)
            if members==(123,) and count==1:self.assertEqual(kernel.members(123),{123})
            else:
                with self.assertRaises(h.v.Refused):kernel.members(123)
    def test_quiet_requires_terminal_eperm_and_exact_original_only(self):
        kernel=self.kernel()
        with patch.object(kernel,'members',return_value={123}),patch.object(h.os,'killpg',side_effect=PermissionError(errno.EPERM,'generated')):
            self.assertFalse(kernel.quiet(123,None));self.assertTrue(kernel.quiet(123,23))
        with patch.object(kernel,'members',return_value={123}),patch.object(h.os,'killpg',side_effect=ProcessLookupError(errno.ESRCH,'generated')):
            with self.assertRaises(h.v.Refused):kernel.quiet(123,23)
        with patch.object(kernel,'members',return_value={123,456}),patch.object(kernel,'owned',side_effect=h.v.Refused('setup-join-uncertain')),patch.object(h.os,'killpg',side_effect=PermissionError(errno.EPERM,'generated')):
            with self.assertRaises(h.v.Refused):kernel.quiet(123,23)
    def test_changed_or_incomplete_group_facts_refuse(self):
        kernel=self.kernel();own=os.getuid()
        info=SimpleNamespace(uid=own,ruid=own,saveduid=own,pgid=123,ppid=os.getpid(),flags=32,seconds=1,microseconds=2)
        for case in ('foreign','moved','unknown','start_stamp','changed'):
            value=SimpleNamespace(**vars(info))
            if case=='foreign':value.uid=own+1
            if case=='moved':value.pgid=999
            if case=='unknown':value=None
            with patch.object(kernel,'members',side_effect=[{123},{123,456}] if case=='changed' else lambda pid:{123}),patch.object(kernel,'member',return_value=value):
                if case in ('foreign','start_stamp'):
                    with self.assertRaises(h.v.Refused):kernel.owned(123,None,(9,9) if case=='start_stamp' else None)
                else:self.assertFalse(kernel.owned(123,None,None)[0])

if __name__=='__main__':unittest.main()
