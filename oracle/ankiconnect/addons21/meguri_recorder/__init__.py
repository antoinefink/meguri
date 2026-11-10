# Meguri recorder driver add-on (AGPL-3.0-or-later, see ../../../LICENSE).
#
# Spike scope (U136): signals readiness once the profile is open and answers
# the AnkiConnect permission dialog (servers/transcripts T7.2, T19.1).
#
#   $ORACLE_RECORDER_DIR/ready        written when the profile is open
#   $ORACLE_RECORDER_DIR/permission   one answer per line, consumed in order:
#                                     allow | deny | deny_ignore
#   $ORACLE_RECORDER_DIR/answered     one line per dialog the add-on answered
#
# The dialog is a QMessageBox the add-on runs with exec() on the main thread,
# so a QTimer on the main thread (real monotonic time) still fires while it is
# open and finds it as the active modal widget.
import json
import os
from pathlib import Path

from aqt import gui_hooks, mw
from aqt.qt import QApplication, QMessageBox, QTimer

DIR = Path(os.environ.get("ORACLE_RECORDER_DIR", "/scratch/recorder"))
DIR.mkdir(parents=True, exist_ok=True)


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


def _poll() -> None:
    box = QApplication.activeModalWidget()
    if not isinstance(box, QMessageBox) or box.checkBox() is None:
        return
    answer = _next_answer()
    if answer is None:
        return
    if answer not in ("allow", "deny", "deny_ignore"):
        raise ValueError(f"unknown permission answer {answer!r}")
    box.checkBox().setChecked(answer == "deny_ignore")
    button = QMessageBox.StandardButton.Yes if answer == "allow" else QMessageBox.StandardButton.No
    with open(DIR / "answered", "a") as out:
        out.write(json.dumps({"answer": answer, "title": box.windowTitle()}) + "\n")
    box.button(button).click()


def _on_profile_open() -> None:
    col = mw.col
    (DIR / "ready").write_text(
        json.dumps({"collection": col.path if col else None, "pid": os.getpid()}) + "\n"
    )


_timer = QTimer(mw)
_timer.timeout.connect(_poll)
_timer.start(50)
gui_hooks.profile_did_open.append(_on_profile_open)
