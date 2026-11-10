#!/usr/bin/env python3
"""CloudAgent build-input admission; deployment and runtime remain unfinished.

All subprocess diagnostics and destination/account values stay private. A build
manifest is packaging evidence and cannot supply a signed scenario result.
"""

import argparse
import contextlib
import posixpath
import datetime
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).parent / "gates"))
from milestone_gate import (Invalid, MG_RUN_ID, _first_sync_bytes,
                            _first_sync_envelope, _first_sync_json, as_time)
sys.path.insert(0, str(Path(__file__).parent.parent / "tests"))
from cloud_build_configuration import check as check_cloud, protected


class Refused(Exception):
    """Its arguments are fixed public categories, never input values."""


def need(condition, category):
    if not condition:
        raise Refused(category)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def contained(boundary, path):
    need(isinstance(path, str) and os.path.isabs(path) and
         ".." not in path.split(os.sep) and "\x00" not in path, "artifact-path")
    need(os.path.normpath(path) == path and
         os.path.commonpath((os.path.abspath(boundary), path)) == os.path.abspath(boundary)
         and path != os.path.abspath(boundary), "artifact-path")
    return path


def command(argv, category):
    """Fixed executable paths; no shell, public overrides or diagnostic echo."""
    try:
        result = subprocess.run(argv, capture_output=True, timeout=60, check=False)
        need(result.returncode == 0 and
             len(result.stdout) + len(result.stderr) <= 16 * 1024 * 1024, category)
        return result
    except (OSError, subprocess.SubprocessError):
        raise Refused(category) from None


def source(root):
    sha = command(["/usr/bin/git", "-C", root, "rev-parse", "HEAD"], "source").stdout.decode().strip()
    need(re.fullmatch(r"[0-9a-f]{40}", sha), "source")
    need(not command(["/usr/bin/git", "-C", root, "status", "--porcelain=v1", "-z"],
                     "source").stdout, "source-dirty")
    return sha


def receipt(root, results, reference, sha):
    need(isinstance(reference, dict) and set(reference) == {"job", "run_id"}, "receipt-reference")
    job, run = reference["job"], reference["run_id"]
    need(isinstance(job, str) and re.fullmatch(r"[a-z][a-z0-9-]*", job) and
         isinstance(run, str) and MG_RUN_ID.fullmatch(run), "receipt-reference")
    witness = {}
    value = json_input(results, os.path.join(results, job, run, "result.json"), witness=witness)
    _first_sync_envelope(value)
    need(value["job"] == job and value["run_id"] == run, "receipt-reference")
    need(value["root"] == root and value["commit"] == {"sha": sha, "dirty": False}, "receipt-source")
    need(value["dry_run"] is False and value["status"] == "pass" and value["exit_code"] == 0
         and value["summary"]["failed"] == 0 and not value["failures"] and not value["user_steps"]
         and value["placement"] == "host" and value["service"] is None, "receipt-outcome")
    need(as_time(value["started_at"]) <= as_time(value["finished_at"]) <=
         datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None), "receipt-time")
    # Every declaration stays in build output, even declarations not consumed.
    paths = [contained(os.path.join(root, "build"), item["path"]) for item in value["artifacts"]]
    need(len(paths) == len(set(paths)), "artifact-declaration")
    return value, witness["sha256"]


def declared_file(root, value, path):
    need(sum(item["path"] == path for item in value["artifacts"]) == 1, "artifact-declaration")
    return _first_sync_bytes(os.path.join(root, "build"), path)


def header(root, path, run, sha, witness=None):
    value = json_input(os.path.join(root, "build"), path, witness=witness)
    fields = {"protocol_version", "job", "run_id", "lane", "source_commit", "tree_clean",
              "device", "device_kind", "os_build", "app_build", "executable_sha256",
              "build_receipt", "build_receipt_sha256", "deployment_receipt", "container",
              "environment", "account_scope", "account_labels", "selection", "fixture_delivery"}
    need(isinstance(value, dict) and set(value) == fields, "header-fields")
    need(type(value["protocol_version"]) is int and value["protocol_version"] == 1, "header-version")
    need(value["job"] == "cloudkit-signed" and value["run_id"] == run and
         value["lane"] == "cloud-signed", "header-run")
    need(value["source_commit"] == sha and value["tree_clean"] is True, "header-source")
    for key in ("executable_sha256", "build_receipt_sha256"):
        need(isinstance(value[key], str) and re.fullmatch(r"[0-9a-f]{64}", value[key]), "header-digest")
    need(value["environment"] == "development" and isinstance(value["container"], str) and
         re.fullmatch(r"iCloud\.[A-Za-z0-9.-]+", value["container"]), "wrong-container")
    need(value["device_kind"] in ("mac", "iphone", "ipad") and
         isinstance(value["app_build"], str) and value["app_build"], "header-product")
    need(isinstance(value["deployment_receipt"], dict) and
         set(value["deployment_receipt"]) == {"job", "run_id"} and
         value["deployment_receipt"]["job"] == "cloudkit-agent-install" and
         isinstance(value["deployment_receipt"]["run_id"], str) and
         MG_RUN_ID.fullmatch(value["deployment_receipt"]["run_id"]), "deployment-producer")
    return value


def plist(raw, category):
    try:
        value = plistlib.loads(raw)
        need(isinstance(value, dict), category)
        return value
    except (ValueError, TypeError, plistlib.InvalidFileException):
        raise Refused(category) from None


# JSON retains Q13.1's 16 MiB cap. Product bytes use bounded reads and a
# separate 16 GiB aggregate limit, including debug libraries and test bundles.
PRODUCT_BYTES = 16 * 1024 * 1024 * 1024
PRODUCT_ENTRIES = 32768
CHUNK_BYTES = 1024 * 1024


def identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def directory_identity(info):
    return (info.st_dev, info.st_ino, info.st_mode)


@contextlib.contextmanager
def authority(path, witness=None):
    """Hold every physical parent and check its binding before releasing it."""
    need(os.path.isabs(path) and os.path.normpath(path) == path and
         "\x00" not in path, "artifact-path")
    held, bindings = [], []
    try:
        fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        held.append(fd)
        for part in path.split("/")[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            held.append(child)
            pin = directory_identity(os.fstat(child))
            bindings.append((fd, part, child, pin))
            fd = child
        def check():
            for parent, name, child, pin in bindings:
                need(directory_identity(os.fstat(child)) == pin and
                     directory_identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
                     "product-changed")
        if witness is not None:
            witness["check"] = check
        yield fd
        check()
    except OSError:
        raise Refused("product-path") from None
    finally:
        for fd in reversed(held):
            os.close(fd)


def stream_file(parent, name, budget):
    """Hash the opened inode; refuse mutation, replacement and hard links."""
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    try:
        info = os.fstat(fd)
        need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, "product-path")
        pin = identity(info)
        need(info.st_size <= budget[0], "product-size")
        hashed, count = hashlib.sha256(), 0
        while True:
            raw = os.read(fd, min(CHUNK_BYTES, budget[0] + 1))
            if not raw:
                break
            count += len(raw)
            budget[0] -= len(raw)
            need(budget[0] >= 0, "product-size")
            hashed.update(raw)
        need(count == info.st_size and identity(os.fstat(fd)) == pin and
             identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
             "product-changed")
        return {"type": "file", "sha256": hashed.hexdigest(), "bytes": count,
                "mode": stat.S_IMODE(info.st_mode)}, pin
    finally:
        os.close(fd)


def binary(boundary, path, budget=None):
    contained(boundary, path)
    with authority(os.path.dirname(path)) as parent:
        return stream_file(parent, os.path.basename(path),
                           [PRODUCT_BYTES] if budget is None else budget)


def relative(path):
    need(isinstance(path, str) and path and not path.startswith("/") and
         "\x00" not in path and all(p not in ("", ".", "..") for p in path.split("/")),
         "product-path")
    return path


def link_terminal(entries, name):
    visited = set()
    while True:
        parts, resolved = name.split("/"), []
        changed = False
        for index, part in enumerate(parts):
            resolved.append(part)
            current = "/".join(resolved)
            need(current in entries, "product-link")
            entry = entries[current]
            if entry["type"] == "symlink":
                need(current not in visited, "product-link")
                visited.add(current)
                target = entry["target"]
                need(isinstance(target, str) and target and not target.startswith("/") and
                     "\x00" not in target, "product-link")
                name = posixpath.normpath(posixpath.join(posixpath.dirname(current), target,
                                                       *parts[index + 1:]))
                relative(name)
                changed = True
                break
            if index < len(parts) - 1:
                need(entry["type"] == "directory", "product-link")
        if not changed:
            return name


def snapshot(boundary, product):
    """Inventory all product inputs through held no-follow directory handles."""
    contained(boundary, product)
    entries, pins, budget = {}, {}, [PRODUCT_BYTES]

    def walk(fd, prefix):
        before = os.fstat(fd)
        names = sorted(os.listdir(fd))
        for name in names:
            path = prefix + name
            relative(path)
            need(len(entries) < PRODUCT_ENTRIES, "product-size")
            info = os.stat(name, dir_fd=fd, follow_symlinks=False)
            if stat.S_ISDIR(info.st_mode):
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                try:
                    pin = directory_identity(info)
                    need(directory_identity(os.fstat(child)) == pin, "product-changed")
                    entries[path] = {"type": "directory", "mode": stat.S_IMODE(info.st_mode)}
                    pins[path] = pin
                    walk(child, path + "/")
                    need(directory_identity(os.stat(name, dir_fd=fd, follow_symlinks=False)) == pin,
                         "product-changed")
                finally:
                    os.close(child)
            elif stat.S_ISLNK(info.st_mode):
                target = os.readlink(name, dir_fd=fd)
                need(identity(os.stat(name, dir_fd=fd, follow_symlinks=False)) == identity(info),
                     "product-changed")
                entries[path] = {"type": "symlink", "target": target}
                pins[path] = identity(info)
            else:
                entry, pin = stream_file(fd, name, budget)
                need(pin == identity(info), "product-changed")
                entries[path], pins[path] = entry, pin
        after = os.fstat(fd)
        need(directory_identity(after) == directory_identity(before) and
             after.st_mtime_ns == before.st_mtime_ns and after.st_ctime_ns == before.st_ctime_ns and
             sorted(os.listdir(fd)) == names, "product-changed")

    with authority(product) as fd:
        pins[""] = directory_identity(os.fstat(fd))
        walk(fd, "")
        for name, entry in entries.items():
            if entry["type"] == "symlink":
                link_terminal(entries, name)
    need(entries, "product-path")
    return entries, pins


def profile_authorizes(key, grant, claim):
    """Only services/environment have authorization forms beyond exact equality."""
    if key == "com.apple.developer.icloud-services":
        if claim != ["CloudKit"]:
            return False
        if grant == "*":
            return True
        choices, required = ("CloudKit", "CloudDocuments"), "CloudKit"
    elif key == "com.apple.developer.icloud-container-environment":
        if claim != "Development":
            return False
        if grant == "Development":
            return True
        choices, required = ("Development", "Production"), "Development"
    else:
        return grant == claim
    return (isinstance(grant, list) and bool(grant) and
            all(isinstance(item, str) and item in choices for item in grant) and
            len(grant) == len(set(grant)) and required in grant)


def profile_check(profile, ent, cert, device, kind):
    team = ent.get("com.apple.developer.team-identifier")
    identity = ent.get("com.apple.application-identifier", ent.get("application-identifier"))
    grants = profile.get("Entitlements", {})
    need(isinstance(team, str) and re.fullmatch(r"[A-Z0-9]{10}", team) and
         identity == team + ".com.antoinefink.meguri.dev", "product-identity")
    need(isinstance(grants, dict) and team in profile.get("TeamIdentifier", []) and
         grants.get("com.apple.developer.team-identifier") == team, "profile-team")
    allowed = grants.get("com.apple.application-identifier", grants.get("application-identifier"))
    need(isinstance(allowed, str) and fnmatch.fnmatchcase(identity, allowed), "profile-identity")
    need(("OSX" if kind == "mac" else "iOS") in profile.get("Platform", []) and
         cert in profile.get("DeveloperCertificates", []) and bool(cert), "profile-signer-platform")
    expiry = profile.get("ExpirationDate")
    need(isinstance(expiry, datetime.datetime) and
         expiry.replace(tzinfo=datetime.timezone.utc) > datetime.datetime.now(datetime.timezone.utc), "profile-expired")
    devices = profile.get("ProvisionedDevices")
    need(isinstance(devices, list) and devices and all(isinstance(d, str) and d for d in devices) and
         (device is None or device in devices) and
         profile.get("ProvisionsAllDevices", False) is False, "profile-device")
    debug_keys = ("get-task-allow", "com.apple.security.get-task-allow")
    need(all(grants[key] is True for key in debug_keys if key in grants) and
         (kind == "mac" or grants.get("get-task-allow") is True), "profile-development")
    for key, value in ent.items():
        if protected(key):
            need(profile_authorizes(key, grants.get(key), value), "profile-grants")
    for key in ("keychain-access-groups", "com.apple.security.application-groups"):
        need(all(isinstance(v, str) and any(isinstance(g, str) and fnmatch.fnmatchcase(v, g)
                                          for g in grants.get(key, []))
                 for v in ent.get(key, [])), "profile-grants")


def binary_platform(path, platform, category):
    arches = command(["/usr/bin/lipo", "-archs", path], category).stdout.decode().split()
    need("arm64" in arches and set(arches) <= {"arm64", "x86_64"}, category)
    load = command(["/usr/bin/xcrun", "vtool", "-show-build", path], category).stdout.decode()
    observed = re.findall(r"\bplatform\s+(\S+)", load)
    need(observed and set(observed) == {"MACOS" if platform == "mac" else "IOSSIMULATOR"}, category)


def independent_product(root, product, expected, settings, vm_device=None):
    """Inspect a VM Mac or reserved simulator product; never delivery proof."""
    kind = expected["device_kind"]
    need(kind in ("mac", "iphone", "ipad"), "product-platform")
    boundary = os.path.join(root, "build")
    contained(boundary, product)
    before = snapshot(boundary, product)
    prefix = "Contents/" if kind == "mac" else ""
    info_path = os.path.join(product, prefix + "Info.plist")
    info = plist(_first_sync_bytes(boundary, info_path), "product-info")
    executable = info.get("CFBundleExecutable")
    need(executable == "Meguri" and info.get("CFBundleIdentifier") == "com.antoinefink.meguri.dev",
         "product-identity")
    need(info.get("MeguriCardInspectionBuild") == "debug", "product-debug")
    need(info.get("CFBundleSupportedPlatforms") == (["MacOSX"] if kind == "mac" else ["iPhoneSimulator"]),
         "product-platform")
    if kind != "mac":
        need(info.get("UIDeviceFamily") == ([1] if kind == "iphone" else [2]) or
             info.get("UIDeviceFamily") == [1, 2], "product-platform")
    executable_path = os.path.join(product, "Contents/MacOS" if kind == "mac" else "", executable)
    need(binary(boundary, executable_path)[0]["sha256"] == expected["executable_sha256"],
         "executable-bytes")
    need(isinstance(info.get("CFBundleVersion"), str) and info["CFBundleVersion"] and
         info["CFBundleVersion"] == expected["app_build"], "app-build")
    binary_platform(executable_path, "mac" if kind == "mac" else "ios-sim", "product-platform")
    debug_path = os.path.join(os.path.dirname(executable_path), "Meguri.debug.dylib")
    if os.path.lexists(debug_path):
        binary_platform(debug_path, "mac" if kind == "mac" else "ios-sim", "product-platform")
    command(["/usr/bin/codesign", "--verify", "--deep", "--strict", product], "product-signature")
    metadata = command(["/usr/bin/codesign", "-d", "--verbose=4", product], "product-signature").stderr.decode()
    ent = plist(command(["/usr/bin/codesign", "-d", "--entitlements", "-", "--xml", product],
                        "product-signature").stdout, "product-entitlements")
    team = ent.get("com.apple.developer.team-identifier")
    need(isinstance(team, str) and re.fullmatch(r"[A-Z0-9]{10}", team) and
         ent.get("com.apple.application-identifier", ent.get("application-identifier")) ==
         team + ".com.antoinefink.meguri.dev", "product-signature")
    lines = metadata.splitlines()
    need("Signature=adhoc" not in lines and any(line.startswith("Authority=") for line in lines) and
         "Identifier=com.antoinefink.meguri.dev" in lines and "TeamIdentifier=" + team in lines,
         "product-signature")
    need(ent.get("com.apple.security.get-task-allow" if kind == "mac" else "get-task-allow") is True,
         "product-debug")
    need(info.get("MeguriCloudContainer") == expected["container"] and
         info.get("MeguriCloudEnvironment") == expected["environment"], "wrong-container")
    try:
        check_cloud(settings, "Debug", "mac" if kind == "mac" else "ios", "team", info, ent)
    except (ValueError, KeyError, TypeError):
        raise Refused("product-cloud-grants") from None
    need(info.get("MeguriCloudSchemaConfigured") is True, "product-schema")
    if kind == "mac":
        profile_path = os.path.join(product, "Contents/embedded.provisionprofile")
        profile_raw = _first_sync_bytes(boundary, profile_path)
        with tempfile.TemporaryDirectory(prefix="meguri-agent-signer-") as temporary:
            temporary = os.path.realpath(temporary)
            certificate_prefix = os.path.join(temporary, "cert")
            command(["/usr/bin/codesign", "-d", "--extract-certificates=" + certificate_prefix, product],
                    "product-signature")
            certificate = _first_sync_bytes(temporary, certificate_prefix + "0")
            profile = plist(command(["/usr/bin/security", "cms", "-D", "-i", profile_path],
                                    "product-profile").stdout, "product-profile")
            # VM coverage requires a separate live observation.
            # None checks the product's profile without claiming destination
            # coverage. Admission still stops for the missing VM/pool producer.
            profile_check(profile, ent, certificate, vm_device, kind)
        need(_first_sync_bytes(boundary, profile_path) == profile_raw, "product-changed")
    # Simulator signing uses team/development grants and the measured simulator
    # Mach-O platform above. No physical-device provisioning is requested.
    need(snapshot(boundary, product) == before, "product-changed")
    return team




def json_input(boundary, path, witness=None):
    before = binary(boundary, path, [16 * 1024 * 1024])
    found = {}
    value = _first_sync_json(boundary, path, witness=found)
    need(binary(boundary, path, [16 * 1024 * 1024]) == before and
         before[0]["sha256"] == found["sha256"], "input-changed")
    if witness is not None:
        witness.update(found)
    return value


def file_reference(boundary, path):
    entry, _ = binary(boundary, path)
    return {"path": path, "sha256": entry["sha256"], "bytes": entry["bytes"]}


def bundle(products, path, platform):
    relative(path)
    full = os.path.join(products, path)
    prefix = "Contents/" if platform == "mac" else ""
    info = plist(_first_sync_bytes(products, full + "/" + prefix + "Info.plist"), "product-info")
    name = info.get("CFBundleExecutable")
    need(isinstance(name, str) and name and "/" not in name and name not in (".", ".."), "product-info")
    executable = prefix + ("MacOS/" if platform == "mac" else "") + name
    entry, _ = binary(products, full + "/" + executable)
    need(isinstance(info.get("CFBundleIdentifier"), str) and info["CFBundleIdentifier"], "product-info")
    return {"path": path, "bundle_id": info["CFBundleIdentifier"], "executable": executable,
            "executable_sha256": entry["sha256"]}, info


def test_inputs(products, path, app, platform):
    """Resolve the actual CloudAgent host/bundle and reject extra test selection."""
    raw = _first_sync_bytes(products, path)
    data = plist(raw, "agent-xctestrun")
    configs = data.get("TestConfigurations")
    need(isinstance(configs, list) and len(configs) == 1 and isinstance(configs[0], dict),
         "agent-xctestrun")
    targets = configs[0].get("TestTargets")
    need(isinstance(targets, list) and len(targets) == 1 and isinstance(targets[0], dict),
         "agent-xctestrun")
    target = targets[0]
    selection = target.get("OnlyTestIdentifiers")
    need(target.get("BlueprintName") == "MeguriSnapshotTests" and
         target.get("IsUITestBundle", False) is False and
         target.get("IsAppHostedTestBundle") is True and
         target.get("TestHostBundleIdentifier") == app["bundle_id"] and
         isinstance(selection, list) and len(selection) in (1, 2) and
         all(isinstance(item, str) and item for item in selection) and
         len(set(selection)) == len(selection) and
         set(selection) in ({"CloudKitSoakTests"},
                            {"CloudKitSoakTests", "CloudKitPathPreflightTests"}) and
         not target.get("SkipTestIdentifiers"), "agent-selection")
    host_bundle = os.path.join(products, app["path"])

    def resolve(value):
        need(isinstance(value, str), "agent-xctestrun")
        value = value.replace("__TESTROOT__", products).replace("__TESTHOST__", host_bundle)
        # Expansion may not introduce a parent traversal or another root.
        return contained(products, value)

    need(resolve(target.get("TestHostPath")) == host_bundle, "agent-host")
    agent_path = resolve(target.get("TestBundlePath"))
    embedded = "Contents/PlugIns/" if platform == "mac" else "PlugIns/"
    need(agent_path == os.path.join(host_bundle, embedded + "MeguriSnapshotTests.xctest"),
         "agent-bundle")
    agent, _ = bundle(products, os.path.relpath(agent_path, products), platform)
    # Bundle and loader paths must stay in the frozen inventory. General SDK
    # macro trust applies only to Xcode's framework/library search variables.
    def input_path(item, search=None):
        sdk = ("__PLATFORMS__/", "__DEVELOPER_DIR__/")
        if search is not None and item.startswith(sdk):
            need(".." not in item.split("/") and "\x00" not in item, "agent-environment")
        elif search == "framework" and item == "__SHAREDFRAMEWORKS__":
            pass
        else:
            need(os.path.lexists(resolve(item)), "agent-environment")

    searches = {"DYLD_FRAMEWORK_PATH": "framework", "DYLD_LIBRARY_PATH": "library",
                "__XPC_DYLD_FRAMEWORK_PATH": "framework", "__XPC_DYLD_LIBRARY_PATH": "library"}
    injection = ("__TESTHOST__/" + ("Contents/Frameworks/" if platform == "mac" else "Frameworks/") +
                 "libXCTestBundleInject.dylib")
    insertions = {
        "EnvironmentVariables": "/usr/lib/libRPAC.dylib",
        "TestingEnvironmentVariables": injection + ":" +
            ("__DEVELOPERUSRLIB__/libMainThreadChecker.dylib:" if platform == "mac" else "") +
            "/usr/lib/libRPAC.dylib",
    }

    def product_paths(node, key=""):
        if isinstance(node, dict):
            for child_key, child in node.items():
                need(isinstance(child_key, str), "agent-xctestrun")
                product_paths(child, child_key)
        elif isinstance(node, list):
            for child in node:
                product_paths(child, key)
        elif key.endswith(("Path", "Paths")) or (isinstance(node, str) and
                  ("__TESTROOT__" in node or "__TESTHOST__" in node)):
            need(isinstance(node, str), "agent-xctestrun")
            for item in node.split(":"):
                input_path(item)

    for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
        environment = target.get(key, {})
        need(isinstance(environment, dict), "agent-environment")
        for name, value in environment.items():
            need(isinstance(name, str) and isinstance(value, str), "agent-environment")
            if name == "DYLD_INSERT_LIBRARIES":
                # Exact captured references only; no SDK/system directory exemption.
                need(value == insertions[key], "agent-environment")
                if key == "TestingEnvironmentVariables":
                    input_path(injection)
                    # Require physical code inside the host app's Frameworks. The
                    # surrounding inventory pins its bytes across --deep --strict.
                    binary(products, resolve(injection))
                continue
            need(not name.startswith("MEGURI_CLOUD_ACCOUNT_") and name != "LD_PRELOAD" and
                 (not name.startswith(("DYLD_", "__XPC_DYLD_")) or name in searches),
                 "agent-environment")
            if name in searches:
                for item in value.split(":"):
                    input_path(item, searches[name])
            else:
                product_paths(value, name)

    # This project reference is Xcode metadata, never a loader input. Restrict
    # the exception to this exact target key and project; other paths still bind.
    if "BlueprintProviderRelativePath" in target:
        need(target["BlueprintProviderRelativePath"] == "Meguri.xcodeproj", "agent-xctestrun")
    if "DependentProductPaths" in target:
        dependencies = target["DependentProductPaths"]
        need(isinstance(dependencies, list) and dependencies and
             all(isinstance(item, str) and item for item in dependencies), "agent-xctestrun")
    for key, value in target.items():
        if key not in ("BlueprintProviderRelativePath", "EnvironmentVariables", "TestingEnvironmentVariables"):
            product_paths(value, key)
    return agent


def freeze_build(root, products, xctestrun, platform, run, sha):
    """Called by the real build-app producer after its last signing/copy step."""
    need(platform in ("mac", "ios-sim") and MG_RUN_ID.fullmatch(run), "build-arguments")
    need(source(root) == sha, "source-changed")
    contained(os.path.join(root, "build"), products)
    contained(products, xctestrun)
    before = snapshot(os.path.join(root, "build"), products)
    app_dir = "Debug" if platform == "mac" else "Debug-iphonesimulator"
    app, info = bundle(products, app_dir + "/Meguri.app", platform)
    need(app["bundle_id"] == "com.antoinefink.meguri.dev" and
         isinstance(info.get("CFBundleVersion"), str) and info["CFBundleVersion"], "app-build")
    agent = test_inputs(products, xctestrun, app, platform)
    signer_id = None
    for item in (app, agent):
        product = os.path.join(products, item["path"])
        command(["/usr/bin/codesign", "--verify", "--deep", "--strict", product], "build-signature")
        metadata = command(["/usr/bin/codesign", "-d", "--verbose=4", product],
                           "build-signature").stderr.decode().splitlines()
        signer_ids = [line.split("=", 1)[1] for line in metadata if line.startswith("TeamIdentifier=")]
        need(len(signer_ids) == 1 and re.fullmatch(r"[A-Z0-9]{10}", signer_ids[0]) and
             "Signature=adhoc" not in metadata and any(line.startswith("Authority=") for line in metadata) and
             "Identifier=" + item["bundle_id"] in metadata, "build-signature")
        need(signer_id is None or signer_id == signer_ids[0], "build-signature")
        signer_id = signer_ids[0]
    xref = file_reference(products, xctestrun)
    xref["path"] = os.path.relpath(xctestrun, products)
    need(snapshot(os.path.join(root, "build"), products) == before and source(root) == sha,
         "product-changed")
    return {"protocol_version": 1, "job": "build-app", "run_id": run, "source_commit": sha,
            "scheme": "Meguri", "configuration": "Debug", "test_plan": "CloudAgent",
            "platform": platform, "signing": "team", "app_build": info["CFBundleVersion"],
            "app": app, "agent": agent, "xctestrun": xref,
            "products": {"path": products, "entries": before[0]}}


def publish_frozen_build(root, products, xctestrun, platform, run, sha, manifest):
    """Publish the producer's first manifest with its unchanged input envelope."""
    producer_pin = snapshot(os.path.join(root, "build"), products)
    value = freeze_build(root, products, xctestrun, platform,
                         run, sha)
    # O_EXCL and held parents prevent a pre-existing symlink or receipt
    # pathname from redirecting the producer's output.
    path = contained(os.path.join(root, "build"), manifest)
    with authority(os.path.dirname(path)) as parent:
        fd = os.open(os.path.basename(path), os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                     0o600, dir_fd=parent)
        with os.fdopen(fd, "wb") as stream:
            raw = (json.dumps(value, sort_keys=True) + "\n").encode()
            need(len(raw) <= 16 * 1024 * 1024, "build-manifest-size")
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
    need(source(root) == sha and
         snapshot(os.path.join(root, "build"), products) == producer_pin and
         producer_pin[0] == value["products"]["entries"],
         "product-changed")
    return file_reference(os.path.join(root, "build"), path)


def check_reference(reference):
    need(isinstance(reference, dict) and set(reference) == {"path", "sha256", "bytes"} and
         isinstance(reference["path"], str) and type(reference["bytes"]) is int and
         reference["bytes"] >= 0 and isinstance(reference["sha256"], str) and
         re.fullmatch(r"[0-9a-f]{64}", reference["sha256"]), "artifact-reference")


def check_entries(entries):
    need(isinstance(entries, dict) and 0 < len(entries) <= PRODUCT_ENTRIES, "product-inventory")
    for path, entry in entries.items():
        relative(path)
        need(isinstance(entry, dict), "product-inventory")
        kind = entry.get("type")
        keys = {"file": {"type", "sha256", "bytes", "mode"},
                "directory": {"type", "mode"}, "symlink": {"type", "target"}}
        need(isinstance(kind, str) and kind in keys and set(entry) == keys[kind], "product-inventory")
        if kind == "symlink":
            need(isinstance(entry["target"], str) and entry["target"], "product-inventory")
        else:
            need(type(entry["mode"]) is int and 0 <= entry["mode"] <= 0o7777, "product-inventory")
            if kind == "file":
                check_reference({"path": path, "sha256": entry["sha256"], "bytes": entry["bytes"]})


def declared_reference(root, value, reference):
    check_reference(reference)
    path = reference["path"]
    need(sum(item["path"] == path for item in value["artifacts"]) == 1, "artifact-declaration")
    need(file_reference(os.path.join(root, "build"), path) == reference, "artifact-bytes")
    return path


def build_product(root, value, expected):
    need(value["job"] == "build-app", "build-producer")
    options, args = {}, iter(value["args"])
    for arg in args:
        need(arg in ("--platform", "--config", "--test-plan") and arg not in options, "build-arguments")
        options[arg] = next(args, None)
        need(options[arg] is not None, "build-arguments")
    platform = options.get("--platform")
    need(platform in ("mac", "ios-sim"), "build-arguments")
    need(options.get("--config", "Debug") == "Debug", "product-debug")
    need(options.get("--test-plan") == "CloudAgent", "build-plan")
    need(platform == ("mac" if expected["device_kind"] == "mac" else "ios-sim"), "product-platform")
    build = value["details"].get("build")
    need(isinstance(build, dict) and build.get("platform") == platform and
         build.get("config") == "Debug" and build.get("test_plan") == "CloudAgent" and
         build.get("action") == "build-for-testing" and build.get("standalone") is False and
         build.get("personal_preview") is False, "build-product")
    reference = value["details"].get("agent_build")
    need(reference is not None, "implementation_missing-agent-build")
    path = declared_reference(root, value, reference)
    need(os.path.basename(path) == "agent-build.json", "build-manifest")
    manifest_witness = {}
    manifest = json_input(os.path.join(root, "build"), path, manifest_witness)
    need(manifest_witness["sha256"] == reference["sha256"], "input-changed")
    need(isinstance(manifest, dict) and set(manifest) == {"protocol_version", "job", "run_id", "source_commit", "scheme", "configuration",
                           "test_plan", "platform", "signing", "app_build", "app", "agent", "xctestrun", "products"},
         "build-manifest")
    for key, wanted in (("protocol_version", 1), ("job", "build-app"), ("run_id", value["run_id"]),
                        ("source_commit", value["commit"]["sha"]), ("scheme", "Meguri"),
                        ("configuration", "Debug"), ("test_plan", "CloudAgent"),
                        ("platform", platform), ("signing", "team"), ("app_build", expected["app_build"])):
        need(type(manifest[key]) is type(wanted) and manifest[key] == wanted, "build-manifest")
    products = manifest["products"]
    need(isinstance(products, dict) and set(products) == {"path", "entries"}, "build-manifest")
    product_root = contained(os.path.join(root, "build"), products["path"])
    need(sum(item["path"] == product_root for item in value["artifacts"]) == 1, "artifact-declaration")
    before = snapshot(os.path.join(root, "build"), product_root)
    check_entries(products["entries"])
    need(before[0] == products["entries"], "product-inventory")
    app_dir = "Debug" if platform == "mac" else "Debug-iphonesimulator"
    app, info = bundle(product_root, app_dir + "/Meguri.app", platform)
    need(app == manifest["app"] and info.get("CFBundleVersion") == manifest["app_build"] and
         app["executable_sha256"] == expected["executable_sha256"], "build-app-binding")
    app_path = os.path.join(product_root, app["path"])
    need(build.get("app") == app_path and
         sum(item["path"] == app_path and item["kind"] == "app" for item in value["artifacts"]) == 1,
         "artifact-declaration")
    xref = manifest["xctestrun"]
    check_reference(xref)
    xpath = os.path.join(product_root, relative(xref["path"]))
    actual = file_reference(product_root, xpath)
    actual["path"] = xref["path"]
    need(actual == xref and test_inputs(product_root, xpath, app, platform) == manifest["agent"],
         "agent-binding")
    need(snapshot(os.path.join(root, "build"), product_root) == before, "product-changed")
    return manifest, before


def independent_agent(root, manifest, team):
    products, platform = manifest["products"]["path"], manifest["platform"]
    claimed = manifest["agent"]
    actual, info = bundle(products, claimed["path"], platform)
    need(actual == claimed and actual["executable"] ==
         ("Contents/MacOS/MeguriSnapshotTests" if platform == "mac" else "MeguriSnapshotTests"),
         "agent-bundle")
    need(info.get("CFBundlePackageType") == "BNDL" and
         info.get("CFBundleSupportedPlatforms") == (["MacOSX"] if platform == "mac" else ["iPhoneSimulator"]),
         "agent-platform")
    path = os.path.join(products, claimed["path"])
    command(["/usr/bin/codesign", "--verify", "--deep", "--strict", path], "agent-signature")
    metadata = command(["/usr/bin/codesign", "-d", "--verbose=4", path], "agent-signature").stderr.decode().splitlines()
    need("Signature=adhoc" not in metadata and "TeamIdentifier=" + team in metadata and
         "Identifier=" + actual["bundle_id"] in metadata and any(v.startswith("Authority=") for v in metadata),
         "agent-signature")
    binary_platform(os.path.join(path, actual["executable"]), platform, "agent-platform")


def admit_product(root, results, expected, vm_device=None):
    """Non-account J2.1 product checks. No normal header or account authority.

    Setup supplies only measurements from its original build receipt/product.
    The normal caller still admits and rechecks its complete immutable header.
    """
    sha = source(root)
    need(expected["source_commit"] == sha, "source-changed")
    build, receipt_hash = receipt(root, results, expected["build_receipt"], sha)
    if "build_receipt_sha256" in expected:
        need(receipt_hash == expected["build_receipt_sha256"], "build-receipt-bytes")
    manifest, frozen = build_product(root, build, expected)
    settings_path = os.path.join(results, build["job"], build["run_id"], "agent-cloud-settings.json")
    settings_hash = digest(declared_file(root, build, settings_path))
    settings_witness = {}
    targets = json_input(os.path.join(root, "build"), settings_path, settings_witness)
    need(settings_witness["sha256"] == settings_hash, "input-changed")
    need(isinstance(targets, list), "product-settings")
    apps = [entry.get("buildSettings") for entry in targets if isinstance(entry, dict) and entry.get("target") == "Meguri"]
    need(len(apps) == 1 and isinstance(apps[0], dict), "product-settings")
    settings = apps[0]
    need(settings.get("CONFIGURATION") == "Debug" and settings.get("MEGURI_CORE_FEATURES") == "ankiweb,test-hooks"
         and "MEGURI_TEST_HOOKS" in settings.get("SWIFT_ACTIVE_COMPILATION_CONDITIONS", "").split(),
         "product-debug-settings")
    agents = [entry.get("buildSettings") for entry in targets if isinstance(entry, dict) and
              entry.get("target") == "MeguriSnapshotTests"]
    host = os.path.join(manifest["products"]["path"], manifest["app"]["path"], manifest["app"]["executable"])
    need(len(agents) == 1 and isinstance(agents[0], dict) and
         agents[0].get("CONFIGURATION") == "Debug" and
         agents[0].get("PRODUCT_BUNDLE_IDENTIFIER") == manifest["agent"]["bundle_id"] and
         agents[0].get("TEST_HOST") == host and agents[0].get("BUNDLE_LOADER") == host,
         "agent-build-settings")
    product = os.path.join(manifest["products"]["path"], manifest["app"]["path"])
    team = independent_product(root, product, expected, settings, vm_device)
    independent_agent(root, manifest, team)
    need(snapshot(os.path.join(root, "build"), manifest["products"]["path"]) == frozen, "product-changed")
    need(digest(declared_file(root, build, settings_path)) == settings_hash, "input-changed")
    declared_reference(root, build, build["details"]["agent_build"])
    need(receipt(root, results, expected["build_receipt"], sha)[1] == receipt_hash, "receipt-changed")
    need(source(root) == sha, "source-changed")
    return build, receipt_hash, manifest, frozen


def admit_build(root, results, path, run, vm_device=None):
    """Reusable admission; only a live delivery caller supplies VM coverage."""
    root, results = os.path.abspath(root), os.path.abspath(results)
    need(isinstance(run, str) and MG_RUN_ID.fullmatch(run), "header-run")
    sha = source(root)
    header_witness = {}
    expected = header(root, path, run, sha, header_witness)
    build, receipt_hash, manifest, frozen = admit_product(root, results, expected, vm_device)
    need(receipt_hash == expected["build_receipt_sha256"], "build-receipt-bytes")
    new_witness = {}
    need(header(root, path, run, sha, new_witness) == expected and new_witness == header_witness
         and source(root) == sha, "input-changed")
    return expected, build, manifest, frozen


def admit(root, results, path, run):
    admit_build(root, results, path, run)
    # There is no live VM/pool routing or installation producer in this unit.
    # Do not inspect the host, choose an attached device, trust a header label,
    # or promote caller-written deployment/account/journal flags to proof.
    raise Refused("implementation_missing-vm-pool-install-runtime-proof")


class Parser(argparse.ArgumentParser):
    def error(self, message):
        # argparse's default error includes arbitrary argument values.
        print(json.dumps({"scope": "INPUT-VALIDATION", "status": "fail", "category": "usage",
                          "outcome": "failed_evidence", "signed_execution": False}))
        self.exit(1)


def main():
    parser = Parser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--results-directory")
    parser.add_argument("--header")
    parser.add_argument("--freeze-build", action="store_true")
    parser.add_argument("--source-commit")
    parser.add_argument("--platform", choices=("mac", "ios-sim"))
    parser.add_argument("--products")
    parser.add_argument("--xctestrun")
    parser.add_argument("--manifest")
    args = parser.parse_args()
    try:
        if args.freeze_build:
            need(all((args.source_commit, args.platform, args.products, args.xctestrun, args.manifest)) and
                 args.header is None and args.results_directory is None, "usage")
            reference = publish_frozen_build(os.path.abspath(args.root), args.products, args.xctestrun,
                                             args.platform, args.run_id, args.source_commit, args.manifest)
            print(json.dumps(reference))
            return 0
        need(args.results_directory and args.header and not any((args.source_commit, args.platform,
             args.products, args.xctestrun, args.manifest)), "usage")
        admit(args.root, args.results_directory, args.header, args.run_id)
        category = "implementation_missing-vm-pool-install-runtime-proof"
    except Refused as error:
        category = error.args[0]
    except Invalid:
        category = "guarded-input"
    except (ValueError, TypeError, KeyError, IndexError, AttributeError, OSError, UnicodeError, RecursionError):
        category = "invalid-input"
    print(json.dumps({"scope": "BUILD-INPUT" if args.freeze_build else "INPUT-VALIDATION",
                      "status": "fail", "category": category,
                      "outcome": ("implementation_missing" if category.startswith("implementation_missing")
                                  else "failed_evidence"), "signed_execution": False}))
    return 1


if __name__ == "__main__":
    sys.exit(main())
