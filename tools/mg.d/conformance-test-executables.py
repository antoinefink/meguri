"""Validate Cargo's complete --tests --no-run JSON stream; emit NUL paths."""
import json
import os
from pathlib import Path
import sys


def executables(stream):
    paths = []
    finished = False
    for line in stream:
        message = json.loads(line)
        if not isinstance(message, dict) or not isinstance(message.get("reason"), str):
            raise ValueError("invalid Cargo message")
        if finished:
            raise ValueError("Cargo message after build-finished")
        if message["reason"] == "build-finished":
            if message.get("success") is not True:
                raise ValueError("Cargo build did not succeed")
            finished = True
        elif message["reason"] == "compiler-artifact":
            profile = message.get("profile")
            if not isinstance(profile, dict) or type(profile.get("test")) is not bool:
                raise ValueError("missing Cargo artifact test profile")
            if not profile["test"]:
                continue
            target = message.get("target")
            if (not isinstance(target, dict) or not isinstance(target.get("name"), str)
                    or not target["name"] or not isinstance(target.get("kind"), list)
                    or not target["kind"]
                    or any(not isinstance(kind, str) or not kind for kind in target["kind"])):
                raise ValueError("invalid Cargo test target")
            path = message.get("executable")
            if (not isinstance(path, str) or not path or "\0" in path
                    or not Path(path).is_absolute() or not Path(path).is_file()
                    or not os.access(path, os.X_OK) or path in paths):
                raise ValueError("missing, invalid or duplicate Cargo test executable")
            paths.append(path)
    if not finished or not paths:
        raise ValueError("incomplete Cargo build or no test executables")
    return paths


if __name__ == "__main__":
    try:
        with open(sys.argv[1], encoding="utf-8") as stream:
            paths = executables(stream)
        # Emit only after every target and the terminal success are validated.
        sys.stdout.buffer.write(b"".join(os.fsencode(path) + b"\0" for path in paths))
    except (OSError, ValueError) as error:
        print("native testkit executables: " + str(error), file=sys.stderr)
        sys.exit(1)
