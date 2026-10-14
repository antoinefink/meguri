# SPDX-License-Identifier: AGPL-3.0-or-later
"""Collection files for the storage scenarios (specs/storage/schema-upgrade.md U16).

Runs inside an image holding one Anki release (Dockerfile.legacy for 2.1.26 to
2.1.44, the oracle image for 26.09.3), under libfaketime with a running clock
read from $FAKETIME_TIMESTAMP_FILE ("@<epoch>", FAKETIME_FMT=%s). A frozen
clock cannot be used: old releases wait for the clock to move when they pick
note type ids. Driven by make.sh; see README.md for the recipes.

  python3 make.py standard OUT --expect-ver N [--downgrade] [--ease-cases] [--drop-keys JSON]
      standard content through the release's own Python API, then the SQL
      edits of U16 on the closed file (factor 1400, a duplicate grave below
      18, meguriFixture, meguriExtra at 11, the ease-case factors)
  python3 make.py names SRC OUT       schema-11-names from schema-11
  python3 make.py version SRC OUT VER [--journal delete|wal]
  python3 make.py journal SRC OUT delete|wal
  python3 make.py v1-scheduler SRC OUT
  python3 make.py summary FILE...     what a fixture holds, as JSON (sqlite3 only)
  python3 make.py upgrade-check FILE  (26.09.3 only) upgrade a copy with Anki and
      report every converted object whose `other` JSON has more than one key
  python3 make.py stock-check FILE... (26.09.3 only) fail when a file still holds
      a piece of this release's stock note type CSS or templates

Only the standard, upgrade-check and stock-check commands import anki; the
others use Python's sqlite3 module on files Anki has closed. standard turns on
secure_delete on Anki's connection right after it opens the new collection and
fails when the file it wrote still holds the stock text of its release.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sqlite3
import struct
import sys
import tempfile

DAY = 86400
# 2026-09-01T08:00:00Z: the standard content is made on day 0 and day 5.
T0 = 1788249600
MEGURI_FIXTURE = (
    '{"b": 1, "a": 2.50, "c": [1.0, 2, 1E2, -0, 0.000001, 1e16, 12345678901234567890, '
    '123456789012345678901], "d": "a/b\\u00e9\\u0001\\""}'
)
DYN_ADDED = 5
# The note types of the standard content (U16).
PAIR_TEMPLATES = [("Forward", "{{Front}}", "{{Front}} = {{Back}}"), ("Reverse", "{{Back}}", "{{Back}} = {{Front}}")]
PAIR_CSS = ".card { font-size: 18px; color: navy; }"
GAP_TEMPLATES = [("Gap", "{{cloze:Text}}", "{{cloze:Text}} / {{Extra}}")]
GAP_CSS = ".cloze { color: teal; }"
OWN_TEXT = [PAIR_CSS, GAP_CSS] + [s for t in PAIR_TEMPLATES + GAP_TEMPLATES for s in t[1:]]
# Stock text is looked for in pieces of this many bytes, so a piece that an
# overflow page boundary or a later write cut off is found too.
STOCK_PIECE = 16


# --- clock ---------------------------------------------------------------------


def set_clock(epoch: int) -> None:
    """Start-at time: the clock runs on from `epoch` (plus the time since the
    process started, which only ever moves it forward)."""
    with open(os.environ["FAKETIME_TIMESTAMP_FILE"], "w") as f:
        f.write("@%d\n" % epoch)


# --- release-neutral API helpers -------------------------------------------------


def first(obj, *names):
    for name in names:
        fn = getattr(obj, name, None)
        if fn is not None:
            return fn
    raise AttributeError("none of %s on %s" % (names, type(obj).__name__))


def open_col(path):
    try:
        from anki.collection import Collection
    except ImportError:  # 2.1.26: anki.storage.Collection is a factory function
        from anki.storage import Collection
    return Collection(path)


def ensure_v2(col) -> None:
    ver = col.schedVer() if hasattr(col, "schedVer") else col.sched_ver()
    if ver == 1:
        if hasattr(col, "changeSchedulerVer"):
            col.changeSchedulerVer(2)
        else:
            col.upgrade_to_v2_scheduler()


def notetype(col, name, cloze, fields, templates, css):
    mm = col.models
    m = mm.new(name)
    for fname in fields:
        first(mm, "add_field", "addField")(m, first(mm, "new_field", "newField")(fname))
    for tname, qfmt, afmt in templates:
        t = first(mm, "new_template", "newTemplate")(tname)
        t["qfmt"] = qfmt
        t["afmt"] = afmt
        first(mm, "add_template", "addTemplate")(m, t)
    m["css"] = css
    if cloze:
        m["type"] = 1
    mm.add(m)
    return first(mm, "by_name", "byName")(name)


def remove_notetype(col, m) -> None:
    if hasattr(col.models, "remove"):
        col.models.remove(int(m["id"]))
    else:
        col.models.rem(m)


def secure_delete(col) -> None:
    """secure_delete on Anki's own connection (U16): from here on SQLite
    overwrites with zeros every cell it deletes and every page it frees, so the
    deleted stock note types leave nothing in the file (R1959)."""
    col.db.execute("pragma secure_delete = on")
    if col.db.scalar("pragma secure_delete") != 1:
        raise SystemExit("secure_delete did not turn on for this release's connection")


def preset(col, name, **new):
    dm = col.decks
    cid = dm.add_config_returning_id(name)
    conf = first(dm, "get_config", "getConf")(cid)
    for key, value in new.items():
        conf["new"][key] = value
    first(dm, "update_config", "updateConf")(conf)
    return cid


def use_preset(col, did, cid) -> None:
    deck = col.decks.get(did)
    deck["conf"] = cid
    col.decks.save(deck)


def add_note(col, m, did, fields, tags):
    if hasattr(col, "new_note"):
        note = col.new_note(m)
    else:
        from anki.notes import Note

        note = Note(col, m)
    for i, value in enumerate(fields):
        note.fields[i] = value
    note.tags = list(tags)
    if hasattr(col, "add_note"):
        col.add_note(note, did)
    else:
        col.addNote(note)
        col.decks.setDeck([c.id for c in note.cards()], did)
    return sorted(note.cards(), key=lambda c: c.ord)


def card_ids(note_cards):
    return [c.id for c in note_cards]


def get_card(col, cid):
    return first(col, "get_card", "getCard")(cid)


def answer(col, cid, ease) -> None:
    card = get_card(col, cid)
    if card.queue < 0:
        raise SystemExit("card %d is not answerable (queue %d)" % (cid, card.queue))
    first(card, "start_timer", "startTimer")()
    col.sched.answerCard(card, ease)


def filtered(col, name, search, limit, order):
    dm = col.decks
    did = first(dm, "new_filtered", "newDyn")(name)
    deck = dm.get(did)
    deck["terms"] = [[search, limit, order]]
    deck["resched"] = True
    dm.save(deck)
    if hasattr(col, "save"):
        col.save()
    first(col.sched, "rebuild_filtered_deck", "rebuildDyn")(did)
    return did


def remove_notes(col, nids) -> None:
    first(col, "remove_notes", "remNotes")(nids)


def remove_deck(col, did) -> None:
    if hasattr(col.decks, "remove"):
        col.decks.remove([did])
    else:
        col.decks.rem(did)


def suspend(col, cids) -> None:
    first(col.sched, "suspend_cards", "suspendCards")(cids)


def bury(col, cids) -> None:
    first(col.sched, "bury_cards", "buryCards")(cids, manual=True)


def reset(col) -> None:
    if hasattr(col.sched, "reset"):
        col.sched.reset()


# --- standard content -------------------------------------------------------------


def standard(out: str, expect_ver: int, downgrade: bool, ease_cases: bool, drop_keys: dict) -> None:
    work = tempfile.mkdtemp(prefix="fx-")
    path = os.path.join(work, "collection.anki2")
    set_clock(T0)
    col = open_col(path)
    # As early as the release allows: before anything rewrites a row that holds
    # stock text, so no stale copy of it is freed without being zeroed.
    secure_delete(col)
    ensure_v2(col)
    stock = list(col.models.all())
    stock_text = stock_texts(stock)
    pair = notetype(col, "Pair", False, ["Front", "Back"], PAIR_TEMPLATES, PAIR_CSS)
    gap = notetype(col, "Gap", True, ["Text", "Extra"], GAP_TEMPLATES, GAP_CSS)
    for m in stock:
        remove_notetype(col, m)

    if ease_cases:
        # A release at 15 stores the legacy initialFactor F as F / 10 (2500 as
        # 250.0), but the Default preset it creates holds 2.5 until the legacy
        # API saves its default starting ease (U16 ease cases: Default 250.0).
        dm = col.decks
        conf = first(dm, "get_config", "getConf")(1)
        conf["new"]["initialFactor"] = 2500
        first(dm, "update_config", "updateConf")(conf)

    lang = col.decks.id("Lang")
    verbs = col.decks.id("Lang::Verbs")
    slow = preset(col, "Slow", perDay=5, initialFactor=2300, delays=[2, 15])
    use_preset(col, verbs, slow)

    # Added first, so the Cram deck (cards in order added, limit 1) takes it.
    g2 = add_note(col, gap, lang, ["Water boils at {{c1::one hundred}} degrees.", "at sea level"], [])
    p1 = add_note(col, pair, lang, ["river", "rio"], ["vocab"])
    p2 = add_note(col, pair, lang, ["bridge", "puente"], ["vocab", "Mixed_Case"])
    g1 = add_note(col, gap, lang, ["{{c1::Rain}} falls and {{c2::snow}} drifts.", "weather"], ["Mixed_Case"])
    p3 = add_note(col, pair, verbs, ["to walk", "andar"], ["vocab::verbs"])
    p4 = add_note(col, pair, verbs, ["to read", "leer"], ["vocab::verbs", "Mixed_Case"])
    doomed = add_note(col, pair, lang, ["dust", "polvo"], ["vocab"])
    gone = col.decks.id("Gone")
    filtered(col, "Cram", "deck:Lang", 1, DYN_ADDED)

    set_clock(T0 + 60)
    reset(col)
    answer(col, p1[0].id, 4)
    answer(col, p2[0].id, 4)

    set_clock(T0 + 5 * DAY)
    reset(col)
    answer(col, p1[0].id, 4)  # review, interval of 10 days or more
    answer(col, p2[0].id, 1)  # relearning
    answer(col, g1[0].id, 1)  # learning
    answer(col, p4[0].id, 3)  # learning (Slow: second step)
    suspend(col, [p3[0].id])
    bury(col, [p3[1].id])
    remove_notes(col, [doomed[0].nid])
    remove_deck(col, gone)

    ease = {}
    if ease_cases:
        presets = {
            "Fresh": preset(col, "Fresh", initialFactor=25),
            "Edge": preset(col, "Edge", initialFactor=1300),
            "Above": preset(col, "Above", initialFactor=1310),
        }
        low = col.decks.id("EaseLow")
        edge = col.decks.id("EaseEdge")
        above = col.decks.id("EaseAbove")
        use_preset(col, low, presets["Fresh"])
        use_preset(col, edge, presets["Edge"])
        use_preset(col, above, presets["Above"])
        l1 = add_note(col, pair, low, ["ease low one", "uno"], [])
        l2 = add_note(col, pair, low, ["ease low two", "dos"], [])
        l3 = add_note(col, pair, low, ["ease low three", "tres"], [])
        e1 = add_note(col, pair, edge, ["edge", "borde"], [])
        a1 = add_note(col, pair, above, ["above", "arriba"], [])
        filtered(col, "EaseCram", "cid:%d" % l3[1].id, 1, DYN_ADDED)
        ease = {
            l1[1].id: 1300, l2[0].id: 2000, l2[1].id: 2001, l3[0].id: 2500, l3[1].id: 1800,
            e1[0].id: 1500, a1[0].id: 1500,
        }

    lapsed = p2[0].id
    if downgrade:
        col.close(downgrade=True)
    else:
        col.close()
    con = connect(path)
    ver = con.execute("select ver from col").fetchone()[0]
    con.close()
    if ver != expect_ver:
        raise SystemExit("this release wrote col.ver %d, not %d" % (ver, expect_ver))
    finish(path, lapsed, ease, drop_keys)
    shutil.copyfile(path, out)
    shutil.rmtree(work)
    no_stock_left([out], stock_text)
    print(json.dumps(summary(out), sort_keys=True))


def connect(path):
    con = sqlite3.connect(path, isolation_level=None)
    con.create_collation("unicase", _unicase)
    return con


def _text(value) -> str:
    """A JSON value stored as TEXT or as a BLOB of UTF-8."""
    return value if isinstance(value, str) else bytes(value).decode()


def _unicase(a, b):
    a, b = a.casefold(), b.casefold()
    return (a > b) - (a < b)


def finish(path, lapsed, ease, drop_keys) -> None:
    """The SQL edits of U16 on the closed file, in one transaction."""
    con = connect(path)
    ver = con.execute("select ver from col").fetchone()[0]
    con.execute("begin")
    con.execute("update cards set factor = 1400 where id = ?", (lapsed,))
    for cid, factor in ease.items():
        con.execute("update cards set factor = ? where id = ?", (factor, cid))
    if ver < 18:
        # The first row stays (U15); the copy repeats its key with another usn.
        con.execute(
            "insert into graves (usn, oid, type) "
            "select usn + 7, oid, type from graves order by rowid limit 1"
        )
    if ver == 11:
        models = json.loads(con.execute("select models from col").fetchone()[0])
        decks = json.loads(con.execute("select decks from col").fetchone()[0])
        for m in models.values():
            if m["name"] == "Pair":
                m["meguriExtra"] = [1]
        for d in decks.values():
            if d["name"] == "Lang":
                d["meguriExtra"] = [1]
        _drop(models, decks, None, drop_keys)
        con.execute("update col set models = ?, decks = ?", (json.dumps(models), json.dumps(decks)))
        conf = con.execute("select conf from col").fetchone()[0].rstrip()
        assert conf.endswith("}") and conf != "{}"
        con.execute("update col set conf = ?", (conf[:-1] + ', "meguriFixture": ' + MEGURI_FIXTURE + "}",))
    else:
        if ver == 14 and drop_keys:
            # Rewrite only what changed, in the storage class Anki used.
            row = con.execute("select models, decks from col").fetchone()
            models, decks = json.loads(row[0]), json.loads(row[1])
            confs = {r[0]: (json.loads(_text(r[1])), type(r[1])) for r in con.execute("select id, config from deck_config")}
            if _drop(models, decks, {k: v[0] for k, v in confs.items()}, drop_keys):
                con.execute("update col set models = ?, decks = ?", (json.dumps(models), json.dumps(decks)))
                for cid, (conf, kind) in confs.items():
                    text = json.dumps(conf)
                    con.execute(
                        "update deck_config set config = ? where id = ?",
                        (text if kind is str else text.encode(), cid),
                    )
        con.execute(
            "insert into config (KEY, usn, mtime_secs, val) values ('meguriFixture', 0, ?, ?)",
            (T0 + 5 * DAY, MEGURI_FIXTURE.encode()),
        )
    con.execute("commit")
    con.close()
    _no_sidecars(path)


def _drop(models, decks, confs, drop_keys) -> bool:
    """Remove legacy keys an old release writes, so that every converted object
    keeps at most one unknown key (U16). True when anything was removed."""
    removed = []

    def pop(obj, kind):
        for key in drop_keys.get(kind, []):
            if key in obj:
                removed.append((kind, key))
                del obj[key]

    for m in models.values():
        pop(m, "notetype")
        for f in m.get("flds", []):
            pop(f, "field")
        for t in m.get("tmpls", []):
            pop(t, "template")
    for d in decks.values():
        pop(d, "deck")
    for c in (confs or {}).values():
        pop(c, "preset")
        for part in ("new", "rev", "lapse"):
            pop(c.get(part, {}), "preset." + part)
    return bool(removed)


def _no_sidecars(path) -> None:
    for suffix in ("-wal", "-shm", "-journal"):
        if os.path.exists(path + suffix):
            raise SystemExit("%s%s was left behind" % (path, suffix))


# --- derived fixtures -------------------------------------------------------------


def names(src: str, out: str) -> None:
    """schema-11-names (U16 name cases), written into the JSON of col."""
    shutil.copyfile(src, out)
    con = connect(out)
    assert con.execute("select ver from col").fetchone()[0] == 11
    models = json.loads(con.execute("select models from col").fetchone()[0])
    decks = json.loads(con.execute("select decks from col").fetchone()[0])
    pair = next(m for m in models.values() if m["name"] == "Pair")
    lang = next(d for d in decks.values() if d["name"] == "Lang")
    top = max(int(k) for k in models)

    def copy_pair(new_id, name):
        m = json.loads(json.dumps(pair))
        m["id"] = new_id
        m["name"] = name
        models[str(new_id)] = m
        return m

    copy_pair(top + 1, "pair")
    copy_pair(top + 2, "PAIR")
    copy_pair(0, "Zero")
    dupes = copy_pair(top + 3, "Dupes")
    dupes["flds"][0]["name"] = "Front"
    dupes["flds"][1]["name"] = "front"
    dupes["tmpls"][0]["name"] = "Card"
    dupes["tmpls"][1]["name"] = "CARD"
    copy_pair(top + 4, "Cafe\u0301")
    for did, name in (
        (10, "Foo"), (11, "foo"), (12, "  Spaced :: Out "), (13, "a::::b"), (14, "x\u0007y"), (15, "Cafe\u0301"),
    ):
        d = json.loads(json.dumps(lang))
        d["id"] = did
        d["name"] = name
        d["mod"] = 1000
        d["usn"] = 5
        decks[str(did)] = d
    con.execute("begin")
    nid = con.execute(
        "select n.id from notes n join cards c on c.nid = n.id where n.mid = ? and c.did = ? "
        "order by n.id limit 1",
        (pair["id"], lang["id"]),
    ).fetchone()[0]
    con.execute("update notes set mid = 0 where id = ?", (nid,))
    con.execute(
        "update col set models = ?, decks = ?, tags = ?",
        (json.dumps(models, ensure_ascii=False), json.dumps(decks, ensure_ascii=False),
         '{"Tag": 3, "tag": 4, "other": -1}'),
    )
    con.execute("commit")
    con.close()
    _no_sidecars(out)
    print(json.dumps(summary(out), sort_keys=True))


def version(src: str, out: str, ver: int, journal: str | None) -> None:
    """A refused file (open-and-connections C1): src with col.ver set, in the
    journal mode asked for (the source's when none is)."""
    shutil.copyfile(src, out)
    con = connect(out)
    if journal:
        con.execute("pragma journal_mode = %s" % journal)
    con.execute("update col set ver = ?", (ver,))
    con.close()
    _no_sidecars(out)
    print(json.dumps(summary(out), sort_keys=True))


def journal(src: str, out: str, mode: str) -> None:
    shutil.copyfile(src, out)
    con = connect(out)
    con.execute("pragma journal_mode = %s" % mode)
    con.close()
    _no_sidecars(out)


def v1_scheduler(src: str, out: str) -> None:
    """schedVer 1 at 18 (config row), or no schedVer member at 11 (col.conf)."""
    shutil.copyfile(src, out)
    con = connect(out)
    ver = con.execute("select ver from col").fetchone()[0]
    con.execute("begin")
    if ver == 11:
        # Remove the member from the text, so every other member keeps its
        # exact text (meguriFixture's -0 would not survive a re-encoding).
        text = con.execute("select conf from col").fetchone()[0]
        edited, n = re.subn(r'"schedVer"\s*:\s*-?[0-9]+\s*,\s*', "", text)
        before, after = json.loads(text), json.loads(edited)
        del before["schedVer"]
        assert n == 1 and before == after
        con.execute("update col set conf = ?", (edited,))
    else:
        n = con.execute("update config set val = ? where KEY = 'schedVer'", (b"1",)).rowcount
        assert n == 1
    con.execute("commit")
    con.close()
    _no_sidecars(out)
    print(json.dumps(summary(out), sort_keys=True))


# --- inspection ---------------------------------------------------------------------


def _varint(buf, i):
    shift = value = 0
    while True:
        b = buf[i]
        i += 1
        value |= (b & 0x7F) << shift
        shift += 7
        if b < 0x80:
            return value, i


def proto_fields(buf):
    """[(field number, wire type, value)] of a protobuf message."""
    out, i = [], 0
    buf = bytes(buf)
    while i < len(buf):
        tag, i = _varint(buf, i)
        num, wire = tag >> 3, tag & 7
        if wire == 0:
            value, i = _varint(buf, i)
        elif wire == 1:
            value, i = buf[i : i + 8], i + 8
        elif wire == 2:
            n, i = _varint(buf, i)
            value, i = buf[i : i + n], i + n
        elif wire == 5:
            value, i = buf[i : i + 4], i + 4
        else:
            raise ValueError("wire type %d" % wire)
        out.append((num, wire, value))
    return out


def initial_ease(blob) -> float | None:
    for num, wire, value in proto_fields(blob):
        if num == 11 and wire == 5:
            return round(struct.unpack("<f", value)[0], 6)
    return None


def other_json(blob):
    for num, wire, value in proto_fields(blob):
        if num == 255 and wire == 2:
            return json.loads(value.decode())
    return None


def summary(path: str) -> dict:
    with open(path, "rb") as f:
        header = f.read(100)
    con = sqlite3.connect("file:%s?mode=ro&immutable=1" % path, uri=True)
    con.create_collation("unicase", _unicase)
    q = lambda sql, *a: con.execute(sql, a).fetchall()  # noqa: E731
    ver = q("select ver from col")[0][0]
    tables = sorted(r[0] for r in q("select name from sqlite_master where type = 'table'"))
    out = {
        "file": os.path.basename(path),
        "ver": ver,
        "wal_header": header[18] == 2 and header[19] == 2,
        "tables": tables,
        "cards": [list(r) for r in q("select type, queue, count(*) from cards group by type, queue order by 1, 2")],
        "card_count": q("select count(*) from cards")[0][0],
        "revlog": q("select count(*) from revlog")[0][0],
        "graves": [list(r) for r in q("select oid, type, usn from graves order by %s" % ("oid, type" if ver >= 18 else "rowid"))],
        "factors": [list(r) for r in q("select factor, count(*) from cards group by factor order by 1")],
        "max_ivl": q("select max(ivl) from cards")[0][0],
        "in_filtered": q("select count(*) from cards where odid != 0")[0][0],
    }
    if "config" not in tables:  # the schema-11 layout (10 to 13 included)
        conf = json.loads(q("select conf from col")[0][0])
        out["config"] = {k: conf.get(k) for k in ("schedVer", "sched2021", "creationOffset", "meguriFixture")}
        out["notetypes"] = sorted(m["name"] for m in json.loads(q("select models from col")[0][0]).values())
        out["decks"] = sorted(d["name"] for d in json.loads(q("select decks from col")[0][0]).values())
        out["presets"] = sorted(
            (c["name"], c["new"]["initialFactor"]) for c in json.loads(q("select dconf from col")[0][0]).values()
        )
        out["tags"] = q("select tags from col")[0][0]
    else:
        cfg = {r[0]: _text(r[1]) for r in q("select KEY, val from config")}
        out["config"] = {k: cfg.get(k) for k in ("schedVer", "sched2021", "creationOffset", "meguriFixture")}
        if "notetypes" in tables:
            out["notetypes"] = sorted(r[0] for r in q("select name from notetypes"))
            out["decks"] = sorted(r[0].replace("\x1f", "::") for r in q("select name from decks"))
            out["presets"] = sorted((r[0], initial_ease(r[1])) for r in q("select name, config from deck_config"))
        else:
            out["notetypes"] = sorted(m["name"] for m in json.loads(q("select models from col")[0][0]).values())
            out["decks"] = sorted(d["name"] for d in json.loads(q("select decks from col")[0][0]).values())
            out["presets"] = sorted(
                (c["name"], c["new"]["initialFactor"])
                for c in (json.loads(_text(r[0])) for r in q("select config from deck_config"))
            )
        out["tags"] = [list(r) for r in q("select * from tags order by 1")]
    con.close()
    out["sidecars"] = [s for s in ("-wal", "-shm", "-journal") if os.path.exists(path + s)]
    return out


def stock_texts(models) -> list:
    """[(label, text)]: the CSS and template text of the note types a release
    created by itself. Kept in memory only, never written anywhere (R1959)."""
    out = []
    for m in models:
        out.append(("%s css" % m["name"], m["css"]))
        for t in m["tmpls"]:
            out.append(("%s/%s front" % (m["name"], t["name"]), t["qfmt"]))
            out.append(("%s/%s back" % (m["name"], t["name"]), t["afmt"]))
    return out


def _encodings(text: str) -> set:
    """The bytes a text takes in a fixture: UTF-8 (a TEXT column or a protobuf
    string) or the inside of a JSON string, escaped by Python or by Anki."""
    return {text.encode(), json.dumps(text)[1:-1].encode(), json.dumps(text, ensure_ascii=False)[1:-1].encode()}


def _pieces(blob: bytes, n: int) -> set:
    n = min(n, len(blob))
    return {blob[i : i + n] for i in range(len(blob) - n + 1)}


def no_stock_left(paths, texts) -> None:
    """Fail when any file still holds a piece of stock text anywhere in its
    bytes, free pages and unused space inside pages included (U16). Pieces that
    the standard content's own note types also hold ({{Front}}, {{cloze:Text}})
    are not Anki's and are skipped."""
    own = b"\0".join(e for t in OWN_TEXT for e in _encodings(t))
    needles = {}
    for label, text in texts:
        for enc in _encodings(text):
            needles.setdefault(label, set()).update(p for p in _pieces(enc, STOCK_PIECE) if p not in own)
    failed = []
    for path in paths:
        with open(path, "rb") as f:
            data = f.read()
        windows = _pieces(data, STOCK_PIECE)
        found = sorted(
            label for label, pieces in needles.items()
            if any(p in windows if len(p) == STOCK_PIECE else p in data for p in pieces)
        )
        if found:
            failed.append("%s: %s" % (os.path.basename(path), ", ".join(found)))
    if failed:
        raise SystemExit("stock note type text left in a fixture:\n  " + "\n  ".join(failed))


def stock_check(paths) -> None:
    """(26.09.3 only) no_stock_left for finished fixtures, against the stock
    note types this release creates, read from a scratch collection."""
    work = tempfile.mkdtemp(prefix="stock-")
    col = open_col(os.path.join(work, "collection.anki2"))
    texts = stock_texts(col.models.all())
    col.close()
    shutil.rmtree(work)
    if not texts:
        raise SystemExit("this release created no stock note type")
    no_stock_left(paths, texts)
    print(json.dumps({"stock_check": [os.path.basename(p) for p in paths]}))


def upgrade_check(path: str) -> dict:
    """Upgrade a copy with this image's Anki (26.09.3) and list the converted
    objects whose `other` JSON has more than one top-level key (unstable in
    Anki's output, U11) and any config key the Python layer added."""
    work = tempfile.mkdtemp(prefix="up-")
    copy = os.path.join(work, "collection.anki2")
    shutil.copyfile(path, copy)
    before = summary(copy)
    col = open_col(copy)
    col.close()
    con = connect(copy)
    problems, others = [], {}

    def look(kind, name, blob):
        other = other_json(blob)
        if other:
            others.setdefault(kind, {})[name] = sorted(other)
            if len(other) > 1:
                problems.append([kind, name, sorted(other)])

    for ntid, name, config in con.execute("select id, name, config from notetypes"):
        look("notetype", name, config)
        for ord_, fname, fconfig in con.execute("select ord, name, config from fields where ntid = ?", (ntid,)):
            look("field", "%s/%s" % (name, fname), fconfig)
        for ord_, tname, tconfig in con.execute("select ord, name, config from templates where ntid = ?", (ntid,)):
            look("template", "%s/%s" % (name, tname), tconfig)
    for name, common in con.execute("select name, common from decks"):
        look("deck", name.replace("\x1f", "::"), common)
    for name, config in con.execute("select name, config from deck_config"):
        look("preset", name, config)
    keys = sorted(r[0] for r in con.execute("select KEY from config"))
    con.close()
    shutil.rmtree(work)
    return {"file": os.path.basename(path), "from": before["ver"], "problems": problems, "others": others,
            "config_keys": keys}


def main(argv) -> int:
    cmd, args = argv[0], argv[1:]
    if cmd == "standard":
        out = args[0]
        drop = {}
        if "--drop-keys" in args:
            drop = json.loads(args[args.index("--drop-keys") + 1])
        expect = int(args[args.index("--expect-ver") + 1])
        standard(out, expect, "--downgrade" in args, "--ease-cases" in args, drop)
    elif cmd == "names":
        names(args[0], args[1])
    elif cmd == "version":
        mode = args[args.index("--journal") + 1] if "--journal" in args else None
        version(args[0], args[1], int(args[2]), mode)
    elif cmd == "journal":
        journal(args[0], args[1], args[2])
    elif cmd == "v1-scheduler":
        v1_scheduler(args[0], args[1])
    elif cmd == "summary":
        for path in args:
            print(json.dumps(summary(path), sort_keys=True, ensure_ascii=False))
    elif cmd == "upgrade-check":
        for path in args:
            print(json.dumps(upgrade_check(path), sort_keys=True, ensure_ascii=False))
    elif cmd == "stock-check":
        stock_check(args)
    else:
        print(__doc__, file=sys.stderr)
        return 64
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
