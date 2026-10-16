# SPDX-License-Identifier: AGPL-3.0-or-later
"""The kitchen-sink fixture of the preservation suite
(specs/conformance/normalized-dump.md N9): conformance/fixtures/preservation/
kitchen-sink.anki2, made from the recipe scenario preservation/recipe/
kitchen-sink on the oracle.

Runs inside the oracle image under run.sh (libfaketime with a frozen clock
file, as a worker has), with the driver on PYTHONPATH. Driven by make.sh; see
README.md.

  python3 make.py recipe RECIPE OUT
      run the recipe's steps without its last observe step, as a worker runs a
      scenario, with SQLite's secure_delete on for every connection Anki opens
      (N9.2 step 3); close the collection without the downgrade and copy the
      file to OUT
  python3 make.py inspect FILE
      sqlite3 checks of a made file: schema 18, WAL journal mode in the header
      and through SQLite, integrity check ok, no -wal, -shm or -journal file
      beside it, the N6.1 pending-change query clean (N9.3); prints a summary
  python3 make.py stock-check FILE RECIPE
      the storage maker's stock-text check (oracle/fixtures/storage/make.py),
      skipping the pieces the recipe's own note types hold
  python3 make.py rebuild-check FILE NOW
      Check Database on a copy at NOW (epoch seconds) changes, outside col and
      config, only the tags table's usn, from the synced state's 9 to -1: it
      rebuilds that table from the notes' tags at usn -1 (collection-check.md
      K5), so `tools/mg oracle check` lists tags among its changed tables for
      any synced collection with tags; prints the differing pointers
  python3 make.py open-check FIXTURES OPEN_SCENARIO RECIPE RECIPE_GOLDEN
      a fresh worker runs preservation/fixture/open on FIXTURES/preservation/
      kitchen-sink.anki2; its observe step must be the recipe's last step, and
      what it observes must equal that step's value in the recipe golden
      exactly (types included); the golden must be the recipe's as it is now

Every command exits non-zero when a check fails; make.sh writes the fixture
only when all of them pass.
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import sqlite3
import sys
import tempfile
from pathlib import Path

from driver import registry, session as session_module, timecontrol
from driver.canon import load_scenario_file, scenario_sha256
from driver.clock import Clock
from driver.harness import WorkerProcess, json_diff
from driver.session import Session

HERE = Path(__file__).resolve().parent
SIDECARS = ("-wal", "-shm", "-journal")
# specs/conformance/normalized-dump.md N6.1, as written there.
PENDING_SQL = (
    "SELECT (SELECT count() FROM notes WHERE usn = -1), (SELECT count() FROM cards WHERE usn = -1), "
    "(SELECT count() FROM revlog WHERE usn = -1), (SELECT count() FROM decks WHERE usn = -1), "
    "(SELECT count() FROM deck_config WHERE usn = -1), (SELECT count() FROM notetypes WHERE usn = -1), "
    "(SELECT count() FROM templates WHERE usn = -1), (SELECT count() FROM tags WHERE usn = -1), "
    "(SELECT count() FROM config WHERE usn = -1), (SELECT count() FROM graves), "
    "(SELECT mod > ls FROM col), (SELECT scm > ls FROM col)"
)


def fail(message: str) -> None:
    raise SystemExit("make.py: " + message)


def _sidecars(path: str) -> list[str]:
    return [s for s in SIDECARS if os.path.exists(path + s)]


def _secure(open_collection):
    """Collection() with secure_delete turned on before anything else runs on
    the connection: from then on SQLite overwrites with zeros every cell it
    deletes and every page it frees, so the stock note types the recipe
    deletes leave nothing in the file (R1959). Anki's DB proxy marks the
    collection modified for the pragma, which only a commit request would
    turn into a col.mod write, and nothing sends one."""

    def opened(path):
        col = open_collection(path)
        col.db.execute("pragma secure_delete = on")
        if col.db.scalar("pragma secure_delete") != 1:
            fail("secure_delete did not turn on for Anki's connection")
        return col

    return opened


# --- recipe ------------------------------------------------------------------------


def recipe(recipe_path: str, out: str) -> None:
    clock_file = os.environ["FAKETIME_TIMESTAMP_FILE"]
    scratch = os.environ["ORACLE_SCRATCH"]
    if timecontrol.check(clock_file, scratch) != "faketime":
        fail("the frozen clock does not reach Python or Anki (run make.py through run.sh)")
    registry.load_all()
    scenario = load_scenario_file(Path(recipe_path))
    steps = scenario.get("steps", [])
    if not steps or not isinstance(steps[-1], dict) or set(steps[-1]) != {"observe"}:
        fail("the recipe's last step is not an observe step")
    scenario = dict(scenario, steps=steps[:-1])
    # Every Collection() of the session (creation, reopen) gets secure_delete.
    session_module.Collection = _secure(session_module.Collection)
    # The process's zone (run.sh: UTC); a recipe in another zone is tz_mismatch.
    session = Session(scenario, os.environ["TZ"], scratch, Clock(clock_file))
    try:
        session.run_all()
        path = session.path
        session.close_collection()
        left = _sidecars(path)
        if left:
            fail("Anki left %s beside the closed collection" % ", ".join(left))
        shutil.copyfile(path, out)
    finally:
        session.close()
    print(json.dumps({"recipe": scenario["id"], "steps": len(scenario["steps"]), "out": os.path.basename(out)}))


# --- inspect -----------------------------------------------------------------------


def _unicase(a, b):
    # Every unicase value in the fixture (deck, note type and tag names) is
    # ASCII, where case folding is Anki's comparison.
    a, b = a.casefold(), b.casefold()
    return (a > b) - (a < b)


def inspect(path: str) -> None:
    left = _sidecars(path)
    if left:
        fail("%s has %s beside it" % (path, ", ".join(left)))
    with open(path, "rb") as f:
        header = f.read(100)
    # File format write and read versions 2: WAL (SQLite file format 1.3.0).
    wal_header = header[18] == 2 and header[19] == 2
    work = tempfile.mkdtemp(prefix="inspect-")
    try:
        copy = os.path.join(work, "collection.anki2")
        shutil.copyfile(path, copy)
        con = sqlite3.connect(copy, isolation_level=None)
        con.create_collation("unicase", _unicase)
        q = lambda sql: con.execute(sql).fetchall()  # noqa: E731
        summary = {
            "file": os.path.basename(path),
            "bytes": os.path.getsize(path),
            "wal_header": wal_header,
            "journal_mode": q("pragma journal_mode")[0][0],
            "integrity_check": [r[0] for r in q("pragma integrity_check")],
            "freelist_count": q("pragma freelist_count")[0][0],
            "ver": q("select ver from col")[0][0],
            "objects": q("select count() from sqlite_master")[0][0],
            "counts": {t: q("select count() from %s" % t)[0][0] for t in (
                "notes", "cards", "revlog", "decks", "deck_config", "notetypes", "fields", "templates",
                "tags", "config", "graves")},
            "pending": list(q(PENDING_SQL)[0]),
        }
        con.close()
    finally:
        shutil.rmtree(work)
    print(json.dumps(summary, sort_keys=True))
    problems = []
    if summary["ver"] != 18:
        problems.append("col.ver is %s, not 18" % summary["ver"])
    if not wal_header or summary["journal_mode"] != "wal":
        problems.append("the journal mode is not WAL (header %s, pragma %s)" % (wal_header, summary["journal_mode"]))
    if summary["integrity_check"] != ["ok"]:
        problems.append("the integrity check reports %s" % summary["integrity_check"][:5])
    if summary["pending"] != [0] * 12:
        problems.append("the N6.1 query is not clean: %s" % summary["pending"])
    if problems:
        fail("; ".join(problems))


# --- stock-check -------------------------------------------------------------------


def own_texts(recipe_path: str) -> list[str]:
    """The CSS and template text of the recipe's own note types: what setup
    gives them and what its legacy_update steps write (qfmt, afmt and the
    browser templates)."""
    scenario = load_scenario_file(Path(recipe_path))
    out = []
    for nt in scenario.get("setup", {}).get("notetypes", []):
        out.append(nt.get("css", ""))
        for t in nt.get("templates", []):
            out += [t.get("front", ""), t.get("back", "")]
    for step in scenario.get("steps", []):
        update = step.get("legacy_update") if isinstance(step, dict) else None
        if not isinstance(update, dict) or "notetype" not in update:
            continue
        patch = update.get("patch", {})
        out.append(patch.get("css", ""))
        for t in patch.get("tmpls", []):
            out += [t.get(k, "") for k in ("qfmt", "afmt", "bqfmt", "bafmt")]
    return [t for t in out if t]


def stock_check(path: str, recipe_path: str) -> None:
    spec = importlib.util.spec_from_file_location("storage_make", HERE.parent / "storage" / "make.py")
    storage = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(storage)
    storage.stock_check([path], own_texts(recipe_path))


# --- rebuild-check -----------------------------------------------------------------


def rebuild_check(path: str, now: str) -> None:
    from anki._backend import RustBackend
    from anki.collection import Collection

    from driver.observe.db_dump import dump
    from driver.ops.maintenance import check_database
    from driver.ops.sync_client import differences

    clock = Clock(os.environ["FAKETIME_TIMESTAMP_FILE"])
    clock.set(int(now))
    work = tempfile.mkdtemp(prefix="rebuild-", dir=os.environ["ORACLE_SCRATCH"])
    try:
        copy = os.path.join(work, "collection.anki2")
        shutil.copyfile(path, copy)
        col = Collection(copy, backend=RustBackend(langs=["en"]))
        try:
            before = dump(col, raw=True)
            counts = check_database(col)
            after = dump(col, raw=True)
        finally:
            col.close(downgrade=False)
    finally:
        shutil.rmtree(work)
    if counts is None or any(counts.values()):
        fail("Check Database reports %s" % counts)
    pointers = differences(after, before)
    other = []
    for pointer in pointers:
        parts = pointer.split("/")
        if parts[1] in ("col", "config"):
            continue
        if len(parts) == 4 and parts[1] == "tags" and parts[3] == "usn":
            tag = parts[2].replace("~1", "/").replace("~0", "~")
            if before["tags"][tag]["usn"] == 9 and after["tags"][tag]["usn"] == -1:
                continue
        other.append(pointer)
    print(json.dumps({"rebuild_check": os.path.basename(path), "check_db": counts, "differences": pointers}))
    if other:
        fail("Check Database changed more than col, config and the tags' usn: %s" % other[:20])


# --- open-check --------------------------------------------------------------------


def open_check(fixtures: str, open_scenario: str, recipe_path: str, recipe_golden: str) -> None:
    scenario = load_scenario_file(Path(open_scenario))
    recipe = load_scenario_file(Path(recipe_path))
    golden = json.loads(Path(recipe_golden).read_text(encoding="utf-8"))
    if golden.get("scenario_sha256") != scenario_sha256(recipe):
        fail("the recipe golden is stale: re-record %s first" % recipe.get("id"))
    steps = scenario.get("steps", [])
    if len(steps) != 2 or set(steps[0]) != {"open"} or steps[1] != recipe["steps"][-1]:
        fail("%s is not an open step followed by the recipe's last step" % scenario.get("id"))
    expected = golden["observations"]["steps"][-1]
    with WorkerProcess(scenario["clock"]["tz"], extra_env={"ORACLE_FIXTURES": fixtures}) as worker:
        resp = worker.request({"cmd": "run", "scenario": scenario})
    if not resp.get("ok"):
        fail("the oracle could not run %s: %s" % (scenario.get("id"), resp.get("error")))
    out = resp["observations"]["steps"]
    if out[0] != {}:
        fail("open answered %s" % out[0])
    diffs = json_diff(expected, out[1])
    if diffs:
        shown = "\n  ".join("%s: %s -> %s" % (d["pointer"], d["expected"], d["actual"]) for d in diffs[:20])
        fail("%d differences from the recipe golden's last step:\n  %s" % (len(diffs), shown))
    print(json.dumps({"open_check": scenario["id"], "equals": recipe["id"] + ", last step"}))


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__, file=sys.stderr)
        return 64
    cmd, args = argv[0], argv[1:]
    if cmd == "recipe" and len(args) == 2:
        recipe(*args)
    elif cmd == "inspect" and len(args) == 1:
        inspect(*args)
    elif cmd == "stock-check" and len(args) == 2:
        stock_check(*args)
    elif cmd == "rebuild-check" and len(args) == 2:
        rebuild_check(*args)
    elif cmd == "open-check" and len(args) == 4:
        open_check(*args)
    else:
        print(__doc__, file=sys.stderr)
        return 64
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
