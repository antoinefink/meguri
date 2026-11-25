#!/usr/bin/env python3
"""Install copies as tart and vm-ssh beside a private remote.json (Python 3.8+)."""

import json
import os
from pathlib import Path
import shlex
import sys


def configuration(launcher):
    config = json.loads(Path(launcher).resolve().with_name("remote.json").read_text())
    host, runtime = config["host"], config["runtime"]
    if (not isinstance(host, str) or not host or host.startswith("-")
            or any(c.isspace() or ord(c) < 32 for c in host)):
        raise ValueError("host must be an SSH alias or user@host")
    if (not isinstance(runtime, str) or not runtime.startswith("/") or "\0" in runtime
            or any(ord(c) < 32 for c in runtime) or ".." in Path(runtime).parts):
        raise ValueError("runtime must be an absolute hosting Mac kit path")
    return host, runtime.rstrip("/")


def ssh_arguments(host, *, dedicated=False):
    args = ["/usr/bin/ssh", "-T", "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=8",
            "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "-o", "ForwardAgent=no", "-o", "RequestTTY=no"]
    if dedicated:
        args += ["-o", "ControlMaster=no", "-o", "ControlPath=none"]
    return [*args, "--", host]


# Executed on the hosting Mac, not the coordinator. Only MG's plain headless
# start takes this path; VNC needs a live session and a separate SSH tunnel.
DETACH = r"""
import os
import subprocess
import sys
import tempfile

runtime, *argv = sys.argv[1:]
try:
    fd, log = tempfile.mkstemp(prefix="tart-run-", suffix=".log", dir=runtime)
    with os.fdopen(fd, "wb") as output:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=output,
                                 stderr=output, cwd="/", close_fds=True,
                                 start_new_session=True)
    print("remote tart: pid %s; hosting Mac log: %s" % (child.pid, log),
          file=sys.stderr, flush=True)
    try:
        status = child.wait(timeout=1)
    except subprocess.TimeoutExpired:
        sys.exit(0)
    if status:
        with open(log, "rb") as source:
            source.seek(0, os.SEEK_END)
            source.seek(max(0, source.tell() - 8192))
            sys.stderr.buffer.write(source.read())
    sys.exit(status if status >= 0 else 128 - status)
except OSError as error:
    print("remote tart unavailable: %s" % error, file=sys.stderr)
    sys.exit(255)
"""


def remote_command(mode, args, runtime):
    tart = runtime.rstrip("/") + "/bin/tart"
    guest = runtime.rstrip("/") + "/bin/vm-ssh"
    # Both wrappers live on the SSD. An absent kit/guest wrapper must queue as
    # SSH unavailable (255), rather than look like a guest command failure.
    guard = (
        "if [ ! -x " + shlex.quote(tart) + " ] || [ ! -x "
        + shlex.quote(guest) + " ]; then "
        "echo 'remote VM kit unavailable: check SSD and bin wrappers' >&2; "
        "exit 255; fi; "
    )
    argv = [tart if mode == "tart" else guest, *args]
    if (mode == "tart" and len(args) == 3
            and args[:2] == ["run", "--no-graphics"]
            and args[2] and not args[2].startswith("-")):
        argv = ["/usr/bin/python3", "-c", DETACH, runtime, *argv]
    return guard + "exec " + shlex.join(argv)


def main():
    try:
        mode = Path(sys.argv[0]).name
        if mode not in ("tart", "vm-ssh"):
            raise ValueError("install this launcher as tart and vm-ssh")
        # Find the adjacent private config through the stable install symlink.
        host, runtime = configuration(sys.argv[0])
        command = remote_command(mode, sys.argv[1:], runtime)
        os.execv("/usr/bin/ssh", [*ssh_arguments(host), command])
    except (OSError, ValueError, KeyError, TypeError) as error:
        print("remote VM unavailable: %s" % error, file=sys.stderr)
        return 255


if __name__ == "__main__":
    sys.exit(main())
