#!/usr/bin/env python3
"""J2.1 single-destination delivery. No launch, accounts or generated data.

The MG shell holds the VM lease for the entire deliver phase. Descriptor 3 is
its private configured selection, never a route supplied by a run header.
Product evidence needs a separate coordinator handoff AFTER MG finalization.
"""

import argparse
import contextlib
import datetime
import hashlib
import importlib.util
import inspect
import json
import os
from pathlib import Path
import plistlib
import re
import shlex
import stat
import subprocess
import sys
import tempfile

_spec = importlib.util.spec_from_file_location(
    "cloud_agent_verify", Path(__file__).with_name("cloud-agent-verify.py"))
v = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = v
_spec.loader.exec_module(v)
need = v.need
CAP = 16 * 1024 * 1024


class Queued(Exception):
    pass


def strict(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            need(key not in result, "duplicate-key")
            result[key] = value
        return result
    need(len(raw) <= CAP, "observation-size")
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(v.Refused("json")))


def raw_json(value):
    raw = (json.dumps(value, sort_keys=True, allow_nan=False) + "\n").encode()
    need(len(raw) <= CAP, "observation-size")
    return raw


def create(boundary, path, raw):
    v.contained(boundary, path)
    with v.authority(os.path.dirname(path)) as parent:
        fd = os.open(os.path.basename(path), os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                     os.O_NOFOLLOW, 0o600, dir_fd=parent)
        with os.fdopen(fd, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
    return v.file_reference(boundary, path)


def freeze(root, directory, template, parent, child):
    need(v.MG_RUN_ID.fullmatch(parent) and v.MG_RUN_ID.fullmatch(child) and parent != child,
         "header-run")
    sha = v.source(root)
    value = v.json_input(os.path.join(root, "build"), template)
    need(isinstance(value, dict) and "deployment_receipt" not in value, "header-template")
    value["deployment_receipt"] = {"job": "cloudkit-agent-install", "run_id": child}
    # Validate through the original strict header reader before doing any VM work.
    path = os.path.join(directory, "run.json")
    create(os.path.join(root, "build"), path, raw_json(value))
    admitted = v.header(root, path, parent, sha)
    need(isinstance(admitted["device"], str) and
         re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,63}", admitted["device"]), "device-label")
    v.admit_build(root, os.path.join(root, "build/mg"), path, parent)
    return path


# Keep the configured MG SSH and simctl routes, including streaming copy stdin.
# This is a private subprocess; none of its diagnostics reaches MG log.txt.
BRIDGE = r'''
MG_TOOLS="$1"; shift
. "$MG_TOOLS/mg.d/mac-vm.sh"
MG_MAC_VM="$CLOUD_AGENT_VM"
MG_SIM_RUNNER=vm
case "$1" in
  copy)
    shift
    mg_mac_vm_ssh_in "$@"
    ;;
  simctl) shift; mg_simctl "$@" ;;
  ssh) shift; "$(jq -r .ssh <<<"$MG_MAC_VM")" "$@" ;;
  *) exit 1 ;;
esac
'''


class VM:
    def __init__(self, root, selection):
        self.root, self.selection = root, selection
        need(isinstance(selection, dict) and set(selection) == {"name", "tart", "ssh", "state"}
             and selection["state"] == "running", "vm-selection")

    def call(self, operation, *args, data=None, stream=None):
        env = dict(os.environ, MG_ROOT=self.root, CLOUD_AGENT_VM=json.dumps(self.selection))
        try:
            result = subprocess.run(["/bin/bash", "-c", BRIDGE, "cloud-agent-delivery",
                                     os.path.join(self.root, "tools"), operation, *args],
                                    input=data, stdin=stream, capture_output=True, env=env, timeout=600)
        except (OSError, subprocess.SubprocessError):
            raise Queued("vm-unavailable") from None
        need(len(result.stdout) + len(result.stderr) <= CAP, "observation-size")
        if result.returncode in (75, 255) or (result.returncode and re.search(
                rb"exec request failed|fork(?: failed)?: (?:retry: )?resource temporarily unavailable",
                result.stderr, re.I)):
            raise Queued("vm-unavailable")
        need(result.returncode == 0, "vm-" + operation)
        return result.stdout

    def guest(self, action, *args):
        cmd = " ".join(shlex.quote(word) for word in
                       ("/usr/bin/python3", "-B", "-I", "-", action, *args))
        return strict(self.call("ssh", cmd, data=guest_source()))

    def copy(self, root, products, entries):
        # Pin the actual host tree until the configured MG SSH copy completes.
        witness = {}
        with v.authority(products, witness) as source:
            with tempfile.TemporaryFile(dir="/tmp") as payload:
                payload.write(raw_json(entries))
                send_products(source, entries, payload)
                witness["check"]()
                payload.seek(0)
                cmd = " ".join(shlex.quote(word) for word in
                               ("/usr/bin/python3", "-B", "-I", "-c",
                                guest_source().decode(), "copy", root, products))
                self.call("copy", cmd, stream=payload)
                witness["check"]()


@contextlib.contextmanager
def transfer_parent(root, path):
    """Resolve a relative entry only below an already held physical tree."""
    v.relative(path)
    held, bindings, fd = [], [], root
    try:
        for part in path.split("/")[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            held.append(child)
            bindings.append((fd, part, child, v.directory_identity(os.fstat(child))))
            fd = child
        yield fd, path.split("/")[-1]
        for parent, name, child, pin in bindings:
            need(v.directory_identity(os.fstat(child)) == pin and
                 v.directory_identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
                 "copy-authority")
    finally:
        for child in reversed(held):
            os.close(child)


def send_products(source, entries, output):
    v.check_entries(entries)
    budget = v.PRODUCT_BYTES
    for path, entry in sorted(entries.items()):
        with transfer_parent(source, path) as (parent, name):
            info = os.stat(name, dir_fd=parent, follow_symlinks=False)
            pin = v.identity(info)
            if entry["type"] == "directory":
                need(stat.S_ISDIR(info.st_mode) and stat.S_IMODE(info.st_mode) == entry["mode"],
                     "copy-source")
            elif entry["type"] == "symlink":
                v.link_terminal(entries, path)
                need(stat.S_ISLNK(info.st_mode) and os.readlink(name, dir_fd=parent) == entry["target"],
                     "copy-source")
            else:
                fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
                try:
                    need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                         v.identity(os.fstat(fd)) == pin and info.st_size == entry["bytes"] and
                         stat.S_IMODE(info.st_mode) == entry["mode"], "copy-source")
                    hashed, count = hashlib.sha256(), 0
                    while True:
                        raw = os.read(fd, min(v.CHUNK_BYTES, budget + 1))
                        if not raw:
                            break
                        budget -= len(raw)
                        need(budget >= 0, "product-size")
                        count += len(raw)
                        hashed.update(raw)
                        output.write(raw)
                    need(count == entry["bytes"] and hashed.hexdigest() == entry["sha256"] and
                         v.identity(os.fstat(fd)) == pin, "copy-source")
                finally:
                    os.close(fd)
            need(v.identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
                 "copy-source")


def receive_products(root, path, stream):
    """Stage and publish through held guest descriptors; never pathname-delete.

    Retain any prior product inside this run's private staging directory. No
    cleanup recursively follows a path that another process could substitute.
    """
    v.contained(os.path.join(root, "build"), path)
    entries = strict(stream.readline(CAP + 1))
    v.check_entries(entries)
    witness = {}
    with v.authority(root, witness) as base, contextlib.ExitStack() as held:
        parent, bindings = base, []
        for part in os.path.relpath(os.path.dirname(path), root).split("/"):
            try:
                os.mkdir(part, 0o755, dir_fd=parent)
            except FileExistsError:
                pass
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
            held.callback(os.close, child)
            info = os.fstat(child)
            need(info.st_uid == os.getuid(), "copy-authority")
            bindings.append((parent, part, child, v.directory_identity(info)))
            parent = child

        def check():
            witness["check"]()
            for fd, name, child, pin in bindings:
                need(v.directory_identity(os.fstat(child)) == pin and
                     v.directory_identity(os.stat(name, dir_fd=fd, follow_symlinks=False)) == pin,
                     "copy-authority")

        stage_name = ".meguri-delivery-" + os.urandom(16).hex()
        os.mkdir(stage_name, 0o700, dir_fd=parent)
        stage = os.open(stage_name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        held.callback(os.close, stage)
        bindings.append((parent, stage_name, stage, v.directory_identity(os.fstat(stage))))
        os.mkdir("new", 0o700, dir_fd=stage)
        new = os.open("new", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=stage)
        held.callback(os.close, new)
        budget = v.PRODUCT_BYTES
        for name, entry in sorted(entries.items()):
            with transfer_parent(new, name) as (fd, leaf):
                if entry["type"] == "directory":
                    os.mkdir(leaf, 0o700, dir_fd=fd)
                elif entry["type"] == "symlink":
                    v.link_terminal(entries, name)
                    # Links are installed last, after all physical writes.
                else:
                    budget -= entry["bytes"]
                    need(budget >= 0, "product-size")
                    output = os.open(leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                                     os.O_NOFOLLOW, 0o600, dir_fd=fd)
                    with os.fdopen(output, "wb") as target:
                        hashed, remaining = hashlib.sha256(), entry["bytes"]
                        while remaining:
                            raw = stream.read(min(v.CHUNK_BYTES, remaining))
                            need(raw, "copy-truncated")
                            remaining -= len(raw)
                            hashed.update(raw)
                            target.write(raw)
                        need(hashed.hexdigest() == entry["sha256"], "copy-source")
                        target.flush()
                        os.fchmod(target.fileno(), entry["mode"])
                        os.fsync(target.fileno())
        need(not stream.read(1), "copy-trailing")
        for name, entry in sorted(entries.items(), reverse=True):
            with transfer_parent(new, name) as (fd, leaf):
                if entry["type"] == "symlink":
                    os.symlink(entry["target"], leaf, dir_fd=fd)
                elif entry["type"] == "directory":
                    child = os.open(leaf, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                    try:
                        os.fchmod(child, entry["mode"])
                    finally:
                        os.close(child)
        # Refuse ancestor substitution before touching the destination entry.
        check()
        need(v.directory_identity(os.stat("new", dir_fd=stage, follow_symlinks=False)) ==
             v.directory_identity(os.fstat(new)), "copy-authority")
        name = os.path.basename(path)
        try:
            previous = os.stat(name, dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            previous = None
        if previous is not None:
            need(stat.S_ISDIR(previous.st_mode), "copy-authority")
            os.rename(name, "previous", src_dir_fd=parent, dst_dir_fd=stage)
            need(v.directory_identity(os.stat("previous", dir_fd=stage, follow_symlinks=False)) ==
                 v.directory_identity(previous), "copy-authority")
        check()
        os.rename("new", name, src_dir_fd=stage, dst_dir_fd=parent)
        need(v.directory_identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) ==
             v.directory_identity(os.fstat(new)), "copy-authority")
        check()


def guest_source():
    """Reuse the existing no-follow inventory verbatim in the actual guest.

    Supply local source directly; copy reserves SSH stdin for product bytes.
    Never load code or routing named by input JSON.
    No verifier dependencies or private account reader are installed in the VM.
    """
    definitions = (v.Refused, v.need, v.digest, v.contained, v.identity,
                   v.directory_identity, v.authority, v.stream_file, v.relative,
                   v.link_terminal, v.snapshot, v.check_reference, v.check_entries,
                   strict, transfer_parent, receive_products)
    preamble = ("import os,sys,stat,json,re,hashlib,posixpath,contextlib,plistlib,subprocess\n"
                "from types import SimpleNamespace\n"
                "PRODUCT_BYTES=16*1024*1024*1024\nPRODUCT_ENTRIES=32768\n"
                "CHUNK_BYTES=1024*1024\nCAP=16*1024*1024\n")
    namespace = [item.__name__ for item in definitions] + ["PRODUCT_BYTES", "CHUNK_BYTES"]
    return (preamble + "\n".join(inspect.getsource(item) for item in definitions) +
            "\nv=SimpleNamespace(**{k:globals()[k] for k in " + repr(namespace) + "})\n" + GUEST).encode()


GUEST = r'''
def command(words):
    p = subprocess.run(words, capture_output=True, timeout=60)
    need(p.returncode == 0 and len(p.stdout)+len(p.stderr) <= CAP, "guest-command")
    return p.stdout

def read_json(path):
    with authority(os.path.dirname(path)) as parent:
        name = os.path.basename(path)
        before = stream_file(parent, name, [CAP])
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=parent)
        try:
            info = os.fstat(fd)
            need(info.st_uid == os.getuid() and identity(info) == before[1], "lease-authority")
            raw = os.read(fd, CAP+1)
            need(identity(os.fstat(fd)) == before[1], "lease-authority")
        finally:
            os.close(fd)
        need(stream_file(parent, name, [CAP]) == before, "lease-authority")
        return strict(raw)

action = sys.argv[1]
try:
    if action == "machine":
        io = plistlib.loads(command(["/usr/sbin/ioreg", "-rd1", "-c", "IOPlatformExpertDevice", "-a"]))
        ids = [entry.get("IOPlatformUUID") for entry in io]
        need(len(ids) == 1 and isinstance(ids[0], str) and
             re.fullmatch(r"[0-9A-Fa-f-]{36}", ids[0]), "machine-instance")
        boot = command(["/usr/sbin/sysctl", "-n", "kern.boottime"]).decode()
        match = re.search(r"sec = ([0-9]+),", boot)
        need(match is not None, "machine-boot")
        xcode = subprocess.run(["/usr/bin/xcodebuild", "-version"], capture_output=True, timeout=60)
        need(len(xcode.stdout)+len(xcode.stderr) <= CAP, "guest-command")
        hardware = strict(command(["/usr/sbin/system_profiler", "SPHardwareDataType", "-json"]))
        hardware = hardware["SPHardwareDataType"][0]
        provisioning = hardware.get("provisioning_UDID")
        if provisioning is None:
            provisioning = hardware.get("platform_UUID")
        need(isinstance(provisioning, str) and provisioning and
             len(provisioning) <= 256 and not re.search(r"[\s\x00]", provisioning), "machine-provisioning")
        result = {"vm_instance": ids[0], "boot_epoch": int(match.group(1)),
                  "provisioning_udid": provisioning,
                  "os_build": command(["/usr/bin/sw_vers", "-buildVersion"]).decode().strip(),
                  "xcode": xcode.stdout.decode().strip() if xcode.returncode == 0 else ""}
    elif action == "copy":
        receive_products(*sys.argv[2:], sys.stdin.buffer)
        result = {"copied": True}
    elif action in ("inventory", "inventory-pins"):
        boundary, path = sys.argv[2:]
        entries, pins = snapshot(boundary, path)
        result = {"path": path, "entries": entries}
        if action == "inventory-pins":
            result["pins"] = pins
    elif action == "lease":
        udid = sys.argv[2]
        need(re.fullmatch(r"[0-9A-Fa-f-]{36}", udid), "lease-target")
        base = os.path.expanduser("~/Library/Caches/meguri/sim-pool/leases")
        result = read_json(os.path.join(base, udid, "lease.json"))
    else:
        raise Refused("guest-action")
    raw = (json.dumps(result, sort_keys=True, allow_nan=False)+"\n").encode()
    need(len(raw) <= CAP, "observation-size")
    sys.stdout.buffer.write(raw)
except Exception:
    # No paths, identifiers or raw command output on failure.
    sys.exit(1)
'''


def machine(vm):
    found = vm.guest("machine")
    need(isinstance(found, dict) and set(found) ==
         {"vm_instance", "provisioning_udid", "boot_epoch", "os_build", "xcode"}
         and isinstance(found["vm_instance"], str) and
         re.fullmatch(r"[0-9A-Fa-f-]{36}", found["vm_instance"]) and
         isinstance(found["provisioning_udid"], str) and 0 < len(found["provisioning_udid"]) <= 256 and
         not re.search(r"[\s\x00]", found["provisioning_udid"]) and
         type(found["boot_epoch"]) is int and found["boot_epoch"] > 0 and
         isinstance(found["os_build"], str) and re.fullmatch(r"[A-Za-z0-9]+", found["os_build"]),
         "machine-observation")
    try:
        host = v.command(["/usr/bin/xcodebuild", "-version"], "host-xcode").stdout.decode().strip()
    except v.Refused:
        raise Queued("xcode-unavailable") from None
    if found["xcode"] != host or not re.match(r"Xcode 27\.[1-9][0-9]*(?:\n|\.)", host):
        raise Queued("xcode-mismatch")
    return found


def lease_input(root, expected, run):
    kind = expected["device_kind"]
    if kind == "mac":
        need(run is None, "lease-unexpected")
        return None
    need(isinstance(run, str) and v.MG_RUN_ID.fullmatch(run), "lease-receipt")
    result, hashed = v.receipt(root, os.path.join(root, "build/mg"),
                               {"job": "sim", "run_id": run}, expected["source_commit"])
    args, detail = result["args"], result["details"]
    udid = detail.get("udid")
    need(isinstance(udid, str) and re.fullmatch(r"[0-9A-Fa-f-]{36}", udid), "lease-target")
    need(len(args) >= 2 and ((args[:2] == ["acquire", kind]) or
                            (args[:2] == ["renew", udid])), "lease-producer")
    options = iter(args[2:])
    seen = set()
    for key in options:
        need(key in ("--ttl-min", "--holder") and key not in seen and
             (key != "--holder" or args[0] == "acquire"), "lease-producer")
        seen.add(key)
        need(isinstance(next(options, None), str), "lease-producer")
    need(detail.get("sim_runner") == "vm" and isinstance(detail.get("lease"), dict)
         and isinstance(detail.get("device"), dict), "lease-receipt")
    return {"udid": udid, "lease_receipt": {"job": "sim", "run_id": run},
            "result": result, "sha256": hashed}


def live_simulator(vm, root, expected, lease, observed, *, checkpoint=None):
    """Pool's exact ownership/kind/expiry + pinned live simctl checks, no acquire/reset."""
    checkpoint = (lambda stage: None) if checkpoint is None else checkpoint
    checkpoint("simulator-lease")
    udid, detail = lease["udid"], lease["result"]["details"]
    actual = vm.guest("lease", udid)
    need(actual == detail["lease"], "lease-changed")
    need(actual.get("udid") == udid and actual.get("worktree") == root and
         actual.get("kind") == expected["device_kind"] and
         actual.get("vm") == vm.selection["name"], "lease-owner-kind")
    now = datetime.datetime.now(datetime.timezone.utc).timestamp()
    need(type(actual.get("acquired_epoch")) is int and
         observed["boot_epoch"] <= actual["acquired_epoch"] <= now and
         type(actual.get("expires_epoch")) is int and actual["expires_epoch"] > now,
         "lease-expired-or-restarted")
    need(detail["device"].get("udid") == udid and
         detail["device"].get("kind") == expected["device_kind"], "lease-target")
    checkpoint("simulator-device")
    pins = v.json_input(root, os.path.join(root, "tools/sim-devices.json"))
    device_type = [d["device_type"] for d in pins["devices"] if d["kind"] == expected["device_kind"]]
    need(len(device_type) == 1, "simulator-kind")
    devices = strict(vm.call("simctl", "list", "devices", "--json"))
    found = [(runtime, device) for runtime, group in devices["devices"].items()
             for device in group if device.get("udid") == udid]
    need(len(found) == 1, "simulator-target")
    runtime, device = found[0]
    need(device.get("state") == "Booted" and device.get("isAvailable") is True and
         device.get("deviceTypeIdentifier") == device_type[0] and
         re.fullmatch(r"Meguri-Pool-" + expected["device_kind"] + r"-[0-9]+", device.get("name", "")),
         "simulator-kind-state")
    checkpoint("simulator-runtime")
    runtimes = strict(vm.call("simctl", "list", "runtimes", "--json"))
    selected = [r for r in runtimes["runtimes"] if r.get("identifier") == runtime]
    need(len(selected) == 1 and selected[0].get("isAvailable") is True and
         runtime == pins["runtime"]["identifier"] and
         selected[0].get("buildversion") == pins["runtime"]["build"] == expected["os_build"],
         "simulator-runtime")
    # CoreSimulator owns this booted runtime environment value. iOS runtimes
    # do not contain macOS's /usr/bin/sw_vers; inventory alone is insufficient.
    checkpoint("simulator-os")
    running = vm.call("simctl", "getenv", udid, "SIMULATOR_RUNTIME_BUILD_VERSION").decode().strip()
    need(running == expected["os_build"], "simulator-runtime")
    checkpoint("simulator-receipt")
    need(v.receipt(root, os.path.join(root, "build/mg"), lease["lease_receipt"],
                   expected["source_commit"])[1] == lease["sha256"], "lease-receipt-changed")


@contextlib.contextmanager
def destinations(parent, sha, label, destination, *, full_map=None):
    """Select one entry from a fully validated frozen map; never append/relabel.

    The selected coordinator freezes its full map before delivery.
    Hold private directory authority and the file witness across the operation.
    """
    support = os.path.expanduser("~/Library/Application Support")
    with v.authority(support) as base:
        info = os.fstat(base)
        need(info.st_uid == os.getuid() and not stat.S_IMODE(info.st_mode) & 0o022,
             "private-authority")
        held, fd = [], base
        try:
            bindings = []
            for name in ("Meguri", "CloudKitSignedPrivate", parent):
                try:
                    os.mkdir(name, 0o700, dir_fd=fd)
                except FileExistsError:
                    pass
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                held.append(child)
                info = os.fstat(child)
                need(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700,
                     "private-authority")
                bindings.append((fd, name, child, v.directory_identity(info)))
                fd = child
            value = {"protocol_version": 1, "job": "cloudkit-signed", "run_id": parent,
                     "source_commit": sha, "destinations": {label: destination} if full_map is None else full_map}
            validate_destinations(value, parent, sha)
            try:
                out = os.open("destinations.json", os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                              os.O_NOFOLLOW, 0o600, dir_fd=fd)
            except FileExistsError:
                pass
            else:
                with os.fdopen(out, "wb") as stream:
                    stream.write(raw_json(value))
                    stream.flush()
                    os.fsync(stream.fileno())

            def check():
                opened = os.open("destinations.json", os.O_RDONLY | os.O_NOFOLLOW |
                                 os.O_NONBLOCK, dir_fd=fd)
                try:
                    info = os.fstat(opened)
                    need(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and
                         stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1 and
                         info.st_size <= CAP, "private-authority")
                    raw = os.read(opened, CAP + 1)
                    pin = v.identity(info)
                    need(v.identity(os.fstat(opened)) == pin and
                         v.identity(os.stat("destinations.json", dir_fd=fd, follow_symlinks=False)) == pin,
                         "private-changed")
                    frozen = strict(raw)
                    validate_destinations(frozen, parent, sha)
                    need(frozen["destinations"].get(label) == destination, "destination-frozen-selected")
                    if full_map is not None:
                        need(frozen["destinations"] == full_map, "destination-frozen-map")
                    return pin, raw
                finally:
                    os.close(opened)
            before = check()
            def recheck():
                need(check() == before, "private-changed")
                for parent_fd, name, child, pin in bindings:
                    need(v.directory_identity(os.fstat(child)) == pin and
                         v.directory_identity(os.stat(name, dir_fd=parent_fd, follow_symlinks=False)) == pin,
                         "private-changed")
            yield recheck
            recheck()
            for parent_fd, name, child, pin in bindings:
                need(v.directory_identity(os.fstat(child)) == pin and
                     v.directory_identity(os.stat(name, dir_fd=parent_fd, follow_symlinks=False)) == pin,
                     "private-changed")
        finally:
            for child in reversed(held):
                os.close(child)



@contextlib.contextmanager
def freeze_destinations(parent, sha, entries):
    """Parent freezes the complete selected map before either delivery."""
    need(isinstance(entries, dict) and set(entries) == {"mac1", "phone1"} and
         entries["mac1"].get("device_kind") == "mac" and
         entries["phone1"].get("device_kind") == "iphone" and
         entries["mac1"].get("vm") == entries["phone1"].get("vm") and
         entries["mac1"].get("vm_instance") == entries["phone1"].get("vm_instance"), "destination-selected-map")
    # Copy before retaining: later caller mutation cannot redefine this map.
    original = strict(raw_json(entries))
    with destinations(parent, sha, "mac1", original["mac1"], full_map=original) as check:
        yield check


def validate_destinations(value, parent, sha):
    need(isinstance(parent, str) and v.MG_RUN_ID.fullmatch(parent) and
         isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha) and
         isinstance(value, dict) and set(value) ==
         {"protocol_version", "job", "run_id", "source_commit", "destinations"} and
         type(value["protocol_version"]) is int and value["protocol_version"] == 1 and
         value["job"] == "cloudkit-signed" and value["run_id"] == parent and
         value["source_commit"] == sha, "destination-envelope")
    entries = value["destinations"]
    need(isinstance(entries, dict) and entries, "destination-map")
    for label, entry in entries.items():
        need(isinstance(label, str) and re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,63}", label) and
             isinstance(entry, dict) and set(entry) == {"device_kind", "vm", "vm_instance", "simulator"},
             "destination-entry")
        need(entry["device_kind"] in ("mac", "iphone", "ipad") and
             isinstance(entry["vm"], str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", entry["vm"]) and
             isinstance(entry["vm_instance"], str) and
             re.fullmatch(r"[0-9A-Fa-f-]{36}", entry["vm_instance"]), "destination-entry")
        simulator = entry["simulator"]
        if entry["device_kind"] == "mac":
            need(simulator is None, "destination-entry")
        else:
            need(isinstance(simulator, dict) and set(simulator) == {"udid", "lease_receipt"} and
                 isinstance(simulator["udid"], str) and re.fullmatch(r"[0-9A-Fa-f-]{36}", simulator["udid"]),
                 "destination-entry")
            receipt = simulator["lease_receipt"]
            need(isinstance(receipt, dict) and set(receipt) == {"job", "run_id"} and
                 receipt["job"] == "sim" and isinstance(receipt["run_id"], str) and
                 v.MG_RUN_ID.fullmatch(receipt["run_id"]), "destination-entry")


# Fixed diagnostics only; never export exception text or private destination data.
DELIVERY_STAGES = {"entry", "source", "header", "vm-context", "vm-observation",
    "destination-observation", "destination-freeze", "recheck-vm", "recheck-build",
    "copy", "copy-readback", "simulator-install", "simulator-readback",
    "final-readback", "deployment", "source-recheck"}
DELIVERY_REFUSALS = {"unclassified", "filesystem-refused", "input-shape", "receipt-invalid",
    "process-refused", "source-dirty", "receipt-source", "receipt-outcome", "receipt-in-progress",
    "artifact-declaration", "device-label", "deployment-producer", "machine-observation",
    "destination-os-build", "destination-envelope", "destination-map", "destination-entry",
    "destination-frozen-selected", "destination-frozen-map", "private-authority", "private-changed",
    "vm-instance-changed", "vm-ssh", "vm-copy", "vm-simctl", "input-changed", "copy-readback",
    "copy-readback-changed", "copy-source", "product-size", "install-observation", "install-readback",
    "install-entries-missing", "install-entries-added", "install-entries-replaced",
    "install-entry-type", "install-entry-mode", "install-entry-content",
    "product-changed", "agent-binding", "agent-environment", "build-signature", "build-signing",
    "product-cloud-grants", "profile-team", "profile-identity", "profile-signer-platform",
    "profile-expired", "profile-device", "profile-development", "profile-grants", "source-changed"}


class InstalledModeDifference(v.Refused):
    def __init__(self, transitions):
        super().__init__("install-entry-mode")
        self.transitions = [{"type": kind, "before": before, "after": after, "count": count}
                            for (kind, before, after), count in sorted(transitions.items())[:32]]


class DeliveryDiagnostic:
    def __init__(self):
        self.stage = "entry"

    def enter(self, stage):
        need(stage in DELIVERY_STAGES, "delivery-diagnostic-stage")
        self.stage = stage

    def failure(self, error):
        code = "unclassified"
        if isinstance(error, InstalledModeDifference):
            code = "install-entry-mode"
        elif isinstance(error, v.Refused) and len(error.args) == 1 and type(error.args[0]) is str:
            if error.args[0] in DELIVERY_REFUSALS:
                code = error.args[0]
        elif isinstance(error, OSError):
            code = "filesystem-refused"
        elif isinstance(error, v.Invalid):
            code = "receipt-invalid"
        elif isinstance(error, (ValueError, TypeError, KeyError, IndexError, AttributeError, UnicodeError, RecursionError)):
            code = "input-shape"
        elif isinstance(error, subprocess.SubprocessError):
            code = "process-refused"
        result = {"stage": self.stage if self.stage in DELIVERY_STAGES else "entry", "refusal": code}
        if isinstance(error, InstalledModeDifference):
            result["mode_transitions"] = error.transitions
        return result


def require_installed_inventory(expected, installed):
    """Explain a failed exact comparison using fixed categories only."""
    if expected == installed:
        return
    need(isinstance(installed, dict), "install-readback")
    missing, added = expected.keys() - installed.keys(), installed.keys() - expected.keys()
    need(not (missing and added), "install-entries-replaced")
    need(not missing, "install-entries-missing")
    need(not added, "install-entries-added")
    transitions = {}
    for name, original in expected.items():
        actual = installed[name]
        need(isinstance(actual, dict), "install-readback")
        need(actual.get("type") == original["type"], "install-entry-type")
        need({k: v for k, v in actual.items() if k != "mode"} ==
             {k: v for k, v in original.items() if k != "mode"}, "install-entry-content")
        before, after = original.get("mode"), actual.get("mode")
        if before != after:
            kind = original["type"]
            need(kind in ("file", "directory") and type(after) is int and 0 <= after <= 0o7777,
                 "install-readback")
            key = (kind, before, after)
            transitions[key] = transitions.get(key, 0) + 1
    raise InstalledModeDifference(transitions)


def deliver(root, directory, parent, child, lease_run, vm, *, checkpoint=None):
    checkpoint = (lambda stage: None) if checkpoint is None else checkpoint
    checkpoint("source")
    boundary = os.path.join(root, "build")
    path = os.path.join(directory, "run.json")
    sha = v.source(root)
    checkpoint("header")
    expected = v.header(root, path, parent, sha)
    need(isinstance(expected["device"], str) and
         re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,63}", expected["device"]), "device-label")
    need(expected["deployment_receipt"] == {"job": "cloudkit-agent-install", "run_id": child},
         "deployment-producer")
    header_ref = v.file_reference(boundary, path)
    checkpoint("vm-observation")
    observed = machine(vm)
    checkpoint("destination-observation")
    lease = lease_input(root, expected, lease_run)
    if lease is None:
        need(expected["os_build"] == observed["os_build"], "destination-os-build")
    else:
        live_simulator(vm, root, expected, lease, observed)
    destination = {"device_kind": expected["device_kind"], "vm": vm.selection["name"],
                   "vm_instance": observed["vm_instance"], "simulator": None if lease is None else
                   {"udid": lease["udid"], "lease_receipt": lease["lease_receipt"]}}
    checkpoint("destination-freeze")
    with destinations(parent, sha, expected["device"], destination):
        def recheck():
            checkpoint("recheck-vm")
            need(machine(vm) == observed, "vm-instance-changed")
            if lease is not None:
                live_simulator(vm, root, expected, lease, observed)
            checkpoint("recheck-build")
            admitted = v.admit_build(root, os.path.join(root, "build/mg"), path, parent,
                                     observed["provisioning_udid"] if lease is None else None)
            need(v.file_reference(boundary, path) == header_ref and
                 admitted[0] == expected and v.source(root) == sha, "input-changed")
            return admitted

        _, build, manifest, frozen = recheck()
        build_ref = build["details"]["agent_build"]
        products = manifest["products"]["path"]
        # Hold host source and guest parent authority through MG SSH transport.
        checkpoint("copy")
        vm.copy(root, products, frozen[0])
        checkpoint("copy-readback")
        delivered = vm.guest("inventory", boundary, products)
        need(isinstance(delivered, dict) and set(delivered) == {"path", "entries"} and
             delivered["path"] == products and delivered["entries"] == frozen[0], "copy-readback")
        delivered_ref = create(boundary, os.path.join(directory, "delivered-products.json"), raw_json(delivered))
        copy_ref = create(boundary, os.path.join(directory, "copy-observation.json"), raw_json({
            "protocol_version": 1, "operation": "mg-vm-copy-and-nofollow-readback",
            "device": expected["device"], "command_exit": 0, "build_manifest": build_ref,
            "delivered_manifest": delivered_ref}))
        recheck()
        installation = None
        if lease is not None:
            product = os.path.join(products, manifest["app"]["path"])
            before = v.snapshot(boundary, product)
            checkpoint("simulator-install")
            vm.call("simctl", "install", lease["udid"], product)
            installed = vm.call("simctl", "get_app_container", lease["udid"],
                                manifest["app"]["bundle_id"], "app").decode().strip()
            # The container is an actual command observation, never header input.
            # It stays private; only relative installed inventory is exported.
            need(installed.startswith("/") and os.path.normpath(installed) == installed and
                 ".." not in installed.split("/") and "\x00" not in installed, "install-observation")
            checkpoint("simulator-readback")
            installed_proof = vm.guest("inventory", os.path.dirname(installed), installed)
            need(isinstance(installed_proof, dict) and set(installed_proof) == {"path", "entries"}
                 and installed_proof["path"] == installed,
                 "install-readback")
            require_installed_inventory(before[0], installed_proof["entries"])
            need(v.snapshot(boundary, product) == before, "product-changed")
            installation = create(boundary, os.path.join(directory, "installation.json"), raw_json({
                "protocol_version": 1, "operation": "mg-vm-simctl-install-and-nofollow-readback",
                "device": expected["device"], "command_exit": 0, "app": manifest["app"],
                "build_manifest": build_ref, "entries": installed_proof["entries"]}))
        recheck()
        # A second guest readback detects post-copy/install replacement as well.
        checkpoint("final-readback")
        need(vm.guest("inventory", boundary, products) == delivered, "copy-readback-changed")
        xpath = os.path.join(products, v.relative(manifest["xctestrun"]["path"]))
        xref = v.file_reference(products, xpath)
        need({**xref, "path": manifest["xctestrun"]["path"]} == manifest["xctestrun"], "agent-binding")
        deployment = {"protocol_version": 1, "job": "cloudkit-agent-install", "run_id": child,
                      "source_commit": sha, "parent_run": parent, "device": expected["device"],
                      "device_kind": expected["device_kind"], "os_build": expected["os_build"],
                      "header_sha256": header_ref["sha256"], "build_receipt": expected["build_receipt"],
                      "build_receipt_sha256": expected["build_receipt_sha256"], "build_manifest": build_ref,
                      "delivered_manifest": delivered_ref, "xctestrun": xref, "installation": installation}
        checkpoint("deployment")
        deployment_ref = create(boundary, os.path.join(directory, "agent-deployment.json"), raw_json(deployment))
        recheck()
    checkpoint("source-recheck")
    need(v.source(root) == sha, "source-changed")
    # This is handoff metadata, not an in-flight MG receipt or scenario result.
    return {"scope": "DELIVERY-ONLY", "outcome": "delivery_only", "signed_execution": False,
            "destination_scope": "single-device-frozen", "agent_deployment": deployment_ref,
            "product_evidence": "implementation_missing-finalized-receipt-handoff",
            "artifacts": [header_ref, deployment_ref, delivered_ref, copy_ref, build_ref, xref] +
                         ([] if installation is None else [installation])}


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise v.Refused("usage")


def main():
    try:
        parser = Parser(description=__doc__)
        parser.add_argument("--phase", choices=("prepare", "deliver"), required=True)
        parser.add_argument("--root", required=True)
        parser.add_argument("--run-directory", required=True)
        parser.add_argument("--run-id", required=True)
        parser.add_argument("--parent-run", required=True)
        parser.add_argument("--source-commit", required=True)
        parser.add_argument("--header-template")
        parser.add_argument("--lease-run")
        args = parser.parse_args()
        root = os.path.abspath(args.root)
        need(v.source(root) == args.source_commit, "receipt-source")
        directory = v.contained(os.path.join(root, "build/mg/cloudkit-agent-install"), args.run_directory)
        need(directory == os.path.join(root, "build/mg/cloudkit-agent-install", args.run_id) and
             v.MG_RUN_ID.fullmatch(args.run_id) and v.MG_RUN_ID.fullmatch(args.parent_run), "deployment-run")
        if args.phase == "prepare":
            need(args.header_template is not None, "usage")
            freeze(root, directory, args.header_template, args.parent_run, args.run_id)
            report = {"scope": "INPUT-VALIDATION", "outcome": "prepared", "signed_execution": False}
        else:
            need(args.header_template is None, "usage")
            with os.fdopen(os.dup(3), "rb") as private:
                selection = strict(private.read(CAP + 1))
            report = deliver(root, directory, args.parent_run, args.run_id, args.lease_run, VM(root, selection))
        print(json.dumps(report, sort_keys=True))
        return 0
    except Queued as error:
        category, outcome, code = error.args[0], "queued", 75
    except v.Refused as error:
        category, outcome, code = error.args[0], "failed_evidence", 1
    except v.Invalid:
        category, outcome, code = "guarded-input", "failed_evidence", 1
    except (OSError, ValueError, TypeError, KeyError, IndexError, AttributeError, UnicodeError, RecursionError):
        category, outcome, code = "invalid-input", "failed_evidence", 1
    print(json.dumps({"scope": "DELIVERY-ONLY", "outcome": outcome, "category": category,
                      "signed_execution": False}))
    return code


if __name__ == "__main__":
    sys.exit(main())
