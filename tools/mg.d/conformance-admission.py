"""Keep the fault service intact while route reservations remain on disk."""
import argparse
import contextlib
import fcntl
import os
import json
import pathlib
import subprocess
import sys
import time
import uuid


class AdmissionError(Exception):
    pass


@contextlib.contextmanager
def locked(stream, exclusive, deadline):
    # Bounded contention, not an operation retry. Never unlink a lock inode.
    mode = fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH
    while True:
        if time.monotonic() >= deadline:
            raise AdmissionError("route admission timed out")
        try:
            fcntl.flock(stream, mode | fcntl.LOCK_NB)
            break
        except BlockingIOError:
            time.sleep(min(0.01, max(0, deadline - time.monotonic())))
    try:
        yield
    finally:
        fcntl.flock(stream, fcntl.LOCK_UN)


class OwnerAdmission:
    """Persistent terminal fence plus shared publication/exclusive sweep."""
    def __init__(self, state):
        self.state = pathlib.Path(state)

    def mark(self, name):
        self.state.mkdir(mode=0o700, parents=True, exist_ok=True)
        # No truncation or deletion: a fresh helper must see prior closure,
        # even if the sweeper died or timed out before obtaining exclusivity.
        with (self.state / name).open("a") as stream:
            stream.flush()
            os.fsync(stream.fileno())

    def require_open(self):
        if (self.state / "closing").exists():
            raise AdmissionError("route owner is closing")

    def begin_mutation(self):
        name = "pending-" + uuid.uuid4().hex
        self.mark(name)
        return self.state / name

    def require_certain(self, exclusive=False):
        if ((self.state / "uncertain").exists()
                or (exclusive and any(self.state.glob("pending-*")))):
            raise AdmissionError("route mutation remains uncertain")

    @contextlib.contextmanager
    def hold(self, deadline, exclusive=False, publishing=False):
        self.state.mkdir(mode=0o700, parents=True, exist_ok=True)
        if exclusive:
            self.mark("closing")
        with (self.state / "admission.lock").open("a") as stream:
            with locked(stream, exclusive, deadline):
                if publishing:
                    self.require_open()
                self.require_certain(exclusive)
                yield


def admitted(root, project, docker):
    # A killed launcher may leave a daemon mutation not yet visible to ps.
    # The infrastructure lock is checkout-wide, as are these reservations.
    routes = pathlib.Path(root) / "build/mg/conformance-routes"
    if any(routes.glob("*/pending-*")) or any(routes.glob("*/uncertain")):
        return False
    # Held workers cover proxy POST-before-reply and allocation-before-token;
    # labeled edges cover lost helper replies and failed removals independently.
    ids = docker("ps", "-aq", "--filter", "label=meguri.mg.checkout=" + root,
                 "--filter", "label=meguri.mg.project=" + project).split()
    for cid in ids:
        state = json.loads(docker("inspect", cid))[0]
        labels = state["Config"]["Labels"]
        if (labels.get("meguri.mg.checkout") != root
                or labels.get("meguri.mg.project") != project):
            raise ValueError("ownership inspection mismatch")
        if labels.get("meguri.mg.purpose") in ("conformance-worker", "conformance-actor-edge"):
            return False
    return True


@contextlib.contextmanager
def launch_admission(root, exclusive=False):
    # Short infrastructure admission only. Worker sessions never hold this
    # lock; all worker launches take shared locks and run concurrently.
    path = pathlib.Path(root) / "build/mg/conformance-routes/admission.lock"
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with path.open("a") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH)
        yield


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--project", required=True)
    parser.add_argument("--ensure", action="store_true")
    args = parser.parse_args()
    root = str(pathlib.Path(args.root).resolve())
    def docker(*argv):
        return subprocess.check_output(["docker", *argv], stderr=subprocess.DEVNULL, timeout=5).decode()
    with launch_admission(root, exclusive=True):
        if args.ensure:
            compose = ["docker", "compose", "-p", args.project, "-f", root + "/docker/compose.yml",
                       "--profile", "oracle-sync"]
            # Reuse healthy service without any operation that could recreate
            # it, even if another run already holds a namespace.
            ids = subprocess.check_output([*compose, "ps", "-q", "sync-faults"], timeout=5).decode().split()
            if len(ids) == 1:
                state = json.loads(docker("inspect", ids[0]))[0]
                if state["State"]["Running"] and state["State"].get("Health", {}).get("Status") == "healthy":
                    return 0
            if not admitted(root, args.project, docker):
                return 1
            env = dict(os.environ, BUILDX_NO_DEFAULT_ATTESTATIONS="1")
            return subprocess.run([*compose, "up", "--detach", "--wait", "--wait-timeout", "60", "sync-faults"],
                                  env=env, stdin=subprocess.DEVNULL, timeout=70).returncode
        return 0 if admitted(root, args.project, docker) else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        sys.exit(1)
