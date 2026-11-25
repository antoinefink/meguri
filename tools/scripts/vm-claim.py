#!/usr/bin/env python3
"""Keeps one Mac's claim on a test VM while that Mac's VM lease is held.

A VM lease (tools/mg.d/mac-vm.sh) is a directory on the Mac that schedules
the job, so two Macs that reach the same VM do not see each other's. The
claim is the lock they share: a flock(2) on a file inside the VM, held by a
process there for as long as this keeper's SSH session feeds it. The VM's
kernel drops it when that process ends, so it never goes stale: a lost
connection, a killed keeper and a restarted VM all free it.

Started detached by mg__mac_vm_claim, one per lease. It writes its state to
--state (starting, then taken, busy, unreachable or error; releasing or lost
at the end) and leaves when no live job holds the lease any more.
Python 3.8+, standard library only.
"""

import argparse
import json
import os
import select
import shlex
import signal
import subprocess
import sys
import time

# Runs in the VM. Line 1 of its output is the verdict. It keeps the lock
# until its input ends or stays silent for 45 s (a hung connection).
GUEST = r"""
use Fcntl qw(:flock);
use File::Basename;
use File::Path qw(make_path);
my ($path, $holder) = @ARGV;
$| = 1;
make_path(dirname($path));
open(my $f, "+>>", $path) or do { print "ERROR $!\n"; exit 1 };
unless (flock($f, LOCK_EX | LOCK_NB)) {
    seek($f, 0, 0);
    my $h = <$f>;
    $h = "" unless defined $h;
    $h =~ s/\s+\z//;
    print "BUSY $h\n";
    exit 3;
}
truncate($f, 0);
syswrite($f, "$holder\n");
print "TAKEN\n";
while (1) {
    my $line = eval {
        local $SIG{ALRM} = sub { die "silent\n" };
        alarm 45;
        my $l = <STDIN>;
        alarm 0;
        $l;
    };
    last unless defined $line;
}
truncate($f, 0);
"""


def alive(pid):
    try:
        os.kill(int(pid), 0)
    except (ProcessLookupError, ValueError, TypeError):
        return False
    except PermissionError:
        return True
    return True


def lease_held(lease):
    """A live job holds the lease or its nested slot, or a guard retains it."""
    if os.path.exists(os.path.join(lease, "vm-xcode-pending")):
        return True
    for slot in (lease, os.path.join(lease, "inner")):
        try:
            with open(os.path.join(slot, "holder.json")) as source:
                if alive(json.load(source).get("pid")):
                    return True
        except (OSError, ValueError, AttributeError):
            pass
    return False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ssh", required=True)
    parser.add_argument("--lease", required=True)
    parser.add_argument("--state", required=True)
    parser.add_argument("--token", required=True)
    parser.add_argument("--holder", required=True)
    parser.add_argument("--path", required=True)
    parser.add_argument("--beat", type=float, default=5)
    parser.add_argument("--answer", type=float, default=60)
    args = parser.parse_args()

    def write(state, **extra):
        record = {"pid": os.getpid(), "token": args.token, "state": state}
        record.update(extra)
        tmp = "%s.%d.tmp" % (args.state, os.getpid())
        with open(tmp, "w") as out:
            out.write(json.dumps(record) + "\n")
        os.replace(tmp, args.state)

    write("starting")
    holder = json.dumps(json.loads(args.holder), separators=(",", ":"))
    command = "exec /usr/bin/perl -e %s %s %s" % (
        shlex.quote(GUEST), shlex.quote(args.path), shlex.quote(holder))
    try:
        session = subprocess.Popen([args.ssh, command], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, cwd="/")
    except OSError as error:
        write("unreachable", reason=str(error))
        return 2

    line = b""
    deadline = time.monotonic() + args.answer
    while not line.endswith(b"\n"):
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([session.stdout], [], [], left)[0]:
            break
        chunk = os.read(session.stdout.fileno(), 4096)
        if not chunk:
            break
        line += chunk
    verdict = line.decode("utf-8", "replace").split("\n", 1)[0].strip()

    if verdict != "TAKEN":
        session.kill()
        status = session.wait()
        if verdict.startswith("BUSY"):
            write("busy", holder=verdict[4:].strip())
            return 3
        if verdict.startswith("ERROR"):
            write("error", reason=verdict[5:].strip())
            return 1
        write("unreachable", reason="no answer from the VM (ssh exit %s)" % status)
        return 2

    # A job that ends nudges its keeper (SIGUSR1), which then looks at the
    # lease at once and often for a moment, instead of at its next beat.
    nudged = [0.0]
    signal.signal(signal.SIGUSR1, lambda *_: nudged.__setitem__(0, time.monotonic() + 3))
    write("taken")
    lost = False
    beat = 0.0
    while lease_held(args.lease):
        now = time.monotonic()
        if now >= beat:
            beat = now + args.beat
            try:
                session.stdin.write(b"\n")
                session.stdin.flush()
            except OSError:
                lost = True
        if lost or session.poll() is not None:
            lost = True
            break
        wake = time.monotonic() + args.beat
        while time.monotonic() < wake and time.monotonic() >= nudged[0]:
            time.sleep(0.1)
        if time.monotonic() < nudged[0]:
            time.sleep(0.1)
    # Said before the lock goes, so a job that takes the lease now starts a
    # keeper of its own instead of adopting this one.
    write("lost" if lost else "releasing")
    try:
        session.stdin.close()
    except OSError:
        pass
    try:
        session.wait(timeout=20)
    except subprocess.TimeoutExpired:
        session.kill()
        session.wait()
    try:
        os.unlink(args.state)
    except OSError:
        pass
    if lost:
        print("vm-claim: the session that held the claim ended; the claim is gone",
              file=sys.stderr)
        return 4
    return 0


if __name__ == "__main__":
    sys.exit(main())
