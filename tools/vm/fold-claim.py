"""M5 scheduling boundary; the deployed resident components stay unchanged.

Admission uses MG's one keeper. Fresh challenges on its original SSH stream
prove its current acquisition without taking a second claim, including after
boot and immediately before input/shutdown publication. Lost authority refuses
new work; the existing token-bound terminal/join paths remain available.
"""
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shlex
import socket
import signal
import subprocess
import sys
import time


class Claim:
    def __init__(self, lease, ssh):
        self.lease, self.ssh = Path(lease), ssh
        self.state = Path(str(lease) + ".claim")
        self.identity = self.witness()
        owner = os.environ.get("MEGURI_FOLD_SCHEDULER_PID")
        if owner is not None and json.loads(self.identity[2]).get("pid") != int(owner):
            raise RuntimeError("fold scheduler no longer owns its lease")

    def witness(self):
        info = self.lease.stat()
        return info.st_dev, info.st_ino, (self.lease / "holder.json").read_bytes()

    def scheduler_check(self):
        if self.witness() != self.identity:
            raise RuntimeError("fold scheduler lease changed; no new guest work")

    def check(self):
        self.scheduler_check()
        raw = self.state.read_bytes()
        record = json.loads(raw)
        if (not isinstance(record, dict) or record.get("state") != "taken"
                or not record.get("proof") or not record.get("acquisition")):
            # Old keepers still serve ordinary master jobs. Yield this fold's
            # local admission so they can release naturally; never replace one.
            raise RuntimeError("keeper cannot prove fold admission; wait for natural release")
        challenge = secrets.token_hex(32)
        request = dict(token=record["token"], acquisition=record["acquisition"], challenge=challenge)
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as channel:
                channel.settimeout(2)
                channel.connect(record["proof"])
                channel.sendall((json.dumps(request) + "\n").encode())
                with channel.makefile("rb") as source:
                    reply = json.loads(source.readline(1025))
        except (OSError, ValueError):
            raise RuntimeError("keeper did not freshly acknowledge fold admission") from None
        if reply != request or self.state.read_bytes() != raw:
            raise RuntimeError("fold VM claim lost; wait for admission before new guest work")
        self.scheduler_check()


def guarded_modules(claim, ssh):
    """Adapt only the coordinator's calls, without changing the resident kit."""
    modules = {}

    def checked(function):
        def call(*args, **kwargs):
            claim.check()
            return function(*args, **kwargs)
        return call

    class Processes:
        def __init__(self, module):
            self.module = module

        def __getattr__(self, name):
            original = getattr(subprocess, name)
            if name not in ("Popen", "run", "check_output", "check_call"):
                return original

            def launch(argv, *args, **kwargs):
                # This fixed token-bound terminal request only cancels/releases
                # our existing guard. Preserve its join path after claim loss.
                terminal = (getattr(self.module, "TERMINAL", None)
                            and len(argv) == 2 and argv[0] == ssh
                            and shlex.split(argv[1])[2:3] == [self.module.TERMINAL])
                if not terminal and (argv[0] == ssh or (len(argv) > 1 and argv[1] == "stop")):
                    claim.check()
                return original(argv, *args, **kwargs)
            return launch

    def load(name):
        if name in modules:
            return modules[name]
        spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        modules[name] = module
        module.subprocess = Processes(module)
        if hasattr(module, "sibling"):
            module.sibling = load
        for attribute, sibling in (("session", "vnc-session"),
                                   ("inspection", "sim-posture-inspect"),
                                   ("wrapper", "remote-wrapper")):
            if hasattr(module, attribute):
                setattr(module, attribute, load(sibling))
        if name == "vnc-session":
            module.hosted_module = lambda: load("vnc-hosted")
            module.ShutdownFence.arm = checked(module.ShutdownFence.arm)
        elif name == "vnc-hosted":
            rpc = module.rpc

            def guarded_rpc(tart, message, **kwargs):
                operation = message.get("operation", {}).get("op")
                if message.get("op") == "drained-stop" or operation in (
                        "capture", "move-capture", "reserve"):
                    claim.check()
                return rpc(tart, message, **kwargs)
            module.rpc = guarded_rpc
            remote_holder = module.RemoteHolder

            def guarded_holder(*args, **kwargs):
                holder = remote_holder(*args, **kwargs)
                publish = holder.publish

                def guarded_publish(fd, payload):
                    try:
                        claim.check()
                    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError):
                        return 0  # No RPC sent; existing guard can disarm.
                    return publish(fd, payload)
                holder.publish = guarded_publish
                return holder
            module.RemoteHolder = guarded_holder
        elif name == "sim-posture":
            module.Holder.batch = checked(module.Holder.batch)
            guest_call = module.Guest._call

            def guarded_call(guest, action, **values):
                # Release/join only the already admitted guard after loss.
                if action not in ("disarm", "finish"):
                    claim.check()
                return guest_call(guest, action, **values)
            module.Guest._call = guarded_call
            publish = module.Holder.publish

            def guarded_publish(holder, fd, payload):
                try:
                    claim.check()
                except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError):
                    return 0
                deadline = float(payload.split()[1])
                if time.monotonic() >= deadline:
                    return 0  # Proven no publication; existing cleanup disarms.
                return publish(holder, fd, payload)
            module.Holder.publish = guarded_publish
            # The resident's existing ticket enforces the five-second bound.
        elif name == "sim-posture-inspect":
            module.capture = checked(module.capture)
        return module

    return load


class Cancelled(BaseException):
    """Must escape the resident client's RuntimeError reconciliation loops."""


def lifecycle_start(claim, script, arguments):
    """Explicit M5 start boundary. The stopped guest cannot answer a proof.

    Bash tracks this controller just like input/shutdown controllers. Signals
    abort it even if it was paused before publication; a detached owner or
    published hosted start retains its original transaction for status/cleanup.
    No resident code or per-module subprocess wrapper is changed for starts.
    """
    claim.scheduler_check()
    spec = importlib.util.spec_from_file_location("fold_start", script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    action, path, *args = arguments
    state = Path(path)
    if Path(script).stem == "vnc-session" and action == "start":
        tart, vm, *_ = args
    elif Path(script).stem == "vnc-hosted" and action == "prepare" and args[-1] == "start":
        tart, vm, *_ = args
    else:
        raise ValueError("unsupported lifecycle start")
    session = module.session if hasattr(module, "session") else module
    # A joined owned drain plus positive stopped state replaces guest authority
    # only for this start. Newly booted guest work must pass master admission.
    with session.pinned_record(state / "recovery-pending") as (token, recovery), \
            session.pinned_record(state / "drain-token") as (drain, drain_info):
        if (not token or token != drain or any(os.path.lexists(state / name)
                for name in ("shutdown-pending", "drain-precommit"))):
            raise RuntimeError("owned drain completion unconfirmed")
        listed = json.loads(subprocess.check_output([tart, "list", "--format", "json"], timeout=10))
        if [entry.get("State") for entry in listed if entry.get("Name") == vm] != ["stopped"]:
            raise RuntimeError("start requires positively stopped guest")
        session.unchanged_record(state / "recovery-pending", recovery)
        session.unchanged_record(state / "drain-token", drain_info)
        claim.scheduler_check()
        sys.argv = [script, *arguments]
        return module.main()


def main():
    action = None
    try:
        def interrupted(*unused):
            if action == "start":
                raise Cancelled()
            raise RuntimeError("fold controller cancelled")
        for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
            signal.signal(sig, interrupted)
        action, lease, ssh, *args = sys.argv[1:]
        claim = Claim(lease, ssh)
        if action == "start":
            return lifecycle_start(claim, args[0], args[1:])
        claim.check()
        if action == "check":
            return 0
        if action != "run":
            raise ValueError()
        script, *arguments = args
        module = guarded_modules(claim, ssh)(Path(script).stem)
        sys.argv = [script, *arguments]
        return module.main()
    except (Cancelled, OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError):
        print("duo-fold: claim admission lost or unconfirmed, or controller cancelled; wait for natural release if keeper cannot prove admission; no new guest work; retain existing fences", file=sys.stderr)
        # A start that never reached a normal resident result is not authority
        # for legacy recovery. Keep it distinct from module.main()'s failure.
        return 125 if action == "start" else 1


if __name__ == "__main__":
    sys.exit(main())
