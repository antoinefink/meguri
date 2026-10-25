"""Trusted launch records used by MG's parent route cleanup."""
import argparse
import json
import os
import pathlib
import re
import sys
import uuid


class ContextError(Exception):
    pass


def location(root, run, run_dir):
    root = pathlib.Path(root).resolve()
    run_dir = pathlib.Path(run_dir).resolve()
    if run_dir.parent.parent != root / "build/mg" or run_dir.name != run:
        raise ContextError("invalid route context directory")
    return str(root), run_dir / "edge-owners"


def validate(record, root, project, run, owner):
    if (not isinstance(record, dict)
            or set(record) != {"version", "root", "project", "run", "owner", "image", "faults"}
            or record["version"] != 1
            or (record["root"], record["project"], record["run"], record["owner"]) != (root, project, run, owner)
            or not re.fullmatch(r"[0-9a-f]{32}", owner)
            or any(not isinstance(record[key], str) or not record[key]
                   or any(ord(c) < 32 or ord(c) == 127 for c in record[key])
                   for key in ("root", "project", "run", "owner", "image", "faults"))):
        raise ContextError("invalid route owner context")
    return record


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def register(root, project, run, run_dir, owner, image, faults):
    root, directory = location(root, run, run_dir)
    record = validate(dict(version=1, root=root, project=project, run=run,
                           owner=owner, image=image, faults=faults), root, project, run, owner)
    directory.mkdir(mode=0o700, exist_ok=True)
    if directory.is_symlink():
        raise ContextError("invalid route context directory")
    path = directory / (owner + ".json")
    pending = directory / ("." + uuid.uuid4().hex + ".tmp")
    try:
        fd = os.open(pending, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as stream:
            json.dump(record, stream, separators=(",", ":"))
            stream.flush()
            os.fsync(stream.fileno())
        try:
            os.link(pending, path)
        except FileExistsError:
            if path.is_symlink() or json.loads(path.read_text()) != record:
                raise ContextError("route owner context conflict")
        sync_directory(directory)
        sync_directory(directory.parent)
        sync_directory(directory.parent.parent)
    finally:
        pending.unlink(missing_ok=True)


def records(root, project, run, run_dir):
    root, directory = location(root, run, run_dir)
    if directory.is_symlink():
        raise ContextError("invalid route context directory")
    if not directory.exists():
        return []
    if not directory.is_dir():
        raise ContextError("invalid route context directory")
    result = []
    for path in sorted(directory.glob("*.json")):
        if not path.is_file() or path.is_symlink():
            raise ContextError("invalid route owner context")
        result.append(validate(json.loads(path.read_text()), root, project, run, path.stem))
    return result


def main():
    parser = argparse.ArgumentParser()
    for key in ("root", "project", "run", "run-dir"):
        parser.add_argument("--" + key, required=True)
    parser.add_argument("--list", action="store_true")
    for key in ("owner", "image", "faults"):
        parser.add_argument("--" + key)
    args = parser.parse_args()
    if args.list:
        # Validate the complete set before passing any authority to the shell.
        trusted = records(args.root, args.project, args.run, args.run_dir)
        for record in trusted:
            print("\t".join(record[key] for key in ("owner", "image", "faults")))
    else:
        register(args.root, args.project, args.run, args.run_dir, args.owner, args.image, args.faults)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("route owner context unavailable; reservations retained", file=sys.stderr)
        sys.exit(1)
