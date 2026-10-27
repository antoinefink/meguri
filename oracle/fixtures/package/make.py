# SPDX-License-Identifier: AGPL-3.0-or-later
"""Collection packages for the package scenarios (specs/conformance/package-ops.md E4).

Runs in the oracle image (Anki 26.9.3) under libfaketime with the clock frozen
at 2030-01-14T12:00:00Z (run.sh). Driven by make.sh; see README.md.

  python3 make.py backup SCHEMA18 OUT
      open a copy of storage/schema-18.anki2 in a scratch profile, make a forced
      backup with Anki's backup writer and copy the package it wrote to OUT
  python3 make.py build STORAGE BACKUP OUT
      write OUT/backup/standard.colpkg and OUT/variants/*.colpkg (E4) from the
      storage fixtures in STORAGE and Anki's backup
  python3 make.py apkg STORAGE OUT
      write the crafted deck-package fixtures OUT/*.apkg (apkg.py, E4) from
      STORAGE/schema-11.anki2 (the 2.1.26 ones come from oldclient.py)
  python3 make.py sqlite-files PACKAGES OUT
      for the stock check: every SQLite file the packages under PACKAGES hold,
      raw or in one of this maker's raw-block frames, and the collection Anki
      imports from backup/standard.colpkg into a scratch profile, as OUT/*.anki2

Archives this maker writes have stored entries dated 1980-01-01 00:00:00 in
the order E4 gives. Its zstd frames hold their content in raw blocks of at most
128 KiB, with the single-segment flag and the content size set and no
checksum (RFC 8878 3.1.1): valid frames that need no zstd library, which the
oracle image does not have. Anki's own frames (the backup's collection.anki21b
and media) are copied byte for byte.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import sqlite3
import sys
import tempfile
import zipfile

BACKUP_ENTRIES = ["meta", "collection.anki21b", "collection.anki2", "media"]
BACKUP_NAME = "backup-2030-01-14-12.00.00.colpkg"  # run.sh's frozen clock, TZ=UTC
SQLITE_HEADER = b"SQLite format 3\x00"
ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"
RAW_BLOCK = 128 * 1024
NOT_A_ZIP = b"this is not a zip archive\n"


def fail(message: str) -> None:
    raise SystemExit("make.py: " + message)


def read(path: str) -> bytes:
    with open(path, "rb") as f:
        return f.read()


# --- zstd and protobuf --------------------------------------------------------------


def raw_frame(data: bytes) -> bytes:
    """One zstd frame: single segment, content size, raw blocks, no checksum."""
    n = len(data)
    if n < 256:
        flag, size = 0, n.to_bytes(1, "little")
    elif n < 65536 + 256:
        flag, size = 1, (n - 256).to_bytes(2, "little")
    elif n < 1 << 32:
        flag, size = 2, n.to_bytes(4, "little")
    else:
        flag, size = 3, n.to_bytes(8, "little")
    out = bytearray(ZSTD_MAGIC + bytes([flag << 6 | 0x20]) + size)
    chunks = [data[i:i + RAW_BLOCK] for i in range(0, n, RAW_BLOCK)] or [b""]
    for k, chunk in enumerate(chunks):
        out += (len(chunk) << 3 | int(k == len(chunks) - 1)).to_bytes(3, "little") + chunk
    return bytes(out)


def raw_frame_content(frame: bytes) -> bytes | None:
    """The content of a frame raw_frame wrote; None for any other bytes
    (Anki's compressed frames)."""
    if len(frame) < 6 or frame[:4] != ZSTD_MAGIC or frame[4] & 0x3F != 0x20:
        return None
    width = (1, 2, 4, 8)[frame[4] >> 6]
    size = int.from_bytes(frame[5:5 + width], "little") + (256 if width == 2 else 0)
    i, out = 5 + width, bytearray()
    while i + 3 <= len(frame):
        header = int.from_bytes(frame[i:i + 3], "little")
        if header >> 1 & 3:
            return None
        length, i = header >> 3, i + 3
        out += frame[i:i + length]
        i += length
        if header & 1:
            return bytes(out) if i == len(frame) and len(out) == size else None
    return None


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


# --- archives -------------------------------------------------------------------------


def entries_of(path: str) -> list[tuple[str, bytes]]:
    with zipfile.ZipFile(path) as z:
        return [(info.filename, z.read(info)) for info in z.infolist()]


def write_archive(path: str, entries: list[tuple[str, bytes]]) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with zipfile.ZipFile(path, "w") as z:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            z.writestr(info, data)


def replace(entries, name: str, data: bytes | None) -> list[tuple[str, bytes]]:
    """entries with `name`'s bytes replaced, or the entry dropped (None)."""
    if name not in [n for n, _ in entries]:
        fail(f"no entry {name}")
    return [(n, data if n == name else d) for n, d in entries if n != name or data is not None]


# --- SQLite ---------------------------------------------------------------------------


def connect(path: str) -> sqlite3.Connection:
    con = sqlite3.connect(path, isolation_level=None)
    con.create_collation("unicase", lambda a, b: (a.casefold() > b.casefold()) - (a.casefold() < b.casefold()))
    return con


def scratch_copy(data: bytes, work: str, name: str) -> str:
    path = os.path.join(work, name)
    with open(path, "wb") as f:
        f.write(data)
    return path


def corrupt_page(schema18: bytes, work: str) -> bytes:
    """schema-18 with the cell count of ix_cards_nid's root page set to 0 (E4)."""
    con = connect(scratch_copy(schema18, work, "probe.anki2"))
    page_size = con.execute("pragma page_size").fetchone()[0]
    (root,), = con.execute("select rootpage from sqlite_master where type = 'index' and name = 'ix_cards_nid'").fetchall()
    con.close()
    data = bytearray(schema18)
    header = (root - 1) * page_size + (100 if root == 1 else 0)
    if data[header] != 0x0A:
        fail(f"the root page of ix_cards_nid has type {data[header]}, not 10 (a leaf index page)")
    if int.from_bytes(data[header + 3:header + 5], "big") == 0:
        fail("the root page of ix_cards_nid has no cell to drop")
    data[header + 3:header + 5] = b"\x00\x00"
    con = connect(scratch_copy(bytes(data), work, "check.anki2"))
    integrity = [r[0] for r in con.execute("pragma integrity_check")]
    ver = con.execute("select ver from col").fetchone()[0]
    con.close()
    if integrity == ["ok"] or ver != 18:
        fail(f"the damaged copy reads integrity {integrity[:1]} and ver {ver}")
    return bytes(data)


# --- commands ------------------------------------------------------------------------


def backup(schema18: str, out: str) -> None:
    from anki.collection import Collection

    work = tempfile.mkdtemp(prefix="profile-")
    col_path, folder = os.path.join(work, "collection.anki2"), os.path.join(work, "backups")
    os.mkdir(folder)
    shutil.copyfile(schema18, col_path)
    col = Collection(col_path)
    try:
        if not col.create_backup(backup_folder=folder, force=True, wait_for_completion=True):
            fail("Anki made no backup")
    finally:
        col.close(downgrade=False)
    if os.listdir(folder) != [BACKUP_NAME]:
        fail(f"the backup folder holds {os.listdir(folder)}, not {BACKUP_NAME}")
    path = os.path.join(folder, BACKUP_NAME)
    with zipfile.ZipFile(path) as z:
        archive_entries = z.infolist()
        layout = [(i.filename, i.compress_type) for i in archive_entries]
        meta = z.read("meta") if "meta" in z.namelist() else None
    if layout != [(n, zipfile.ZIP_STORED) for n in BACKUP_ENTRIES] or meta != b"\x08\x03":
        fail(f"Anki's backup has entries {layout} and meta {meta!r}, not E4's")
    shutil.copyfile(path, out)
    shutil.rmtree(work)


def build(storage: str, anki_backup: str, out: str) -> None:
    work = tempfile.mkdtemp(prefix="build-")
    schema11, schema18 = read(os.path.join(storage, "schema-11.anki2")), read(os.path.join(storage, "schema-18.anki2"))
    ver12 = read(os.path.join(storage, "ver-12-delete.anki2"))
    backup_entries = replace(entries_of(anki_backup), "collection.anki2", schema11)
    media = dict(backup_entries)["media"]
    abc_sha1 = hashlib.sha1(b"abc").digest()

    def with_media(name: str, sha1: bytes = abc_sha1) -> list[tuple[str, bytes]]:
        before = [(n, d) for n, d in backup_entries if n != "media"]
        return before + [("0", raw_frame(b"abc")), ("media", raw_frame(media_map(name, 3, sha1)))]

    packages = {
        "backup/standard": backup_entries,
        "variants/schema-11-inside": [
            ("meta", b"\x08\x03"), ("collection.anki21b", raw_frame(schema11)), ("collection.anki2", schema11),
            ("media", media),
        ],
        "variants/meta-version-4": replace(backup_entries, "meta", b"\x08\x04"),
        "variants/meta-version-0": replace(backup_entries, "meta", b""),
        "variants/meta-not-protobuf": replace(backup_entries, "meta", b"\xff"),
        "variants/no-collection-entry": replace(backup_entries, "collection.anki21b", None),
        "variants/collection-not-zstd": replace(backup_entries, "collection.anki21b", schema18),
        "variants/corrupt-page": replace(backup_entries, "collection.anki21b", raw_frame(corrupt_page(schema18, work))),
        "variants/schema-12": replace(backup_entries, "collection.anki21b", raw_frame(ver12)),
        "variants/media-traversal": with_media("../x.jpg"),
        "variants/media-unnormalized": with_media("con.jpg"),
        "variants/media-sha1-short": with_media("x.jpg", abc_sha1[:19]),
        "variants/no-media-map": replace(backup_entries, "media", None),
    }
    for name, entries in packages.items():
        write_archive(os.path.join(out, name + ".colpkg"), entries)
    with open(os.path.join(out, "variants", "not-a-zip.colpkg"), "wb") as f:
        f.write(NOT_A_ZIP)
    shutil.rmtree(work)
    for name in sorted(packages):
        with zipfile.ZipFile(os.path.join(out, name + ".colpkg")) as z:
            if any(i.compress_type != zipfile.ZIP_STORED or i.date_time != (1980, 1, 1, 0, 0, 0) for i in z.infolist()):
                fail(f"{name}: an entry is not stored or not dated 1980-01-01")
            print(json.dumps({"package": name + ".colpkg", "entries": [i.filename for i in z.infolist()]}))


def sqlite_files(packages: str, out: str) -> None:
    from anki.collection import Collection
    from anki.media import media_paths_from_col_path

    os.makedirs(out, exist_ok=True)
    written = 0
    for folder, _, files in sorted(os.walk(packages)):
        for file in sorted(files):
            path = os.path.join(folder, file)
            if not zipfile.is_zipfile(path):
                continue
            stem = os.path.splitext(os.path.relpath(path, packages))[0].replace("/", "_")
            for index, (name, data) in enumerate(entries_of(path)):
                content = data if data.startswith(SQLITE_HEADER) else raw_frame_content(data)
                if content is not None and content.startswith(SQLITE_HEADER):
                    with open(os.path.join(out, f"{stem}.{index}.anki2"), "wb") as f:
                        f.write(content)
                    written += 1
    # The collection Anki imports from the backup fixture (E4).
    work = tempfile.mkdtemp(prefix="import-")
    col_path = os.path.join(work, "collection.anki2")
    col = Collection(col_path)
    backend = col._backend
    col.close(downgrade=False)
    media_folder, media_db = media_paths_from_col_path(col_path)
    backend.import_collection_package(col_path=col_path, backup_path=os.path.join(packages, "backup", "standard.colpkg"),
                                      media_folder=media_folder, media_db=media_db)
    col = Collection(col_path, backend=backend)
    ver = col.db.scalar("select ver from col")
    col.close(downgrade=False)
    if ver != 18:
        fail(f"the imported backup is at {ver}")
    shutil.copyfile(col_path, os.path.join(out, "backup_standard.imported.anki2"))
    shutil.rmtree(work)
    print(json.dumps({"sqlite_files": written + 1}))


def main(argv) -> int:
    cmd, args = (argv[0], argv[1:]) if argv else ("", [])
    if cmd == "backup" and len(args) == 2:
        backup(*args)
    elif cmd == "build" and len(args) == 3:
        build(*args)
    elif cmd == "apkg" and len(args) == 2:
        import apkg  # this directory's apkg.py (standard library only)

        apkg.make(*args)
    elif cmd == "sqlite-files" and len(args) == 2:
        sqlite_files(*args)
    else:
        print(__doc__, file=sys.stderr)
        return 64
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
