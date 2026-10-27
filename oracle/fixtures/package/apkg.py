# SPDX-License-Identifier: AGPL-3.0-or-later
"""Deck-package refusal fixtures (specs/package/apkg-import.md I18.1;
specs/conformance/package-ops.md E4, E6.4). Oracle maintainers only.

    python3 -I -B oracle/fixtures/package/apkg.py STORAGE OUT

Writes OUT/*.apkg from STORAGE/schema-11.anki2 (no stock note type, schema
11), standard library only, the same bytes every run. make.sh runs it (as
`make.py apkg`) after the collection packages, stock-checks the SQLite files
it holds and copies OUT to conformance/fixtures/package/apkg/:

    meta-version-4.apkg       meta 08 04, collection.anki21b (the maker's raw-block
                              zstd frame of schema-11.anki2), the placeholder, an
                              empty media map frame
    legacy-traversal.apkg     no meta (legacy 2): collection.anki21 and the
                              placeholder (raw schema-11.anki2), entry 0 = abc,
                              the JSON map {"0": "../x.jpg"}
    legacy-map-not-json.apkg  as legacy-traversal without entry 0, map `not json`
    not-a-zip.apkg            the 26 ASCII bytes `this is not a zip archive` LF

Stored entries dated 1980-01-01 00:00:00, in the order above. Every other
deck-package input of the apkg scenarios is the scenario's own export.
"""

from __future__ import annotations

import os
import sys
import zipfile

ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"
RAW_BLOCK = 128 * 1024
NOT_A_ZIP = b"this is not a zip archive\n"


def raw_frame(data: bytes) -> bytes:
    """One zstd frame (RFC 8878 3.1.1): single segment, content size, raw
    blocks of at most 128 KiB, no checksum."""
    n = len(data)
    if n < 256:
        flag, size = 0, n.to_bytes(1, "little")
    elif n < 65536 + 256:
        flag, size = 1, (n - 256).to_bytes(2, "little")
    else:
        flag, size = 2, n.to_bytes(4, "little")
    out = bytearray(ZSTD_MAGIC + bytes([flag << 6 | 0x20]) + size)
    chunks = [data[i:i + RAW_BLOCK] for i in range(0, n, RAW_BLOCK)] or [b""]
    for k, chunk in enumerate(chunks):
        out += (len(chunk) << 3 | int(k == len(chunks) - 1)).to_bytes(3, "little") + chunk
    return bytes(out)


def write_zip(path: str, entries: list[tuple[str, bytes]]) -> None:
    with zipfile.ZipFile(path, "w") as z:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            z.writestr(info, data)


def make(storage: str, out: str) -> None:
    with open(os.path.join(storage, "schema-11.anki2"), "rb") as f:
        schema11 = f.read()
    os.makedirs(out, exist_ok=True)
    legacy = [("collection.anki21", schema11), ("collection.anki2", schema11)]
    write_zip(os.path.join(out, "meta-version-4.apkg"), [
        ("meta", b"\x08\x04"), ("collection.anki21b", raw_frame(schema11)), ("collection.anki2", schema11),
        ("media", raw_frame(b"")),
    ])
    write_zip(os.path.join(out, "legacy-traversal.apkg"), legacy + [("0", b"abc"), ("media", b'{"0": "../x.jpg"}')])
    write_zip(os.path.join(out, "legacy-map-not-json.apkg"), legacy + [("media", b"not json")])
    with open(os.path.join(out, "not-a-zip.apkg"), "wb") as f:
        f.write(NOT_A_ZIP)
    print(f"apkg.py: wrote 4 packages to {out}", file=sys.stderr)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[3].strip(), file=sys.stderr)
        return 64
    make(*argv)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
