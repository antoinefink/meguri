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
    card-ids-ahead.apkg       legacy 2 as legacy-traversal, no entry 0, map {}:
                              collection.anki21 is schema-11.anki2 with its card
                              ids, in ascending order, set to 2030-01-18T10:00:00Z
                              in ms plus 0, 1, ... (revlog cid follows)
    deck-tree.apkg            the same layout: decks Lang renamed A::B and
                              Lang::Verbs renamed F::x (no deck A, no deck F)
    media-subdir.apkg         as meta-version-4 with meta 08 03, plus entry 0 (a
                              frame of abc) before media, a frame of a P5 map
                              with one record: a/b.jpg, size 3, SHA-1 of abc
    legacy-subdir.apkg        as legacy-traversal, map {"0": "a/b.jpg"}
    legacy-no-media-map.apkg  as legacy-traversal without entry 0 and media

Stored entries dated 1980-01-01 00:00:00, in the order above. The two edited
collections are rewritten with sqlite3 on a scratch copy, secure_delete on. Every other
deck-package input of the apkg scenarios is the scenario's own export.
"""

from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import sys
import tempfile
import zipfile

ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"
RAW_BLOCK = 128 * 1024
NOT_A_ZIP = b"this is not a zip archive\n"
# 2030-01-18T10:00:00Z: three days after the apkg scenarios' clock start.
AHEAD_MS = 1894960800000


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


def varint(n: int) -> bytes:
    out = bytearray()
    while True:
        byte, n = n & 0x7F, n >> 7
        out.append(byte | (0x80 if n else 0))
        if not n:
            return bytes(out)


def media_map(name: str, size: int, sha1: bytes) -> bytes:
    """package/container.md P5: one media entry (name 1, size 2, sha1 3) in field 1."""
    raw = name.encode()
    entry = b"\x0a" + varint(len(raw)) + raw + b"\x10" + varint(size) + b"\x1a" + varint(len(sha1)) + sha1
    return b"\x0a" + varint(len(entry)) + entry


def write_zip(path: str, entries: list[tuple[str, bytes]]) -> None:
    with zipfile.ZipFile(path, "w") as z:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            z.writestr(info, data)


def edited(schema11: bytes, edit) -> bytes:
    """schema11 after edit(connection), in one transaction."""
    work = tempfile.mkdtemp(prefix="apkg-")
    path = os.path.join(work, "collection.anki2")
    with open(path, "wb") as f:
        f.write(schema11)
    con = sqlite3.connect(path, isolation_level=None)
    con.create_collation("unicase", lambda a, b: (a.casefold() > b.casefold()) - (a.casefold() < b.casefold()))
    con.execute("pragma secure_delete = on")
    if con.execute("select ver from col").fetchone()[0] != 11:
        raise SystemExit("apkg.py: schema-11.anki2 is not at schema 11")
    con.execute("begin")
    edit(con)
    con.execute("commit")
    con.close()
    for suffix in ("-wal", "-shm", "-journal"):
        if os.path.exists(path + suffix):
            raise SystemExit("apkg.py: %s was left behind" % suffix)
    with open(path, "rb") as f:
        data = f.read()
    os.unlink(path)
    os.rmdir(work)
    return data


def card_ids_ahead(con) -> None:
    old = [r[0] for r in con.execute("select id from cards order by id")]
    if not old or max(old) >= AHEAD_MS:
        raise SystemExit("apkg.py: schema-11.anki2 has no card or a card already ahead")
    for k, cid in enumerate(old):
        con.execute("update cards set id = ? where id = ?", (AHEAD_MS + k, cid))
        con.execute("update revlog set cid = ? where cid = ?", (AHEAD_MS + k, cid))


def deck_tree(con) -> None:
    decks = json.loads(con.execute("select decks from col").fetchone()[0])
    renames = {"Lang": "A::B", "Lang::Verbs": "F::x"}
    names = sorted(d["name"] for d in decks.values())
    if not set(renames) <= set(names) or {"A", "F"} & set(names):
        raise SystemExit("apkg.py: schema-11.anki2 has decks %s" % names)
    for d in decks.values():
        d["name"] = renames.get(d["name"], d["name"])
    con.execute("update col set decks = ?", (json.dumps(decks, ensure_ascii=False),))


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
    for name, edit in (("card-ids-ahead", card_ids_ahead), ("deck-tree", deck_tree)):
        write_zip(os.path.join(out, name + ".apkg"), [
            ("collection.anki21", edited(schema11, edit)), ("collection.anki2", schema11), ("media", b"{}"),
        ])
    write_zip(os.path.join(out, "media-subdir.apkg"), [
        ("meta", b"\x08\x03"), ("collection.anki21b", raw_frame(schema11)), ("collection.anki2", schema11),
        ("0", raw_frame(b"abc")), ("media", raw_frame(media_map("a/b.jpg", 3, hashlib.sha1(b"abc").digest()))),
    ])
    write_zip(os.path.join(out, "legacy-subdir.apkg"), legacy + [("0", b"abc"), ("media", b'{"0": "a/b.jpg"}')])
    write_zip(os.path.join(out, "legacy-no-media-map.apkg"), legacy)
    print(f"apkg.py: wrote 9 packages to {out}", file=sys.stderr)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[3].strip(), file=sys.stderr)
        return 64
    make(*argv)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
