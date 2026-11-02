#!/usr/bin/env python3
"""Internal J2.1 post-finalization receipt transport; never launches an agent.

FD 3 is MG-owned private context, not a public route or destination option.
Mac uses an actual app-hosted OS path preflight; mobile uses the retained
reserved simulator's data-container observation. Routing and setup observations
remain private. Original artifact paths resolve only on the host.
"""

import contextlib
import ctypes
import importlib.util
import inspect
import copy
import hashlib
import plistlib
import signal
import struct
import subprocess
import json
import os
from pathlib import Path
import re
import shlex
import stat
import sys
import tempfile

_spec = importlib.util.spec_from_file_location(
    "cloud_agent_install", Path(__file__).with_name("cloud-agent-install.py"))
i = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = i
_spec.loader.exec_module(i)
v, need, strict, CAP = i.v, i.need, i.strict, i.CAP
ROLES = ("build-result.json", "agent-build.json", "deployment-result.json", "agent-deployment.json")


def directory_pin(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid)


def file_pin(info):
    return (*v.identity(info), info.st_uid, info.st_gid)


class Directory:
    """Retain the entire physical chain, including ownership and permissions."""
    def __init__(self, path):
        need(os.path.isabs(path) and os.path.normpath(path) == path and
             "\x00" not in path, "directory-authority")
        self.held, self.bindings = [], []
        try:
            fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            self.held.append(fd)
            for name in path.split("/")[1:]:
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                self.held.append(child)
                self.bindings.append((fd, name, child, directory_pin(os.fstat(child))))
                fd = child
            self.fd = fd
            self.check()
        except BaseException:
            self.close()
            raise

    def check(self):
        for parent, name, fd, pin in self.bindings:
            need(directory_pin(os.fstat(fd)) == pin and
                 directory_pin(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
                 "directory-changed")
        info = os.fstat(self.fd)
        need(info.st_uid == os.getuid() and not info.st_mode & 0o022, "directory-authority")

    def close(self):
        for fd in reversed(self.held):
            os.close(fd)
        self.held = []


class Original:
    def __init__(self, path):
        self.parent, self.fd = Directory(os.path.dirname(path)), None
        self.name = os.path.basename(path)
        try:
            self.fd = os.open(self.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                              dir_fd=self.parent.fd)
            info = os.fstat(self.fd)
            need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                 info.st_uid == os.getuid() and not info.st_mode & 0o022 and
                 0 < info.st_size <= CAP, "file-authority")
            self.pin = file_pin(info)
            chunks, count = [], 0
            while True:
                raw = os.read(self.fd, min(1024 * 1024, CAP + 1 - count))
                if not raw:
                    break
                chunks.append(raw)
                count += len(raw)
                need(count <= CAP, "evidence-size")
            self.raw = b"".join(chunks)
            self.value = strict(self.raw)
            self.reference = {"path": path, "sha256": v.digest(self.raw), "bytes": len(self.raw)}
            self.check()
        except BaseException:
            self.close()
            raise

    def check(self):
        self.parent.check()
        need(file_pin(os.fstat(self.fd)) == self.pin and
             file_pin(os.stat(self.name, dir_fd=self.parent.fd, follow_symlinks=False)) == self.pin,
             "file-changed")

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None
        self.parent.close()


def exited(run, directory):
    # MG writes its result before removing .mg and exiting. Neither a marker
    # nor a complete-looking envelope proves that the child has returned.
    need(not any(os.path.lexists(os.path.join(directory, name))
                 for name in (".mg", "result.json.tmp")), "receipt-in-progress")
    pid = int(run.rsplit("-", 1)[1])
    need(pid > 1, "receipt-process")
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return
    except PermissionError:
        pass
    raise v.Refused("receipt-in-progress")


def options(args):
    result, words = {}, iter(args)
    for key in words:
        need(key in ("--parent-run", "--header-template", "--lease-run") and key not in result,
             "install-arguments")
        result[key] = next(words, None)
        need(isinstance(result[key], str) and result[key], "install-arguments")
    need({"--parent-run", "--header-template"} <= set(result), "install-arguments")
    return result


@contextlib.contextmanager
def inputs(root, parent, child):
    need(isinstance(parent, str) and v.MG_RUN_ID.fullmatch(parent) and
         isinstance(child, str) and v.MG_RUN_ID.fullmatch(child) and parent != child, "run-reference")
    sha, results = v.source(root), os.path.join(root, "build/mg")
    directory = os.path.join(results, "cloudkit-agent-install", child)
    with contextlib.ExitStack() as held:
        originals = []

        def retain(path):
            v.contained(os.path.join(root, "build"), path)
            original = Original(path)
            held.callback(original.close)
            need(not any(original.pin[:2] == other.pin[:2] for other in originals), "aliased-role")
            originals.append(original)
            return original

        def receipt(reference):
            path = os.path.join(results, reference["job"], reference["run_id"], "result.json")
            original = retain(path)
            value, hashed = v.receipt(root, results, reference, sha)
            need(value == original.value and hashed == original.reference["sha256"], "receipt-changed")
            exited(reference["run_id"], os.path.dirname(path))
            return original

        header = retain(os.path.join(directory, "run.json"))
        expected = v.header(root, header.reference["path"], parent, sha)
        need(expected == header.value and re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,63}", expected["device"])
             and expected["deployment_receipt"] == {"job": "cloudkit-agent-install", "run_id": child}
             and expected["build_receipt"].get("job") == "build-app", "header-binding")
        build = receipt(expected["build_receipt"])
        deployment = receipt(expected["deployment_receipt"])
        need(build.reference["sha256"] == expected["build_receipt_sha256"], "build-receipt-bytes")
        args = options(deployment.value["args"])
        need(args["--parent-run"] == parent, "install-parent")
        v.contained(os.path.join(root, "build"), args["--header-template"])

        def declared(owner, reference, kind=None):
            v.check_reference(reference)
            matches = [a for a in owner.value["artifacts"] if a["path"] == reference["path"]]
            need(len(matches) == 1 and (kind is None or matches[0]["kind"] == kind), "artifact-declaration")
            original = retain(reference["path"])
            need(original.reference == reference, "artifact-bytes")
            return original

        need(sum(a["path"] == header.reference["path"] and a["kind"] == "json"
                 for a in deployment.value["artifacts"]) == 1, "artifact-declaration")
        build_manifest = declared(build, build.value["details"].get("agent_build"), "json")
        deployment_manifest = declared(deployment, deployment.value["details"].get("agent_deployment"), "json")
        need(build_manifest.name == "agent-build.json" and deployment_manifest.name == "agent-deployment.json",
             "manifest-role")
        manifest, _ = v.build_product(root, build.value, expected)
        need(manifest == build_manifest.value, "build-manifest")
        d = deployment_manifest.value
        bindings = {"protocol_version": 1, "job": "cloudkit-agent-install", "run_id": child,
                    "source_commit": sha, "parent_run": parent, "device": expected["device"],
                    "device_kind": expected["device_kind"], "os_build": expected["os_build"],
                    "header_sha256": header.reference["sha256"], "build_receipt": expected["build_receipt"],
                    "build_receipt_sha256": expected["build_receipt_sha256"],
                    "build_manifest": build_manifest.reference}
        need(isinstance(d, dict) and set(d) == set(bindings) | {"delivered_manifest", "xctestrun", "installation"}
             and all(type(d[k]) is type(wanted) and d[k] == wanted for k, wanted in bindings.items()),
             "deployment-binding")
        # The install receipt declares the same original build manifest too.
        need(sum(a["path"] == build_manifest.reference["path"] and a["kind"] == "json"
                 for a in deployment.value["artifacts"]) == 1, "artifact-declaration")
        delivered = declared(deployment, d["delivered_manifest"], "json")
        need(isinstance(delivered.value, dict) and set(delivered.value) == {"path", "entries"}
             and delivered.value["entries"] == manifest["products"]["entries"], "delivery-inventory")
        v.contained(os.path.join(root, "build"), delivered.value["path"])
        v.declared_reference(root, deployment.value, d["xctestrun"])
        need(d["xctestrun"] == {**manifest["xctestrun"], "path": os.path.join(
            manifest["products"]["path"], v.relative(manifest["xctestrun"]["path"]))}, "xctestrun-binding")
        if expected["device_kind"] == "mac":
            need(d["installation"] is None and "--lease-run" not in args, "installation-binding")
        else:
            installation = declared(deployment, d["installation"], "json").value
            prefix = manifest["app"]["path"] + "/"
            entries = {name[len(prefix):]: entry for name, entry in manifest["products"]["entries"].items()
                       if name.startswith(prefix)}
            need(installation == {"protocol_version": 1, "operation": "mg-vm-simctl-install-and-nofollow-readback",
                 "device": expected["device"], "command_exit": 0, "app": manifest["app"],
                 "build_manifest": build_manifest.reference, "entries": entries}, "installation-binding")
        files = dict(zip(ROLES, (build, build_manifest, deployment, deployment_manifest)))

        def check():
            for original in originals:
                original.check()
            for original in (build, deployment):
                exited(original.value["run_id"], os.path.dirname(original.reference["path"]))
            need(v.source(root) == sha, "source-changed")

        check()
        yield expected, header, files, args, check
        check()


@contextlib.contextmanager
def frozen_destination(parent, sha, label):
    # Read-only: missing maps are not created, extended, or relabelled here.
    path = os.path.join(os.path.expanduser("~/Library/Application Support"),
                        "Meguri/CloudKitSignedPrivate", parent, "destinations.json")
    original = Original(path)
    try:
        need(stat.S_IMODE(os.fstat(original.fd).st_mode) == 0o600, "private-authority")
        need(all(pin[3] == os.getuid() and stat.S_IMODE(pin[2]) == 0o700
                 for _, _, _, pin in original.parent.bindings[-3:]), "private-authority")
        i.validate_destinations(original.value, parent, sha)
        selected = original.value["destinations"].get(label)
        need(selected is not None, "destination-missing")
        yield selected, original.check
        original.check()
    finally:
        original.close()


def select(root, parent, child, configured):
    with inputs(root, parent, child) as (expected, _, _, _, _):
        with frozen_destination(parent, expected["source_commit"], expected["device"]) as (destination, _):
            need(isinstance(configured, list), "configured-vm")
            found = [entry for entry in configured if isinstance(entry, dict) and
                     entry.get("name") == destination["vm"]]
            need(len(found) == 1, "configured-vm")
            if found[0].get("state") != "running":
                raise i.Queued("vm-unavailable")
            i.VM(root, found[0])
            return found[0]


def publish_exclusive(source, name, destination, leaf):
    """Atomic directory publication with an OS-enforced no-replace condition."""
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "darwin":
        fn, flag = libc.renameatx_np, 0x00000004  # RENAME_EXCL
    else:
        fn, flag = libc.renameat2, 1  # RENAME_NOREPLACE (generated Linux tests)
    fn.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    fn.restype = ctypes.c_int
    need(fn(source, os.fsencode(name), destination, os.fsencode(leaf), flag) == 0, "publication-conflict")


def private_child(parent, name, held):
    fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
    held.callback(os.close, fd)
    pin = directory_pin(os.fstat(fd))
    need(pin[3] == os.getuid() and not pin[2] & 0o022, "directory-authority")

    def check():
        need(directory_pin(os.fstat(fd)) == pin and directory_pin(os.stat(
            name, dir_fd=parent, follow_symlinks=False)) == pin, "directory-changed")
    return fd, check


def write_new(fd, name, raw):
    out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=fd)
    with os.fdopen(out, "wb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def guest_evidence(root, parent, label, stream, copy, expected_root=None):
    """Fixed MG path only. Stage all roles, read back, exclusively publish.

    A failed stage stays private and unreferenced. No recursive cleanup and no
    replacement of a pre-existing role, partial publication, or live journal.
    """
    need(v.MG_RUN_ID.fullmatch(parent) and re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,63}", label), "run-reference")
    metadata = strict(stream.readline(CAP + 1))
    need(isinstance(metadata, dict) and set(metadata) == {"run.json", *ROLES}, "evidence-roles")
    for ref in metadata.values():
        need(isinstance(ref, dict) and set(ref) == {"sha256", "bytes"} and
             isinstance(ref["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", ref["sha256"]) and
             type(ref["bytes"]) is int and 0 < ref["bytes"] <= CAP, "evidence-reference")
    base = Directory(root)
    if expected_root is not None:
        need(root_state(base, root) == expected_root, "application-support-changed")
    with contextlib.ExitStack() as held:
        held.callback(base.close)
        checks, fd = [base.check], base.fd
        # These names are MG-owned, never taken from original JSON paths.
        for name in ("Meguri", "CloudKitSigned", parent):
            if copy:
                try:
                    os.mkdir(name, 0o700, dir_fd=fd)
                except FileExistsError:
                    pass
            fd, check = private_child(fd, name, held)
            checks.append(check)
        parent_fd = fd

        def check_all():
            for check in checks:
                check()

        existing = label in os.listdir(parent_fd)
        if existing:
            run, check = private_child(parent_fd, label, held)
            checks.append(check)
            need(set(os.listdir(run)) == ({"run.json"} if copy else {"run.json", "product-evidence"}),
                 "run-not-prelaunch")
        else:
            need(copy, "run-missing")
            run = None
        # Retain an existing admitted header throughout staging/publication.
        header = None
        run_path = os.path.join(root, "Meguri/CloudKitSigned", parent, label)
        if existing:
            header = Original(os.path.join(run_path, "run.json"))
            held.callback(header.close)
            checks.append(header.check)
            need({k: header.reference[k] for k in ("sha256", "bytes")} == metadata["run.json"], "header-changed")

        if copy:
            stage_name = ".meguri-evidence-" + os.urandom(16).hex()
            os.mkdir(stage_name, 0o700, dir_fd=parent_fd)
            stage, stage_check = private_child(parent_fd, stage_name, held)
            checks.append(stage_check)
            os.mkdir("product-evidence", 0o700, dir_fd=stage)
            evidence, check = private_child(stage, "product-evidence", held)
            # This binding changes at publication; validate it before then.
            for name in ("run.json", *ROLES):
                remaining, chunks = metadata[name]["bytes"], []
                while remaining:
                    raw = stream.read(min(1024 * 1024, remaining))
                    need(raw, "evidence-truncated")
                    remaining -= len(raw)
                    chunks.append(raw)
                raw = b"".join(chunks)
                strict(raw)
                need(v.digest(raw) == metadata[name]["sha256"], "evidence-bytes")
                if name == "run.json" and header is not None:
                    need(raw == header.raw, "header-changed")
                else:
                    write_new(stage if name == "run.json" else evidence, name, raw)
            need(not stream.read(1), "evidence-trailing")
            # Read all staged bytes through no-follow file handles before any
            # role becomes visible. Their handles survive the rename.
            staged = []
            stage_path = os.path.join(os.path.dirname(run_path), stage_name)
            for name in ROLES:
                original = Original(os.path.join(stage_path, "product-evidence", name))
                held.callback(original.close)
                staged.append(original)
                need({k: original.reference[k] for k in ("sha256", "bytes")} == metadata[name], "evidence-readback")
            staged_header = header
            if header is None:
                staged_header = Original(os.path.join(stage_path, "run.json"))
                held.callback(staged_header.close)
                need({k: staged_header.reference[k] for k in ("sha256", "bytes")} == metadata["run.json"],
                     "header-changed")
            check_all()
            check()
            for original in staged:
                original.check()
            staged_header.check()
            os.fsync(evidence)
            os.fsync(stage)
            if run is None:
                # Publish header + all four roles as one previously absent run.
                publish_exclusive(parent_fd, stage_name, parent_fd, label)
                run = stage
                checks.pop()  # staging name is intentionally no longer bound
            else:
                publish_exclusive(stage, "product-evidence", run, "product-evidence")
            os.fsync(run)
            os.fsync(parent_fd)
            check_all()
            # Retained file descriptors still bind the published roles after
            # their staging pathname intentionally ceases to exist.
            for name, original in zip(ROLES, staged):
                need(file_pin(os.fstat(original.fd)) == original.pin and file_pin(os.stat(
                    name, dir_fd=evidence, follow_symlinks=False)) == original.pin, "file-changed")
            need(file_pin(os.stat("run.json", dir_fd=run, follow_symlinks=False)) == staged_header.pin,
                 "header-changed")
        else:
            need(not stream.read(1), "evidence-trailing")

        # A fresh, retained read of the published names supplies the readback.
        readback, pins = {}, {}
        authority = Directory(run_path)
        held.callback(authority.close)
        need(directory_pin(os.fstat(authority.fd)) == directory_pin(os.fstat(run)), "directory-changed")
        need(set(os.listdir(run)) == {"run.json", "product-evidence"}, "run-not-prelaunch")
        for name in ("run.json", *ROLES):
            path = os.path.join(run_path, name if name == "run.json" else "product-evidence/" + name)
            original = Original(path)
            held.callback(original.close)
            checks.append(original.check)
            need(not any(original.pin[:2] == tuple(pin[:2]) for pin in pins.values()), "aliased-role")
            readback[name] = {k: original.reference[k] for k in ("sha256", "bytes")}
            pins[name] = original.pin
        need(readback == metadata, "evidence-readback")
        evidence_dir = Directory(os.path.join(run_path, "product-evidence"))
        held.callback(evidence_dir.close)
        if copy:
            need(directory_pin(os.fstat(evidence_dir.fd)) == directory_pin(os.fstat(evidence)),
                 "directory-changed")
            need(all(tuple(pins[name]) == staged[index].pin for index, name in enumerate(ROLES)),
                 "file-changed")
        need(set(os.listdir(evidence_dir.fd)) == set(ROLES), "evidence-roles")
        evidence_dir.check()
        authority.check()
        check_all()
        return {"root": root_state(base, root), "files": readback, "pins": pins, "run": directory_pin(os.fstat(run)),
                "evidence": directory_pin(os.fstat(evidence_dir.fd))}


def root_state(directory, path):
    directory.check()
    return {"path": path, "chain": [list(pin) for _, _, _, pin in directory.bindings]}


def guest_support(container=None, root=None):
    # Mobile authority is the actual data result, not the installed app variant.
    # Create only the fixed OS support suffix below that observed container.
    with contextlib.ExitStack() as held:
        if container is not None:
            base = Directory(container)
            held.callback(base.close)
            checks, fd = [base.check], base.fd
            for name in ("Library", "Application Support"):
                try:
                    os.mkdir(name, 0o700, dir_fd=fd)
                except FileExistsError:
                    pass
                fd, check = private_child(fd, name, held)
                checks.append(check)
            root = container + "/Library/Application Support"
            for check in checks:
                check()
        directory = Directory(root)
        held.callback(directory.close)
        return root_state(directory, root)


def checked_raw(path):
    # Bounded setup input/attachment only, never account or run-header input.
    parent = Directory(os.path.dirname(path))
    try:
        fd = os.open(os.path.basename(path), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent.fd)
        try:
            info = os.fstat(fd)
            need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == os.getuid()
                 and not info.st_mode & 0o022 and 0 < info.st_size <= CAP, "setup-file")
            raw = bytearray()
            while len(raw) <= CAP:
                chunk = os.read(fd, min(1024 * 1024, CAP + 1 - len(raw)))
                if not chunk:
                    break
                raw.extend(chunk)
            need(len(raw) == info.st_size and file_pin(os.fstat(fd)) == file_pin(info) and
                 file_pin(os.stat(os.path.basename(path), dir_fd=parent.fd, follow_symlinks=False)) == file_pin(info),
                 "setup-file-changed")
            parent.check()
            return bytes(raw)
        finally:
            os.close(fd)
    finally:
        parent.close()


def setup_cancel(signal_number, frame):
    raise v.Refused("setup-cancelled")


@contextlib.contextmanager
def setup_launch():
    pending = []
    handlers = {number: signal.signal(number, lambda signal_number, frame: pending.append(signal_number))
                for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    try:
        yield
    finally:
        for number, handler in handlers.items():
            signal.signal(number, handler)
    need(not pending, "setup-cancelled")


def setup_join(child):
    # Only a child launched with start_new_session=True: never a process
    # lookup or a host-wide kill. Finish this join despite parent cancellation.
    signals = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
    handlers = {number: signal.signal(number, signal.SIG_IGN) for number in signals}
    try:
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pass
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait()
    finally:
        for number, handler in handlers.items():
            signal.signal(number, handler)


def setup_command(argv):
    # No shell, no ambient test-runner overrides, no diagnostic export. A lost
    # bounded child is joined on every exit, including SIGHUP/cancellation.
    environment = {k: val for k, val in os.environ.items() if not k.startswith(("TEST_RUNNER_", "MEGURI_CLOUD_AGENT_"))}
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        child = None
        try:
            with setup_launch():
                child = subprocess.Popen(argv, stdout=out, stderr=err, env=environment, start_new_session=True)
            child.wait(timeout=180)
        except subprocess.TimeoutExpired:
            raise v.Refused("setup-timeout") from None
        finally:
            if child is not None:
                setup_join(child)
        need(child.returncode == 0 and out.tell() + err.tell() <= CAP, "setup-command")
        out.seek(0)
        return out.read(CAP + 1)


def guest_preflight(manifest):
    products = manifest["products"]["path"]
    xpath = os.path.join(products, manifest["xctestrun"]["path"])
    raw = checked_raw(xpath)
    need(len(raw) == manifest["xctestrun"]["bytes"] and v.digest(raw) == manifest["xctestrun"]["sha256"], "setup-xctestrun")
    plan = plistlib.loads(raw)
    targets = plan["TestConfigurations"][0]["TestTargets"]
    need(len(targets) == 1 and targets[0]["BlueprintName"] == "MeguriSnapshotTests" and
         targets[0]["OnlyTestIdentifiers"] == ["CloudKitSoakTests", "CloudKitPathPreflightTests"], "setup-selection")
    # Private derived selection, from this exact verified original. Frozen build
    # inputs remain unchanged. No newest-xctestrun search or normal fixture route.
    plan = copy.deepcopy(plan)
    target = plan["TestConfigurations"][0]["TestTargets"][0]
    target["OnlyTestIdentifiers"] = ["CloudKitPathPreflightTests/testObserveOSRootAndLoadedImages"]
    env = target.setdefault("EnvironmentVariables", {})
    need(not any(k.startswith(("MEGURI_CLOUD_AGENT_", "TEST_RUNNER_")) for k in env), "setup-environment")
    env["MEGURI_CLOUD_AGENT_PATH_PREFLIGHT"] = "1"
    # Preserve __TESTROOT__ expansion from the original xctestrun even though
    # the setup-only selection lives in a new MG-owned private temp directory.
    def expand(value):
        if isinstance(value, str):
            return value.replace("__TESTROOT__", os.path.dirname(xpath))
        if isinstance(value, list):
            return [expand(item) for item in value]
        if isinstance(value, dict):
            return {key: expand(item) for key, item in value.items()}
        return value
    plan = expand(plan)
    temporary = os.path.realpath(tempfile.gettempdir())  # trusted OS temp root only
    stage = tempfile.mkdtemp(prefix="meguri-cloud-path-", dir=temporary)
    os.chmod(stage, 0o700)
    authority = Directory(stage)
    try:
        write_new(authority.fd, "setup.xctestrun", plistlib.dumps(plan))
        result = stage + "/setup.xcresult"
        method = "CloudKitPathPreflightTests/testObserveOSRootAndLoadedImages()"
        setup_command(["/usr/bin/xcodebuild", "test-without-building", "-xctestrun", stage + "/setup.xctestrun",
                       "-destination", "platform=macOS", "-only-testing:MeguriSnapshotTests/" + method.removesuffix("()"),
                       "-parallel-testing-enabled", "NO", "-collect-test-diagnostics", "never", "-resultBundlePath", result])
        summary = strict(setup_command(["/usr/bin/xcrun", "xcresulttool", "get", "test-results", "summary", "--path", result]))
        need(summary.get("passedTests") == 1 and summary.get("failedTests") == 0 and
             summary.get("skippedTests") == 0, "setup-result")
        output = stage + "/attachments"
        setup_command(["/usr/bin/xcrun", "xcresulttool", "export", "attachments", "--path", result,
                       "--output-path", output, "--test-id", method, "--filter", "cloud-agent-path-preflight_0_*.json"])
        exported = strict(checked_raw(output + "/manifest.json"))
        need(isinstance(exported, list) and len(exported) == 1 and exported[0].get("testIdentifier") == method,
             "setup-attachment")
        attachments = exported[0].get("attachments")
        need(isinstance(attachments, list) and len(attachments) == 1, "setup-attachment")
        attachment = attachments[0]
        name = attachment.get("exportedFileName")
        uuid = r"[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}"
        need(isinstance(name, str) and re.fullmatch(uuid + r"\.json", name) and
             attachment.get("isAssociatedWithFailure") is False and re.fullmatch(
                 "cloud-agent-path-preflight_0_" + uuid + r"\.json", attachment.get("suggestedHumanReadableName", "")), "setup-attachment")
        observed = strict(checked_raw(output + "/" + name))
        need(set(os.listdir(output)) == {"manifest.json", name}, "setup-attachment")
        need(checked_raw(xpath) == raw, "setup-xctestrun-changed")
        authority.check()
        return observed
    finally:
        # Private evidence stays private; never recursively remove a pathname.
        authority.close()


def loaded_image_identity(path, wanted):
    # Compare actual dyld CPU/subtype/LC_UUID with the independently verified
    # original disk bytes. Full byte hashes remain covered by inventory checks.
    with v.authority(os.path.dirname(path)) as parent:
        fd = os.open(os.path.basename(path), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        try:
            before = os.fstat(fd)
            need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1, "setup-image")
            def read(offset, length):
                need(0 <= offset <= before.st_size and 0 <= length <= min(1048608, before.st_size - offset), "setup-image")
                raw = os.pread(fd, length, offset)
                need(len(raw) == length, "setup-image")
                return raw
            head = read(0, 32)
            magic = struct.unpack_from("<I", head)[0]
            start = 0
            limit = before.st_size
            if magic != 0xfeedfacf:
                need(magic in (0xbebafeca, 0xcafebabe, 0xbfbafeca, 0xcafebabf), "setup-image")
                endian = ">" if magic in (0xbebafeca, 0xbfbafeca) else "<"
                fat64 = magic in (0xbfbafeca, 0xcafebabf)
                count = struct.unpack_from(endian + "I", head, 4)[0]
                stride = 32 if fat64 else 20
                need(0 < count <= 64, "setup-image")
                table = read(8, count * stride)
                matches = []
                for n in range(count):
                    cpu, subtype = struct.unpack_from(endian + "II", table, n * stride)
                    offset, size = struct.unpack_from(endian + ("QQ" if fat64 else "II"), table, n * stride + 8)
                    need(offset >= 8 + count * stride and size >= 32 and offset + size <= before.st_size, "setup-image")
                    if (cpu, subtype) == (wanted["cpu"], wanted["subtype"]):
                        matches.append((offset, size))
                need(len(matches) == 1, "setup-image")
                start, limit = matches[0]
                head = read(start, 32)
            magic, cpu, subtype, _, commands, size = struct.unpack_from("<6I", head)
            need(magic == 0xfeedfacf and (cpu, subtype) == (wanted["cpu"], wanted["subtype"]) and
                 0 < commands <= 4096 and size <= min(1048576, limit - 32), "setup-image")
            data = read(start + 32, size)
            cursor, uuids = 0, []
            for _ in range(commands):
                need(cursor + 8 <= size, "setup-image")
                command, length = struct.unpack_from("<II", data, cursor)
                need(length >= 8 and length % 8 == 0 and cursor + length <= size, "setup-image")
                if command == 0x1b:
                    need(length == 24, "setup-image")
                    uuids.append(data[cursor + 8:cursor + 24].hex())
                cursor += length
            need(cursor == size and uuids == [wanted["uuid"]] and wanted["uuid"] != "0" * 32 and
                 file_pin(os.fstat(fd)) == file_pin(before) and file_pin(os.stat(
                     os.path.basename(path), dir_fd=parent, follow_symlinks=False)) == file_pin(before), "setup-image")
        finally:
            os.close(fd)


def admit_preflight(root, manifest, expected, observation):
    fields = {"protocol_version", "scope", "application_support", "application_support_chain", "host", "agent", "app_build", "os_build",
              "executable_path", "anchor_image_path", "main_image_path", "images"}
    need(isinstance(observation, dict) and set(observation) == fields and observation["protocol_version"] == 1 and
         observation["scope"] == "PATH-PREPARATION-ONLY" and observation["app_build"] == manifest["app_build"] and
         observation["os_build"] == expected["os_build"], "setup-observation")
    products, entries = manifest["products"]["path"], manifest["products"]["entries"]
    for role in ("host", "agent"):
        bundle = observation[role]
        source = manifest["app" if role == "host" else "agent"]
        path = os.path.join(products, source["path"])
        physical = os.path.join(products, v.link_terminal(entries, source["path"]))
        need(bundle == {"root": path, "canonical_root": physical, "bundle_id": source["bundle_id"],
                        "executable": path + "/" + source["executable"]}, "setup-bundle")
    def key(path):
        need(isinstance(path, str) and path.startswith(products + "/"), "setup-image-path")
        relative = v.relative(path[len(products) + 1:])
        need(relative.startswith(manifest["app"]["path"] + "/"), "setup-image-path")
        return v.link_terminal(entries, relative)
    main = key(observation["host"]["executable"])
    agent = key(observation["agent"]["executable"])
    need(all(key(observation[field]) == main for field in ("executable_path", "main_image_path")) and
         key(observation["anchor_image_path"]) == agent, "setup-loaded-binding")
    required = {main, agent}
    debug = observation["host"]["executable"] + ".debug.dylib"
    if debug[len(products) + 1:] in entries:
        required.add(key(debug))
    images = observation["images"]
    need(isinstance(images, list) and 0 < len(images) <= 128, "setup-images")
    loaded = set()
    for image in images:
        need(isinstance(image, dict) and set(image) == {"path", "cpu", "subtype", "uuid"} and
             all(type(image[k]) is int and 0 <= image[k] <= 0xffffffff for k in ("cpu", "subtype")) and
             isinstance(image["uuid"], str) and re.fullmatch(r"[0-9a-f]{32}", image["uuid"]), "setup-image")
        physical = key(image["path"])
        need(physical not in loaded and entries[physical]["type"] == "file", "setup-images")
        loaded.add(physical)
        path = os.path.join(products, physical)
        before = v.binary(products, path)
        need(before[0] == entries[physical], "setup-original-bytes")
        loaded_image_identity(path, image)
        need(v.binary(products, path) == before, "setup-original-bytes")
    need(required <= loaded, "setup-missing-image")
    support = observation["application_support"]
    need(isinstance(support, str) and support.startswith("/") and len(support) <= 4096 and
         os.path.normpath(support) == support and "\x00" not in support and
         all(part not in ("", ".", "..") for part in support.split("/")[1:]), "setup-support-root")
    return support


@contextlib.contextmanager
def frozen_layout(parent, sha, label, state):
    # Independent of public receipts and account input. Pin the first actual OS
    # root/product/VM observations; every later handoff must match, never move.
    with frozen_destination(parent, sha, label):
        path = os.path.join(os.path.expanduser("~/Library/Application Support"),
                            "Meguri/CloudKitSignedPrivate", parent, "product-directory-" + label + ".json")
        authority = Directory(os.path.dirname(path))
        try:
            if os.path.basename(path) not in os.listdir(authority.fd):
                write_new(authority.fd, os.path.basename(path), i.raw_json(state))
                os.fsync(authority.fd)
            original = Original(path)
            try:
                need(stat.S_IMODE(os.fstat(original.fd).st_mode) == 0o600 and original.value == state, "frozen-layout-changed")
                yield original.check
                authority.check()
                original.check()
            finally:
                original.close()
        finally:
            authority.close()


def guest_source():
    definitions = (v.Refused, v.need, v.digest, v.identity, strict, directory_pin, file_pin,
                   Directory, Original, publish_exclusive, private_child, write_new, root_state, guest_support,
                   checked_raw, setup_cancel, setup_launch, setup_join, setup_command, guest_preflight, guest_evidence)
    preamble = ("import os,sys,stat,json,re,hashlib,contextlib,ctypes,copy,plistlib,signal,subprocess,tempfile\nfrom types import SimpleNamespace\n"
                "CAP=16*1024*1024\nROLES=" + repr(ROLES) + "\n")
    names = ("need", "digest", "identity", "MG_RUN_ID")
    return (preamble + "\n".join(inspect.getsource(item) for item in definitions) +
            "\nMG_RUN_ID=re.compile(" + repr(v.MG_RUN_ID.pattern) + ")\n" +
            "v=SimpleNamespace(**{k:globals()[k] for k in " + repr(names) + "}, Refused=Refused)\n" +
            "for number in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP): signal.signal(number,setup_cancel)\n" +
            "try:\n"
            " if sys.argv[1] in ('copy','read'):\n"
            "  result=guest_evidence(*sys.argv[2:5],sys.stdin.buffer,sys.argv[1]=='copy',strict(sys.argv[5]) if len(sys.argv)>5 else None)\n"
            " elif sys.argv[1]=='support':\n"
            "  result=guest_support(**strict(sys.stdin.buffer.read(CAP+1)))\n"
            " elif sys.argv[1]=='preflight':\n"
            "  result=guest_preflight(strict(sys.stdin.buffer.read(CAP+1)))\n"
            " else: raise Refused('guest-action')\n"
            " sys.stdout.write(json.dumps(result,sort_keys=True))\nexcept Exception:\n sys.exit(1)\n").encode()


class VM(i.VM):
    def __init__(self, root, selection, lease_check):
        super().__init__(root, selection)
        self.lease_check = lease_check

    def call(self, operation, *args, data=None, stream=None):
        self.lease_check()
        # The delivery bridge is unchanged. Retain and join its original
        # session, including SSH, before MG's leased VM drain can begin. run()
        # on timeout would kill only the bridge shell and could leave SSH able
        # to submit a late launch after the drain.
        env = dict(os.environ, MG_ROOT=self.root, CLOUD_AGENT_VM=json.dumps(self.selection))
        with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
            child = None
            try:
                with setup_launch():
                    child = subprocess.Popen(["/bin/bash", "-c", i.BRIDGE, "cloud-agent-delivery",
                                              os.path.join(self.root, "tools"), operation, *args],
                                             stdin=subprocess.PIPE if data is not None else stream,
                                             stdout=out, stderr=err, env=env, start_new_session=True)
                child.communicate(input=data, timeout=600)
            except (OSError, subprocess.SubprocessError):
                raise i.Queued("vm-unavailable") from None
            finally:
                if child is not None:
                    setup_join(child)
            need(out.tell() + err.tell() <= CAP, "observation-size")
            out.seek(0)
            err.seek(0)
            result, diagnostic = out.read(CAP + 1), err.read(CAP + 1)
            if child.returncode in (75, 255) or (child.returncode and re.search(
                    rb"exec request failed|fork(?: failed)?: (?:retry: )?resource temporarily unavailable",
                    diagnostic, re.I)):
                raise i.Queued("vm-unavailable")
            need(child.returncode == 0, "vm-" + operation)
        self.lease_check()
        return result

    def setup(self, action, value):
        need(action in ("support", "preflight"), "setup-action")
        # This supervisor is part of the test run, including preparation before
        # xcodebuild exists. Give it the existing mvl_stop_tests process tag so
        # the checked drain stops it too, preventing a delayed original launch.
        tag = ("xcodebuild test-without-building",) if action == "preflight" else ()
        cmd = " ".join(shlex.quote(word) for word in ("/usr/bin/python3", "-B", "-I", "-c", guest_source().decode(), action, *tag))
        return strict(self.call("copy", cmd, data=i.raw_json(value)))

    def evidence(self, root, parent, label, header, files, copy, expected_root=None):
        with tempfile.TemporaryFile(dir="/tmp") as payload:
            originals = {"run.json": header, **files}
            metadata = {name: {k: original.reference[k] for k in ("sha256", "bytes")}
                        for name, original in originals.items()}
            payload.write(i.raw_json(metadata))
            if copy:
                for name in ("run.json", *ROLES):
                    originals[name].check()
                    payload.write(originals[name].raw)
            payload.seek(0)
            cmd = " ".join(shlex.quote(word) for word in ("/usr/bin/python3", "-B", "-I", "-c",
                           guest_source().decode(), "copy" if copy else "read", root, parent, label, json.dumps(expected_root)))
            result = strict(self.call("copy", cmd, stream=payload))
            for original in originals.values():
                original.check()
            need(isinstance(result, dict) and set(result) == {"root", "files", "pins", "run", "evidence"}
                 and result["files"] == metadata and result["root"] == expected_root, "evidence-readback")
            return result


def handoff_product_evidence(root, parent_run, install_run, vm, lease_check):
    """Internal producer API. MG retains its selected VM lease throughout.

    This stage prepares paths and delivers originals only. It does not select
    accounts, deliver fixtures, launch the smoke or grant signed acceptance.
    """
    with inputs(root, parent_run, install_run) as (expected, header, files, args, check):
        with frozen_destination(parent_run, expected["source_commit"], expected["device"]) as (destination, destination_check):
            lease_check()
            observed = i.machine(vm)
            need(destination["vm"] == vm.selection["name"] and destination["vm_instance"] == observed["vm_instance"]
                 and destination["device_kind"] == expected["device_kind"], "destination-frozen-selected")
            simulator = destination["simulator"]
            lease_run = args.get("--lease-run")
            if simulator is not None:
                need(lease_run == simulator["lease_receipt"]["run_id"], "destination-lease")
            lease = i.lease_input(root, expected, lease_run)
            need((None if lease is None else {"udid": lease["udid"], "lease_receipt": lease["lease_receipt"]})
                 == simulator, "destination-lease")
            manifest = files["agent-build.json"].value
            products = manifest["products"]["path"]
            boundary = os.path.join(root, "build")
            admitted_host = None

            def recheck():
                nonlocal admitted_host
                lease_check()
                destination_check()
                check()
                need(i.machine(vm) == observed, "vm-instance-changed")
                if lease is None:
                    need(observed["os_build"] == expected["os_build"], "destination-os-build")
                else:
                    i.live_simulator(vm, root, expected, lease, observed)
                admitted = v.admit_build(root, os.path.join(root, "build/mg"), header.reference["path"], parent_run,
                                        observed["provisioning_udid"] if lease is None else None)
                need(admitted[0] == expected and admitted[2] == manifest, "product-changed")
                if admitted_host is None:
                    admitted_host = admitted[3]
                need(admitted[3] == admitted_host, "product-changed")
                lease_check()
                destination_check()

            def product_state(path, entries):
                found = vm.guest("inventory-pins", boundary if path == products else os.path.dirname(path), path)
                need(isinstance(found, dict) and set(found) == {"path", "entries", "pins"} and
                     found["path"] == path and found["entries"] == entries and
                     isinstance(found["pins"], dict) and set(found["pins"]) == {"", *entries}, "product-readback")
                return found

            def container_query(variant):
                raw = vm.call("simctl", "get_app_container", lease["udid"], manifest["app"]["bundle_id"], variant)
                path = raw.decode().removesuffix("\n")
                need(path.startswith("/") and len(path) <= 4096 and "\x00" not in path and
                     all(part not in ("", ".", "..") for part in path.split("/")[1:]), "container-observation")
                return path

            recheck()
            delivered = product_state(products, manifest["products"]["entries"])
            installed_path = data_container = installed = None
            if lease is None:
                # An older exact product may lack the new setup entrypoint. This
                # is absent code, not a new account/user-setup requirement.
                xpath = os.path.join(products, manifest["xctestrun"]["path"])
                plan = plistlib.loads(v._first_sync_bytes(products, xpath))
                selected = plan["TestConfigurations"][0]["TestTargets"][0].get("OnlyTestIdentifiers")
                if selected != ["CloudKitSoakTests", "CloudKitPathPreflightTests"]:
                    return {"scope": "PRODUCT-EVIDENCE-ONLY", "outcome": "implementation_missing",
                            "category": "mac-path-preflight-entrypoint", "signed_execution": False}
                setup = vm.setup("preflight", manifest)
                support = admit_preflight(root, manifest, expected, setup)
                support_state = vm.setup("support", {"root": support})
                need(support_state.get("chain") == setup["application_support_chain"], "setup-support-authority")
            else:
                installed_path = container_query("app")
                prefix = manifest["app"]["path"] + "/"
                entries = {name[len(prefix):]: entry for name, entry in manifest["products"]["entries"].items()
                           if name.startswith(prefix)}
                installed = product_state(installed_path, entries)
                data_container = container_query("data")
                need(data_container != installed_path and not data_container.startswith(installed_path + "/"), "data-container-observation")
                support = data_container + "/Library/Application Support"
                support_state = vm.setup("support", {"container": data_container})
            need(isinstance(support_state, dict) and set(support_state) == {"path", "chain"} and
                 support_state["path"] == support and isinstance(support_state["chain"], list) and
                 len(support_state["chain"]) == len(support.split("/")) - 1 and
                 all(isinstance(pin, list) and len(pin) == 5 and all(type(n) is int for n in pin)
                     for pin in support_state["chain"]), "application-support-observation")
            frozen = {"protocol_version": 1, "header_sha256": header.reference["sha256"],
                      "machine": observed, "simulator": simulator, "support": support_state,
                      "products": delivered, "installed": installed, "data_container": data_container}
            recheck()
            with frozen_layout(parent_run, expected["source_commit"], expected["device"], frozen) as layout_check:
                def recheck_layout():
                    layout_check()
                    recheck()
                    need(product_state(products, delivered["entries"]) == delivered, "product-changed")
                    if lease is not None:
                        need(container_query("app") == installed_path and container_query("data") == data_container,
                             "container-changed")
                        need(product_state(installed_path, installed["entries"]) == installed, "installed-product-changed")
                    need(vm.setup("support", {"root": support}) == support_state, "application-support-changed")
                    layout_check()
                    recheck()

                recheck_layout()
                copied = vm.evidence(support, parent_run, expected["device"], header, files, True, support_state)
                recheck_layout()
                need(vm.evidence(support, parent_run, expected["device"], header, files, False, support_state) == copied,
                     "evidence-readback-changed")
                recheck_layout()
    return {"scope": "PRODUCT-EVIDENCE-ONLY", "outcome": "product_evidence_delivered", "signed_execution": False}


def main():
    try:
        parser = i.Parser(description=__doc__)
        parser.add_argument("--root", required=True)
        parser.add_argument("--parent-run", required=True)
        parser.add_argument("--install-run", required=True)
        parser.add_argument("--select", action="store_true")
        args = parser.parse_args()
        with os.fdopen(os.dup(3), "rb") as private:
            context = strict(private.read(CAP + 1))
        if args.select:
            print(json.dumps(select(args.root, args.parent_run, args.install_run, context)))
            return 0
        need(isinstance(context, dict) and set(context) == {"selection", "lease_slot"}, "private-context")
        holder = Original(os.path.join(context["lease_slot"], "holder.json"))
        try:
            need(holder.value.get("worktree") == args.root and holder.value.get("run_id") == args.parent_run,
                 "vm-lease-owner")
            need(holder.value.get("job") == "cloudkit-signed", "vm-lease-owner")
            need(type(holder.value.get("pid")) is int and holder.value["pid"] > 1, "vm-lease-owner")

            def lease_check():
                holder.check()
                os.kill(holder.value["pid"], 0)
                # Use the same process ancestry authority as mvl_lease_held;
                # merely finding a live, unrelated holder is insufficient.
                raw = v.command(["/bin/ps", "-A", "-o", "pid=", "-o", "ppid="], "vm-lease-owner").stdout
                parents = dict(tuple(map(int, line.split())) for line in raw.splitlines())
                pid = os.getpid()
                for _ in range(200):
                    if pid == holder.value["pid"]:
                        return
                    pid = parents.get(pid, 0)
                raise v.Refused("vm-lease-owner")

            report = handoff_product_evidence(args.root, args.parent_run, args.install_run,
                                             VM(args.root, context["selection"], lease_check), lease_check)
        finally:
            holder.close()
        print(json.dumps(report, sort_keys=True))
        return 78 if report["outcome"] == "implementation_missing" else 0
    except i.Queued:
        outcome, category, code = "queued", "vm-unavailable", 75
    except (v.Refused, v.Invalid, OSError, ValueError, TypeError, KeyError, IndexError,
            AttributeError, UnicodeError, RecursionError):
        outcome, category, code = "failed_evidence", "product-evidence-refused", 1
    # Never export private context, destination ids, routes, or raw diagnostics.
    print(json.dumps({"scope": "PRODUCT-EVIDENCE-ONLY", "outcome": outcome,
                      "category": category, "signed_execution": False}))
    return code


if __name__ == "__main__":
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(number, setup_cancel)
    sys.exit(main())
