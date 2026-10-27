# SPDX-License-Identifier: AGPL-3.0-or-later
"""Collection packages (specs/conformance/package-ops.md E1-E3) through real
workers: the collection package form of export, the collection package forms
of import with their rejections, the scenario rules of E2.4 and the
package_listing observation. Packages are Anki's own exports, made here with
the anki library, and packages this file writes itself (zip entries and
raw-block zstd frames as RFC 8878 defines them). Expected values come from the
specs (package/container.md P3, P5, P7, P11, P13; package-ops.md), the
scenario clock and SQLite reads of the files this file made, never from the
driver."""

from __future__ import annotations

import hashlib
import os
import shutil
import sqlite3
import tempfile
import unittest
import zipfile

from anki.collection import Collection

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch

START = "2030-01-15T10:00:00"
SQLITE_HEADER = b"SQLite format 3\x00"
ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"
LATEST_ENTRIES = ["meta", "collection.anki21b", "collection.anki2", "media"]
EXPORT = {"export": {"format": "colpkg", "name": "x.colpkg"}}
OPEN = {"open": {"fixture": "storage/v18.anki2"}}


def sc(steps, **extra):
    return {"id": "t/package", "clock": {"tz": "UTC", "start": START}, "steps": steps, **extra}


def varint(n: int) -> bytes:
    out = bytearray()
    while True:
        byte, n = n & 0x7F, n >> 7
        out.append(byte | (0x80 if n else 0))
        if not n:
            return bytes(out)


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
    chunks = [data[i:i + 131072] for i in range(0, n, 131072)] or [b""]
    for k, chunk in enumerate(chunks):
        out += (len(chunk) << 3 | int(k == len(chunks) - 1)).to_bytes(3, "little") + chunk
    return bytes(out)


def media_map(name: bytes, size: int, sha1: bytes) -> bytes:
    """container.md P5: field 1 holds one media entry (name 1, size 2, sha1 3)."""
    entry = b"\x0a" + varint(len(name)) + name + b"\x10" + varint(size) + b"\x1a" + varint(len(sha1)) + sha1
    return b"\x0a" + varint(len(entry)) + entry


def write_zip(path: str, entries, deflated=()) -> None:
    with zipfile.ZipFile(path, "w") as z:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED if name in deflated else zipfile.ZIP_STORED
            z.writestr(info, data)


def read_entries(path: str) -> list[tuple[str, bytes]]:
    with zipfile.ZipFile(path) as z:
        return [(i.filename, z.read(i)) for i in z.infolist()]


def replaced(entries, **new):
    """entries with some replaced (None drops one), in their order."""
    return [(n, new.get(n.replace(".", "_"), d)) for n, d in entries if new.get(n.replace(".", "_"), d) is not None]


def sql_facts(path: str) -> dict:
    work = tempfile.mkdtemp()
    try:
        copy = os.path.join(work, "c")
        shutil.copyfile(path, copy)
        con = sqlite3.connect(copy)
        (ver,), = con.execute("select ver from col").fetchall()
        counts = {t: con.execute(f"select count() from {t}").fetchone()[0] for t in ("notes", "cards", "revlog")}
        con.close()
    finally:
        shutil.rmtree(work)
    return {"ver": ver, **counts}


def make_fixtures(root: str) -> dict:
    """storage/v18.anki2 (WAL), v11.anki2 (rollback), v12.anki2, and
    package/*.colpkg; returns Anki's export entries."""
    storage = os.path.join(root, "storage")
    package = os.path.join(root, "package")
    os.makedirs(storage)
    os.makedirs(package)
    for name, downgrade in (("v18", False), ("v11", True)):
        work = tempfile.mkdtemp(dir=root)
        col = Collection(os.path.join(work, "collection.anki2"))
        did = col.decks.id("Lang")
        for i in range(2):
            note = col.new_note(col.models.by_name("Basic"))
            note["Front"], note["Back"] = f"q{i}", f"a{i}"
            col.add_note(note, did)
        col.close(downgrade=downgrade)
        shutil.copyfile(os.path.join(work, "collection.anki2"), os.path.join(storage, name + ".anki2"))
        shutil.rmtree(work)
    v12 = os.path.join(storage, "v12.anki2")
    shutil.copyfile(os.path.join(storage, "v11.anki2"), v12)
    con = sqlite3.connect(v12)
    con.execute("update col set ver = 12")
    con.commit()
    con.close()
    # Anki's own collection-only export of v18.
    work = tempfile.mkdtemp(dir=root)
    shutil.copyfile(os.path.join(storage, "v18.anki2"), os.path.join(work, "collection.anki2"))
    col = Collection(os.path.join(work, "collection.anki2"))
    base = os.path.join(package, "base.colpkg")
    col.export_collection_package(base, include_media=False, legacy=False)
    shutil.rmtree(work)
    entries = read_entries(base)
    assert [n for n, _ in entries] == LATEST_ENTRIES, entries
    with open(os.path.join(storage, "v18.anki2"), "rb") as f:
        v18_raw = f.read()
    with open(os.path.join(storage, "v11.anki2"), "rb") as f:
        v11_raw = f.read()
    with open(v12, "rb") as f:
        v12_raw = f.read()
    abc = hashlib.sha1(b"abc").digest()

    def with_media(name, sha1=abc):
        out = [e for e in entries if e[0] != "media"] + [("0", raw_frame(b"abc"))]
        return out + [("media", raw_frame(media_map(name, 3, sha1)))]

    variants = {
        "meta-4": replaced(entries, meta=b"\x08\x04"),
        "meta-empty": replaced(entries, meta=b""),
        "meta-ff": replaced(entries, meta=b"\xff"),
        "no-collection": replaced(entries, collection_anki21b=None),
        "raw-collection": replaced(entries, collection_anki21b=v18_raw),
        "v12-inside": replaced(entries, collection_anki21b=raw_frame(v12_raw)),
        "v11-inside": replaced(entries, collection_anki21b=raw_frame(v11_raw)),
        "no-media": replaced(entries, media=None),
        "media-traversal": with_media(b"../x.jpg"),
        "media-con": with_media(b"con.jpg"),
        "media-sha1-short": with_media(b"x.jpg", abc[:19]),
        "media-ok": with_media(b"x.jpg"),
    }
    for name, content in variants.items():
        write_zip(os.path.join(package, name + ".colpkg"), content)
    with open(os.path.join(package, "not-a-zip.colpkg"), "wb") as f:
        f.write(b"this is not a zip archive\n")
    return {"entries": entries}


class Packages(unittest.TestCase):
    def setUp(self):
        # The TAP runner runs each test on its own, so no class fixtures.
        scratch = os.environ.get("ORACLE_SCRATCH_ROOT")
        self.root = tempfile.mkdtemp(prefix="fixtures-", dir=scratch)
        self.made = make_fixtures(self.root)
        self.handoff = tempfile.mkdtemp(prefix="handoff-", dir=scratch)
        self.worker = WorkerProcess("UTC", extra_env={"ORACLE_FIXTURES": self.root, "ORACLE_HANDOFF": self.handoff})

    def tearDown(self):
        self.worker.close()
        shutil.rmtree(self.root, ignore_errors=True)
        shutil.rmtree(self.handoff, ignore_errors=True)

    def run_steps(self, steps, **extra):
        return self.worker.request({"cmd": "run", "scenario": sc(steps, **extra)})

    def ok(self, steps, **extra):
        resp = self.run_steps(steps, **extra)
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"]

    def error(self, steps, **extra):
        resp = self.run_steps(steps, **extra)
        self.assertFalse(resp["ok"], resp)
        return resp["error"]

    def handed(self, steps, rel="out"):
        """Run steps, then hand the session directory over to rel; its path."""
        resp = self.worker.request({
            "cmd": "run", "scenario": sc(steps + [{"handoff": "meguri"}]), "steps": [0, len(steps)], "handoff": rel,
        })
        self.assertTrue(resp["ok"], resp)
        return os.path.join(self.handoff, rel), resp

    def fixture(self, rel):
        return os.path.join(self.root, rel)


class Export(Packages):
    def test_export_writes_the_latest_layout_and_changes_nothing(self):
        dump = {"observe": ["db_dump"]}
        out, resp = self.handed([OPEN, dump, EXPORT, dump])
        steps = resp["observations"]["steps"]
        self.assertEqual(steps[2], {})
        self.assertEqual(steps[1], steps[3])
        path = os.path.join(out, "x.colpkg")
        # container.md P11 (and Anki's exports): four stored entries in order.
        with zipfile.ZipFile(path) as z:
            archive_entries = z.infolist()
            self.assertEqual([i.filename for i in archive_entries], LATEST_ENTRIES)
            self.assertEqual([i.compress_type for i in archive_entries], [zipfile.ZIP_STORED] * 4)
            self.assertEqual(z.read("meta"), b"\x08\x03")
            self.assertTrue(z.read("collection.anki21b").startswith(ZSTD_MAGIC))
            self.assertTrue(z.read("media").startswith(ZSTD_MAGIC))
            placeholder = os.path.join(self.root, "placeholder")
            with open(placeholder, "wb") as f:
                f.write(z.read("collection.anki2"))
        self.assertEqual(sql_facts(placeholder)["ver"], 11)

    def test_export_replaces_a_file_of_that_name(self):
        handed = os.path.join(self.handoff, "in")
        os.makedirs(handed)
        shutil.copyfile(self.fixture("storage/v18.anki2"), os.path.join(handed, "collection.anki2"))
        with open(os.path.join(handed, "x.colpkg"), "wb") as f:
            f.write(b"old")
        steps = [OPEN, {"handoff": "oracle"}, EXPORT, {"handoff": "meguri"}]
        resp = self.worker.request({
            "cmd": "run", "scenario": sc(steps), "steps": [2, 3], "handoff": "out",
            "resume": {"dir": "in", "now": epoch(START, "UTC"), "notes": [], "cards": []},
        })
        self.assertTrue(resp["ok"], resp)
        self.assertEqual([n for n, _ in read_entries(os.path.join(self.handoff, "out", "x.colpkg"))], LATEST_ENTRIES)

    def test_the_export_crosses_a_handoff_and_imports_on_the_other_side(self):
        steps = [OPEN, EXPORT, {"handoff": "oracle"}, {"import": {"format": "colpkg", "name": "x.colpkg"}},
                 {"db_exec": {"sql": "SELECT ver FROM col"}}]
        first = self.worker.request({"cmd": "run", "scenario": sc(steps), "steps": [0, 2], "handoff": "a"})
        self.assertTrue(first["ok"], first)
        second = self.worker.request({"cmd": "run", "scenario": sc(steps), "steps": [3, 5], "resume": first["state"]})
        self.assertTrue(second["ok"], second)
        self.assertEqual(second["observations"]["steps"], [{}, {"db_exec": {"rows": [[18]]}}])

    def test_malformed_collection_package_forms_are_invalid_before_any_collection(self):
        bad = [
            {"format": "colpkg"}, {"format": "colpkg", "name": "x.colpkg", "with_media": False},
            {"format": "colpkg", "name": "x.colpkg", "legacy": 1}, {"format": "colpkg", "media": True},
            {"format": "colpkg", "name": "x.apkg", "media": True},
        ] + [{"format": "colpkg", "name": name} for name in (
            "", "x", "x.apkg", "x.COLPKG", ".x.colpkg", "-x.colpkg", "a/x.colpkg", "x.colpkg\n", 1, None)]
        for args in bad:
            err = self.error([{"advance": "1m"}, {"export": args}], setup={})
            with self.subTest(args=args):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)
        for name in ("x.colpkg", "X-1_a.b.colpkg", "9.colpkg"):
            with self.subTest(name=name):
                self.assertEqual(self.ok([OPEN, {"export": {"format": "colpkg", "name": name}}]), [{}, {}])

    def test_other_export_forms_stay_unsupported(self):
        # E8, the legacy deck form, and E9, the collection package options,
        # are drafts (E7's form is test_apkg.py).
        e8 = {"format": "apkg", "name": "x.apkg", "limit": "collection", "scheduling": True, "presets": True,
              "media": False, "legacy": True}
        e9 = [{"format": "colpkg", "name": "x.colpkg", **opts}
              for opts in ({"media": True}, {"legacy": True}, {"media": True, "legacy": False})]
        for args in (e8, *e9, {"format": "csv", "name": "x.csv"}):
            err = self.error([{"advance": "1m"}, {"export": args}])
            with self.subTest(args=args):
                self.assertEqual(err, {"kind": "unsupported_op", "op": "export", "step": 1})


def import_fixture(rel):
    return {"import": {"format": "colpkg", "fixture": rel}}


class Import(Packages):
    def test_an_import_of_an_export_brings_its_rows_back_with_scm_and_mod_stamped(self):
        dump = {"observe": ["db_dump"]}
        out = self.ok([
            OPEN, dump, EXPORT,
            {"db_exec": {"sql": "UPDATE cards SET flags = 3"}},
            {"db_exec": {"sql": "DELETE FROM notes WHERE id = (SELECT min(id) FROM notes)"}},
            {"advance": "1h"},
            {"import": {"format": "colpkg", "name": "x.colpkg"}},
            dump,
            {"db_exec": {"sql": "SELECT * FROM pragma_integrity_check"}},
        ])
        before, imported = out[1]["db_dump"], out[7]["db_dump"]
        self.assertEqual(out[6], {})
        # engine/collection-replace.md R7: scm and mod are the import instant
        # in milliseconds; nothing else changes.
        now_ms = (epoch(START, "UTC") + 3600) * 1000
        self.assertEqual((imported["col"]["1"]["scm"], imported["col"]["1"]["mod"]), (now_ms, now_ms))
        for row in (before["col"]["1"], imported["col"]["1"]):
            del row["scm"], row["mod"]
        self.assertEqual(imported, before)
        self.assertEqual(out[8], {"db_exec": {"rows": [["ok"]]}})

    def test_a_fixture_import_and_a_schema_11_collection_upgraded_on_import(self):
        expect = sql_facts(self.fixture("storage/v18.anki2"))
        query = {"db_exec": {"sql": "SELECT ver, scm, mod, (SELECT count() FROM notes), (SELECT count() FROM cards) FROM col"}}
        now_ms = epoch(START, "UTC") * 1000
        for rel in ("package/base.colpkg", "package/v11-inside.colpkg", "package/media-ok.colpkg"):
            out = self.ok([OPEN, import_fixture(rel), query])
            with self.subTest(rel=rel):
                self.assertEqual(out[1], {})
                self.assertEqual(out[2], {"db_exec": {"rows": [[18, now_ms, now_ms, expect["notes"], expect["cards"]]]}})

    def test_refusals_record_their_kind_and_change_nothing(self):
        # package/container.md P3, P4, P5, P6, P13 and package-ops.md E2.3.
        cases = {
            "meta-4": "too_new", "meta-empty": "too_new", "meta-ff": "corrupt", "no-collection": "corrupt",
            "raw-collection": "corrupt", "not-a-zip": "corrupt", "v12-inside": "corrupt", "no-media": "corrupt",
            "media-traversal": "corrupt", "media-con": "corrupt", "media-sha1-short": "corrupt",
        }
        dump = {"observe": ["db_dump"]}
        imports = [import_fixture(f"package/{name}.colpkg") for name in cases]
        baseline, _ = self.handed([OPEN, dump], rel="baseline")
        out, resp = self.handed([OPEN, dump] + imports + [dump, {"db_exec": {"sql": "SELECT count() FROM notes"}}])
        steps = resp["observations"]["steps"]
        self.assertEqual(steps[2:2 + len(cases)], [{"import": {"error": kind}} for kind in cases.values()])
        self.assertEqual(steps[1], steps[2 + len(cases)])
        self.assertEqual(steps[-1], {"db_exec": {"rows": [[sql_facts(self.fixture("storage/v18.anki2"))["notes"]]]}})
        # No file was left in the session directory or its media folder.
        listing = lambda d: sorted(os.path.relpath(os.path.join(p, f), d) for p, _, fs in os.walk(d) for f in fs)  # noqa: E731
        self.assertEqual(listing(out), listing(baseline))

    def test_missing_packages(self):
        err = self.error([OPEN, {"import": {"format": "colpkg", "name": "none.colpkg"}}])
        self.assertEqual((err["kind"], err["ref"], err["step"]), ("unknown_ref", "none.colpkg", 1), err)
        begin = self.worker.request({"cmd": "begin", "scenario": sc([OPEN])})
        self.assertTrue(begin["ok"], begin)
        try:
            self.assertTrue(self.worker.request({"cmd": "step"})["ok"])
            os.mkdir(self.fixture("package/dir.colpkg"))
            for rel in ("package/none.colpkg", "package/dir.colpkg"):
                resp = self.worker.request({"cmd": "step", "step": import_fixture(rel)})
                self.assertEqual(resp["error"]["kind"], "fixture_error", resp)
            # The collection stayed open.
            read = self.worker.request({"cmd": "step", "step": {"db_exec": {"sql": "SELECT ver FROM col"}}})
            self.assertEqual(read["observation"], {"db_exec": {"rows": [[18]]}}, read)
        finally:
            self.worker.request({"cmd": "end"})
        with WorkerProcess("UTC") as w:
            resp = w.request({"cmd": "run", "scenario": sc([OPEN])})
            self.assertEqual(resp["error"]["kind"], "fixture_error", resp)

    def test_forms(self):
        bad = [
            None, "x.colpkg", {"format": "colpkg"}, {"format": "colpkg", "name": "x.colpkg", "fixture": "p/x.colpkg"},
            {"format": "colpkg", "name": "x.apkg"}, {"format": "colpkg", "name": "../x.colpkg"},
            {"format": "colpkg", "fixture": "package/base.apkg"}, {"format": "colpkg", "fixture": "/package/base.colpkg"},
            {"format": "colpkg", "fixture": "Package/base.colpkg"}, {"format": "colpkg", "fixture": "package/base.colpkg\n"},
            {"format": "colpkg", "name": "x.colpkg", "with_media": True},
        ]
        for args in bad:
            err = self.error([OPEN, {"import": args}])
            with self.subTest(args=args):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)
        # The deck package form is E6 (test_apkg.py): checked before any
        # collection; formats the package area does not have are unsupported.
        err = self.error([OPEN, {"import": {"format": "apkg", "fixture": "package/x.apkg"}}])
        self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)
        err = self.error([OPEN, {"import": {"format": "csv", "fixture": "package/x.csv"}}])
        self.assertEqual(err, {"kind": "unsupported_op", "op": "import", "step": 1})

    def test_scenario_rules(self):
        # package-ops.md E2.4: no setup, open first, no logical id anywhere.
        imp = import_fixture("package/base.colpkg")
        cases = [
            ([imp], {}),
            ([{"advance": "1m"}, OPEN, imp], {}),
            ([OPEN, imp], {"setup": {"decks": ["A"]}}),
            ([OPEN, {"add_note": {"notetype": "Basic", "fields": ["q", "a"]}}, imp], {}),
            ([OPEN, imp, {"observe": ["card(#c1)"]}], {}),
            ([OPEN, imp, {"set_flag": {"cards": ["#c2"], "flag": 1}}], {}),
            ([OPEN, imp, {"edit_note": {"note": "#n1", "tags": ["x"]}}], {}),
        ]
        for steps, extra in cases:
            err = self.error(steps, **extra)
            with self.subTest(steps=steps, extra=extra):
                self.assertEqual(err["kind"], "invalid_scenario", err)
        # A scenario without an import may do all of that.
        self.assertTrue(self.run_steps([OPEN, {"db_exec": {"sql": "SELECT '#c1'"}}])["ok"])


class Listing(Packages):
    def listing(self, request, steps=(OPEN,)):
        out = self.ok(list(steps) + [{"observe": [{"package_listing": request}]}])
        return out[-1]["package_listing"]

    def test_anki_export_listing(self):
        # package/container.md P1, P3, P7, P11: Anki's layout and placeholder
        # (version 11, rollback journal, one note, one card, no review).
        value = self.listing({"name": "x.colpkg"}, steps=[OPEN, EXPORT])
        self.assertEqual(value, {
            "entries": [{"name": n, "method": 0} for n in LATEST_ENTRIES],
            "meta_version": 3,
            "sqlite": {"collection.anki2": {"ver": 11, "notes": 1, "cards": 1, "revlog": 0, "journal": "rollback",
                                            "opens": True}},
        })
        self.assertEqual(self.listing({"fixture": "package/base.colpkg"}), value)

    def test_meta_versions(self):
        # container.md P3: field 1 as int32, the last occurrence counts, other
        # fields skipped by wire type; a malformed message is invalid.
        cases = [
            (None, None), (b"", 0), (b"\x08\x03", 3), (b"\x08\x04", 4), (b"\xff", "invalid"),
            (b"\x08\x83\x80\x80\x80\x10", 3), (b"\x0a\x01\x33", "invalid"), (b"\x08\xff\xff\xff\xff\x0f", -1),
            (b"\x08\x01\x08\x02", 2), (b"\x18\x05\x08\x02", 2), (b"\x1b\x08\x07\x1c\x08\x03", 3), (b"\x1b", "invalid"),
            (b"\x0c", "invalid"), (b"\x08\x01\x0e\x00", "invalid"), (b"\x00\x00", "invalid"),
            (b"\x08\xff\xff\xff\xff\xff\xff\xff\xff\xff\x01", -1), (b"\x08\xff\xff\xff\xff\xff\xff\xff\xff\xff\x02", "invalid"),
            (b"\x22\x05\x00", "invalid"), (b"\x11" + bytes(8) + b"\x08\x03", 3), (b"\x0d" + bytes(4), "invalid"),
        ]
        for meta, expected in cases:
            entries = [("collection.anki2", b"not sqlite")] + ([] if meta is None else [("meta", meta)])
            write_zip(self.fixture("package/meta.colpkg"), entries)
            value = self.listing({"fixture": "package/meta.colpkg"})
            with self.subTest(meta=meta):
                self.assertEqual(value["meta_version"], expected)
                self.assertEqual(value["sqlite"], {})

    def test_raw_sqlite_entries(self):
        # Entries whose content after the zip method starts with the SQLite
        # header, deflated ones included; zstd frames are never decoded.
        with open(self.fixture("storage/v18.anki2"), "rb") as f:
            v18 = f.read()
        with open(self.fixture("storage/v12.anki2"), "rb") as f:
            v12 = f.read()
        write_zip(self.fixture("package/raw.apkg"), [
            ("text", b"SQLite format 2\x00"), ("a", v18), ("zstd", raw_frame(v18)), ("b", v12), ("short", SQLITE_HEADER[:8]),
        ], deflated=("a",))
        value = self.listing({"fixture": "package/raw.apkg"})
        self.assertEqual(value["entries"], [{"name": "text", "method": 0}, {"name": "a", "method": 8},
                                            {"name": "zstd", "method": 0}, {"name": "b", "method": 0},
                                            {"name": "short", "method": 0}])
        self.assertIsNone(value["meta_version"])
        v18_facts, v12_facts = sql_facts(self.fixture("storage/v18.anki2")), sql_facts(self.fixture("storage/v12.anki2"))
        self.assertEqual(value["sqlite"], {
            "a": {**v18_facts, "journal": "wal", "opens": True},
            # Anki refuses schema 12 (storage-ops.md P1).
            "b": {**v12_facts, "journal": "rollback", "opens": False},
        })

    def test_listing_never_changes_the_session(self):
        dump = {"observe": ["db_dump"]}
        listing = {"observe": [{"package_listing": {"name": "x.colpkg"}}]}
        out, resp = self.handed([OPEN, EXPORT, dump, listing, {"observe": [{"package_listing": {"fixture": "package/base.colpkg"}}]}, dump])
        steps = resp["observations"]["steps"]
        self.assertEqual(steps[2], steps[5])
        self.assertEqual(sorted(os.listdir(out)), sorted(os.listdir(self.handed([OPEN, EXPORT], rel="b")[0])))

    def test_errors(self):
        err = self.error([OPEN, {"observe": [{"package_listing": {"name": "none.colpkg"}}]}])
        self.assertEqual((err["kind"], err["ref"]), ("unknown_ref", "none.colpkg"), err)
        err = self.error([OPEN, {"observe": [{"package_listing": {"fixture": "package/none.apkg"}}]}])
        self.assertEqual(err["kind"], "fixture_error", err)
        err = self.error([OPEN, {"observe": [{"package_listing": {"fixture": "package/not-a-zip.colpkg"}}]}])
        self.assertEqual(err["kind"], "internal_error", err)
        for args in (None, {}, {"name": "x.colpkg", "fixture": "package/base.colpkg"}, {"name": "x.zip"},
                     {"fixture": "package/base.zip"}, {"name": "x.colpkg", "extra": 1}):
            err = self.error([OPEN, EXPORT, {"observe": [{"package_listing": args}]}])
            with self.subTest(args=args):
                self.assertEqual(err["kind"], "invalid_scenario", err)
        # At most one package_listing per observe step, checked before any
        # step runs: the missing package of step 1 is never reached.
        twice = {"observe": [{"package_listing": {"name": "x.colpkg"}}, {"package_listing": {"fixture": "package/base.colpkg"}}]}
        err = self.error([OPEN, {"import": {"format": "colpkg", "name": "none.colpkg"}}, twice])
        self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 2), err)


if __name__ == "__main__":
    unittest.main()
