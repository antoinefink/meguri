# SPDX-License-Identifier: AGPL-3.0-or-later
"""The graphs-page runner (README.md): one offscreen QtWebEngine view that
loads Anki's pinned graphs page from the worker's local server and runs
probe.js on it.

Started by driver/observe/stats_presentation.py, one per worker, and kept for
the worker's life. Protocol: one JSON object per line on stdin,
{"url", "plan", "timeout_s"}, answered by one JSON line on the original
stdout: {"result": <probe value>} or {"error": <code>}. Nothing else is
written there: fd 1 points at /dev/null for Qt and Chromium, and every page
console message is dropped (their text can hold Anki's source names; the
worker's stderr reaches implementer-readable logs).
"""

from __future__ import annotations

import json
import os
import sys
import time

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ.setdefault("QTWEBENGINE_DISABLE_SANDBOX", "1")
os.environ.setdefault("QTWEBENGINE_CHROMIUM_FLAGS", "--no-sandbox --disable-gpu --log-level=3")

# QtWebEngineWidgets must be imported before the QApplication exists.
from PyQt6.QtCore import QEventLoop, QTimer, QUrl  # noqa: E402
from PyQt6.QtWebEngineCore import QWebEnginePage, QWebEngineProfile, QWebEngineScript  # noqa: E402
from PyQt6.QtWebEngineWidgets import QWebEngineView  # noqa: E402
from PyQt6.QtWidgets import QApplication  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
POLL_MS = 50

# Captures the page's browser hand-off (bridgeCommand) before its scripts run.
BRIDGE = (
    "window.__oracleCommands = [];"
    "window.bridgeCommand = function (cmd) { window.__oracleCommands.push(String(cmd)); };"
)


class Page(QWebEnginePage):
    def javaScriptConsoleMessage(self, level, message, line, source):  # noqa: N802
        pass


def main() -> int:
    out = os.fdopen(os.dup(1), "w", buffering=1)
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 1)
    with open(os.path.join(HERE, "probe.js"), encoding="utf-8") as fh:
        probe = fh.read()

    app = QApplication([sys.argv[0]])
    profile = QWebEngineProfile()  # off the record: no disk cache or storage
    profile.setHttpCacheType(QWebEngineProfile.HttpCacheType.NoCache)
    script = QWebEngineScript()
    script.setSourceCode(BRIDGE)
    script.setInjectionPoint(QWebEngineScript.InjectionPoint.DocumentCreation)
    script.setWorldId(QWebEngineScript.ScriptWorldId.MainWorld)
    script.setRunsOnSubFrames(False)
    profile.scripts().insert(script)
    view = QWebEngineView()
    page = Page(profile, view)
    view.setPage(page)
    # Wide enough for the page's one-column layout breakpoints not to matter:
    # the probe reads data, never geometry.
    view.resize(1600, 4000)
    view.show()

    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            req = json.loads(line)
            answer = run_one(app, page, probe, req["url"], req["plan"], float(req["timeout_s"]))
        except Exception as exc:  # noqa: BLE001 - reported as a code, never text
            answer = {"error": f"runner_{type(exc).__name__}"}
        out.write(json.dumps(answer, ensure_ascii=False) + "\n")
        out.flush()
    del page, view, profile, app
    return 0


def run_one(app, page, probe: str, url: str, plan: dict, timeout_s: float) -> dict:
    loop = QEventLoop()
    state: dict = {}
    deadline = time.monotonic() + timeout_s
    source = f"({probe})({json.dumps(plan)})"

    def finish(value: dict) -> None:
        if not state:
            state.update(value)
            loop.quit()

    def poll() -> None:
        if state:
            return
        if time.monotonic() > deadline:
            finish({"error": "timeout"})
            return
        page.runJavaScript(source, QWebEngineScript.ScriptWorldId.MainWorld, got)

    def got(value) -> None:
        if state:
            return
        if isinstance(value, str):
            try:
                parsed = json.loads(value)
            except ValueError:
                finish({"error": "probe_output"})
                return
            if isinstance(parsed, dict) and "error" in parsed:
                finish({"error": str(parsed["error"])})
            else:
                finish({"result": parsed})
            return
        QTimer.singleShot(POLL_MS, poll)

    def loaded(ok: bool) -> None:
        if not ok:
            finish({"error": "load_failed"})
        else:
            QTimer.singleShot(POLL_MS, poll)

    page.loadFinished.connect(loaded)
    try:
        page.setUrl(QUrl(url))
        loop.exec()
    finally:
        page.loadFinished.disconnect(loaded)
        # Leave the page empty, so nothing of this request runs on.
        blank = QEventLoop()
        page.loadFinished.connect(blank.quit)
        page.setUrl(QUrl("about:blank"))
        blank.exec()
        page.loadFinished.disconnect(blank.quit)
    return dict(state)


if __name__ == "__main__":
    sys.exit(main())
