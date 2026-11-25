"""One leased Duo, one verified pose click, through the existing FIFO holder.

Called under duo-fold.sh's scheduler lease. The guest guard holds the existing
pool mutex, excluding even legacy release/renew/reassignment. All native output
and temporary pixels stay private; stdout is a bounded, content-free receipt.
"""
import base64
import contextlib
import fcntl
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import select
import shlex
import signal
import stat
import subprocess
import sys
import tempfile
import termios
import time
import uuid

from PIL import Image, ImageChops


def sibling(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


inspection = sibling("sim-posture-inspect")
session = sibling("vnc-session")
CLICK_BUDGET = 300  # Holder callback and final readback admission, never a lease renewal.
LIMIT = 24 * 1024 * 1024  # Each PNG/protocol reply, never unbounded native data.


class MachineUnavailable(RuntimeError):
    pass


def require(ok, message="posture evidence mismatch"):
    if not ok:
        raise RuntimeError(message)


def strict_json(data):
    def pairs(rows):
        result = {}
        for key, value in rows:
            require(key not in result, "duplicate JSON field")
            result[key] = value
        return result
    def constant(value):
        raise ValueError("nonfinite JSON number")
    return json.loads(data, object_pairs_hook=pairs, parse_constant=constant)


# The selected Xcode, signed Hub, unique pinned booted Duo and exact live lease
# admission are shared with inspect. Only this fixed guard adds pool exclusion.
# The owner file is never replaced. Its last byte is active, published or
# terminal; flock orders marker installation and cancellation on that inode.
POSTURE_STATE = r'''
import contextlib, fcntl
@contextlib.contextmanager
def posture_state(owner, token):
    fcntl.flock(owner, fcntl.LOCK_EX)
    try:
        raw = os.pread(owner, 256, 0)
        yield raw[-1:] if raw in [(token + "\n" + state).encode() for state in "APT"] else None
    finally:
        fcntl.flock(owner, fcntl.LOCK_UN)
def posture_mark(owner, token, state):
    if os.pwrite(owner, state, len(token.encode()) + 1) != 1:
        raise RuntimeError("posture state write failed")
'''
# Shared verbatim with the guest and behavioral tests. Native evidence carries
# separate front-app and window-owner identities; only the owner is pinned
# between fresh checks, after both have passed exact installation admission.
WINDOW_ADMISSION = r'''
import stat
signed_installation = {}
def installation_fact(path):
    require(path.resolve(strict=True) == path)
    paths = [path]
    if path == hub:
        paths += [hub / "Contents/Info.plist", hub / "Contents/_CodeSignature/CodeResources"]
    facts = []
    for item in paths:
        try:
            info = item.lstat()
        except FileNotFoundError:
            require(item == hub / "Contents/_CodeSignature/CodeResources")
            facts.append(None)  # A later addition also invalidates the proof.
            continue
        require(not stat.S_ISLNK(info.st_mode))
        facts.append(tuple(getattr(info, key) for key in (
            "st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink",
            "st_size", "st_mtime_ns", "st_ctime_ns")))
    return facts

def admit_signature(path, final):
    fact = installation_fact(path)
    if final:
        # Reuse only this guard's verified, unchanged signed installation.
        # No codesign process or signature hashing inside M3's short ticket.
        require(signed_installation.get(path) == fact)
    else:
        run("/usr/bin/codesign", "--verify", "--strict", "-R", hub_requirement, str(path))
        require(installation_fact(path) == fact)
        signed_installation[path] = fact

def kernel_executable(pid):
    import ctypes
    require(type(pid) is int and 0 < pid <= 2147483647)
    libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
    lookup = libproc.proc_pidpath
    lookup.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
    lookup.restype = ctypes.c_int
    buffer = ctypes.create_string_buffer(4096)  # PROC_PIDPATHINFO_MAXSIZE
    size = lookup(pid, buffer, len(buffer))
    require(0 < size <= len(buffer) and b"\0" in buffer.raw)
    path = buffer.value.decode("utf-8")
    require(path.startswith("/"))
    return path

def admit_window(evidence, final=False):
    global stage
    stage = "window-installation"
    require(hub.resolve(strict=True) == hub)
    info = plistlib.loads((hub / "Contents/Info.plist").read_bytes())
    require(info.get("CFBundleIdentifier") == "com.apple.dt.Devices"
            and info.get("CFBundleExecutable") == "DevicesTrampoline")
    admit_signature(hub, final)
    def admit_app(app, owner=False):
        require(app.get("active") is True and app.get("terminated") is False
                and app.get("finished_launching") is True and app["bundle"] == str(hub)
                and app["bundle_id"] == "com.apple.dt.Devices")
        executable = pathlib.Path(app["executable"])
        allowed = (hub / "Contents/MacOS/DeviceHub",) if owner else (
            hub / "Contents/MacOS/DevicesTrampoline", hub / "Contents/MacOS/DeviceHub")
        require(executable in allowed
                and executable.resolve(strict=True) == executable)
        admit_signature(executable, final)
    stage = "window-front"
    front_pid = evidence["front"].get("pid")
    require(type(front_pid) is int and (front_pid > 0 or front_pid == -1))
    admit_app(evidence["front"])
    stage = "window-unique"
    normal = evidence["normal"]
    require(0 < len(normal) <= 256)
    ids = [row.get("id") for row in normal]
    require(all(type(value) is int and value > 0 for value in ids)
            and len(set(ids)) == len(ids))
    def selected_executable(value):
        return isinstance(value, str) and hub in pathlib.Path(value).parents
    owned = [w for w in normal if w.get("bundle") == str(hub)
             or selected_executable(w.get("executable"))]
    unknown = [w for w in normal if w not in owned]
    # A CG owner cannot simultaneously claim selected and unrelated identity.
    require(not ({w.get("pid") for w in owned} & {w.get("pid") for w in unknown}))
    for row in unknown:
        pid = row.get("pid")
        executable = kernel_executable(pid)
        if "lookup_pid" in row:
            require(type(row["lookup_pid"]) is int and row["lookup_pid"] in (pid, -1))
        require(not row.get("executable") or row["executable"] == executable)
        selected = selected_executable(executable)
        require(not selected or not row.get("bundle"))
        if selected:
            owned.append(row)
    # Bind CG's globally frontmost normal window, never a title-list match.
    # Missing secondary AppKit identity still needs kernel exclusion from the
    # selected bundle; selected launcher/companion windows cannot be excluded.
    window = normal[0]
    require(window in owned)
    stage = "window-owner-lookup"
    pid, lookup_pid = window.get("pid"), window.get("lookup_pid")
    # CG supplies the bound positive PID. -1 is admitted only for this lookup's
    # verified actual owner; a positive AppKit PID must agree with CG.
    require(type(pid) is int and 0 < pid <= 2147483647
            and type(lookup_pid) is int and lookup_pid in (pid, -1))
    stage = "window-owner-kernel"
    require(kernel_executable(pid) == window.get("executable")
            == str(hub / "Contents/MacOS/DeviceHub"))
    stage = "window-owner-identity"
    admit_app(window, owner=True)
    stage = "window-unique"
    # Apple's device canvas may leave a device/management window behind it.
    # Only fully identified, signed actual DeviceHub owners with distinct CG
    # IDs may remain below the bound window. No lower title supplies evidence.
    for row in owned:
        if row is window:
            continue
        lower_pid, lower_lookup = row.get("pid"), row.get("lookup_pid")
        require(type(lower_pid) is int and 0 < lower_pid <= 2147483647
                and type(lower_lookup) is int and lower_lookup in (lower_pid, -1))
        require(kernel_executable(lower_pid) == row.get("executable")
                == str(hub / "Contents/MacOS/DeviceHub"))
        admit_app(row, owner=True)
    stage = "window-geometry"
    require(window["id"] > 0 and window["rect"][2] > 100 and window["rect"][3] > 150)
    return dict(window, screen=evidence["screen"], pointer=evidence["pointer"])

def observe_window(initial, final=False):
    global stage
    # Only the first pre-input read may wait for open's foreground transition.
    deadline = time.monotonic() + 5 if initial else 0
    while True:
        stage = "lease"
        check_lease()
        stage = "window-native"
        evidence = json.loads(run(str(visual), "window"))
        try:
            return admit_window(evidence, final=final)
        except RuntimeError:
            if not initial or time.monotonic() >= deadline:
                raise
        time.sleep(.25)
'''
GUEST = r'''
import base64, hashlib, json, os, pathlib, select, signal, stat, subprocess, sys, tempfile, time
''' + POSTURE_STATE + r'''
request = json.loads(sys.argv[1])
pool = pathlib.Path(request["pool"]) if request["pool"] else pathlib.Path.home() / "Library/Caches/meguri/sim-pool"
lock = pool / "pool.lock"
owned = None
lock_fd = owner_fd = None
pending = False
armed = False
request_id = 0
def expired(*unused):
    raise TimeoutError("posture guard expired")
signal.signal(signal.SIGALRM, expired)
signal.signal(signal.SIGHUP, expired)
signal.signal(signal.SIGINT, expired)
signal.signal(signal.SIGTERM, expired)
signal.alarm(480)  # Below sim-pool's ten-minute stale-lock threshold.
try:
    deadline = time.monotonic() + 10
    while True:
        try:
            lock.mkdir()
            lock_fd = os.open(lock, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            info = os.fstat(lock_fd)
            owned = (info.st_dev, info.st_ino)
            owner_fd = os.open("posture-owner", os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                               0o600, dir_fd=lock_fd)
            with os.fdopen(os.dup(owner_fd), "w") as owner:
                owner.write(request["guard"] + "\nA")
            with os.fdopen(os.open("pid", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                                   0o600, dir_fd=lock_fd), "w") as pid:
                pid.write(str(os.getpid()))
            break
        except FileExistsError:
            if time.monotonic() >= deadline:
                raise TimeoutError("pool busy")
            time.sleep(.1)
''' + "".join("    " + line + "\n" for line in
              (inspection.GUEST_VALIDATE + WINDOW_ADMISSION).splitlines()) + r'''
    stage = "binding"
    xinfo = plistlib.loads((developer.parent / "Info.plist").read_bytes())
    version = plistlib.loads((developer.parent / "version.plist").read_bytes())
    binding = dict(xcode=str(developer), version=xinfo["CFBundleShortVersionString"],
                   build=version["ProductBuildVersion"], hub=str(hub),
                   executable_sha256=hashlib.sha256((hub / "Contents/MacOS/DevicesTrampoline").read_bytes()).hexdigest(),
                   vm=request["vm"], udid=request["udid"], name=device["name"], runtime=runtime)
    with tempfile.TemporaryDirectory(prefix="meguri-posture-guest-") as folder:
        folder = pathlib.Path(folder)
        source = folder / "visual.swift"
        source.write_text(request["visual"])
        visual = folder / "visual"
        env = dict(os.environ, DEVELOPER_DIR=str(developer))
        stage = "compile"
        subprocess.run([str(developer / "Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc"),
                        str(source), "-o", str(visual)], env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
        open_hub()
        print(json.dumps(dict(ready=True, binding=binding)), flush=True)
        first_window = True
        while True:
            line = sys.stdin.buffer.readline(41943041)
            if not line:
                break
            require(len(line) <= 41943040)
            require(line.endswith(b"\n"))
            def pairs(rows):
                result = {}
                for key, value in rows:
                    require(key not in result)
                    result[key] = value
                return result
            message = json.loads(line, object_pairs_hook=pairs,
                                 parse_constant=lambda value: (_ for _ in ()).throw(ValueError()))
            require(type(message) is dict and type(message.get("id")) is int
                    and message["id"] == request_id + 1)
            request_id = message["id"]
            action = message["action"]
            require(set(message) == {"id", "action"})
            stage = action
            if action not in ("disarm", "finish"):
                check_lease()
            if action in ("window", "click-window"):
                if action == "click-window":
                    require(armed and pending)
                initial = first_window and not armed and not pending
                first_window = False
                result = observe_window(initial, final=action == "click-window")
            elif action == "displays":
                result = {}
                for display in (1, 3):
                    image = folder / (str(display) + ".png")
                    run(simctl, "io", request["udid"], "screenshot", "--display=" + str(display), str(image))
                    require(image.stat().st_size <= 16777216)
                    result[str(display)] = base64.b64encode(image.read_bytes()).decode()
                    image.unlink()
            elif action == "arm":
                first_window = False  # Arming permanently ends pre-input settling.
                require(not armed and not pending)
                check_lease()
                expiry = json.loads(lease_witness[2])["expires_epoch"]
                require(expiry - time.time() >= 300)
                armed = True
                result = {"armed": True, "remaining": expiry - time.time()}
            elif action == "publish":
                require(armed and not pending)
                check_lease()
                require(json.loads(lease_witness[2])["expires_epoch"] - time.time() >= 300)
                with posture_state(owner_fd, request["guard"]) as state:
                    require(state == b"A")  # Terminal cancellation cannot publish again.
                    os.mkdir("sim-posture-pending", dir_fd=lock_fd)
                    pending = True
                    posture_mark(owner_fd, request["guard"], b"P")
                result = {"published": True, "remaining": json.loads(lease_witness[2])["expires_epoch"] - time.time()}
            elif action == "disarm":
                require(armed)
                with posture_state(owner_fd, request["guard"]) as state:
                    require(state is not None)
                    posture_mark(owner_fd, request["guard"], b"T")
                    with contextlib.suppress(FileNotFoundError):
                        os.rmdir("sim-posture-pending", dir_fd=lock_fd)
                pending = armed = False
                result = True
            elif action == "finish":
                require(not pending)
                print(json.dumps(dict(id=request_id, ok=True, result=True)), flush=True)
                break
            else:
                raise ValueError("unknown posture operation")
            if action not in ("arm", "publish", "disarm"):
                check_lease()
            print(json.dumps(dict(id=request_id, ok=True, result=result)), flush=True)
    require(not pending)
finally:
    for sig in (signal.SIGALRM, signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, signal.SIG_IGN)
    signal.alarm(0)
    # Unjoined input retains both fences, even after this bounded guard exits.
    # Never reclaim a foreign lock or signal a PID read from metadata.
    try:
        if owner_fd is not None:
            with posture_state(owner_fd, request["guard"]) as state:
                require(state is not None)
                try:
                    os.stat("sim-posture-pending", dir_fd=lock_fd, follow_symlinks=False)
                except FileNotFoundError:
                    posture_mark(owner_fd, request["guard"], b"T")
                    for name in ("pid", "posture-owner"):
                        with contextlib.suppress(FileNotFoundError):
                            os.unlink(name, dir_fd=lock_fd)
                    with contextlib.suppress(FileNotFoundError):
                        info = lock.lstat()
                        if (info.st_dev, info.st_ino) == owned:
                            lock.rmdir()
    finally:
        for fd in (owner_fd, lock_fd):
            if fd is not None:
                os.close(fd)
'''
GUEST_STAGES = set(inspection.GUEST_STAGES) | {"pool", "binding", "compile", "window", "click-window", "displays", "arm", "publish", "disarm", "finish",
    "window-installation", "window-front", "window-unique", "window-owner-lookup",
    "window-owner-kernel", "window-owner-identity", "window-geometry", "window-native"}
GUEST = "try:\n" + "".join("    " + line + "\n" for line in GUEST.splitlines()) + \
    'except Exception:\n    import json, sys\n    stage = globals().get("stage", "pool")\n' + \
    '    print(json.dumps({"id": globals().get("request_id", 0), "error": stage if stage in ' + repr(sorted(GUEST_STAGES)) + ' else "pool"}), flush=True)\n' + \
    '    sys.exit(1)\n'


# A terminal/nonexecution proof may outlive the original SSH channel. This is
# disarm of the same guard, never stale-PID reclamation or a new input attempt.
TERMINAL = r'''
import json, os, pathlib, sys
''' + POSTURE_STATE + r'''
request = json.loads(sys.argv[1])
pool = pathlib.Path(request["pool"]) if request["pool"] else pathlib.Path.home() / "Library/Caches/meguri/sim-pool"
try:
    fd = os.open(pool / "pool.lock", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
except FileNotFoundError:
    fd = None
if fd is not None:
    try:
        try:
            owner = os.open("posture-owner", os.O_RDWR | os.O_NOFOLLOW, dir_fd=fd)
        except FileNotFoundError:
            owner = None  # The original guard already retired this directory.
        if owner is not None:
            try:
                # The pinned directory and stable file remain the same even if
                # this cleanup is delayed until a replacement guard exists.
                with posture_state(owner, request["guard"]) as state:
                    if state is not None:
                        posture_mark(owner, request["guard"], b"T")
                        with contextlib.suppress(FileNotFoundError):
                            os.rmdir("sim-posture-pending", dir_fd=fd)
            finally:
                os.close(owner)
    finally:
        os.close(fd)
print('{"released":true}', flush=True)
'''


class Guest:
    def __init__(self, ssh, request, log):
        # The wrapper accepts one POSIX-quoted guest command; data is never code.
        self.admitted = False
        self.ssh, self.request = ssh, request
        self.sequence = 0
        self.child = subprocess.Popen([ssh, shlex.join(["python3", "-c", GUEST, json.dumps(request)])],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, bufsize=0)
        os.set_blocking(self.child.stdin.fileno(), False)
        try:
            self.ready = self.read(150)
        except (OSError, RuntimeError):
            if self.child.poll() == 255:
                raise MachineUnavailable("mac-vm transport unavailable") from None
            raise
        require(type(self.ready) is dict and set(self.ready) == {"ready", "binding"}
                and self.ready["ready"] is True, "guest admission failed")
        self.admitted = True

    def read(self, timeout):
        # Unbuffered os.read avoids select/readline partial-line deadlocks.
        deadline = time.monotonic() + timeout
        data = bytearray()
        while time.monotonic() < deadline:
            readable, _, _ = select.select([self.child.stdout], [], [], max(0, deadline - time.monotonic()))
            require(readable, "guest evidence timed out")
            part = os.read(self.child.stdout.fileno(), 65536)
            require(part and len(data) + len(part) <= 2 * LIMIT, "guest evidence unavailable or oversized")
            data.extend(part)
            if b"\n" in data:
                require(data.count(b"\n") == 1 and data.endswith(b"\n"), "malformed guest frame")
                reply = strict_json(data)
                require(type(reply) is dict, "malformed guest reply")
                if "error" in reply:
                    require(set(reply) == {"id", "error"} and type(reply["id"]) is int
                            and reply["id"] == self.sequence and reply["error"] in GUEST_STAGES,
                            "malformed guest error")
                    raise RuntimeError("posture-" + reply["error"])
                return reply
        raise RuntimeError("guest evidence timed out")

    def call(self, action, **values):
        require(not getattr(self, "failed", False), "guest protocol already lost")
        try:
            return self._call(action, **values)
        except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError):
            self.failed = True
            raise

    def _call(self, action, **values):
        self.sequence += 1
        payload = memoryview((json.dumps(dict(id=self.sequence, action=action, **values)) + "\n").encode())
        require(len(payload) <= 40 * 1024 * 1024)
        deadline = time.monotonic() + 30
        offset = 0
        while offset < len(payload):
            _, writable, _ = select.select([], [self.child.stdin], [], max(0, deadline - time.monotonic()))
            require(writable and time.monotonic() < deadline, "guest request timed out")
            try:
                count = os.write(self.child.stdin.fileno(), payload[offset:offset + 65536])
                require(count > 0, "guest request disconnected")
                offset += count
            except BlockingIOError:
                continue
        reply = self.read(70)
        require(type(reply) is dict and set(reply) == {"id", "ok", "result"}
                and type(reply["id"]) is int and reply["id"] == self.sequence
                and reply["ok"] is True, "guest evidence refused")
        if action in ("finish", "disarm"):
            require(reply["result"] is True, "guest release refused")
        return reply["result"]

    def terminal(self):
        result = subprocess.run([self.ssh, shlex.join(["python3", "-c", TERMINAL,
                                 json.dumps({k: self.request[k] for k in ("pool", "guard")})])],
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, timeout=15)
        require(result.returncode == 0 and result.stdout == b'{"released":true}\n',
                "guest terminal disarm unconfirmed")


def image_size(path):
    require(path.stat().st_size <= LIMIT, "oversized private capture")
    with Image.open(path) as image:
        require(image.format == "PNG" and 0 < image.width * image.height <= 20000000)
        image.verify()
        return image.size


class OffscreenText:
    """One private coordinator compiler invocation; saved-frame OCR only."""
    def __init__(self, folder):
        self.visual = folder / "ocr"
        self.output = folder / "ocr.json"
        source = folder / "ocr.swift"
        source.write_text(Path(__file__).with_name("sim-posture-ocr.swift").read_text())
        # Use the coordinator's default selected Xcode, not the guest selection
        # or an ambient DEVELOPER_DIR override. Never invoke the window helper.
        env = dict(os.environ)
        env.pop("DEVELOPER_DIR", None)
        result = subprocess.run(["/usr/bin/xcrun", "swiftc", str(source), "-o", str(self.visual)],
                                env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, timeout=60)
        require(result.returncode == 0, "local offscreen OCR compilation unavailable")

    def recognize(self, path):
        # The bound holder already saved this exact frame on the coordinator.
        # Keep native output private and bounded before parsing it.
        try:
            with self.output.open("wb") as output:
                result = subprocess.run([str(self.visual), str(path)], stdin=subprocess.DEVNULL,
                                        stdout=output, stderr=subprocess.DEVNULL, timeout=30)
            require(result.returncode == 0, "local offscreen OCR unavailable")
            require(self.output.stat().st_size <= LIMIT, "oversized private OCR reply")
            return strict_json(self.output.read_bytes())
        finally:
            self.output.unlink(missing_ok=True)


def text_boxes(ocr, path):
    width, height = image_size(path)
    result = ocr.recognize(path)
    require(isinstance(result, list) and len(result) <= 2000)
    boxes = []
    for row in result:
        require(isinstance(row["text"], str) and len(row["text"]) <= 1024)
        box = row["box"]
        require(len(box) == 4 and all(type(v) in (int, float) and math.isfinite(v) and 0 <= v <= 1 for v in box))
        require(type(row["confidence"]) in (int, float) and math.isfinite(row["confidence"])
                and 0 <= row["confidence"] <= 1)
        if row["confidence"] >= .9:
            boxes.append((row["text"], (box[0] * width, box[1] * height, box[2] * width, box[3] * height)))
    return boxes


def inside(box, rect):
    x, y, w, h = box
    a, b, c, d = rect
    return a <= x and b <= y and x + w <= a + c and y + h <= b + d


def intersects(box, rect):
    x, y, w, h = box
    a, b, c, d = rect
    return x < a + c and a < x + w and y < b + d and b < y + h


def window_binding(window, dimensions, name):
    rect, screen = window["rect"], window["screen"]
    require(len(rect) == len(screen) == 4 and all(type(v) in (int, float) and math.isfinite(v)
            for v in rect + screen))
    require(screen[:2] == [0, 0] and inside(rect, screen), "unsupported guest monitor geometry")
    sx, sy = dimensions[0] / screen[2], dimensions[1] / screen[3]
    require(sx == sy and sx in (1, 2), "unsupported guest VNC scale")
    require(window["title"] in ("", name), "foreground window title differs from leased simulator")
    require(window["id"] > 0 and window["pid"] > 0)
    return dict(size=rect[2:], screen=screen, scale=sx)


def title_evidence(boxes, window, scale, name):
    x, y, w, _ = window["rect"]
    titlebar = (x * scale, y * scale, w * scale, 40 * scale)
    # Always corroborate even when CG supplies a title. Without Screen Recording
    # CG may omit it; only exact high-confidence title-bar OCR admits that case.
    matches = [text for text, box in boxes if inside(box, titlebar)]
    if matches != [name]:
        exact_boxes = [box for text, box in boxes if text == name]
        reason = ("no-exact-row" if not exact_boxes else
                  "outside-titlebar" if name not in matches else "multiple-qualified-labels")
        # Only the signed front window's geometry and exact requested-name boxes
        # may leave the private evidence; never include OCR strings or other rows.
        require(False, "exact leased window title lacks qualified visual evidence"
                f"; reason={reason}; front_rect_pt={window['rect']}; scale={scale}"
                f"; qualified_titlebar_rows={len(matches)}; exact_row_boxes_px={exact_boxes[:4]}")


def tooltip_region(point, scale):
    x, y = point
    return (x - 120 * scale, y - 65 * scale, 240 * scale, 130 * scale)


def tooltip_crop_box(point, scale, dimensions):
    x, y, width, height = tooltip_region(point, scale)
    frame_width, frame_height = dimensions
    box = (max(0, math.floor(x)), max(0, math.floor(y)),
           min(frame_width, math.ceil(x + width)), min(frame_height, math.ceil(y + height)))
    require(box[0] < box[2] and box[1] < box[3], "tooltip rectangle outside saved frame")
    return box


def tooltip_text_boxes(ocr, path, point, scale):
    # Reuse the compiled Accurate helper on this saved frame's fixed nearby
    # rectangle. Full-frame title OCR and changed-pixel proof keep the original.
    bounds = tooltip_crop_box(point, scale, image_size(path))
    with tempfile.TemporaryDirectory(prefix="tooltip-", dir=path.parent) as folder:
        crop = Path(folder) / "crop.png"
        fd = os.open(crop, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "wb") as output, Image.open(path) as image:
            image.crop(bounds).save(output, format="PNG")
        boxes = text_boxes(ocr, crop)
        # Vision's top-left normalized coordinates were scaled to crop pixels
        # by text_boxes. Add the *clamped* origin to recover guest desktop pixels.
        return [(text, (x + bounds[0], y + bounds[1], width, height))
                for text, (x, y, width, height) in boxes]


def tooltip_evidence(before, after, point, scale, label):
    x, y = point
    region = tooltip_region(point, scale)
    poses = {"Open", "Closed", "Partially Open"}
    near_before = [(text, box) for text, box in before if text in poses and inside(box, region)]
    near_after = [(text, box) for text, box in after if text in poses and inside(box, region)]
    require(not near_before and len(near_after) == 1 and near_after[0][0] == label,
            "fresh localized pose tooltip absent or ambiguous")
    require(not any("partial" in text.lower() for text, box in after if inside(box, region)),
            "partial-open tooltip is not full Open evidence")
    # A pose word elsewhere in the device content is never control evidence.
    box = near_after[0][1]
    require(abs(box[0] + box[2] / 2 - x) <= 75 * scale
            and box[1] + box[3] >= y - 35 * scale and box[1] <= y + 55 * scale,
            "tooltip is not scoped to candidate control")


# Private, per-calibration masks only. No Apple templates are shipped or learned
# globally. Fixed small patches exclude button edges and never contain a frame.
GLYPH_SIDE = 22
POSES = ("closed", "partial", "open")


def toolbar_points(controls, geometry):
    left, right = controls["closed"], controls["open"]
    width, height = geometry["size"]
    spacing = (right[0] - left[0]) / 2
    require(24 <= spacing <= 48 and abs(left[1] - right[1]) <= 2
            and all(12 <= x <= width - 12 and height - 90 <= y <= height - 12
                    for x, y in (left, right)), "implausible bottom pose toolbar")
    return dict(closed=left, partial=[(left[0] + right[0]) / 2, (left[1] + right[1]) / 2], open=right)


def glyph_bits(value):
    require(type(value) is str and re.fullmatch(r"[0-9a-f]{121}", value) is not None,
            "invalid private glyph mask")
    number = int(value, 16)
    bits = {i for i in range(GLYPH_SIDE ** 2) if number & (1 << i)}
    require(12 <= len(bits) <= 220, "blank or unbounded toolbar glyph")
    xs, ys = [i % GLYPH_SIDE for i in bits], [i // GLYPH_SIDE for i in bits]
    require(0 < min(xs) < max(xs) < GLYPH_SIDE - 1
            and 0 < min(ys) < max(ys) < GLYPH_SIDE - 1
            and max(xs) - min(xs) >= 5 and max(ys) - min(ys) >= 5
            and abs((min(xs) + max(xs)) / 2 - 10.5) <= 3
            and abs((min(ys) + max(ys)) / 2 - 10.5) <= 3,
            "clipped or off-center toolbar glyph")
    return bits


def glyph_similarity(a, b):
    return len(a & b) / len(a | b)


def validate_glyphs(glyphs):
    require(type(glyphs) is dict and set(glyphs) == set(POSES), "three glyphs required")
    shapes = {pose: glyph_bits(glyphs[pose]) for pose in POSES}
    require(all(glyph_similarity(shapes[a], shapes[b]) <= .80
                for i, a in enumerate(POSES) for b in POSES[i + 1:]),
            "duplicate or ambiguous toolbar glyphs")
    return shapes


def toolbar_glyphs(path, window, geometry, controls):
    points = toolbar_points(controls, geometry)
    scale = geometry["scale"]
    x, y, _, _ = window["rect"]
    require(window_binding(window, image_size(path), window["title"]) == geometry,
            "toolbar frame geometry changed")
    result = {}
    with Image.open(path) as frame:
        for pose, (px, py) in points.items():
            cx, cy = (x + px) * scale, (y + py) * scale
            radius = GLYPH_SIDE * scale / 2
            crop = (round(cx - radius), round(cy - radius), round(cx + radius), round(cy + radius))
            require(0 <= crop[0] < crop[2] <= frame.width and 0 <= crop[1] < crop[3] <= frame.height,
                    "toolbar glyph outside frame")
            pixels = list(frame.crop(crop).convert("RGB").resize((GLYPH_SIDE, GLYPH_SIDE),
                          Image.Resampling.BOX).getdata())
            border = [v for i, v in enumerate(pixels) if i % GLYPH_SIDE in (0, GLYPH_SIDE - 1)
                      or i // GLYPH_SIDE in (0, GLYPH_SIDE - 1)]
            background = [sorted(v[c] for v in border)[len(border) // 2] for c in range(3)]
            # Distance from the local background ignores uniform selection tint
            # and foreground polarity, but retains the glyph's exact position.
            # BOX averages edge coverage without ringing or clipping brighter
            # selected strokes differently from their neutral counterparts.
            contrast = [max(abs(v[c] - background[c]) for c in range(3)) for v in pixels]
            peak = max(contrast)
            require(peak >= 24, "blank or low-contrast toolbar glyph")
            bits = sum(1 << i for i, value in enumerate(contrast) if value >= max(18, peak * .4))
            result[pose] = format(bits, "0121x")
    validate_glyphs(result)
    return result


def toolbar_evidence(path, window, geometry, record):
    expected = validate_glyphs(record["glyphs"])
    observed = validate_glyphs(toolbar_glyphs(path, window, geometry, record["controls"]))
    require(all(glyph_similarity(observed[pose], expected[pose]) >= .95
                and all(glyph_similarity(observed[pose], expected[other]) <= .80
                        for other in POSES if other != pose) for pose in POSES),
            "fresh three-glyph toolbar does not match private calibration")


def localized_poses(boxes, point, scale):
    # Broad nearby region for vetoes. Positive evidence separately proves scope.
    return [(text, box) for text, box in boxes
            if (text in ("Open", "Closed", "Partially Open") or "partial" in text.lower())
            and inside(box, tooltip_region(point, scale))]


def nearby_text(boxes, point, scale, *, control_local=False):
    # text_boxes already admits only high-confidence rows. Keep arbitrary OCR
    # text private. Hover changes always use the entire wide region.
    x, y = point
    return [(text, box) for text, box in boxes
            if text.strip() and inside(box, tooltip_region(point, scale))
            and (not control_local or (abs(box[0] + box[2] / 2 - x) <= 75 * scale
                 and box[1] + box[3] >= y - 35 * scale and box[1] <= y + 55 * scale))]


def neutral_target_text(boxes, point, scale, window):
    # Baseline admission only: background behind the signed window can appear
    # in the wide crop. Use original desktop-pixel boxes, never clipped boxes
    # or containment (a partially overlapping row still intersects).
    rect = tuple(value * scale for value in window["rect"])
    return [(text, box) for text, box in nearby_text(boxes, point, scale, control_local=True)
            if intersects(box, rect)]


def display_evidence(path):
    size = image_size(path)
    with Image.open(path) as image:
        rgb = image.convert("RGB")
        # Entire display, not just a lit patch. Near-black/dim/sleeping images
        # cannot satisfy lit proof. No pixels, OCR or content hashes escape.
        red, green, blue = rgb.split()
        histogram = ImageChops.lighter(ImageChops.lighter(red, green), blue).histogram()
        total = size[0] * size[1]
        black = sum(histogram[:9]) / total
        lit = sum(histogram[25:]) / total
        state = "black" if black == 1 else "lit" if lit >= .01 else "ambiguous"
        return dict(state=state, width=size[0], height=size[1],
                    black_percent=round(black * 100), lit_percent=round(lit * 100))


def display_state(displays, strict=True):
    pair = (displays["1"]["state"], displays["3"]["state"])
    if pair not in (("lit", "black"), ("black", "lit")):
        require(not strict, "sleeping or ambiguous device displays")
        return None
    return "outer" if pair[0] == "lit" else "inner"


def private_directory(path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid() and not info.st_mode & 0o077
            and path.resolve() == path.absolute(), "private directory unavailable")
    require(not any((p / ".git").exists() for p in (path, *path.parents)), "private storage must be outside Git")
    return path


def calibration_path(vm, udid):
    folder = private_directory(Path.home() / "Library/Application Support/meguri/sim-posture")
    return folder / (hashlib.sha256((vm + "\0" + udid).encode()).hexdigest() + ".json")


def read_calibration(path):
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd) as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1
                and not info.st_mode & 0o077 and info.st_size <= 8192, "untrusted calibration record")
        return json.loads(source.read(8193))


def validate_calibration(record, binding, geometry):
    legacy = (set(record) == {"schema", "binding", "geometry", "compact", "controls"}
              and type(record["schema"]) is int and record["schema"] == 1 and record["compact"] is True)
    current = (set(record) == {"schema", "binding", "geometry", "viewport", "controls"}
               and type(record["schema"]) is int and record["schema"] == 2
               and record["viewport"] in ("compact", "expanded"))
    glyph_record = (set(record) == {"schema", "binding", "geometry", "viewport", "controls", "glyphs"}
                    and type(record["schema"]) is int and record["schema"] == 3
                    and record["viewport"] in ("compact", "expanded"))
    require((legacy or current or glyph_record) and record["binding"] == binding and record["geometry"] == geometry,
            "calibration does not match selected Xcode, target or confirmed viewport geometry")
    points = record["controls"]
    require(set(points) == {"closed", "open"})
    width, height = geometry["size"]
    for point in points.values():
        require(type(point) is list and len(point) == 2 and all(type(v) in (int, float)
                and math.isfinite(v) for v in point) and 20 < point[0] < width - 10
                and 50 < point[1] < height - 5, "invalid calibration control location")
    require(points["closed"][0] < points["open"][0]
            and abs(points["closed"][1] - points["open"][1]) <= 2,
            "calibration does not match Apple's pose control order")
    if glyph_record:
        toolbar_points(points, geometry)
        validate_glyphs(record["glyphs"])
    return points


class Holder:
    """Join old and current holders without replacing/reconnecting either.

    A unique shot after input is a FIFO execution barrier. Its complete PNG and
    the exact pinned log suffix prove the preceding command returned success.
    Same-size images alone never prove freshness: joined capture plus fresh
    title and positive tooltip or private three-glyph evidence are required.
    """
    def __init__(self, state, connected, guest, folder, lease=None):
        self.state, self.connected, self.guest, self.folder = state, connected, guest, folder
        self.lease = lease
        self.lease_identity = None if lease is None else (lease.stat().st_dev, lease.stat().st_ino)
        self.armed = False
        self.pending = False
        self.sequence = 0
        fd = os.open(state / "holder.log", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        self.log = os.fdopen(fd, "rb", buffering=0)
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and not info.st_mode & 0o022)
        self.identity = (info.st_dev, info.st_ino)
        self.capable = b"posture-click-v1\n" in self.log.read(4096).splitlines(keepends=True)

    def batch(self, command=None):
        require(command is None or re.fullmatch(r"move [0-9]+ [0-9]+", command), "read-only holder batch expected")
        require(self.connected(), "existing VNC transport disconnected")
        info = (self.state / "holder.log").lstat()
        require((info.st_dev, info.st_ino) == self.identity)
        self.log.seek(0, os.SEEK_END)
        offset = self.log.tell()
        self.sequence += 1
        path = self.folder / ("frame-" + str(self.sequence) + ".png")
        require(not path.exists())
        lines = ([command] if command else []) + ["shot " + str(path)]
        payload = ("\n".join(lines) + "\n").encode()
        require(b"\r" not in payload and len(payload.splitlines()) == len(lines))
        expected = b"".join(("ok " + line.split(" ")[0] + "\n").encode() for line in lines)
        fd = os.open(self.state / "commands", os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
        try:
            info = os.fstat(fd)
            require(stat.S_ISFIFO(info.st_mode) and info.st_uid == os.getuid() and not info.st_mode & 0o022)
            require(len(payload) <= os.fpathconf(fd, "PC_PIPE_BUF"))
            require(os.write(fd, payload) == len(payload), "FIFO publication uncertain")
        finally:
            os.close(fd)
        deadline = time.monotonic() + 130  # At most input + capture, each holder timeout is 60s.
        suffix = b""
        while time.monotonic() < deadline:
            suffix += self.log.read(4096)
            require(len(suffix) <= len(expected) and expected.startswith(suffix),
                    "holder command failed or concurrent FIFO traffic; fences retained")
            require(self.connected(), "VNC transport lost; fences retained")
            require(self.log.tell() >= offset, "holder log changed")
            if suffix == expected and path.exists():
                require(path.stat().st_size <= LIMIT, "oversized private capture")
                if inspection.complete_png(path):
                    image_size(path)
                    return path
            time.sleep(.1)
        raise RuntimeError("exact VNC command completion unproven; fences retained")

    @contextlib.contextmanager
    def scheduler(self):
        # Same kernel mutex as host-lease-mutex.sh; never wait past handoff.
        with Path(str(self.lease) + ".mutex").open("a") as lock:
            deadline = time.monotonic() + 1
            while True:
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    require(time.monotonic() < deadline, "scheduler mutex busy before click")
                    time.sleep(.05)
            info = self.lease.stat()
            require((info.st_dev, info.st_ino) == self.lease_identity, "scheduler lease replaced")
            yield

    def drop_marker(self, marker):
        # Caller holds the existing scheduler mutex.
        info = marker.stat()
        require((info.st_dev, info.st_ino) == self.marker_identity, "scheduler fence replaced")
        (marker / "intent.json").unlink(missing_ok=True)
        marker.rmdir()

    def clear_marker(self, marker):
        with self.scheduler():
            self.drop_marker(marker)

    def open_publication(self):
        fd = os.open(self.state / "commands", os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
        try:
            info = os.fstat(fd)
            require(stat.S_ISFIFO(info.st_mode) and info.st_uid == os.getuid() and not info.st_mode & 0o022)
            return fd
        except BaseException:
            os.close(fd)
            raise

    def publication(self, fd, latest, receipt):
        deadline = min(time.monotonic() + 5, latest)
        payload = ("posture-click " + repr(deadline) + " " + str(receipt) + "\n").encode()
        require(len(payload) <= os.fpathconf(fd, "PC_PIPE_BUF") and payload.count(b"\n") == 1
                and b"\r" not in payload)
        return deadline, payload

    def click_intent(self, receipt, deadline):
        return dict(receipt=str(receipt), deadline=deadline, guard=self.guest.request["guard"])

    def publish(self, fd, payload):
        return os.write(fd, payload)

    def click_receipt(self, receipt):
        if receipt.exists():
            require(receipt.stat().st_size <= 1024)
            return strict_json(receipt.read_bytes())
        return None

    def click(self, window, point):
        """One bounded synchronous reactor command, with its own terminal receipt.

        Caller supplies the exact signed window and guest-point pointer position
        used by the fresh hover proof. Reobserve after arm/ACK/ticket reservation,
        under the scheduler mutex, immediately before the input fence and send.
        Refusal sends no input; a submitted uncertain click stays fenced and is
        never retried. The ticket must still be live AFTER the observation.

        A screenshot is deliberately outside this join: failed readback cannot
        turn a completed input into an unknown in-flight click.
        """
        require(self.capable, "existing holder needs posture-click-v1; coordinator preparation required")
        fd = self.open_publication()
        try:
            start = time.monotonic()
            arm = self.guest.call("arm")
            self.armed = True
            require(type(arm) is dict and set(arm) == {"armed", "remaining"} and arm["armed"] is True
                    and type(arm["remaining"]) in (int, float) and math.isfinite(arm["remaining"])
                    and arm["remaining"] - (time.monotonic() - start) >= CLICK_BUDGET,
                    "lease too short for bounded click/readback")
            receipt = self.folder / "click.json"
            require(not receipt.exists())
            start = time.monotonic()
            ack = self.guest.call("publish")
            require(type(ack) is dict and set(ack) == {"published", "remaining"} and ack["published"] is True
                    and type(ack["remaining"]) in (int, float) and math.isfinite(ack["remaining"]),
                    "invalid click publication acknowledgment")
            latest = start + ack["remaining"] - CLICK_BUDGET
            # Local callbacks share this clock. Hosted publication separately
            # samples/bounds the resident clock before any input is published.
            deadline, payload = self.publication(fd, latest, receipt)
            require(self.connected() and time.monotonic() < deadline, "lease/transport unavailable before click")
            marker = self.lease / "sim-posture-pending"
            # Signals cannot interrupt the final admission/write or its join.
            with uninterrupted():
                with self.scheduler():
                    stable_window(self.guest, window, point, final=True)
                    require(time.monotonic() < deadline, "lease too short at atomic click handoff")
                    marker.mkdir(mode=0o700)
                    info = marker.stat()
                    self.marker_identity = (info.st_dev, info.st_ino)
                    try:
                        (marker / "intent.json").write_text(json.dumps(self.click_intent(receipt, deadline)))
                    except OSError:
                        self.drop_marker(marker)
                        raise
                    self.pending = True
                    try:
                        count = self.publish(fd, payload)
                    except OSError:
                        self.pending = False  # Atomic nonblocking PIPE_BUF write wrote nothing.
                        self.drop_marker(marker)
                        raise
                    if count == 0:
                        self.pending = False
                        self.drop_marker(marker)
                    require(count == len(payload), "click publication uncertain")
                until = time.monotonic() + 10
                while time.monotonic() < until:
                    result = self.click_receipt(receipt)
                    if result is not None:
                        require(type(result) is dict and set(result) in
                                ({"joined", "executed"}, {"joined", "executed", "error"})
                                and result["joined"] is True and type(result["executed"]) is bool
                                and ("error" not in result or result["error"] in ("expired", "input")),
                                "malformed click completion")
                        self.clear_marker(marker)
                        self.pending = False
                        self.armed = False
                        self.guest.call("disarm")
                        require(result["executed"] and "error" not in result, "holder click refused or failed")
                        return
                    # Continue joining our command even after transport/caller loss.
                    time.sleep(.05)
                raise RuntimeError("click completion unproven; fences retained")
        finally:
            if fd is not None:
                os.close(fd)
            if self.armed and not self.pending:
                self.armed = False
                self.guest.call("disarm")


@contextlib.contextmanager
def uninterrupted():
    signals = (signal.SIGHUP, signal.SIGINT, signal.SIGTERM)
    previous = {sig: signal.signal(sig, signal.SIG_IGN) for sig in signals}
    try:
        yield
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def snapshot_displays(guest, folder):
    reply = guest.call("displays")
    require(set(reply) == {"1", "3"})
    result = {}
    for key in ("1", "3"):
        data = base64.b64decode(reply[key], validate=True)
        require(len(data) <= LIMIT)
        path = folder / ("display-" + key + ".png")
        try:
            path.write_bytes(data)
            result[key] = display_evidence(path)
        finally:
            path.unlink(missing_ok=True)
    return result


def stable_window(guest, original, point=None, *, final=False):
    require(not final or point is not None, "expected click pointer unavailable")
    current = guest.call("click-window" if final else "window")
    require({k: v for k, v in current.items() if k not in ("pointer", "lookup_pid")}
            == {k: v for k, v in original.items() if k not in ("pointer", "lookup_pid")},
            "foreground window changed; no pose input permitted")
    if point is not None:
        require(len(point) == len(current["pointer"]) == 2
                and all(type(v) in (int, float) and math.isfinite(v) for v in (*point, *current["pointer"]))
                and all(abs(a - b) <= 1 for a, b in zip(current["pointer"], point)),
                "guest pointer moved away from verified control")


# Acquisition includes joined PNG transport, two bounded OCR calls and signed
# window observations. It is independent of the unchanged five-second input
# tickets: none is reserved until acquisition has completed successfully.
HOVER_ACQUISITION_SECONDS = 240
LEGACY_POLL_SECONDS = 8


def neutral_frame(guest, holder, window, geometry, name, ocr):
    """Move only, through the held VNC connection, then observe a fresh baseline."""
    stable_window(guest, window)
    x, y, w, _ = window["rect"]
    scale = geometry["scale"]
    away = (int((x + w / 2) * scale), int((y + 12) * scale))
    pointer = [v / scale for v in away]
    holder.batch("move %d %d" % away)
    stable_window(guest, window, pointer)
    time.sleep(1.2)  # Hide old tooltips before the image offered for approval.
    path = holder.batch()
    boxes = text_boxes(ocr, path)
    title_evidence(boxes, window, scale, name)
    stable_window(guest, window, pointer)
    return path, pointer, boxes


def hover(guest, holder, window, geometry, name, point, label, ocr, record=None):
    scale = geometry["scale"]
    x, y, _, _ = window["rect"]
    baseline, _, before = neutral_frame(guest, holder, window, geometry, name, ocr)
    glyph_proof = record is not None and record["schema"] == 3
    if glyph_proof:
        toolbar_evidence(baseline, window, geometry, record)
    absolute = (round((x + point[0]) * scale), round((y + point[1]) * scale))
    before_tooltip = tooltip_text_boxes(ocr, baseline, absolute, scale)
    samples, after_tooltip = 0, []
    path = None
    saw_pose = False
    reason = "no tooltip samples"
    fixed_reasons = (
        "fresh localized pose tooltip absent or ambiguous",
        "partial-open tooltip is not full Open evidence",
        "tooltip is not scoped to candidate control",
    )

    def reject(why):
        # Failure only: fixed vocabulary, never recognized content or geometry.
        poses = ("Closed", "Open", "Partially Open")
        region = tooltip_region(absolute, scale)
        raise RuntimeError("tooltip hover rejected; " + json.dumps(dict(
            requested=label if label in poses else "unknown",
            baseline_known_pose=any(text in poses and inside(box, region) for text, box in before_tooltip),
            after_labels=[pose for pose in poses
                          if any(text == pose and inside(box, region) for text, box in after_tooltip)],
            samples=samples, reason=why)))

    # A visible old label is not a neutral baseline. Refuse instead of allowing
    # a correct-but-stale tooltip or a later unlabelled frame to enable glyphs.
    if (localized_poses(before, absolute, scale) or localized_poses(before_tooltip, absolute, scale)
            or neutral_target_text(before, absolute, scale, window)
            or neutral_target_text(before_tooltip, absolute, scale, window)):
        reject("fresh localized pose tooltip absent or ambiguous")
    stable_window(guest, window)
    deadline = time.monotonic() + HOVER_ACQUISITION_SECONDS
    started = time.monotonic()
    path = holder.batch("move %d %d" % absolute)
    poll_remaining = LEGACY_POLL_SECONDS
    while True:
        require(time.monotonic() < deadline, "tooltip proof deadline expired")
        stable_window(guest, window, [x + point[0], y + point[1]])
        after = text_boxes(ocr, path)
        title_evidence(after, window, scale, name)
        after_tooltip = tooltip_text_boxes(ocr, path, absolute, scale)
        stable_window(guest, window, [x + point[0], y + point[1]])
        samples += 1
        # Both OCR views are of this exact frame. Check each view independently:
        # agreement across views is fine, but duplicates/conflicts in either view
        # or any wrong pose fail immediately, including before a later no-label
        # sample. The broad nearby rectangle also blocks out-of-scope fallback.
        for boxes in (after, after_tooltip):
            localized = localized_poses(boxes, absolute, scale)
            if len(localized) > 1 or any(text != label for text, _ in localized):
                reject("fresh localized pose tooltip absent or ambiguous")
            saw_pose = saw_pose or bool(localized)
        # An unknown/localized overlay is a veto too, even beside a correct
        # label or before a later blank frame. Compare each OCR view with its
        # own baseline so stable canvas text cannot masquerade as a new overlay.
        for old, boxes in ((before, after), (before_tooltip, after_tooltip)):
            prior = nearby_text(old, absolute, scale)
            for row in nearby_text(boxes, absolute, scale):
                if row[0] != label:
                    if row not in prior:
                        reject("fresh localized pose tooltip absent or ambiguous")
                    prior.remove(row)  # Each baseline row admits at most one unchanged row.
        if glyph_proof:
            toolbar_evidence(path, window, geometry, record)
        require(time.monotonic() < deadline, "tooltip proof deadline expired")
        try:
            tooltip_evidence(before_tooltip, after_tooltip, absolute, scale, label)
        except RuntimeError as error:
            reason = next((fixed for fixed in fixed_reasons if str(error) == fixed),
                          "tooltip predicate rejected")
            if glyph_proof and not saw_pose:
                # One complete, in-budget observation is sufficient. Never
                # discard it to start another expensive joined PNG/OCR sample.
                require(time.monotonic() < deadline, "tooltip proof deadline expired")
                return "fresh private three-glyph toolbar for " + label
            finished = time.monotonic()
            # Legacy tooltip-only proof may need a delayed tooltip. Allow
            # eight seconds of polling delays, separately from capture/transfer
            # and OCR time. Admit a repeat only with acquisition-budget room
            # for the last complete sample. An overrun still refuses all input.
            if poll_remaining <= 0 or finished + .25 + (finished - started) >= deadline:
                break
            time.sleep(.25)
            poll_remaining -= .25
            started = time.monotonic()
            path = holder.batch()
            continue
        with Image.open(baseline) as a, Image.open(path) as b:
            require(a.size == b.size)
            region = tooltip_crop_box(absolute, scale, a.size)
            require(a.crop(region).tobytes() != b.crop(region).tobytes(), "stale VNC tooltip image")
        require(time.monotonic() < deadline, "tooltip proof deadline expired")
        return "fresh en-US " + label + " tooltip"
    reject(reason)


def calibrate_toolbar(guest, holder, window, geometry, name, controls, approved, ocr):
    """Observe only: the operator approved all three glyphs in a neutral frame."""
    points = toolbar_points(controls, geometry)
    x, y, _, _ = window["rect"]
    pointer = window["pointer"]
    require(len(pointer) == 2 and all(abs(pointer[0] - x - px) > 24 or abs(pointer[1] - y - py) > 65
                                    for px, py in points.values()),
            "calibration needs the verified neutral pointer")
    record = dict(controls=controls, glyphs=toolbar_glyphs(approved, window, geometry, controls))
    approved_text = {}
    # Include the neutralized, operator-approved frame, then two fresh joined
    # resident captures. No buttons, keyboard input or further pointer movement.
    for path in (approved, None, None):
        stable_window(guest, window, pointer)
        if path is None:
            time.sleep(.2)
            path = holder.batch()
        boxes = text_boxes(ocr, path)
        title_evidence(boxes, window, geometry["scale"], name)
        for pose, (px, py) in points.items():
            absolute = ((x + px) * geometry["scale"], (y + py) * geometry["scale"])
            crop_boxes = tooltip_text_boxes(ocr, path, absolute, geometry["scale"])
            require(not localized_poses(boxes, absolute, geometry["scale"])
                    and not localized_poses(crop_boxes, absolute, geometry["scale"])
                    and not neutral_target_text(boxes, absolute, geometry["scale"], window)
                    and not neutral_target_text(crop_boxes, absolute, geometry["scale"], window),
                    "calibration requires a neutral toolbar")
            # Each view must remain stable against the operator-approved
            # frame in BOTH fresh parked captures, including outside-window
            # text. Keep contents/bounds in memory only, never in calibration.
            nearby = [sorted(nearby_text(view, absolute, geometry["scale"]))
                      for view in (boxes, crop_boxes)]
            if pose not in approved_text:
                approved_text[pose] = nearby
            require(nearby == approved_text[pose], "calibration requires stable parked text")
        toolbar_evidence(path, window, geometry, record)
        stable_window(guest, window, pointer)
    return record["glyphs"]


def close_guest(guest, pending):
    """Prove remote guard exit, or keep scheduler/pool fences. Own child only."""
    acknowledged = False
    code = None
    try:
        if not pending:
            if guest.admitted:
                guest.call("finish")
                acknowledged = True
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
        if not pending:
            with contextlib.suppress(OSError, ValueError, RuntimeError, subprocess.SubprocessError):
                guest.terminal()
    finally:
        with contextlib.suppress(OSError):
            guest.child.stdin.close()
        try:
            code = guest.child.wait(timeout=10 if not pending else 1)
        except subprocess.TimeoutExpired:
            guest.child.terminate()
            try:
                guest.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                guest.child.kill()
                guest.child.wait(timeout=5)
        guest.child.stdout.close()
    # Before click publication a lost transport cannot have emitted button input.
    # The read-only guest guard releases on EOF or its own bounded alarm.
    safe = not pending
    return safe, safe and acknowledged and code == 0


def operator_terminal():
    try:
        fd = os.open("/dev/tty", os.O_RDWR | os.O_NOCTTY)
    except OSError:
        raise RuntimeError("calibration requires the foreground operator terminal") from None
    try:
        require(os.tcgetpgrp(fd) == os.getpgrp() and termios.tcgetattr(fd)[3] & termios.ICANON,
                "calibration requires the foreground canonical operator terminal")
        return fd
    except BaseException:
        os.close(fd)
        raise


def operator_controls(path, name, geometry):
    # Authenticated local foreground terminal only. No file/path/executable input.
    fd = operator_terminal()
    try:
        deadline = time.monotonic() + 180
        def prompt(message):
            data = message.encode()
            require(os.write(fd, data) == len(data))
        def answer(limit):
            readable, _, _ = select.select([fd], [], [], max(0, deadline - time.monotonic()))
            require(readable, "operator calibration timed out")
            line = os.read(fd, limit).decode()
            require(line.endswith("\n"), "operator calibration input too long")
            return line
        prompt("Private guest capture: " + str(path) + "\nConfirm exactly the leased target " + name
               + " is visible with all three distinct, unoccluded pose glyphs below its bezel in documented "
               "Closed (left), Partially Open (middle), Open (right) order, in compact or expanded canvas. "
               "Confirm a neutral toolbar with the pointer away and no tooltip or overlay. "
               "A sidebar row or generic library is insufficient. Type COMPACT " + name
               + " or EXPANDED " + name + ":\n")
        confirmation = answer(256).strip()
        require(confirmation in ("COMPACT " + name, "EXPANDED " + name),
                "operator target/viewport confirmation refused")
        viewport = confirmation.split(" ", 1)[0].lower()
        controls = {}
        for pose in ("closed", "open"):
            prompt(pose + ": enter x y in guest window points, relative to its top-left (PNG pixels / scale "
                   + str(geometry["scale"]) + ", then subtract window origin):\n")
            line = answer(128)
            require(re.fullmatch(r"[0-9]+(?:\.[0-9]+)? [0-9]+(?:\.[0-9]+)?\n", line) is not None)
            controls[pose] = [float(v) for v in line.split()]
        return viewport, controls
    finally:
        os.close(fd)


def run(state, ssh, vm, checkout, udid, tart, action, lease):
    connected = lambda: session.status(state, vm, tart)["connected"]
    folder = None
    guest = None
    holder = None
    joined = False
    log = None
    try:
        require(re.fullmatch(r"[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}", udid)
                and action in ("open", "closed", "calibrate"), "expected pose and leased simulator UDID")
        if action == "calibrate":
            os.close(operator_terminal())  # Fail before guest launch, compilation or capture.
        require(connected(), "existing VNC holder unavailable; posture never prepares or reconnects")
        base = Path(tempfile.gettempdir()).resolve()
        require(not any((p / ".git").exists() for p in (base, *base.parents)))
        folder = Path(tempfile.mkdtemp(prefix="meguri-posture-", dir=base))
        folder.chmod(0o700)
        ocr = OffscreenText(folder)
        log = (folder / "guest.log").open("wb")
        request = dict(vm=vm, checkout=checkout, udid=udid, guard=uuid.uuid4().hex,
                       pin=json.loads((Path(checkout) / "tools/sim-devices.json").read_text())["foldable"]["runtime"],
                       pool=os.environ.get("MEGURI_SIM_POOL_DIR", ""),
                       visual=Path(__file__).with_name("sim-posture-visual.swift").read_text())
        # Assign before admission so a failed ready reply still joins our child.
        guest = Guest.__new__(Guest)
        guest.__init__(ssh, request, log)
        binding = guest.ready["binding"]
        window = guest.call("window")
        if session.hosted_state(state):
            holder = sibling("vnc-hosted").RemoteHolder(state, connected, guest, folder, lease)
        else:
            holder = Holder(state, connected, guest, folder, lease)
        path = holder.batch()
        geometry = window_binding(window, image_size(path), binding["name"])
        title_evidence(text_boxes(ocr, path), window, geometry["scale"], binding["name"])
        record_path = calibration_path(vm, udid)
        if action == "calibrate":
            # Initial signed exact leased window and title have been proved.
            # Neutralize BEFORE presenting an image to the operator. Keep the
            # original window binding, with the actually observed safe pointer.
            path, pointer, _ = neutral_frame(guest, holder, window, geometry, binding["name"], ocr)
            window = dict(window, pointer=pointer)
            print("sim-posture: guest window rect (points) " + json.dumps(window["rect"]), file=sys.stderr)
            viewport, controls = operator_controls(path, binding["name"], geometry)
            # Validate the existing endpoint constraints before deriving a middle.
            record = dict(schema=2, binding=binding, geometry=geometry, viewport=viewport, controls=controls)
        else:
            record = read_calibration(record_path)
        controls = validate_calibration(record, binding, geometry)
        before = snapshot_displays(guest, folder)
        display_state(before)
        if action == "calibrate":
            glyphs = calibrate_toolbar(guest, holder, window, geometry, binding["name"], controls, path, ocr)
            record.update(schema=3, glyphs=glyphs)
            validate_calibration(record, binding, geometry)
            fd, temporary = tempfile.mkstemp(prefix=".calibration-", dir=record_path.parent)
            try:
                with os.fdopen(fd, "w") as output:
                    json.dump(record, output)
                os.replace(temporary, record_path)
            finally:
                Path(temporary).unlink(missing_ok=True)
            result = dict(calibrated=True, vm=vm, udid=udid, click_count=0)
        else:
            evidence = hover(guest, holder, window, geometry, binding["name"], controls[action], action.title(), ocr,
                             record=record)
            # Exactly one synchronous real click, with a separate terminal join.
            # Display activity alone never identifies full Open versus Partially Open.
            holder.click(window, [window["rect"][0] + controls[action][0],
                                  window["rect"][1] + controls[action][1]])
            deadline = time.monotonic() + 25
            after = None
            matches = 0
            while time.monotonic() < deadline:
                require(connected(), "VNC transport lost during display proof")
                after = snapshot_displays(guest, folder)
                observed = display_state(after, strict=False)
                if observed == ("inner" if action == "open" else "outer"):
                    matches += 1
                    if matches == 2:
                        break
                else:
                    matches = 0
                time.sleep(.5)
            else:
                raise RuntimeError("verified pose click did not reach expected display state")
            require(connected(), "VNC transport lost during display proof")
            stable_window(guest, window)
            result = dict(vm=vm, udid=udid, requested=action, click_count=1,
                          control_evidence=evidence + "; joined click",
                          before=before, after=after)
        return result
    finally:
        # One bounded cleanup, including repeated operator signals.
        with uninterrupted():
            failed = sys.exc_info()[0] is not None
            if holder is not None:
                holder.log.close()
            if guest is not None and hasattr(guest, "child"):
                joined, clean = close_guest(guest, holder is not None and holder.pending)
            else:
                joined = True  # No remote child or FIFO operation was started.
                clean = True
            if log is not None:
                log.close()
            if joined:
                if folder is not None:
                    for entry in folder.iterdir():
                        entry.unlink()
                    folder.rmdir()
            else:
                print("sim-posture: completion unproven; scheduler/pool fences and private evidence retained for coordinator", file=sys.stderr)
            # A successful pose is insufficient if the exact guard did not join.
            require(joined, "posture cleanup/join unproven; coordinator reconciliation required")
            require(clean or failed, "guest guard exited unsuccessfully")


def main():
    os.umask(0o077)
    def interrupted(*unused):
        raise RuntimeError("posture interrupted")
    for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, interrupted)
    try:
        state, ssh, vm, checkout, udid, tart, action, lease = sys.argv[1:]
        print(json.dumps(run(Path(state), ssh, vm, checkout, udid, tart, action, Path(lease))))
        return 0
    except MachineUnavailable:
        print("sim-posture: mac-vm transport unavailable", file=sys.stderr)
        return 5
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
        # Raw native errors may carry device/card/window content or credentials.
        message = str(error) if isinstance(error, RuntimeError) else "private posture evidence unavailable"
        print("sim-posture: " + message, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
