"""Manage APFS scratch for native conformance."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import signal
import stat
import subprocess
import sys


class ScratchError(Exception):
    pass


def require(ok, message):
    if not ok:
        raise ScratchError(message)


def identity(path, directory=False):
    info = path.lstat()
    require(info.st_uid == os.getuid() and
            (stat.S_ISDIR(info.st_mode) if directory else
             stat.S_ISREG(info.st_mode) and info.st_nlink == 1),
            "foreign or linked scratch resource")
    return [info.st_dev, info.st_ino, info.st_mode, info.st_uid]


def selected(root, argv):
    # Same file/suffix and literal/*/? selectors as Suite::discover. This is
    # allocation planning only; the CLI still owns parsing and diagnostics.
    if argv[:2] != ["conformance", "run"]:
        return argv[:2] in (["conformance", "exec"], ["conformance", "diff"])
    filters, tree = [], root / "conformance" / "scenarios"
    values = {"--lane", "--report", "--handoff-dir", "--oracle-cmd",
              "--meguri-edge-cmd", "--jobs", "--conformance"}
    it = iter(argv[2:])
    for arg in it:
        if arg == "--":
            filters.extend(it)
        elif arg in values:
            value = next(it)
            if arg == "--conformance":
                tree = Path(value) / "scenarios"
        elif not arg.startswith("--"):
            filters.append(arg)

    def glob(pattern, text):
        return re.fullmatch("".join(".*" if c == "*" else "." if c == "?"
                                  else re.escape(c) for c in pattern), text,
                            flags=re.DOTALL) is not None

    def walk(directory):
        if not directory.exists():
            return
        with os.scandir(directory) as entries:
            for entry in entries:
                if entry.is_dir(follow_symlinks=False):
                    yield from walk(Path(entry.path))
                elif entry.is_file(follow_symlinks=False):
                    yield Path(entry.path)

    for path in walk(tree):
        if (path.name.startswith(".") or path.name in ("area.yaml", "area.yml")
                or path.suffix not in (".yaml", ".yml", ".json")):
            continue
        full = path.relative_to(tree).as_posix()
        rel = full[:-len(path.suffix)]
        if not filters:
            return True
        for original in filters:
            f = original.strip("/")
            for suffix in (".yaml", ".yml", ".json"):
                if f.endswith(suffix):
                    f = f[:-len(suffix)]
            if rel == f or rel.startswith(f + "/") or glob(f, rel) or glob(original.strip("/"), full):
                return True
    return False


class Scratch:
    def __init__(self, root, run, run_dir, token):
        self.root, self.run, self.run_dir = Path(root), run, Path(run_dir)
        require(self.root.is_absolute() and self.root.resolve() == self.root and
                self.run_dir.parent.parent == self.root / "build/mg" and
                self.run_dir.name == run and re.fullmatch(r"[0-9a-f]{32}", token),
                "invalid scratch run location or owner")
        for directory in (self.root / "build", self.root / "build/mg",
                          self.run_dir.parent, self.run_dir):
            identity(directory, directory=True)
        self.directory = self.run_dir / "native-scratch"
        self.mount = self.directory / "mount"
        self.image = self.directory / "fixture.sparseimage"
        self.record = self.directory / "owner.json"
        self.token = token
        self.cancelled = False

    def command(self, *args, plist=False):
        result = subprocess.run(["hdiutil", *args], stdout=subprocess.PIPE, check=False)
        require(result.returncode == 0, "hdiutil " + args[0] + " failed")
        return plistlib.loads(result.stdout) if plist else None

    def save(self, owner):
        pending = self.directory / "owner.pending"
        with pending.open("x") as stream:
            json.dump(owner, stream)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(pending, self.record)

    def load(self):
        current = identity(self.directory, directory=True)
        require(stat.S_IMODE(current[2]) == 0o700, "scratch root mode changed")
        identity(self.record)
        owner = json.loads(self.record.read_text())
        require(owner["root"] == str(self.root) and owner["run"] == self.run and
                owner["directory"] == current and owner["token"] == self.token,
                "scratch owner mismatch")
        if owner["image"] is not None or self.image.exists() or self.image.is_symlink():
            require(identity(self.image) == owner["image"], "scratch image changed")
        return owner

    def attachment(self):
        images = self.command("info", "-plist", plist=True)["images"]
        matches = []
        for item in images:
            path = item.get("image-path", "")
            entities = item.get("system-entities", [])
            mounts = [e["mount-point"] for e in entities if "mount-point" in e]
            ours = bool(path) and Path(path).resolve() == self.image
            require(not any(Path(p).resolve() == self.mount for p in mounts) or ours,
                    "scratch mount belongs to another image")
            if ours:
                matches.append(item)
        require(len(matches) <= 1, "ambiguous scratch attachment")
        if not matches:
            require(not os.path.ismount(self.mount), "unidentified scratch mount")
            return None
        entities = matches[0]["system-entities"]
        mounts = [e["mount-point"] for e in entities if "mount-point" in e]
        require(all(Path(p) == self.mount for p in mounts) and len(mounts) <= 1,
                "scratch image mounted outside its run")
        devices = [e["dev-entry"] for e in entities if "dev-entry" in e]
        require(devices and all(re.fullmatch(r"/dev/disk[0-9]+(?:s[0-9]+)*", d)
                                for d in devices), "invalid scratch device")
        # Detach the image's whole disk, never a disk guessed from mount output.
        # APFS also lists a synthesized container/volume on another disk number.
        require(re.fullmatch(r"/dev/disk[0-9]+", devices[0]), "missing whole image disk")
        return devices[0], mounts

    def mounted(self):
        require(os.path.ismount(self.mount) and not self.mount.is_symlink() and
                self.mount.stat().st_dev != self.directory.stat().st_dev,
                "scratch is not a separate mounted filesystem")

    def check(self):
        owner = self.load()
        attached = self.attachment()
        require(attached is not None and attached[1] == [str(self.mount)], "scratch is not mounted")
        self.mounted()
        require(identity(self.mount / "tmp", directory=True) == owner["tmp"], "scratch tmp changed")
        identity(self.mount / ".mg-owner")
        require((self.mount / ".mg-owner").read_text() == owner["token"], "mounted owner mismatch")

    def checkpoint(self):
        require(not self.cancelled, "scratch setup cancelled")

    def prepare(self):
        # Always create a fresh owner record.
        self.directory.mkdir(mode=0o700)
        owner = dict(root=str(self.root), run=self.run, token=self.token,
                     directory=identity(self.directory, directory=True), image=None)
        self.save(owner)
        try:
            self.mount.mkdir(mode=0o700)
            owner["mount"] = identity(self.mount, directory=True)
            self.save(owner)
            self.checkpoint()
            try:
                # Logical ceiling 2 GiB; sparse backing allocates only used blocks.
                self.command("create", "-size", "2g", "-type", "SPARSE", "-fs",
                             "Case-sensitive APFS", "-volname", "mg-conformance", str(self.image))
            finally:
                if self.image.exists() or self.image.is_symlink():
                    owner["image"] = identity(self.image)
                    self.save(owner)
            self.checkpoint()
            self.command("attach", "-nobrowse", "-noautoopen", "-mountpoint", str(self.mount), str(self.image))
            self.checkpoint()
            attached = self.attachment()
            require(attached is not None and attached[1] == [str(self.mount)], "wrong scratch mount")
            self.mounted()
            (self.mount / ".mg-owner").write_text(owner["token"])
            tmp = self.mount / "tmp"
            tmp.mkdir(mode=0o700)
            owner["tmp"] = identity(tmp, directory=True)
            self.save(owner)
            # Exclusive creation must succeed twice with exact names and inodes.
            names = ("case-probe", "Case-probe")
            for name in names:
                with (tmp / name).open("x") as stream:
                    stream.write(name)
            require(sorted(os.listdir(tmp)) == sorted(names) and
                    (tmp / names[0]).stat().st_ino != (tmp / names[1]).stat().st_ino and
                    all((tmp / name).read_text() == name for name in names), "case probe collision")
            for name in names:
                (tmp / name).unlink()
            self.checkpoint()
            self.check()
        except BaseException:
            self.cleanup()
            raise

    def cleanup(self):
        if not self.directory.exists() and not self.directory.is_symlink():
            return
        owner = self.load()
        require("cleanup" not in owner, "earlier scratch cleanup incomplete; resources retained")
        require(set(os.listdir(self.directory)) <= {"owner.json", "mount", "fixture.sparseimage"},
                "unexpected scratch root entries; resources retained")
        attached = self.attachment()
        if attached is not None:
            require(owner["image"] is not None, "attached image ownership missing")
            if "tmp" in owner:
                self.check()
        # A failed/interrupted destructive cleanup is never attempted again by
        # another exit hook. Keep its owner evidence for diagnosis instead.
        owner["cleanup"] = "started"
        self.save(owner)
        if attached is not None:
            self.command("detach", attached[0])
            require(self.attachment() is None, "scratch image still attached")
        if self.mount.exists() or self.mount.is_symlink():
            require(identity(self.mount, directory=True) == owner["mount"], "underlying mount directory changed")
            self.mount.rmdir()
        if self.image.exists():
            require(identity(self.image) == owner["image"], "scratch image changed")
            self.image.unlink()
        self.record.unlink()
        self.directory.rmdir()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ("root", "run", "run-dir"):
        parser.add_argument("--" + key, required=True)
    parser.add_argument("--owner", default="")
    parser.add_argument("action", choices=("selected", "prepare", "check", "cleanup"))
    parser.add_argument("argv", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.action == "selected":
        print("true" if selected(Path(args.root), args.argv) else "false")
        return
    scratch = Scratch(args.root, args.run, args.run_dir, args.owner)
    # Let an in-flight hdiutil finish/reap after MG sends TERM to the process
    # tree, then clean using its actual attachment state, including failed attach.
    for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(sig, lambda *_: setattr(scratch, "cancelled", True))
    getattr(scratch, args.action)()


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print("native conformance scratch: " + str(error), file=sys.stderr)
        sys.exit(1)
