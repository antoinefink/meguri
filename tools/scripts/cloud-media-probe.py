#!/usr/bin/env python3
"""Bounded normal development-app media diagnostic (J2/J6, K9.4).

No account queries, app launches, database access or device writes. CLI inputs
are MG run IDs only. Native output never enters the public report.
prepare-ui freezes private files only; authorized Finder publication and normal
app sync are separate observations. A peer byte match is diagnostic, not Q7.
"""

import argparse
import contextlib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import pwd
import re
import stat
import subprocess
import sys

# Reuse only the OS no-replace primitive; no signed-agent operations are called.
_spec = importlib.util.spec_from_file_location("media_probe_handoff", Path(__file__).with_name("cloud-agent-handoff.py"))
_handoff = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_handoff)

RUN = re.compile(r"[0-9]{8}T[0-9]{6}Z-[0-9]+")
SIZE = 10_240
CAP = 16 * 1024 * 1024
PREVIEW = "build/app/PersonalPreviewDerivedData/Build/Products/Release/Meguri.app"
PHONE = "build/app/DerivedData/Build/Products/Debug-iphoneos/Meguri.app"
SCOPE = "NORMAL-APP-MEDIA-PROBE"


class Refused(Exception):
    pass


def need(condition, category):
    if not condition:
        raise Refused(category)


def run_id(value):
    need(isinstance(value, str) and RUN.fullmatch(value) is not None, "receipt-refused")
    return value


def filename(run):
    return "meguri-media-probe-" + run_id(run) + ".bin"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def directory_pin(info):
    # APFS link counts change when children are added.
    return info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid


def file_pin(info):
    return (*directory_pin(info), info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


class Directory:
    """Pin every nofollow binding, including the original physical ancestors."""
    def __init__(self, path, owned_from=None):
        self.path = str(path)
        self.fds, self.bindings = [], []
        need(os.path.isabs(self.path) and os.path.normpath(self.path) == self.path and
             "\0" not in self.path, "path-refused")
        try:
            fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            self.fds.append(fd)
            current = ""
            for name in self.path.split("/")[1:]:
                current += "/" + name
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                self.fds.append(child)
                info = os.fstat(child)
                owned = owned_from is not None and (current == str(owned_from) or current.startswith(str(owned_from) + "/"))
                need(info.st_uid in ((os.getuid(),) if owned else (0, os.getuid())), "path-refused")
                # A sticky system temp ancestor is allowed for generated tests;
                # user-owned roots and all descendants must deny other writers.
                need(not info.st_mode & 0o022 or
                     (not owned and info.st_uid == 0 and info.st_mode & stat.S_ISVTX), "path-refused")
                self.bindings.append((fd, name, child, directory_pin(info)))
                fd = child
            self.fd = fd
            self.check()
        except BaseException:
            self.close()
            raise

    def check(self):
        for parent, name, child, pin in self.bindings:
            need(directory_pin(os.fstat(child)) == pin and
                 directory_pin(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin, "path-refused")

    def close(self):
        for fd in reversed(self.fds):
            os.close(fd)
        self.fds = []


class Original:
    def __init__(self, held, path, limit=CAP):
        self.parent = held.enter_context(contextlib.closing(Directory(Path(path).parent)))
        self.name = Path(path).name
        self.fd = os.open(self.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=self.parent.fd)
        held.callback(os.close, self.fd)
        info = os.fstat(self.fd)
        need(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1 and
             not info.st_mode & 0o022 and 0 <= info.st_size <= limit, "path-refused")
        self.pin = file_pin(info)
        chunks = []
        while True:
            chunk = os.read(self.fd, min(1024 * 1024, limit + 1))
            if not chunk:
                break
            chunks.append(chunk)
            limit -= len(chunk)
            need(limit >= 0, "path-refused")
        self.raw = b"".join(chunks)
        self.check()

    def check(self):
        self.parent.check()
        need(file_pin(os.fstat(self.fd)) == self.pin and file_pin(os.stat(
            self.name, dir_fd=self.parent.fd, follow_symlinks=False)) == self.pin, "path-refused")

    def json(self):
        def pairs(items):
            out = {}
            for key, value in items:
                need(key not in out, "receipt-refused")
                out[key] = value
            return out
        return json.loads(self.raw, object_pairs_hook=pairs)


class Inputs:
    def __init__(self, held, root):
        self.held, self.root, self.originals = held, Path(root), []

    def read(self, path, limit=CAP):
        original = Original(self.held, path, limit)
        self.originals.append(original)
        return original

    def check(self):
        for original in self.originals:
            original.check()

    def receipt(self, job, run):
        original = self.read(self.root / "build/mg" / job / run_id(run) / "result.json")
        value = original.json()
        need(value.get("schema_version") == 1 and value.get("job") == job and value.get("run_id") == run and
             value.get("root") == str(self.root) and value.get("status") == "pass" and
             value.get("exit_code") == 0 and value.get("dry_run") is False and
             value.get("placement") == "host", "receipt-refused")
        return value, {"run_id": run, "sha256": digest(original.raw)}


def options(args, allowed):
    need(isinstance(args, list) and len(args) % 2 == 0, "receipt-refused")
    result = {}
    for key, value in zip(args[::2], args[1::2]):
        need(key in allowed and key not in result and isinstance(value, str), "receipt-refused")
        result[key] = value
    return result


def native(private, label, args):
    # Private regular log files, no shell, no console inheritance or retries.
    with open(private / (label + ".stdout"), "xb") as out, open(private / (label + ".stderr"), "xb") as err:
        try:
            result = subprocess.run(args, stdin=subprocess.DEVNULL, stdout=out, stderr=err, timeout=75, check=False)
        except (OSError, subprocess.SubprocessError):
            raise Refused("native-command-failed") from None
    need(result.returncode == 0, "native-command-failed")


def admit_preview(inputs, run, private):
    value, ref = inputs.receipt("build-app", run)
    args = value["args"]
    need(isinstance(args, list) and args.count("--personal-preview") == 1, "product-refused")
    args = [arg for arg in args if arg != "--personal-preview"]
    opts = options(args, ("--platform", "--config"))
    need(opts.get("--platform") == "mac" and opts.get("--config", "Release") == "Release", "product-refused")
    build, product = value["details"]["build"], value["details"]["personal_preview_product"]
    app = inputs.root / PREVIEW
    need(build.get("platform") == "mac" and build.get("config") == "Release" and
         build.get("app") == str(app) and build.get("personal_preview") is True and
         build.get("standalone") is False and build.get("action") == "build", "product-refused")
    need(all(product.get(key) is True for key in ("signature_verified", "provisioning_verified", "native_visible_application")) and
         all(product.get(key) is False for key in ("test_hooks", "fixture_read_scope")) and
         product.get("cloud_environment") == "development" and product.get("card_inspection_build") == "personalRelease", "product-refused")
    bundle = product.get("bundle_id")
    need(isinstance(bundle, str) and re.fullmatch(r"[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+\.dev", bundle), "product-refused")
    settings = inputs.read(inputs.root / "build/mg/build-app" / run / "personal-preview-settings.json").json()
    need(isinstance(settings, list), "product-refused")
    selected = [row["buildSettings"] for row in settings if row.get("target") == "Meguri"]
    need(len(selected) == 1, "product-refused")
    settings = selected[0]
    container = settings.get("MEGURI_CLOUD_CONTAINER_development")
    need(isinstance(container, str) and re.fullmatch(r"iCloud\.[A-Za-z0-9.-]+", container) and
         container != settings.get("MEGURI_CLOUD_CONTAINER_production") and
         settings.get("MEGURI_SIGNING") == "team" and settings.get("PRODUCT_BUNDLE_IDENTIFIER") == bundle and
         settings.get("MEGURI_CLOUD_CONFIGURATION") == "development" and
         settings.get("MEGURI_CLOUD_ENVIRONMENT") == "development" and
         settings.get("MEGURI_CLOUD_CONTAINER") == container and
         settings.get("MEGURI_CLOUD_SCHEMA_CONFIGURED_development") == "YES", "product-refused")
    info = plistlib.loads(inputs.read(app / "Contents/Info.plist").raw)
    need(info.get("CFBundleIdentifier") == bundle and info.get("CFBundleExecutable") == "Meguri" and
         info.get("MeguriCloudEnvironment") == "development" and info.get("MeguriCloudContainer") == container and
         info.get("MeguriCloudSchemaConfigured") is True and info.get("MeguriCardInspectionBuild") == "personalRelease" and
         info.get("LSUIElement") is False, "product-refused")
    executable = inputs.read(app / "Contents/MacOS/Meguri", 256 * 1024 * 1024)
    need(digest(executable.raw) == product.get("executable_sha256"), "product-refused")
    inputs.check()
    native(private, "codesign", ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)])
    inputs.check()
    return bundle, ref


def private_path(home, run):
    return Path(home) / (".meguri-media-probe-" + run_id(run))


def new_private(held, home, run):
    parent = held.enter_context(contextlib.closing(Directory(home, owned_from=home)))
    path = private_path(home, run)
    parent.check()
    os.mkdir(path.name, 0o700, dir_fd=parent.fd)
    private = held.enter_context(contextlib.closing(Directory(path, owned_from=home)))
    need(stat.S_IMODE(os.fstat(private.fd).st_mode) == 0o700, "path-refused")
    os.fsync(parent.fd)
    return private


def write_new(parent, name, raw):
    parent.check()
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent.fd)
    with os.fdopen(fd, "wb") as out:
        out.write(raw)
        out.flush()
        os.fsync(out.fileno())
    os.fsync(parent.fd)


def public(outcome, **fields):
    return {"scope": SCOPE, "outcome": outcome, "signed_lane_acceptance": False,
            "transfer_verified": outcome == "peer-byte-match", **fields}


def stage(inputs, held, home, run, preview_run, private):
    bundle, ref = admit_preview(inputs, preview_run, Path(private.path))
    collection_path = Path(home) / "Library/Containers" / bundle / "Data/Library/Application Support/Collection"
    collection = held.enter_context(contextlib.closing(Directory(collection_path, owned_from=home)))
    media = held.enter_context(contextlib.closing(Directory(collection_path / "collection.media", owned_from=home)))
    need(os.fstat(media.fd).st_dev == os.fstat(collection.fd).st_dev, "path-refused")
    raw = os.urandom(SIZE)
    write_new(private, "source.bin", raw)
    source = inputs.read(Path(private.path) / "source.bin", SIZE)
    staging_name = ".meguri-media-probe-" + run_id(run) + ".staging"
    inputs.check(); private.check(); collection.check(); media.check()
    os.mkdir(staging_name, 0o700, dir_fd=collection.fd)
    staging = held.enter_context(contextlib.closing(Directory(collection_path / staging_name, owned_from=home)))
    os.fsync(collection.fd)
    write_new(staging, "payload.bin", source.raw)
    payload = Original(held, collection_path / staging_name / "payload.bin", SIZE)
    need(payload.raw == source.raw, "source-refused")
    inputs.check(); private.check(); collection.check(); media.check(); staging.check()
    payload.check()
    try:
        _handoff.publish_exclusive(staging.fd, "payload.bin", media.fd, filename(run))
    except _handoff.v.Refused:
        raise Refused("publication-conflict") from None
    os.fsync(media.fd)
    os.fsync(staging.fd)
    installed = inputs.read(collection_path / "collection.media" / filename(run), SIZE)
    need(installed.raw == source.raw and file_pin(os.fstat(payload.fd)) == installed.pin, "source-refused")
    inputs.check(); collection.check(); media.check(); staging.check()
    # Only this run's proven original, empty staging directory is removed.
    os.rmdir(staging_name, dir_fd=collection.fd)
    os.fsync(collection.fd)
    return public("staged-only", filename=filename(run), size=SIZE, sha256=digest(raw), build_receipt=ref)


def prepare_ui(inputs, run, preview_run, private):
    # No app-data path is constructed or accessed. Publication belongs to the
    # separately authorized Finder workflow, never an automatic stage fallback.
    _, ref = admit_preview(inputs, preview_run, Path(private.path))
    raw = os.urandom(SIZE)
    write_new(private, "source.bin", raw)
    source = inputs.read(Path(private.path) / "source.bin", SIZE)
    need(len(source.raw) == SIZE and source.raw == raw, "source-refused")
    write_new(private, filename(run), source.raw)
    ui_copy = inputs.read(Path(private.path) / filename(run), SIZE)
    need(ui_copy.raw == source.raw and ui_copy.pin[:2] != source.pin[:2], "source-refused")
    inputs.check(); private.check()
    return public("prepared-only", filename=filename(run), size=SIZE, sha256=digest(source.raw), build_receipt=ref)


def admit_preparation(inputs, run, bundle):
    value, ref = inputs.receipt("run", run)
    opts = options(value["args"], ("--platform", "--config", "--physical-device"))
    need(set(opts) == {"--platform", "--config", "--physical-device"} and
         opts["--platform"] == "iphone" and opts["--config"] == "Debug" and
         re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9-]{7,79}", opts["--physical-device"]), "preparation-refused")
    details = value["details"]
    build = details["build"]
    need(details.get("normal_launch_verified") is True and "launch_arguments" not in details and
         build.get("platform") == "ios-device" and build.get("config") == "Debug" and
         build.get("app") == str(inputs.root / PHONE) and build.get("personal_preview") is False,
         "preparation-refused")
    prepared = inputs.read(inputs.root / "build/mg/run" / run / "physical/preparation.json").json()
    app = prepared["artifacts"]["products"]["app"]
    need(prepared.get("version") == 2 and prepared.get("job") == "run" and
         prepared.get("configuration") == "Debug" and prepared.get("normal_launch_verified") is True and
         prepared.get("device") == opts["--physical-device"] and app.get("bundle_id") == bundle and
         app.get("path") == build["app"] and app["signature"].get("verified") is True and
         app["signature"].get("development") is True, "preparation-refused")
    return opts["--physical-device"], ref


def verify_phone(inputs, home, stage_run, preparation_run, private):
    value, stage_ref = inputs.receipt("cloud-media-probe", stage_run)
    need(value["args"][:1] in (["stage"], ["prepare-ui"]) and len(value["args"]) == 3 and
         value["args"][1] == "--preview-build", "receipt-refused")
    source_action = value["args"][0]
    expected_outcome = "staged-only" if source_action == "stage" else "prepared-only"
    staged = value["details"]["probe"]
    build_run = run_id(value["args"][2])
    need(staged == public(expected_outcome, filename=filename(stage_run), size=SIZE,
                         sha256=staged.get("sha256"), build_receipt=staged.get("build_receipt")), "receipt-refused")
    bundle, build_ref = admit_preview(inputs, build_run, Path(private.path))
    need(staged["build_receipt"] == build_ref, "receipt-refused")
    source = inputs.read(private_path(home, stage_run) / "source.bin", SIZE)
    need(stat.S_IMODE(os.fstat(source.parent.fd).st_mode) == 0o700 and
         len(source.raw) == SIZE and digest(source.raw) == staged["sha256"], "source-refused")
    device, preparation_ref = admit_preparation(inputs, preparation_run, bundle)
    # This is the only permitted remote path. No manifest field selects it.
    remote = "Library/Application Support/Collection/collection.media/" + filename(stage_run)
    inputs.check(); private.check()
    native(Path(private.path), "phone-copy", [
        "/usr/bin/xcrun", "devicectl", "device", "copy", "from", "--device", device,
        "--source", remote, "--domain-type", "appDataContainer", "--domain-identifier", bundle,
        "--destination", str(Path(private.path) / "peer.bin"), "--timeout", "60",
        "--json-output", str(Path(private.path) / "phone-copy.json")])
    inputs.check(); private.check()
    response = inputs.read(Path(private.path) / "phone-copy.json").json()
    need(response.get("info", {}).get("outcome") == "success" and isinstance(response.get("result"), dict),
         "native-response-refused")
    peer = inputs.read(Path(private.path) / "peer.bin", SIZE)
    need(len(peer.raw) == SIZE and digest(peer.raw) == staged["sha256"] and peer.raw == source.raw,
         "peer-byte-mismatch")
    inputs.check()
    return public("peer-byte-match", filename=filename(stage_run), size=SIZE, sha256=staged["sha256"],
                  source_action=source_action, stage_receipt=stage_ref,
                  preparation_receipt=preparation_ref, build_receipt=build_ref)


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise Refused("usage-refused")


def main(argv=None):
    try:
        parser = Parser(description=__doc__)
        parser.add_argument("--root", required=True)
        parser.add_argument("--run-id", required=True)
        sub = parser.add_subparsers(dest="action", required=True, parser_class=Parser)
        for action in ("stage", "prepare-ui"):
            source_parser = sub.add_parser(action)
            source_parser.add_argument("--preview-build", required=True)
        verify_parser = sub.add_parser("verify-phone")
        verify_parser.add_argument("--stage", required=True)
        verify_parser.add_argument("--preparation", required=True)
        args = parser.parse_args(argv)
        run_id(args.run_id)
        for value in ([args.preview_build] if args.action in ("stage", "prepare-ui") else [args.stage, args.preparation]):
            run_id(value)
        # Linux/Docker cannot reach home/app data or launch any Apple tool.
        need(sys.platform == "darwin", "platform-refused")
        os.umask(0o077)
        home = pwd.getpwuid(os.getuid()).pw_dir  # Never trust caller-supplied HOME.
        with contextlib.ExitStack() as held:
            private = new_private(held, home, args.run_id)
            inputs = Inputs(held, args.root)
            if args.action == "stage":
                report = stage(inputs, held, home, args.run_id, args.preview_build, private)
            elif args.action == "prepare-ui":
                report = prepare_ui(inputs, args.run_id, args.preview_build, private)
            else:
                report = verify_phone(inputs, home, args.stage, args.preparation, private)
        print(json.dumps(report, sort_keys=True))
        return 0
    except Refused as error:
        category = str(error)
    except Exception:
        category = "input-or-filesystem-refused"
    print(json.dumps(public("failed", category=category), sort_keys=True))
    return 1


if __name__ == "__main__":
    sys.exit(main())
