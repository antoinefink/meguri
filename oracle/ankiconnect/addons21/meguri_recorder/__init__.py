# Meguri recorder driver add-on (AGPL-3.0-or-later, see ../../../LICENSE).
#
# Runs inside the pinned Anki next to AnkiConnect (servers/transcripts T5,
# T7.2, T19.1). Everything happens on Anki's main thread, from one 50 ms
# QTimer (real monotonic time, so it also fires inside the add-on's modal
# permission dialog):
#
#   $ORACLE_RECORDER_DIR/ready            written when the profile is open
#   $ORACLE_RECORDER_DIR/permission       one answer per line, consumed in order:
#                                         allow | deny | deny_ignore
#   $ORACLE_RECORDER_DIR/answered         one line per dialog answered
#   $ORACLE_RECORDER_DIR/prompt_waiting   written when a dialog is up and no
#                                         answer is queued (prompt_not_answered)
#   $ORACLE_RECORDER_DIR/request.json     one control request from the recorder;
#                                         the reply is response-<id>.json,
#                                         {"ok": value} or {"error": {kind, ...}}
#
# Control ops (recorder/__main__.py):
#   setup {scenario}          the scenario's setup on Anki's open collection,
#                             through the oracle driver's setup sections ->
#                             {notes, cards, fixture_base}
#   snapshot {}               ids and identities of notes, cards, decks, note
#                             types (with note and card mods, for the clock
#                             check); the media folder, profile name and the
#                             TTS message render-ops Q5.4 replaces
#   observe {specs, notes, cards, step}  the driver's observations; notes and
#                             cards are the real ids by logical number
#   now {}                    Anki's clock (time.time() under libfaketime), so
#                             the recorder sees when an advance has reached it
#
# The driver works on a Session built around mw.col (never its own
# collection). Ops are never run, and an observation that would reopen the
# collection is refused: closing Anki's collection under AnkiConnect is not a
# recording. Errors carry the driver's kinds and fields, never Anki's text.
import json
import os
import tempfile
from pathlib import Path

from aqt import gui_hooks, mw
from aqt.qt import QApplication, QMessageBox, QTimer

DIR = Path(os.environ.get("ORACLE_RECORDER_DIR", "/scratch/recorder"))
DIR.mkdir(parents=True, exist_ok=True)
_session = None


def _write(path: Path, value) -> None:
    tmp = path.with_name(f".{path.name}.tmp")
    tmp.write_text(json.dumps(value), encoding="utf-8")
    os.replace(tmp, path)


def _next_answer() -> str | None:
    path = DIR / "permission"
    try:
        lines = path.read_text().splitlines()
    except FileNotFoundError:
        return None
    lines = [line.strip() for line in lines if line.strip()]
    if not lines:
        return None
    path.write_text("".join(line + "\n" for line in lines[1:]))
    return lines[0]


def _answer_prompt() -> None:
    box = QApplication.activeModalWidget()
    if not isinstance(box, QMessageBox) or box.checkBox() is None:
        return
    answer = _next_answer()
    if answer is None:
        marker = DIR / "prompt_waiting"
        if not marker.exists():
            marker.write_text(json.dumps({"title": "permission"}) + "\n")
        return
    if answer not in ("allow", "deny", "deny_ignore"):
        raise ValueError(f"unknown permission answer {answer!r}")
    box.checkBox().setChecked(answer == "deny_ignore")
    button = QMessageBox.StandardButton.Yes if answer == "allow" else QMessageBox.StandardButton.No
    with open(DIR / "answered", "a") as out:
        out.write(json.dumps({"answer": answer}) + "\n")
    box.button(button).click()


# --- the driver session ------------------------------------------------------


def _no_reopen():
    from driver.errors import DriverError

    raise DriverError("unsupported_observation", detail="it reopens the collection, which the recorder cannot do under AnkiConnect")


def _new_session(scenario: dict):
    from driver import registry
    from driver.clock import Clock
    from driver.session import Actor, Session

    registry.load_all()
    s = Session.__new__(Session)
    s.actor_names = []
    s.scenario = scenario
    clock_file = os.environ.get("FAKETIME_TIMESTAMP_FILE") or os.path.join(os.environ.get("ORACLE_SCRATCH", "/scratch/worker"), "clock")
    s.clock = Clock(clock_file)
    s.scratch = os.environ.get("ORACLE_SCRATCH", "/scratch/worker")
    s.dir = tempfile.mkdtemp(prefix="session-", dir=s.scratch)
    s.caller_actors = frozenset()
    actor = Actor(None, os.path.dirname(mw.col.path))
    actor.col = mw.col
    s.actors = {None: actor}
    s.current = actor
    s.notes, s.cards = [], []
    s.setup_ids = {"notes": [], "cards": []}
    s.steps, s.outputs = [], []
    s.next_step, s.step_index, s.caller_dumps, s.fsrs_context = 0, None, None, None
    s.resources = {}
    s.reopen = _no_reopen
    return s


def _setup(req: dict) -> dict:
    global _session
    from driver import registry

    if _session is not None:
        from driver.errors import DriverError

        raise DriverError("protocol_error", detail="setup runs once")
    scenario = req["scenario"]
    s = _new_session(scenario)
    s._guard(None, "setup", lambda: s._setup(scenario.get("setup", {}) or {}))
    for name, factory in registry.SESSION_RESOURCES.items():
        resource = factory(s)
        if resource is not None:
            s.resources[name] = resource
    _session = s
    fixtures = s.resources.get("http_fixtures")
    return {"notes": list(s.notes), "cards": list(s.cards), "fixture_base": getattr(fixtures, "origin", None)}


def _snapshot(_req: dict) -> dict:
    col = mw.col
    tr = col.tr
    needle = "[" + tr.errors_bad_directive(directive="anki:tts", error=tr.errors_option_not_set(option="lang")) + "]"
    return {
        "notes": col.db.all("select id, guid, mod from notes"),
        "cards": col.db.all("select id, nid, ord, mod from cards"),
        "decks": col.db.all("select id, name from decks"),
        "notetypes": col.db.all("select id, name from notetypes"),
        "media_dir": col.media.dir(),
        "profile": mw.pm.name,
        "tts_needle": needle,
    }


def _observe(req: dict):
    from driver.errors import DriverError

    s = _session
    if s is None:
        raise DriverError("protocol_error", detail="observe before setup")
    s.notes, s.cards = list(req["notes"]), list(req["cards"])
    step = req.get("step")
    return s._guard(step, "observe", lambda: s._observe(step, req["specs"]))


def _now(_req: dict) -> int:
    import time

    return int(time.time())


OPS = {"setup": _setup, "snapshot": _snapshot, "observe": _observe, "now": _now}


def _control() -> None:
    path = DIR / "request.json"
    if not path.exists():
        return
    req = json.loads(path.read_text(encoding="utf-8"))
    path.unlink()
    out = DIR / f"response-{req['id']}.json"
    try:
        from driver.errors import DriverError, log_exception
    except ImportError:
        _write(out, {"error": {"kind": "internal_error", "detail": "the oracle driver is not importable"}})
        return
    try:
        handler = OPS.get(req.get("op"))
        if handler is None:
            raise DriverError("protocol_error", detail="unknown control op")
        _write(out, {"ok": handler(req)})
    except DriverError as exc:
        _write(out, {"error": exc.to_json()})
    except Exception as exc:  # noqa: BLE001 - report the class only, never Anki's text
        log_exception(exc, f"recorder control {req.get('op')}")
        _write(out, {"error": {"kind": "internal_error", "exception": type(exc).__name__}})


def _poll() -> None:
    _answer_prompt()
    if mw.col is not None and QApplication.activeModalWidget() is None:
        _control()


def _on_profile_open() -> None:
    col = mw.col
    (DIR / "ready").write_text(
        json.dumps({"collection": col.path if col else None, "pid": os.getpid()}) + "\n"
    )


_timer = QTimer(mw)
_timer.timeout.connect(_poll)
_timer.start(50)
gui_hooks.profile_did_open.append(_on_profile_open)
