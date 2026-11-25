#!/usr/bin/env python3
"""Keeps one Mac's claim on a test VM while that Mac's VM lease is held.

A VM lease (tools/mg.d/mac-vm.sh) is a directory on the Mac that schedules
the job, so two Macs that reach the same VM do not see each other's. The
claim is the lock they share: a flock(2) on a file inside the VM, held by a
process there for as long as this keeper's SSH session feeds it. The VM's
kernel drops it when that process ends, so it never goes stale: a lost
connection, a killed keeper and a restarted VM all free it.

Started detached by mg__mac_vm_claim, one per lease. It writes its state to
--state (starting, then taken, busy, unreachable or error; lost while it asks
again after its session ended; releasing at the end) and leaves when no live
job holds the lease any more.
Python 3.8+, standard library only.
"""

import argparse
import json
import os
import secrets
import select
import shlex
import socket
import subprocess
import sys
import tempfile
import time

# Runs in the VM. Line 1 of its output is the verdict. It keeps the lock
# until its input ends or stays silent for 45 s (a hung connection).
GUEST = r"""
use Fcntl qw(:flock);
use File::Basename;
use File::Path qw(make_path);
my ($path, $holder, $token, $acquisition) = @ARGV;
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
    if ($line =~ /^PROVE (\S+) (\S+) ([0-9a-f]{64})\n$/) {
        my ($t, $a, $challenge) = ($1, $2, $3);
        my @opened = stat($f); my @bound = stat($path);
        last unless @bound && $opened[0] == $bound[0] && $opened[1] == $bound[1];
        # This process has never dropped its flock. Reply on the same SSH
        # stream that acquired it; file contents are not ownership evidence.
        print "OWNED $t $a $challenge\n" if $t eq $token && $a eq $acquisition;
    }
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
    if any(os.path.exists(os.path.join(lease, guard)) for guard in
           ("vm-xcode-pending", "sim-posture-pending", "vm-shutdown-pending")):
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
        record = {"pid": os.getpid(), "token": args.token, "state": state,
                  "holder": args.holder}
        if state == "taken":
            record.update(proof=proof_path, acquisition=acquisition)
        record.update(extra)
        tmp = "%s.%d.tmp" % (args.state, os.getpid())
        with open(tmp, "w") as out:
            out.write(json.dumps(record) + "\n")
        os.replace(tmp, args.state)

    holder = json.dumps(json.loads(args.holder), separators=(",", ":"))
    def ask():
        """One attempt: (session, None) when taken, else (None, (state, extra))."""
        nonlocal acquisition
        acquisition = secrets.token_hex(32)
        command = "exec /usr/bin/perl -e %s %s %s %s %s" % (
            shlex.quote(GUEST), shlex.quote(args.path), shlex.quote(holder),
            shlex.quote(args.token), shlex.quote(acquisition))
        try:
            session = subprocess.Popen([args.ssh, command], stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, cwd="/")
        except OSError as error:
            return None, ("unreachable", {"reason": str(error)})
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
        if verdict == "TAKEN":
            return session, None
        session.kill()
        status = session.wait()
        if verdict.startswith("BUSY"):
            return None, ("busy", {"holder": verdict[4:].strip()})
        if verdict.startswith("ERROR"):
            return None, ("error", {"reason": verdict[5:].strip()})
        return None, ("unreachable", {"reason": "no answer from the VM (ssh exit %s)" % status})

    def end(session):
        try:
            session.stdin.close()
        except OSError:
            pass
        try:
            session.wait(timeout=20)
        except subprocess.TimeoutExpired:
            session.kill()
            session.wait()

    # Short private socket path works with macOS sockaddr_un, even when the
    # scheduler's checkout path is long. Ordinary jobs need no proof request.
    proof_dir = tempfile.TemporaryDirectory(prefix="mg-claim-", dir="/tmp")
    proof_path = os.path.join(proof_dir.name, "proof")
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(proof_path)
    listener.listen(4)
    acquisition = None

    def prove():
        if not select.select([listener], [], [], 0)[0]:
            return
        channel, _ = listener.accept()
        with channel:
            channel.settimeout(.2)
            try:
                with channel.makefile("rb") as source:
                    request = json.loads(source.readline(1025))
                challenge = request["challenge"]
                if (request != dict(token=args.token, acquisition=acquisition, challenge=challenge)
                        or not isinstance(challenge, str) or len(challenge) != 64
                        or any(c not in "0123456789abcdef" for c in challenge)
                        or session is None or session.poll() is not None):
                    return
                expected = ("OWNED %s %s %s\n" % (args.token, acquisition, challenge)).encode()
                session.stdin.write(("PROVE %s %s %s\n" % (args.token, acquisition, challenge)).encode())
                session.stdin.flush()
                line = b""
                deadline = time.monotonic() + 1
                while len(line) <= 1024:
                    left = deadline - time.monotonic()
                    if left <= 0 or not select.select([session.stdout], [], [], left)[0]:
                        return
                    chunk = os.read(session.stdout.fileno(), 1024)
                    if not chunk:
                        return
                    line += chunk
                    # A late answer from a timed-out earlier request cannot
                    # authorize this one or poison every future challenge.
                    while b"\n" in line:
                        reply, line = line.split(b"\n", 1)
                        if reply + b"\n" == expected:
                            if session.poll() is None and lease_held(args.lease):
                                channel.sendall((json.dumps(request) + "\n").encode())
                            return
            except (OSError, ValueError, KeyError, TypeError):
                pass  # No fresh answer means no fold authority. Keep ordinary jobs alive.

    write("starting")
    session, refused = ask()
    if session is None:
        write(refused[0], **refused[1])
        return {"busy": 3, "error": 1}.get(refused[0], 2)

    # A job that ends nudges its keeper by creating <state>.nudge (a file,
    # never a signal: a pid read from a file may belong to another process by
    # the time it is signalled). The keeper then looks at the lease at once
    # and often for a moment, instead of at its next beat.
    nudge = args.state + ".nudge"
    nudged = [0.0]

    def heard():
        if os.path.exists(nudge):
            try:
                os.unlink(nudge)
            except OSError:
                pass
            nudged[0] = time.monotonic() + 3
        return time.monotonic() < nudged[0]

    write("taken")
    beat = 0.0
    while lease_held(args.lease):
        prove()
        now = time.monotonic()
        lost = False
        if session is not None and now >= beat:
            beat = now + args.beat
            try:
                session.stdin.write(b"\n")
                session.stdin.flush()
            except OSError:
                lost = True
        if session is not None and (lost or session.poll() is not None):
            # The session ended while the lease is still held (a lost
            # connection, a restarted VM): the claim is gone with it. The
            # lease's job may be retaining the VM (vm-xcode-pending), so the
            # keeper asks for the claim again until the lease ends. Another
            # Mac can take it meanwhile; nothing held in the VM can prevent
            # that once its holder's session is gone.
            print("vm-claim: the session that held the claim ended; asking again",
                  file=sys.stderr, flush=True)
            end(session)
            session = None
            write("lost")
        if session is None:
            session, refused = ask()
            if session is not None:
                print("vm-claim: claim taken again", file=sys.stderr, flush=True)
                write("taken")
                beat = 0.0
                continue
        wake = time.monotonic() + args.beat
        while time.monotonic() < wake and not heard():
            prove()
            time.sleep(0.1)
        if heard():
            time.sleep(0.1)
    # Said before the lock goes, so a job that takes the lease now starts a
    # keeper of its own instead of adopting this one.
    write("releasing")
    if session is not None:
        end(session)
    for leftover in (args.state, nudge):
        try:
            os.unlink(leftover)
        except OSError:
            pass
    listener.close()
    proof_dir.cleanup()
    return 0


if __name__ == "__main__":
    sys.exit(main())
