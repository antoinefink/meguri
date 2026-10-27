# SPDX-License-Identifier: AGPL-3.0-or-later
"""Deck packages written by an old Anki client (specs/package/apkg-import.md
I18.1; specs/package/notetype-merge.md J15.1 by-name). Oracle maintainers only.

Runs in the 2.1.26 legacy image of oracle/fixtures/storage (Dockerfile.legacy,
wheels pinned by hash) through oldclient-run.sh, with the storage maker
mounted at /storage-maker for its release-neutral helpers:

    python3 oldclient.py OUT

Writes OUT/v1-scheduler.apkg and OUT/legacy-1.apkg: Anki 2.1.26's own deck
package export (AnkiPackageExporter) of one v1-scheduler collection, with and
without scheduling. With the v1 scheduler 2.1.26 writes the legacy 1 layout
(`collection.anki2` at schema 11, a JSON media map, no `meta`). Its note type
stores no field or template ids. The archives are then rewritten with the same
entries in the same order, stored and dated 1980-01-01 00:00:00 (the
exporter's zip times are the real clock's), each with its exact bytes except
`collection.anki2`, whose `col` JSON columns get their keys sorted (canonical).

Same bytes every run: Python's clock is replaced by one that moves 1 ms per
read from the instant set_clock gives (2.1.26 reads it for every id and waits
for it to move when it picks a note type id), libfaketime freezes the clock
the Rust backend and the C library read at that instant, and `random` is
seeded (note GUIDs, fuzz).

Stock note types (R1959): secure_delete is turned on on the connection of the
source collection and of the exporter's new collection right after 2.1.26
creates them (it writes its five stock note types into both), and each
exported collection is checked against 2.1.26's stock text here and against
26.09.3's by make.sh.
"""

from __future__ import annotations

import importlib.util
import json
import os
import random
import shutil
import sys
import tempfile
import time
import zipfile

DAY = 86400
T0 = 1788249600  # 2026-09-01T08:00:00Z, day 0 (the storage fixtures' day 0)
MEDIA_NAME = "dot.png"
MEDIA_BYTES = b"dot fixture\n"

_clock = [float(T0)]


def _fake_time() -> float:
    _clock[0] += 0.001
    return _clock[0]


def set_clock(epoch: int) -> None:
    _clock[0] = float(epoch)
    with open(os.environ["FAKETIME_TIMESTAMP_FILE"], "w") as f:
        f.write("%d\n" % epoch)


time.time = _fake_time
random.seed(20260901)

_spec = importlib.util.spec_from_file_location("storage_make", "/storage-maker/make.py")
sm = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(sm)


def preset(col, name):
    dm = col.decks
    cid = dm.add_config_returning_id(name)
    conf = dm.getConf(cid)
    conf["new"]["delays"] = [1, 1440]
    conf["lapse"]["delays"] = [10, 1440]
    dm.updateConf(conf)
    return cid


def filtered(col, name, nids, resched, delays):
    dm = col.decks
    did = dm.newDyn(name)
    deck = dm.get(did)
    deck["terms"] = [["nid:" + ",".join(str(n) for n in nids), 100, sm.DYN_ADDED]]
    deck["resched"] = resched
    deck["delays"] = delays
    dm.save(deck)
    col.save()
    col.sched.rebuildDyn(did)
    return did


COL_JSON = ("conf", "models", "decks", "dconf", "tags")


def canonical(data: bytes, work: str) -> bytes:
    """The collection with the JSON columns of `col` written with their keys
    sorted: 2.1.26's backend writes them in hash-map order, which changes from
    run to run. secure_delete zeroes what the rewrite frees."""
    path = os.path.join(work, "canonical.anki2")
    with open(path, "wb") as f:
        f.write(data)
    con = sm.connect(path)
    con.execute("pragma secure_delete = on")
    row = con.execute("select %s from col" % ", ".join(COL_JSON)).fetchone()
    values = [json.dumps(json.loads(v), sort_keys=True, separators=(",", ":"), ensure_ascii=False) for v in row]
    if [len(v.encode()) for v in values] != [len(v.encode()) for v in row]:
        raise SystemExit("a JSON column of col changed length when its keys were sorted")
    con.execute("begin")
    con.execute("update col set %s" % ", ".join("%s = ?" % c for c in COL_JSON), values)
    con.execute("commit")
    con.close()
    sm._no_sidecars(path)
    with open(path, "rb") as f:
        out = f.read()
    os.unlink(path)
    return out


def rewrite(path: str, out: str, work: str) -> list:
    with zipfile.ZipFile(path) as z:
        entries = [(i.filename, z.read(i)) for i in z.infolist()]
    entries = [(n, canonical(d, work) if n == "collection.anki2" else d) for n, d in entries]
    with zipfile.ZipFile(out, "w") as z:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            z.writestr(info, data)
    return entries


def make(out: str) -> None:
    from anki import exporting

    stock_text = []
    made = []

    def collection(path):
        col = sm.open_col(path)
        sm.secure_delete(col)
        made.append(path)
        return col

    exporting.Collection = collection
    work = tempfile.mkdtemp(prefix="old-")
    set_clock(T0)
    col = collection(os.path.join(work, "collection.anki2"))
    if col.schedVer() != 1:
        raise SystemExit("2.1.26 made a collection with scheduler %d, not v1" % col.schedVer())
    stock = list(col.models.all())
    stock_text = sm.stock_texts(stock)
    pair = sm.notetype(col, "Pair", False, ["Front", "Back"], sm.PAIR_TEMPLATES[:1], sm.PAIR_CSS)
    for m in stock:
        sm.remove_notetype(col, m)
    lang = col.decks.id("Lang")
    sub = col.decks.id("Lang::Sub")
    steps = preset(col, "Steps")
    sm.use_preset(col, lang, steps)
    sm.use_preset(col, sub, steps)
    with open(os.path.join(col.media.dir(), MEDIA_NAME), "wb") as f:
        f.write(MEDIA_BYTES)
    words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu"]
    tags = {"alpha": ["marked"], "beta": ["leech"], "zeta": ["media"]}
    cards = {}
    for i, w in enumerate(words):
        back = w.upper() + (' <img src="%s">' % MEDIA_NAME if w == "zeta" else "")
        did = sub if i % 4 == 3 else lang
        cards[w] = sm.add_note(col, pair, did, [w, back], tags.get(w, []))[0]
    nid = {w: c.nid for w, c in cards.items()}
    cid = {w: c.id for w, c in cards.items()}

    set_clock(T0 + 60)
    sm.reset(col)
    for w in ("alpha", "beta", "gamma", "eta", "iota", "lambda"):
        sm.answer(col, cid[w], 3)  # easy from new: review, interval 3 or 4 days (fuzz)

    set_clock(T0 + 5 * DAY)
    sm.reset(col)
    for w in ("alpha", "iota", "lambda"):
        sm.answer(col, cid[w], 3)  # review, good: not due again soon
    sm.answer(col, cid["beta"], 1)  # relearning, first step (10 minutes)
    sm.answer(col, cid["gamma"], 1)
    sm.answer(col, cid["delta"], 1)  # learning, first step (1 minute)
    sm.answer(col, cid["epsilon"], 2)  # learning, second step (1 day)
    set_clock(T0 + 5 * DAY + 11 * 60)
    sm.reset(col)
    sm.answer(col, cid["gamma"], 2)  # relearning, second step (1 day)
    # eta is due (interval 3 or 4 days, fuzz), iota is not, theta is new.
    cram = filtered(col, "Cram", [nid["eta"], nid["theta"], nid["iota"]], True, [5, 30, 60])
    # kappa and mu are new, lambda is a review not due.
    peek = filtered(col, "Peek", [nid["kappa"], nid["lambda"], nid["mu"]], False, None)
    sm.reset(col)
    sm.answer(col, cid["theta"], 1)  # learning in Cram, its own three delays
    sm.answer(col, cid["mu"], 1)  # learning in Peek (no rescheduling)
    col.save()
    states = {w: list(col.db.first("select did, odid, type, queue from cards where id = ?", c)) for w, c in cid.items()}
    for w, expect in (("beta", (2, 1)), ("gamma", (2, 3)), ("delta", (1, 1)), ("epsilon", (1, 3)),
                      ("eta", (2, 2)), ("iota", (2, 0)), ("theta", (1, 1)), ("lambda", (2, 0)), ("mu", (1, 1))):
        if tuple(states[w][2:]) != expect:
            raise SystemExit("%s has type and queue %s, not %s" % (w, states[w][2:], list(expect)))
    if states["theta"][0] != cram or states["mu"][0] != peek:
        raise SystemExit("the answered filtered cards left their filtered decks")

    os.makedirs(out, exist_ok=True)
    for name, sched in (("v1-scheduler", True), ("legacy-1", False)):
        exp = exporting.AnkiPackageExporter(col)
        exp.includeSched = sched
        exp.includeMedia = True
        exp.did = None
        raw = os.path.join(work, name + ".apkg")
        exp.exportInto(raw)
        entries = rewrite(raw, os.path.join(out, name + ".apkg"), work)
        layout = [n for n, _ in entries]
        if layout != ["collection.anki2", "0", "media"] or json.loads(dict(entries)["media"]) != {"0": MEDIA_NAME}:
            raise SystemExit("2.1.26 wrote %s with entries %s" % (name, layout))
        inner = os.path.join(work, name + ".inner.anki2")
        with open(inner, "wb") as f:
            f.write(dict(entries)["collection.anki2"])
        sm.no_stock_left([inner], stock_text)
        s = sm.summary(inner)
        print(json.dumps({"package": name + ".apkg", "ver": s["ver"], "cards": s["cards"], "revlog": s["revlog"],
                          "decks": s["decks"], "config": s["config"], "notetypes": s["notetypes"]}, sort_keys=True))
    col.close()
    if len(made) != 3:
        raise SystemExit("expected the source and two exported collections, made %d" % len(made))
    os.chdir("/")
    shutil.rmtree(work)


def main(argv) -> int:
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 64
    make(argv[0])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
