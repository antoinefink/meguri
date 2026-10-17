#!/usr/bin/env python3
"""Scans the object files of a Rust library (.rlib) for the word "ankiweb" in
any letter case (KTD21; specs/sync/login-endpoints.md L3).

tools/mg core-lint builds meguri-sync without its `ankiweb` and `test-hooks`
features and runs this on the library: the code a build without the switch
links must hold no AnkiWeb name, debug information included. The crate's
metadata member (lib.rmeta) is not linked into a binary and is not scanned.

Usage: ankiweb-scan.py <library.rlib>
Prints one JSON line per hit ({member, offset, text}) and exits 1 when there
is one, 2 when the archive cannot be read, 0 otherwise.
"""

import json
import sys

NEEDLE = b"ankiweb"


def members(data):
    """(name, offset of the body, body) for each member of a GNU or BSD ar
    archive, symbol tables and the GNU name table left out."""
    if not data.startswith(b"!<arch>\n"):
        raise ValueError("not an ar archive")
    pos = 8
    long_names = b""
    while pos + 60 <= len(data):
        header = data[pos : pos + 60]
        raw_name = header[:16].decode("ascii", "replace").rstrip()
        size = int(header[48:58].decode("ascii").strip())
        start = pos + 60
        body = data[start : start + size]
        pos = start + size + (size % 2)
        if raw_name == "//":
            long_names = body
        elif raw_name in ("/", "/SYM64/", "__.SYMDEF", "__.SYMDEF SORTED"):
            continue
        elif raw_name.startswith("#1/"):
            length = int(raw_name[3:])
            name = body[:length].rstrip(b"\0").decode("utf-8", "replace")
            yield name, start + length, body[length:]
        elif raw_name.startswith("/") and raw_name[1:].isdigit():
            offset = int(raw_name[1:])
            end = long_names.find(b"/\n", offset)
            name = long_names[offset : end if end >= 0 else len(long_names)].decode("utf-8", "replace")
            yield name, start, body
        else:
            yield raw_name.rstrip("/"), start, body


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    try:
        with open(argv[1], "rb") as f:
            data = f.read()
        found = []
        objects = 0
        for name, offset, body in members(data):
            if not name.endswith(".o"):
                continue
            objects += 1
            lower = body.lower()
            at = lower.find(NEEDLE)
            while at >= 0:
                text = body[at : at + len(NEEDLE)].decode("ascii", "replace")
                found.append({"member": name, "offset": offset + at, "text": text})
                at = lower.find(NEEDLE, at + 1)
    except (OSError, ValueError) as err:
        print(f"ankiweb-scan: {argv[1]}: {err}", file=sys.stderr)
        return 2
    if objects == 0:
        print(f"ankiweb-scan: {argv[1]} holds no object file", file=sys.stderr)
        return 2
    for hit in found:
        print(json.dumps(hit, sort_keys=True))
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
