"""Fixed hardware-pilot inspection; called only under duo-fold's VM lease.

Uses the already connected holder, including holders started before this script
was added. No new VNC connection; inventory and lease checks are read-only.
"""
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import stat
import struct
import subprocess
import sys
import tempfile
import time
import zlib


GUEST_STAGES = ("console", "xcode-select", "bundle", "signature", "simctl-devices",
                "simctl-runtimes", "simctl-devicetypes", "lease", "open")
GUEST_VALIDATE = r'''
import json, math, os, pathlib, plistlib, re, stat, subprocess, sys, time
stage = "console"
request = json.loads(sys.argv[1])
def require(ok):
    if not ok:
        raise RuntimeError("guest inspection precondition failed")
def run(*args):
    return subprocess.check_output(args, stderr=subprocess.DEVNULL, timeout=30)
uid = os.getuid()
require(uid != 0 and pathlib.Path("/dev/console").stat().st_uid == uid)
stage = "xcode-select"
developer = pathlib.Path(run("/usr/bin/xcode-select", "-p").decode().strip()).resolve(strict=True)
require(developer.name == "Developer" and developer.parent.name == "Contents"
        and developer.parent.parent.suffix == ".app")
stage = "bundle"
hub = developer.parent / "Applications/DeviceHub.app"
require(hub.resolve(strict=True) == hub)
info = plistlib.loads((hub / "Contents/Info.plist").read_bytes())
require(info.get("CFBundleIdentifier") == "com.apple.dt.Devices"
        and info.get("CFBundleExecutable") == "DevicesTrampoline")
stage = "signature"
hub_requirement = '=anchor apple and identifier "com.apple.dt.Devices"'
run("/usr/bin/codesign", "--verify", "--strict", "-R", hub_requirement, str(hub))
trampoline = hub / "Contents/MacOS/DevicesTrampoline"
require(trampoline.resolve(strict=True) == trampoline)
run("/usr/bin/codesign", "--verify", "--strict", "-R", hub_requirement, str(trampoline))
# Use the selected Xcode's own tools, without an ambient DEVELOPER_DIR override.
simctl = str(developer / "usr/bin/simctl")
stage = "simctl-devices"
devices = json.loads(run(simctl, "list", "devices", "--json"))["devices"]
matches = [(rt, d) for rt, ds in devices.items() for d in ds if d["udid"] == request["udid"]]
require(len(matches) == 1)
runtime, device = matches[0]
require(re.fullmatch(r"Meguri-Pool-foldable-[1-9][0-9]*", device["name"]) is not None)
require(sum(d["name"] == device["name"] for ds in devices.values() for d in ds) == 1)
require(device.get("isAvailable") is True and device["state"] == "Booted")
pin = request["pin"]
require(runtime == pin["identifier"])
stage = "simctl-runtimes"
runtimes = json.loads(run(simctl, "list", "runtimes", "--json"))["runtimes"]
actual = [r for r in runtimes if r["identifier"] == runtime]
require(len(actual) == 1 and actual[0].get("isAvailable") is True
        and actual[0]["version"] == pin["version"] and actual[0]["buildversion"] == pin["build"])
stage = "simctl-devicetypes"
types = json.loads(run(simctl, "list", "devicetypes", "--json"))["devicetypes"]
require(sum(t["identifier"] == device["deviceTypeIdentifier"] and t["name"] == "iPhone Duo"
            and t.get("productFamily") == "iPhone" for t in types) == 1)
stage = "lease"
pool = pathlib.Path(request["pool"]) if request["pool"] else pathlib.Path.home() / "Library/Caches/meguri/sim-pool"
lease_witness = None
def check_lease():
    global lease_witness
    fd = os.open(pool / "leases" / request["udid"] / "lease.json", os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd) as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= 65536)
        raw = source.read(65537)
    witness = (info.st_dev, info.st_ino, raw)
    require(lease_witness is None or lease_witness == witness)
    lease_witness = witness
    lease = json.loads(raw)
    expiry = lease["expires_epoch"]
    require(lease["udid"] == request["udid"] and lease["kind"] == "foldable"
            and lease["worktree"] == request["checkout"] and lease["vm"] == request["vm"]
            and type(expiry) in (int, float) and math.isfinite(expiry) and expiry > time.time())
check_lease()  # Fresh expiry/ownership check immediately before launch.
'''
GUEST = GUEST_VALIDATE + r'''
# Open the exact selected bundle. Its trampoline may launch another executable;
# foreground/process identity is not a prerequisite for a read-only capture.
stage = "open"
run("/usr/bin/open", "-a", str(hub), "--args", "-CurrentDeviceUDID", request["udid"])
stage = "lease"
check_lease()
print("ready")
'''
# Only fixed stage tokens cross the private guest boundary on failure.
GUEST = "try:\n" + "".join("    " + line + "\n" for line in GUEST.splitlines()) + \
    'except Exception:\n    print("inspect-" + stage, flush=True)\n    raise RuntimeError("guest inspection failed") from None\n'


def complete_png(path):
    """A capture is ready only after every PNG chunk, including IEND, is closed."""
    try:
        data = path.read_bytes()
        if data[:8] != b"\x89PNG\r\n\x1a\n":
            return False
        offset = 8
        while offset + 12 <= len(data):
            size = struct.unpack_from(">I", data, offset)[0]
            end = offset + size + 12
            if end > len(data):
                return False
            chunk = data[offset + 4:end - 4]
            if zlib.crc32(chunk) != struct.unpack_from(">I", data, end - 4)[0]:
                return False
            if chunk[:4] == b"IEND":
                return size == 0 and end == len(data)
            offset = end
    except (OSError, struct.error):
        pass
    return False


def capture(state, connected):
    # No caller-controlled output path. Never put desktop pixels in a checkout,
    # MG's publishable artifacts, or on the host screen.
    base = Path(tempfile.gettempdir()).resolve()
    if any((parent / ".git").exists() for parent in (base, *base.parents)):
        raise RuntimeError("private capture directory must be outside Git checkouts")
    folder = Path(tempfile.mkdtemp(prefix="meguri-sim-posture-", dir=base))
    path = folder / "guest-desktop.png"
    command = ("shot " + str(path) + "\n").encode()
    if b"\n" in os.fsencode(str(path)) or b"\r" in command:
        raise RuntimeError("invalid private capture path")
    fifo = state / "commands"
    if not stat.S_ISFIFO(fifo.lstat().st_mode):
        raise RuntimeError("holder command FIFO unavailable")
    fd = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        if len(command) > os.fpathconf(fd, "PC_PIPE_BUF") or os.write(fd, command) != len(command):
            raise RuntimeError("holder capture request failed")
    finally:
        os.close(fd)
    deadline = time.monotonic() + 65
    while time.monotonic() < deadline:
        if complete_png(path):
            path.chmod(0o600)
            return path
        if not connected():
            raise RuntimeError("existing holder disconnected during capture")
        time.sleep(0.1)
    raise RuntimeError("holder capture did not complete; private partial capture retained")


def main():
    state, ssh, vm, checkout, udid, tart = sys.argv[1:]
    try:
        if not re.fullmatch(r"[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}", udid):
            raise RuntimeError("inspect requires a leased simulator UDID")
        spec = importlib.util.spec_from_file_location("vnc_session", Path(__file__).with_name("vnc-session.py"))
        session = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(session)
        state = Path(state)
        connected = lambda: session.status(state, vm, tart)["connected"]
        if not connected():
            raise RuntimeError("existing VNC holder unavailable; inspection never prepares or reconnects it")
        pins = json.loads((Path(checkout) / "tools/sim-devices.json").read_text())
        request = dict(vm=vm, checkout=checkout, udid=udid, pin=pins["foldable"]["runtime"],
                       pool=os.environ.get("MEGURI_SIM_POOL_DIR", ""))
        result = subprocess.run([ssh, shlex.join(["python3", "-c", GUEST, json.dumps(request)])],
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=180)
        if result.returncode or result.stdout.strip() != b"ready":
            token = result.stdout.strip()
            stages = {("inspect-" + stage).encode() for stage in GUEST_STAGES}
            raise RuntimeError(token.decode() if token in stages else "inspect-guest-transport")
        path = capture(state, connected)
        print("sim-posture: private guest desktop: " + str(path), file=sys.stderr)
        print(json.dumps({"ready": True, "vm": vm, "udid": udid, "capture": str(path)}))
        return 0
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.TimeoutExpired) as error:
        # Native output may contain private desktop/session details. Do not echo it.
        message = str(error) if isinstance(error, RuntimeError) else "private inspection unavailable"
        print("sim-posture: " + message, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
