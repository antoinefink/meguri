#!/usr/bin/env python3
"""Copy tracked, permitted regular files; never traverse a forbidden tree."""

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess


ROOTS = {"core", "app", "specs", "conformance", "docs", "tools", "docker"}
ROOT_FILES = {"AGENTS.md", ".gitignore", ".gitattributes"}
FORBIDDEN = {"oracle", "refs", "local-data", "build", ".git", ".codex", ".claude"}


def permitted(name: str) -> bool:
    path = PurePosixPath(name)
    return (
        bool(path.parts)
        and not path.is_absolute()
        and ".." not in path.parts
        and not FORBIDDEN.intersection(path.parts)
        and (path.parts[0] in ROOTS or name in ROOT_FILES)
    )


def regular_file(root: Path, name: str) -> Path:
    current = root
    for part in PurePosixPath(name).parts:
        current /= part
        mode = current.lstat().st_mode
        if stat.S_ISLNK(mode):
            raise ValueError("source view refuses symbolic links")
    info = current.stat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError("source view requires regular, unlinked files")
    return current


def prepare(root: Path, target: Path) -> dict:
    if target.exists():
        raise ValueError("source view destination already exists")
    output = subprocess.check_output(["git", "-C", str(root), "ls-files", "--stage", "-z"])
    entries = []
    for row in output.split(b"\0"):
        if not row:
            continue
        metadata, raw_name = row.split(b"\t", 1)
        name = os.fsdecode(raw_name)
        if not permitted(name):
            continue
        mode, _, stage = metadata.split()
        if mode not in (b"100644", b"100755") or stage != b"0":
            raise ValueError("source view refuses links, submodules and unresolved entries")
        entries.append((name, regular_file(root, name)))
    target.mkdir()
    for name, source in entries:
        dest = target / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, dest)
        dest.chmod(source.stat().st_mode & 0o777)
    return {"files": len(entries), "excluded": sorted(FORBIDDEN), "real_history": False}


def validate(root: Path) -> None:
    def walk_error(error: OSError) -> None:
        raise error

    for directory, dirs, files in os.walk(root, followlinks=False, onerror=walk_error):
        for entry in dirs + files:
            path = Path(directory) / entry
            name = path.relative_to(root).as_posix()
            if path.is_symlink():
                raise ValueError("runner produced a symbolic link")
            if entry in files:
                if not permitted(name):
                    raise ValueError("runner produced a forbidden output path")
                regular_file(root, name)


def seed(root: Path, patch: Path) -> None:
    """Apply reviewed Meguri work, refusing unsafe output before model access."""
    command = ["git", "-C", str(root), "apply"]
    subprocess.run(command + ["--check", str(patch)], check=True)
    subprocess.run(command + [str(patch)], check=True)
    validate(root)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["prepare", "validate", "seed"])
    parser.add_argument("root", type=Path)
    parser.add_argument("target", type=Path, nargs="?")
    args = parser.parse_args()
    if args.action == "prepare":
        if args.target is None:
            parser.error("prepare needs a destination")
        print(json.dumps(prepare(args.root, args.target)))
    elif args.action == "seed":
        if args.target is None:
            parser.error("seed needs a patch")
        seed(args.root, args.target)
    else:
        validate(args.root)
