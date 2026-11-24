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
''' + "".join("    " + line + "\n" for line in inspection.GUEST_VALIDATE.splitlines()) + r'''
    # Verify Apple authorship, not just a self-consistent signature.
    stage = "signature"
    run("/usr/bin/codesign", "--verify", "--strict", "-R", "anchor apple", str(hub))
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
        stage = "open"
        run("/usr/bin/open", "-a", str(hub), "--args", "-CurrentDeviceUDID", request["udid"])
        print(json.dumps(dict(ready=True, binding=binding)), flush=True)
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
            require(set(message) == ({"id", "action", "png"} if action == "text" else {"id", "action"}))
            stage = action
            if action not in ("disarm", "finish"):
                check_lease()
            if action == "window":
                window = json.loads(run(str(visual), "window"))
                require(window["bundle"] == str(hub))
                executable = pathlib.Path(window["executable"])
                require(hub in executable.parents)
                run("/usr/bin/codesign", "--verify", "--strict", "-R", "anchor apple", str(executable))
                result = window
            elif action == "text":
                image = folder / "vnc.png"
                payload = base64.b64decode(message["png"], validate=True)
                require(len(payload) <= 25165824)
                image.write_bytes(payload)
                try:
                    result = json.loads(run(str(visual), "text", str(image)))
                finally:
                    image.unlink(missing_ok=True)
            elif action == "displays":
                result = {}
                for display in (1, 3):
                    image = folder / (str(display) + ".png")
                    run(simctl, "io", request["udid"], "screenshot", "--display=" + str(display), str(image))
                    require(image.stat().st_size <= 16777216)
                    result[str(display)] = base64.b64encode(image.read_bytes()).decode()
                    image.unlink()
            elif action == "arm":
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
GUEST_STAGES = set(inspection.GUEST_STAGES) | {"pool", "binding", "compile", "window", "text", "displays", "arm", "publish", "disarm", "finish"}
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


def text_boxes(guest, path):
    width, height = image_size(path)
    result = guest.call("text", png=base64.b64encode(path.read_bytes()).decode())
    require(isinstance(result, list) and len(result) <= 2000)
    boxes = []
    for row in result:
        require(isinstance(row["text"], str) and len(row["text"]) <= 1024)
        box = row["box"]
        require(len(box) == 4 and all(type(v) in (int, float) and math.isfinite(v) and 0 <= v <= 1 for v in box))
        if row["confidence"] >= .9:
            boxes.append((row["text"], (box[0] * width, box[1] * height, box[2] * width, box[3] * height)))
    return boxes


def inside(box, rect):
    x, y, w, h = box
    a, b, c, d = rect
    return a <= x and b <= y and x + w <= a + c and y + h <= b + d


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
    require(matches == [name], "exact leased window title lacks qualified visual evidence")


def tooltip_evidence(before, after, point, scale, label):
    x, y = point
    region = (x - 120 * scale, y - 65 * scale, 240 * scale, 130 * scale)
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
    require(set(record) == {"schema", "binding", "geometry", "compact", "controls"}
            and record["schema"] == 1 and record["compact"] is True
            and record["binding"] == binding and record["geometry"] == geometry,
            "calibration does not match selected Xcode, target or compact geometry")
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
    return points


class Holder:
    """Join old and current holders without replacing/reconnecting either.

    A unique shot after input is a FIFO execution barrier. Its complete PNG and
    the exact pinned log suffix prove the preceding command returned success.
    Same-size images alone never prove freshness: title + changed tooltip do.
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

    def click(self):
        """One bounded synchronous reactor command, with its own terminal receipt.

        A screenshot is deliberately outside this join: failed readback cannot
        turn a completed input into an unknown in-flight click.
        """
        require(self.capable, "existing holder needs posture-click-v1; coordinator preparation required")
        fd = os.open(self.state / "commands", os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
        try:
            info = os.fstat(fd)
            require(stat.S_ISFIFO(info.st_mode) and info.st_uid == os.getuid() and not info.st_mode & 0o022)
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
            # Both the caller and the reactor check this exact monotonic handoff.
            deadline = min(time.monotonic() + 5, latest)
            payload = ("posture-click " + repr(deadline) + " " + str(receipt) + "\n").encode()
            require(len(payload) <= os.fpathconf(fd, "PC_PIPE_BUF") and payload.count(b"\n") == 1
                    and b"\r" not in payload)
            require(self.connected() and time.monotonic() < deadline, "lease/transport unavailable before click")
            marker = self.lease / "sim-posture-pending"
            # Signals cannot interrupt the final admission/write or its join.
            with uninterrupted():
                with self.scheduler():
                    require(time.monotonic() < deadline, "lease too short at atomic click handoff")
                    marker.mkdir(mode=0o700)
                    info = marker.stat()
                    self.marker_identity = (info.st_dev, info.st_ino)
                    try:
                        (marker / "intent.json").write_text(json.dumps(dict(
                            receipt=str(receipt), deadline=deadline,
                            guard=self.guest.request["guard"])))
                    except OSError:
                        self.drop_marker(marker)
                        raise
                    self.pending = True
                    try:
                        count = os.write(fd, payload)
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
                    if receipt.exists():
                        require(receipt.stat().st_size <= 1024)
                        result = strict_json(receipt.read_bytes())
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


def stable_window(guest, original, point=None):
    current = guest.call("window")
    require({k: v for k, v in current.items() if k != "pointer"}
            == {k: v for k, v in original.items() if k != "pointer"},
            "foreground window changed; no pose input permitted")
    if point is not None:
        require(len(current["pointer"]) == 2
                and all(abs(a - b) <= 1 for a, b in zip(current["pointer"], point)),
                "guest pointer moved away from verified control")


def hover(guest, holder, window, geometry, name, point, label):
    stable_window(guest, window)
    scale = geometry["scale"]
    x, y, w, _ = window["rect"]
    away = (int((x + w / 2) * scale), int((y + 12) * scale))
    holder.batch("move %d %d" % away)
    time.sleep(1.2)  # Hide old tooltips, then capture a pointer-away baseline.
    baseline = holder.batch()
    before = text_boxes(guest, baseline)
    title_evidence(before, window, scale, name)
    absolute = (round((x + point[0]) * scale), round((y + point[1]) * scale))
    stable_window(guest, window)
    holder.batch("move %d %d" % absolute)
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        stable_window(guest, window, [x + point[0], y + point[1]])
        path = holder.batch()
        after = text_boxes(guest, path)
        title_evidence(after, window, scale, name)
        stable_window(guest, window, [x + point[0], y + point[1]])
        try:
            tooltip_evidence(before, after, absolute, scale, label)
        except RuntimeError:
            time.sleep(.25)
            continue
        with Image.open(baseline) as a, Image.open(path) as b:
            require(a.size == b.size)
            region = (max(0, absolute[0] - 120 * scale), max(0, absolute[1] - 65 * scale),
                      min(a.width, absolute[0] + 120 * scale), min(a.height, absolute[1] + 65 * scale))
            require(a.crop(region).tobytes() != b.crop(region).tobytes(), "stale VNC tooltip image")
        return
    raise RuntimeError("fresh localized pose tooltip absent or ambiguous")


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
        prompt("Private guest capture: " + str(path) + "\nConfirm exactly this leased simulator in COMPACT view, "
               "English controls below bezel. Type COMPACT " + name + ":\n")
        require(answer(256).strip() == "COMPACT " + name, "operator compact confirmation refused")
        controls = {}
        for pose in ("closed", "open"):
            prompt(pose + ": enter x y in guest window points, relative to its top-left (PNG pixels / scale "
                   + str(geometry["scale"]) + ", then subtract window origin):\n")
            line = answer(128)
            require(re.fullmatch(r"[0-9]+(?:\.[0-9]+)? [0-9]+(?:\.[0-9]+)?\n", line) is not None)
            controls[pose] = [float(v) for v in line.split()]
        return controls
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
        holder = Holder(state, connected, guest, folder, lease)
        path = holder.batch()
        geometry = window_binding(window, image_size(path), binding["name"])
        title_evidence(text_boxes(guest, path), window, geometry["scale"], binding["name"])
        record_path = calibration_path(vm, udid)
        if action == "calibrate":
            print("sim-posture: guest window rect (points) " + json.dumps(window["rect"]), file=sys.stderr)
            record = dict(schema=1, binding=binding, geometry=geometry, compact=True,
                          controls=operator_controls(path, binding["name"], geometry))
        else:
            record = read_calibration(record_path)
        controls = validate_calibration(record, binding, geometry)
        before = snapshot_displays(guest, folder)
        display_state(before)
        if action == "calibrate":
            for pose in ("closed", "open"):
                hover(guest, holder, window, geometry, binding["name"], controls[pose], pose.title())
            fd, temporary = tempfile.mkstemp(prefix=".calibration-", dir=record_path.parent)
            try:
                with os.fdopen(fd, "w") as output:
                    json.dump(record, output)
                os.replace(temporary, record_path)
            finally:
                Path(temporary).unlink(missing_ok=True)
            result = dict(calibrated=True, vm=vm, udid=udid, click_count=0)
        else:
            hover(guest, holder, window, geometry, binding["name"], controls[action], action.title())
            # Exactly one synchronous real click, with a separate terminal join.
            # Display activity alone never identifies full Open versus Partially Open.
            holder.click()
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
                          control_evidence="fresh en-US " + action.title() + " tooltip; joined click",
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
