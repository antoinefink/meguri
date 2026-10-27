#!/usr/bin/env python3
import glob
import json
import math
import os
import plistlib
import re
import secrets
import stat
import subprocess
import sys
from contextlib import contextmanager
from pathlib import Path

# Reuse the retained descriptor and exclusive-copy authority of F11.1 without
# changing its selector, fixture baseline, attachment manifest or proof scope.
import importlib.util
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location("native_export", Path(__file__).with_name("native-media-export-capture.py"))
export = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(export)
ROOT = export.ROOT
METHOD = "CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExport()"
MEDIA = ["_meguri-unused.txt", "meguri-review.png", "meguri-review.wav"]
PHASES = ["import", "accepted", "closed", "reopened"]
require = export.require
identity = export.file_identity
read = export.read_record
COMPILED = Path("/cargo-target/debug/meguri-cli")
CODE_LIMIT = 512 * 1024 * 1024


@contextmanager
def owned_directory(path):
    require(path.is_relative_to(ROOT), "owned directory scope")
    with export.retained_directory(path) as (directory, verify):
        def check():
            verify()
            # Retain/check every admitted ancestor, including the checkout.
            for current in (path, *path.parents):
                if not current.is_relative_to(ROOT):
                    break
                info = current.lstat()
                require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
                        and info.st_mode & 0o022 == 0, "owned directory owner/mode")
            verify()
        check()
        yield directory, check
        check()


def fresh_directory(path):
    with owned_directory(path.parent) as (parent, verify):
        os.mkdir(path.name, mode=0o700, dir_fd=parent)
        with owned_directory(path) as (directory, check):
            require(not os.listdir(directory), "fresh empty directory")
            check()
        verify()


def decoder_root(run):
    # Empty mount points only. The actual generated parent and output leaf
    # replace these two leaves; no input bytes or checkout enter this tree.
    with owned_directory(run / "source-preparation") as (directory, _):
        require(not os.listdir(directory), "fresh preparation output")
    with owned_directory(ROOT / "build/fixtures/recipes/local-review-1"):
        pass
    skeleton = run / "decoder-root"
    for name in ("", "build", "build/fixtures", "build/fixtures/recipes",
                 "build/fixtures/recipes/local-review-1", "build/mg", "build/mg/ui-test",
                 "build/mg/ui-test/" + run.name,
                 "build/mg/ui-test/" + run.name + "/source-preparation"):
        fresh_directory(skeleton / name)
    fresh_directory(run / "empty-cargo-home")


def write(path, value):
    with owned_directory(path.parent) as (directory, _):
        fd = os.open(path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o400, dir_fd=directory)
        with os.fdopen(fd, "w") as stream:
            json.dump(value, stream, sort_keys=True, separators=(",", ":"), allow_nan=False)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())


def source_files():
    # Current own source only; no history, external source or private roots.
    paths = subprocess.check_output(["git", "-C", str(ROOT), "ls-files", "-z", "--", "app", "core", "tools/mg.d/uitest.sh",
                                     "tools/scripts/native-media-review-capture.py", "tools/scripts/native-media-export-capture.py",
                                     "tools/scripts/package-interop-check.py"]).split(b"\0")
    files = {}
    for raw in paths:
        if not raw:
            continue
        name = os.fsdecode(raw)
        if name.startswith("app/Snapshots/") or "/vendor/" in name:
            continue
        path = ROOT / name
        if path.is_file():
            files[name] = identity(path)
    # Include this proposal before it is added to the integrator's index.
    for name in ["tools/scripts/native-media-review-capture.py"]:
        files[name] = identity(ROOT / name)
    require(files and "core/crates/meguri-testkit/src/native_review.rs" in files, "own source inventory")
    return files


def revision():
    value = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
    require(re.fullmatch(r"[a-f0-9]{40}", value), "current revision")
    return value


def source_start(run, platform):
    require(platform in ("mac", "iphone", "ipad"), "platform")
    require(not (run / "native-review-source.json").exists(), "fresh native case required")
    write(run / "native-review-source.json", {"revision": revision(), "platform": platform, "files": source_files()})
    fresh_directory(run / "source-preparation")
    fresh_directory(run / "native-review-export")


def source_unchanged(run):
    source = read(run / "native-review-source.json")
    require(source == {"revision": revision(), "platform": source["platform"], "files": source_files()}, "source changed")
    return source


def admitted(root, relative):
    require(isinstance(relative, str), "input path")
    parts = relative.split("/")
    require(parts and all(p and p not in (".", "..", "refs", "oracle", "local-data")
                          and not p.startswith("similarity") for p in parts), "input path scope")
    path = root / relative
    require(path.is_absolute() and path.resolve(strict=True) == path, "input path links")
    return path


def frozen(run):
    relative = run.relative_to(ROOT).as_posix()
    expected = run / "expected-replay"
    header = read(expected / "freeze.json")
    require(header["schema_version"] == 1 and header["run"] == relative and header["stage"] == "expected-states-before-app", "freeze header")
    names = ["input.json", "input.colpkg", "replay.json"]
    for phase in ("source", "import", "review"):
        names += [phase + "/collection.anki2", phase + "/manifest.json"]
        names += [phase + "/collection.media/" + n for n in MEDIA]
    require(set(header["files"]) == set(names) and len(names) == 18, "frozen file set")
    entries = set(os.listdir(expected))
    artifacts = {"input.json", "input.colpkg", "replay.json", "freeze.json", "source", "import", "review"}
    require(entries in (artifacts, artifacts | {"work"}), "expected artifact layout")
    if "work" in entries:
        # Rust retains its scratch directory; it is never an expected input.
        with export.retained_directory(expected / "work") as (directory, verify):
            metadata = os.fstat(directory)
            require(metadata.st_uid == os.getuid() and metadata.st_mode & 0o022 == 0, "runtime directory owner/mode")
            verify()
    require(set(os.listdir(run / "source-preparation")) == {"fixture", "preparation.json"}, "preparation layout")
    for phase in ("source", "import", "review"):
        require(set(os.listdir(expected / phase)) == {"collection.anki2", "manifest.json", "collection.media"}
                and set(os.listdir(expected / phase / "collection.media")) == set(MEDIA), "expected state layout")
    empty = admitted(ROOT, "app/Fixtures/CollectionEntry/EmptyCollection")
    require(set(os.listdir(empty)) == {".gitkeep"} and identity(empty / ".gitkeep")["bytes"] == 0, "sole empty bootstrap marker")
    files = {"app/Fixtures/CollectionEntry/EmptyCollection/.gitkeep": identity(empty / ".gitkeep")}
    for name in names:
        path = admitted(expected, name)
        require(identity(path) == header["files"][name], "frozen file changed")
        files[path.relative_to(ROOT).as_posix()] = identity(path)
    files[(expected / "freeze.json").relative_to(ROOT).as_posix()] = identity(expected / "freeze.json")
    value = read(expected / "input.json")
    require(value["run"] == relative and value["platform"] == header["platform"] and value["code"]["build_profile"] == "dev"
            and value["code"]["revision"] == revision(), "input metadata/code")
    require(value["preparation"]["path"] == relative + "/source-preparation/preparation.json"
            and value["prepared_source"] == relative + "/source-preparation/fixture"
            and value["parent_source"] == "build/fixtures/recipes/local-review-1"
            and value["package"]["path"] == relative + "/expected-replay/input.colpkg", "source parents")
    require(value["seeds"] == {"source_open": 101, "package_write": 102, "inspection_and_initial_activation": 103, "reopen": 104}, "declared runtime seeds")
    require(value["answer"]["rating"] == "easy" and value["answer"]["elapsed_ms"] == 1000 and value["answer"]["count"] == 1
            and value["answer"]["preset_cap_answer_time_secs"] * 1000 >= 1000 and value["no_old_database"] is True, "answer input")
    for field in ("parent", "prepared"):
        root = admitted(ROOT, value[field + "_source"])
        required = {"collection.anki2", "manifest.json"} | ({"collection.media/" + n for n in MEDIA} if field == "prepared" else set())
        require(set(value[field + "_files"]) == required, "source file set")
        require(set(os.listdir(root)) == {"collection.anki2", "manifest.json", "collection.media"}, "source layout")
        require(set(os.listdir(root / "collection.media")) == (set(MEDIA) if field == "prepared" else set()), "source media set")
        for name, wanted in value[field + "_files"].items():
            path = admitted(root, name)
            require(identity(path) == wanted, "source parent changed")
            files[path.relative_to(ROOT).as_posix()] = wanted
    prep = admitted(ROOT, value["preparation"]["path"])
    require(identity(prep) == value["preparation"]["identity"], "source preparation changed")
    files[prep.relative_to(ROOT).as_posix()] = identity(prep)
    require(value["code"]["replay_source"] == identity(ROOT / "core/crates/meguri-testkit/src/native_review.rs"), "Rust source identity")
    return value, files


def app_files(bundle, target):
    if target == "mac":
        product = "Debug"
    else:
        product = "Debug-iphonesimulator"
    require(
        bundle == ROOT / "build/app/DerivedData/Build/Products" / product / "Meguri.app",
        "built app path",
    )
    folder = bundle / "Contents/MacOS" if target == "mac" else bundle
    info = bundle / "Contents/Info.plist" if target == "mac" else bundle / "Info.plist"
    with info.open("rb") as stream:
        data = plistlib.load(stream)
    require(data["CFBundleIdentifier"] == "com.antoinefink.meguri.dev", "development app")
    executable = data["CFBundleExecutable"]
    require(isinstance(executable, str) and re.fullmatch(r"[A-Za-z0-9._-]+", executable), "app executable")
    names = sorted(
        name for name in os.listdir(folder)
        if name == executable or name.endswith(".dylib")
    )
    require(executable in names and names, "missing app executable")
    return {name: identity(folder / name) for name in names}


def bind(run, app, configuration):
    source = source_unchanged(run)
    require(configuration == "Debug", "native review requires Debug/dev")
    value, files = frozen(run)
    require(value["platform"] == source["platform"], "platform binding")
    require(read(run / "native-review-rust.json") == value["code"], "actual Rust executable binding")
    published_code(run, value["code"])
    stamp = ROOT / "app/Packages/MeguriCore/Binaries/.stamp"
    lines = stamp.read_text().splitlines()
    require(all(s in lines for s in ("configuration=Debug", "profile=dev", "features=ankiweb,test-hooks")), "native Rust configuration")
    write(run / "native-review-binding.json", {"version": 1, "case": run.name, "run": value["run"],
        "platform": value["platform"], "configuration": configuration, "revision": source["revision"],
        "source_identity": identity(run / "native-review-source.json"), "freeze": identity(run / "expected-replay/freeze.json"),
        "input": identity(run / "expected-replay/input.json"), "files": files, "app": str(app), "mach_o": app_files(app, value["platform"]),
        "core_stamp": identity(stamp), "rust": value["code"],
        "native_filename": "Meguri-Media-" + secrets.token_hex(16) + ".colpkg"})


def recheck(run):
    source_unchanged(run)
    value, files = frozen(run)
    binding = read(run / "native-review-binding.json")
    published_code(run, value["code"])
    require(binding["version"] == 1 and binding["case"] == run.name and binding["run"] == value["run"]
            and binding["platform"] == value["platform"] and binding["configuration"] == "Debug"
            and binding["revision"] == revision(), "caller run/config binding")
    export.package_name(binding["native_filename"])
    require(binding["files"] == files and binding["freeze"] == identity(run / "expected-replay/freeze.json")
            and binding["input"] == identity(run / "expected-replay/input.json")
            and binding["source_identity"] == identity(run / "native-review-source.json")
            and binding["rust"] == value["code"] == read(run / "native-review-rust.json")
            and binding["mach_o"] == app_files(Path(binding["app"]), binding["platform"])
            and binding["core_stamp"] == identity(ROOT / "app/Packages/MeguriCore/Binaries/.stamp"), "native binding changed")
    return value, binding


def code_signature(info):
    return [*export.node_identity(info), info.st_uid, info.st_gid,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns]


def code_hash(fd):
    # Bound reads even if a compiler output grows while being inspected.
    import hashlib
    digest, size = hashlib.sha256(), 0
    os.lseek(fd, 0, os.SEEK_SET)
    while True:
        block = os.read(fd, min(1024 * 1024, CODE_LIMIT + 1 - size))
        if not block:
            break
        size += len(block)
        require(size <= CODE_LIMIT, "dev executable size")
        digest.update(block)
    return {"sha256": digest.hexdigest(), "bytes": size}


@contextmanager
def compiled_code():
    # Read-only compiler output may have Cargo's deps/ hardlink. This authority
    # is used only for code; input/capture reads retain their one-link guard.
    with export.retained_directory(COMPILED.parent) as (directory, verify_directory):
        fd = os.open(COMPILED.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        try:
            before = os.fstat(fd)
            require(stat.S_ISREG(before.st_mode) and 0 < before.st_size <= CODE_LIMIT
                    and before.st_mode & 0o022 == 0 and before.st_mode & 0o111, "dev executable bound")
            pin = code_signature(before)
            def verify():
                require(pin == code_signature(os.fstat(fd))
                        == code_signature(os.stat(COMPILED.name, dir_fd=directory, follow_symlinks=False)),
                        "dev executable changed")
                verify_directory()
            actual = code_hash(fd)
            verify()
            require(actual["bytes"] == before.st_size, "dev executable size changed")
            yield fd, verify, pin, actual
            verify()
        finally:
            os.close(fd)


def publish_code(run, source_fd, verify_source, pin, actual):
    directory = run / "run-code"
    fresh_directory(directory)
    binary = directory / "meguri-cli"
    with owned_directory(directory) as (parent, verify_directory):
        fd = os.open(binary.name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
        try:
            os.lseek(source_fd, 0, os.SEEK_SET)
            remaining = actual["bytes"]
            while remaining:
                block = os.read(source_fd, min(1024 * 1024, remaining))
                require(block, "compiled code truncated")
                remaining -= len(block)
                pending = memoryview(block)
                while pending:
                    count = os.write(fd, pending)
                    require(count > 0, "code write progress")
                    pending = pending[count:]
            require(not os.read(source_fd, 1), "compiled code grew")
            verify_source()
            require(code_hash(source_fd) == actual, "compiled code hash changed")
            verify_source()
            os.fsync(fd)
            os.fchmod(fd, 0o500)
            before = export.file_metadata(os.fstat(fd))
            require(code_hash(fd) == actual and before == export.file_metadata(os.fstat(fd))
                    == export.file_metadata(os.stat(binary.name, dir_fd=parent, follow_symlinks=False)), "published code changed")
            verify_directory()
        finally:
            os.close(fd)
    write(run / "native-review-code.json", {"version": 1, "run": run.relative_to(ROOT).as_posix(),
          "revision": revision(), "build_profile": "dev", "compiler": str(COMPILED), "compiler_pin": pin,
          "compiled": actual, "published": binary.relative_to(ROOT).as_posix(), "executable": actual})
    return binary


def published_code(run, code):
    record = read(run / "native-review-code.json")
    binary = run / "run-code/meguri-cli"
    require(record["version"] == 1 and record["run"] == run.relative_to(ROOT).as_posix()
            and record["revision"] == code["revision"] == revision()
            and record["build_profile"] == code["build_profile"] == "dev"
            and record["compiler"] == str(COMPILED) and len(record["compiler_pin"]) == 9
            and record["published"] == binary.relative_to(ROOT).as_posix(), "published code binding")
    with owned_directory(binary.parent) as (directory, _):
        with export.retained_file(directory, binary.name) as (fd, verify):
            info = os.fstat(fd)
            require(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o500, "published code owner/mode")
            require(record["compiled"] == record["executable"] == code["executable"] == code_hash(fd), "published compiler hash")
            verify()
    return record


def rust_identity(run):
    require(ROOT == Path("/work"), "Rust identity runs in dev")
    value = read(run / "expected-replay/input.json")
    record = published_code(run, value["code"])
    with compiled_code() as (_, _, pin, actual):
        require(record["compiler_pin"] == pin and record["compiled"] == actual, "compiler identity changed")
    write(run / "native-review-rust.json", value["code"])


def derive(run, platform, current_revision):
    require(ROOT == Path("/work") and platform in ("mac", "iphone", "ipad")
            and current_revision == revision(), "dev derivation arguments")
    with compiled_code() as (fd, verify, pin, actual):
        binary = publish_code(run, fd, verify, pin, actual)
        code = {"revision": current_revision, "build_profile": "dev", "executable": actual}
        published_code(run, code)
        with owned_directory(binary.parent) as (directory, _):
            with export.retained_file(directory, binary.name) as (published, verify_published):
                require(code_hash(published) == actual, "pre-derive executable hash")
                verify_published()
                verify()
                subprocess.run([str(binary), "internal-native-review", str(ROOT), run.name, platform,
                                current_revision, "101", "102", "103", "104"], check=True)
                verify_published()
                require(code_hash(published) == actual, "post-derive executable hash")
                verify_published()
        verify()
        require(code_hash(fd) == actual, "compiler hash after derive")
        verify()
        rust_identity(run)


def sim_capture(run, container, device):
    binding = read(run / "native-review-binding.json")
    filename = export.package_name(binding["native_filename"])
    source = export.simulator_source(container, device, filename)
    destination = run / "native-review-export"
    export.copy_package(source, destination / filename)
    receipt_root = Path(container) / "Library/Application Support/test-evidence/native-review" / run.name / binding["platform"]
    require(binding["platform"] in ("iphone", "ipad"), "simulator platform")
    for phase in PHASES:
        export.copy_package(receipt_root / (phase + ".json"), destination / ("native-review-" + phase + ".json"))


def mac_capture(run, bundle, attestation_only=False):
    binding = read(run / "native-review-binding.json")
    filename = export.package_name(binding["native_filename"])
    require(bundle == run / (binding["platform"] + ".xcresult"), "own result path")
    require(attestation_only or binding["platform"] == "mac", "Mac capture platform")
    # Selectively export only this method's six owned filenames. No screenshots,
    # hierarchies, other test methods or surrounding app-container enumeration.
    names = ["native-review-app.json"] if attestation_only else [filename, "native-review-app.json"] + ["native-review-" + p + ".json" for p in PHASES]
    for name in names:
        directory = run / ("native-review-attachments-" + Path(name).stem)
        directory.mkdir(mode=0o700)
        # XCTest inserts _0_UUID before the extension; the filter is a glob
        # over that human-readable name, independent of exportedFileName.
        selector = glob.escape(Path(name).stem) + "_0_*" + glob.escape(Path(name).suffix)
        subprocess.run(["xcrun", "xcresulttool", "export", "attachments", "--path", str(bundle), "--output-path", str(directory),
                        "--test-id", METHOD, "--filter", selector], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        with export.retained_directory(directory) as (fd, verify_directory):
            manifest = read(directory / "manifest.json")
            require(isinstance(manifest, list) and len(manifest) == 1 and manifest[0].get("testIdentifier") == METHOD, "attachment method")
            attachments = manifest[0].get("attachments")
            require(isinstance(attachments, list) and len(attachments) == 1, "one named attachment")
            entry = attachments[0]
            basename = entry.get("exportedFileName")
            extension = re.escape(Path(name).suffix)
            require(isinstance(basename, str) and re.fullmatch(export.UUID + extension, basename), "attachment basename")
            require(entry.get("isAssociatedWithFailure") is False and
                    re.fullmatch(re.escape(Path(name).stem) + "_0_" + export.UUID + extension,
                                 entry.get("suggestedHumanReadableName", "")), "attachment name/failure")
            require(set(os.listdir(fd)) == {"manifest.json", basename}, "selective export set")
            export.copy_package(directory / basename, run / "native-review-export" / name)
            verify_directory()
    require(read(run / "native-review-export/native-review-app.json") == binding, "XCTest actual built app binding")


def finite(value):
    return type(value) in (int, float) and math.isfinite(value)


def proof_receipts(run):
    value, binding = recheck(run)
    output = run / "native-review-export"
    require(read(output / "native-review-app.json") == binding, "actual built app attestation")
    receipts = {p: read(output / ("native-review-" + p + ".json")) for p in PHASES}
    imported, accepted, closed, reopened = (receipts[p] for p in PHASES)
    for phase, record in receipts.items():
        require(record["version"] == 1 and record["run"] == value["run"] and record["platform"] == value["platform"]
                and record["freezeIdentity"] == binding["freeze"] and record["input"] == binding["input"]
                and record.get("failure") is None, "receipt identity/failure")
        require(record["phase"] == {"import": "activated", "accepted": "accepted", "closed": "closed", "reopened": "reopened"}[phase], "actual phase")
        require(record["seeds"] == ({"inspection_and_initial_activation": 103, "reopen": 104} if phase == "reopened"
                                    else {"inspection_and_initial_activation": 103}), "actual seed consumers")
        inspection = record["inspection"]
        source_facts = read(run / "source-preparation/preparation.json")["source_facts"]
        require(inspection["sha256"] == value["package"]["identity"]["sha256"] and inspection["notes"] == 1 and inspection["cards"] == 1
                and inspection["reviews"] == source_facts["row_counts"]["revlog"] and inspection["replacing"] is False
                and inspection["mediaCount"] == 3 and inspection["mediaBytes"] == source_facts["media_bytes"], "actual inspection")
        media = record["activation"]["media"]
        require(media == {"incoming": 3, "added": 3, "reused": 0, "trashed": 0, "decodedBytes": source_facts["media_bytes"]}
                and record["prepared"] == media and record["activation"].get("backup") is None
                and record["activation"].get("retention") == read(run / "expected-replay/replay.json").get("media_retention_reference"), "actual P16 outcome")
        journal = record["journal"]
        require(journal["generation"] == record["activation"]["generation"] and journal["generation"] > 0
                and journal["root"] == record["collection"] + "/collection.media"
                and [n["name"] for n in journal["names"]] == MEDIA and len({n["caseKey"] for n in journal["names"]}) == 3
                and all(n["caseKey"] for n in journal["names"]), "actual initialized journal")
        require(record["collection"] == imported["collection"], "same installed collection")
    require(imported.get("admission") is None and imported.get("completion") is None and imported["ended"] is False, "import precedes answer")
    require(imported["owner"] == accepted["owner"] == closed["owner"] and reopened["owner"] != closed["owner"], "fresh reopened owner")
    admission = accepted["admission"]
    require(all(r["admission"] == admission and r["completion"] == "accepted" for r in (accepted, closed, reopened)), "separate Accepted completion")
    require(admission["owner"] == accepted["owner"] and admission["deck"] == value["chosen"]["deck_id"]
            and admission["card"] == value["chosen"]["card_id"] and admission["rating"] == "easy" and admission["elapsed"] == 1000
            and admission["wall"] == value["wall"]["now_ms"] and admission["cap"] == value["answer"]["preset_cap_answer_time_secs"]
            and admission["elapsed"] <= admission["cap"] * 1000 and len(admission["token"]) == 32
            and all(type(n) is int and 0 <= n <= 255 for n in admission["token"])
            and admission["generation"] == accepted["activation"]["generation"], "actual frozen answer values/token/cap")
    require(accepted["ended"] is False and closed["ended"] is True and reopened["ended"] is True, "normal session end")
    require(accepted["selectedDeck"] == value["chosen"]["deck_id"] and accepted["selectedDeckName"] == value["chosen"]["deck_name"]
            and 1 <= accepted["selectionCalls"] <= 8 and closed["selectionCalls"] == reopened["selectionCalls"] == accepted["selectionCalls"], "one study selection")
    image, audio = accepted["image"], accepted["audio"]
    require(image["binding"] == audio["binding"], "same actual renderer visit")
    render = image["binding"]
    require(render["coordinator"] == admission["coordinator"] and render["session"] == admission["session"]
            and render["generation"] == admission["generation"] and render["card"] == admission["card"]
            and render["update"] > 0 and render["side"] == "answer" and re.fullmatch(export.UUID, render["document"]), "current card/update/document binding")
    require(image["name"] == "meguri-review.png" and image["width"] == image["height"] == 4
            and all(finite(image[n]) for n in ("x", "y", "displayWidth", "displayHeight"))
            and image["displayWidth"] > 0 and image["displayHeight"] > 0, "real PNG decode/display")
    require(audio["name"] == "meguri-review.wav" and audio["index"] == 0 and audio["success"] is True and audio["samples"] >= 2
            and audio["duration"] == 2 and all(finite(audio[n]) for n in ("firstPosition", "lastPosition", "firstObservedAt", "lastObservedAt", "completedAt"))
            and 0 < audio["firstPosition"] < .25 and 1.75 < audio["lastPosition"] <= 2
            and audio["firstObservedAt"] < audio["lastObservedAt"] < audio["completedAt"]
            and audio["completedAt"] - audio["firstObservedAt"] >= 1.5
            and abs((audio["lastPosition"] - audio["firstPosition"]) - (audio["lastObservedAt"] - audio["firstObservedAt"])) < .5, "native natural WAV playback")
    for record in (closed, reopened):
        require(record["image"] == image and record["audio"] == audio, "media evidence retained")
    files = closed["closedFiles"]
    require(set(files) == {"collection.anki2"} | {"collection.media/" + n for n in MEDIA}, "closed DB and all media")
    for name in MEDIA:
        require(files["collection.media/" + name] == value["prepared_files"]["collection.media/" + name], "closed media bytes")
    require(reopened.get("closedFiles") is None and reopened["reopenedJournal"]["root"] == reopened["collection"] + "/collection.media"
            and reopened["reopenedJournal"]["generation"] > 0
            and reopened["reopenedJournal"]["names"] == imported["journal"]["names"], "no-reset reopened media")
    package = output / export.package_name(binding["native_filename"])
    write(run / "native-review-capture.json", {"version": 1, "binding": identity(run / "native-review-binding.json"),
        "package": identity(package), "receipts": {p: identity(output / ("native-review-" + p + ".json")) for p in PHASES},
        "expected": value["run"] + "/expected-replay/review", "external_content_client_proof": "pending"})


def proof_finish(run, result_path):
    _, binding = recheck(run)
    result = read(result_path)
    expected_args = ["--package", str(run / "native-review-export" / binding["native_filename"]),
        "--expected", str(run / "expected-replay/review"), "--now", "2030-01-15T10:00:00+01:00", "--tz", "Europe/Paris"]
    require(result_path.parent.parent == ROOT / "build/mg/package-interop-check" and result_path.name == "result.json"
            and result["job"] == "package-interop-check" and result["status"] == "pass" and result["dry_run"] is False
            and result["args"] == expected_args and result["commit"]["sha"] == binding["revision"]
            and not result["failures"] and not result["user_steps"], "strict independent package/client proof")
    capture = read(run / "native-review-capture.json")
    require(capture["package"] == identity(run / "native-review-export" / binding["native_filename"]), "saved native package changed")
    for phase in PHASES:
        require(capture["receipts"][phase] == identity(run / "native-review-export" / ("native-review-" + phase + ".json")), "phase receipt changed")
    write(run / "native-review-proof.json", dict(capture, external_content_client_proof="passed",
        package_interop_result=str(result_path.relative_to(ROOT)), package_interop_identity=identity(result_path),
        scope="one F11.2 native case; root verification and all milestone gates remain required"))


def main():
    action, value, *args = sys.argv[1:]
    run = export.own_run(value)
    if action == "start":
        platform, = args
        source_start(run, platform)
    elif action == "decoder-root":
        require(not args, "decoder arguments")
        decoder_root(run)
    elif action == "derive":
        platform, current_revision = args
        derive(run, platform, current_revision)
    elif action == "rust-code":
        require(not args, "Rust arguments")
        rust_identity(run)
    elif action == "bind":
        app, configuration = args
        bind(run, Path(app), configuration)
    elif action == "stage-paths":
        require(not args, "stage arguments")
        _, binding = recheck(run)
        prefix = "build/mg/ui-test/" + run.name
        paths = {prefix + "/expected-replay", prefix + "/source-preparation", "build/fixtures/recipes/local-review-1", prefix + "/native-review-binding.json", "app/Fixtures/CollectionEntry/EmptyCollection"}
        # Staging preserves the admitted absolute checkout paths for all frozen
        # files, source-preparation inputs and the actual generated parent.
        for relative in sorted(paths):
            print(admitted(ROOT, relative))
    elif action == "simulator":
        sim_capture(run, *args)
    elif action == "mac":
        bundle, = args
        mac_capture(run, Path(bundle))
    elif action == "attestation":
        bundle, = args
        mac_capture(run, Path(bundle), attestation_only=True)
    elif action == "receipts":
        require(not args, "receipt arguments")
        proof_receipts(run)
    elif action == "proof":
        result, = args
        proof_finish(run, Path(result))
    else:
        raise ValueError("unknown native review operation")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        sys.exit("FIXED-PHASE:native-review")
