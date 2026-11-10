# SPDX-License-Identifier: AGPL-3.0-or-later
"""stats_presentation (specs/conformance/stats-ops.md H14,
stats/graph-presentation P22): what Anki's pinned graphs page presents from a
graph's data, read from the page itself.

    {stats_presentation: [{scope, history, graph, controls}, ...]}
      -> [{data, page}, ...]

Only the graphs-page image (oracle/graphs-page/Dockerfile) loads this module
(ORACLE_EXTRA_MODULES): the page is the bundle of the pinned aqt wheel, which
only that image installs, and it runs in that image's offscreen QtWebEngine
(stats_presentation_page/runner.py, one per worker, with probe.js; they sit
in the driver so that driver_sha256 covers them). The page and its assets
never leave the image (stats/graphs-common G12 decision 2, R1960); the
observation holds the values the page shows and bound to its elements.

How a request runs:
- The graph data is the `stats` kind's (ops/stats.py): the scope's search
  text runs alone first, then the backend's graph data for that text and the
  history's days. `data` is the graph's part of that response in the `stats`
  kind's form. A search the backend refuses, or a row it cannot read, is a run
  error here (P22 defines no error value).
- A local server on 127.0.0.1 serves the page and answers its backend calls:
  `graphs` with that same response, whatever search and days the page sends;
  `getGraphPreferences` with the collection's preferences, `backlog` and
  `separate` replaced by the request's (P22: drawn as if stored);
  `setGraphPreferences` with nothing, so no preference write reaches the
  collection (G11); `i18nResources` through the backend. Any other call is
  answered 404 and fails the request (`page_call`).
- The page is loaded fresh for each request, at its own defaults (the Deck
  range, `deck:current`, and 1 year of history). The runner clicks
  `all history` for history `all`, waits for the page's second graph call,
  then clicks the graph's range radio by its English label (P22's control
  values are Anki's wording), and reads:
  - `no_data`: the graph's no-data overlay takes the pointer (Anki's page
    sets that and the overlay's opacity together);
  - `bins`, in axis order, from the d3 data of the graph's hover columns:
    the edges, the value and `drawn` (x1 > x0). The value is the sum of the
    bin's entries' counts (P4.3 item 4): an entry is a [key, count] pair of
    the graph's map, or for Intervals and Stability, whose page bins one
    value per card, a single card. For a drawn bin
    the runner sends the column a mouse move and reads the tooltip's lines
    (split at <br>, entities decoded, U+2068, U+2069 and U+200E removed), then
    a click and reads the page's browser hand-off, `browserSearch: <page
    search> <query>`, keeping <query>; no hand-off is null. An undrawn bin
    has no tooltip or query (P4.4): both null. Events are sent to the column
    element whatever its width, so a drawn bin narrower than a pixel is read
    too (P21);
  - `table`: each row's cells, the label without the colon the page adds;
  - Card Counts: the rows' cells without the colour swatch (trailing empty
    cells dropped), the pie's seven values in row order as `sectors`,
    `no_data` false and no bins (the graph has neither).
- Text is the page's English text, Anki's own (R1448's exception, as
  studied_today_text): P22 compares it exactly.

Supported graphs: the histogram graphs (future_due, intervals, stability,
difficulty, retrievability, added) and card_counts. A scenario that asks for
reviews, hours, buttons, retention or calendar fails before any collection as
unsupported_observation (README.md says what they need).
"""

from __future__ import annotations

import json
import os
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

import anki.errors

from ..errors import DriverError, invalid
from ..ops.stats import HISTORY_DAYS, _part, _search_failure, _search_text
from ..observe.search import Reasons
from ..registry import observation, scenario_check

KIND = "stats_presentation"
HISTORY_LABELS = {"year": None, "all": "all history"}
# The page's default search (RangeBox's Deck range): the prefix of every
# browser hand-off.
PAGE_SEARCH = "deck:current"
PAGE_TIMEOUT_S = 120.0
# runner.py and probe.js: in the driver, so driver_sha256 covers them.
RUNNER_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "stats_presentation_page")

DAYS = ("1 month", "3 months", "1 year", "all")
SHARE = ("1 month", "50%", "95%", "all")
PERCENT = ("50%", "95%", "100%", "all")
# Every graph of P22's control table -> its controls (a tuple of allowed
# range labels, bool, int, or a tuple of words).
CONTROLS: dict[str, dict[str, Any]] = {
    "future_due": {"range": DAYS, "backlog": bool},
    "calendar": {"year": int},
    "reviews": {"range": DAYS, "time": bool},
    "card_counts": {"separate": bool},
    "intervals": {"range": SHARE},
    "stability": {"range": SHARE},
    "difficulty": {"range": PERCENT},
    "retrievability": {"range": PERCENT},
    "retention": {"mode": ("young", "mature", "all")},
    "hours": {"range": DAYS},
    "buttons": {"range": DAYS},
    "added": {"range": DAYS},
}
# The graphs the runner reads -> (probe kind, the page's English title, the
# stats part). The others fail as unsupported_observation (README.md).
GRAPHS: dict[str, tuple[str, str, str]] = {
    "future_due": ("histogram", "Future Due", "future_due"),
    "intervals": ("histogram", "Review Intervals", "intervals"),
    "stability": ("histogram", "Card Stability", "stability"),
    "difficulty": ("histogram", "Card Difficulty", "difficulty"),
    "retrievability": ("histogram", "Card Retrievability", "retrievability"),
    "added": ("histogram", "Added", "added"),
    "card_counts": ("card_counts", "Card Counts", "card_counts"),
}


# --- requests ------------------------------------------------------------------


def _requests(args: Any) -> list[dict[str, Any]]:
    if not isinstance(args, list) or not args:
        raise invalid("stats_presentation takes a non-empty list of {scope, history, graph, controls}")
    out = []
    for req in args:
        if not isinstance(req, dict) or set(req) != {"scope", "history", "graph", "controls"}:
            raise invalid("a stats_presentation request is {scope, history, graph, controls}")
        scope = req["scope"]
        if scope == "collection":
            parsed: tuple[str, str | None] = ("collection", None)
        elif isinstance(scope, dict) and len(scope) == 1 and isinstance(scope.get("deck"), str) and scope["deck"]:
            parsed = ("deck", scope["deck"])
        elif isinstance(scope, dict) and len(scope) == 1 and isinstance(scope.get("search"), str):
            parsed = ("search", scope["search"])
        else:
            raise invalid("a stats_presentation scope is collection, {deck: <name>} or {search: <text>}")
        history = req["history"]
        if not isinstance(history, str) or history not in HISTORY_DAYS:
            raise invalid("a stats_presentation history is year or all")
        graph = req["graph"]
        if not isinstance(graph, str) or graph not in CONTROLS:
            raise invalid(f"a stats_presentation graph is one of {sorted(CONTROLS)}")
        controls = req["controls"]
        wanted = CONTROLS[graph]
        if not isinstance(controls, dict) or set(controls) != set(wanted):
            raise invalid(f"the {graph} controls are exactly {sorted(wanted)}")
        for name, allowed in wanted.items():
            value = controls[name]
            if allowed is bool or allowed is int:
                if type(value) is not allowed:
                    raise invalid(f"the {graph} control {name} is {'a boolean' if allowed is bool else 'an integer'}")
            elif not isinstance(value, str) or value not in allowed:
                raise invalid(f"the {graph} control {name} is one of {list(allowed)}")
        if graph not in GRAPHS:
            raise DriverError("unsupported_observation", observation=KIND, detail=f"graph {graph} is not read yet")
        out.append({"scope": parsed, "history": history, "graph": graph, "controls": controls})
    return out


@scenario_check
def check_presentation_steps(scenario: dict) -> None:
    """Requests the runner does not read fail before any collection."""
    steps = scenario.get("steps", [])
    for index, step in enumerate(steps if isinstance(steps, list) else []):
        if not isinstance(step, dict) or len(step) != 1 or not isinstance(step.get("observe"), list):
            continue
        for spec in step["observe"]:
            if isinstance(spec, dict) and len(spec) == 1 and KIND in spec:
                try:
                    _requests(spec[KIND])
                except DriverError as exc:
                    exc.fields.setdefault("step", index)
                    raise


# --- the page server -----------------------------------------------------------


def _bundle_root() -> str:
    try:
        import _aqt  # the aqt wheel's data package (graphs-page image only)
    except ImportError as exc:
        raise DriverError("internal_error", detail="no aqt bundle: stats_presentation runs only on the graphs-page image") from exc
    root = os.path.join(list(_aqt.__path__)[0], "data", "web", "sveltekit")
    if not os.path.isfile(os.path.join(root, "index.html")):
        raise DriverError("internal_error", detail="the aqt wheel has no sveltekit bundle")
    return os.path.realpath(root)


TYPES = {".html": "text/html", ".mjs": "text/javascript", ".js": "text/javascript", ".css": "text/css",
         ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2"}


class _Served:
    """What the server answers for the current request."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.backend = None
        self.graphs = b""
        self.prefs = b""
        self.graph_calls = 0
        self.stray: list[str] = []


class _Handler(BaseHTTPRequestHandler):
    root = ""
    served: _Served

    def log_message(self, *_args) -> None:
        pass

    def _send(self, code: int, body: bytes, ctype: str) -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0]
        if path == "/_oracle/state":
            with self.served.lock:
                body = json.dumps({"graphs": self.served.graph_calls}).encode()
            self._send(200, body, "application/json")
            return
        if path.startswith("/_app/"):
            target = os.path.realpath(os.path.join(self.root, path[1:]))
            if not target.startswith(self.root + os.sep) or not os.path.isfile(target):
                self._send(404, b"", "text/plain")
                return
        elif path == "/graphs":
            target = os.path.join(self.root, "index.html")
        else:
            self._send(404, b"", "text/plain")
            return
        with open(target, "rb") as fh:
            body = fh.read()
        self._send(200, body, TYPES.get(os.path.splitext(target)[1], "application/octet-stream"))

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)
        name = self.path.split("?", 1)[0].removeprefix("/_anki/")
        served = self.served
        with served.lock:
            if name == "graphs":
                served.graph_calls += 1
                out = served.graphs
            elif name == "getGraphPreferences":
                out = served.prefs
            elif name == "setGraphPreferences":
                out = b""
            elif name == "i18nResources":
                out = None
            else:
                served.stray.append(name[:40])
                self._send(404, b"", "text/plain")
                return
        if out is None:
            out = served.backend.i18n_resources_raw(body)
        self._send(200, out, "application/binary")


class _Page:
    """The worker's page server and runner, started at the first request."""

    def __init__(self) -> None:
        if not os.environ.get("ORACLE_GRAPHS_PAGE"):
            raise DriverError("internal_error", detail="no ORACLE_GRAPHS_PAGE: stats_presentation runs only on the graphs-page image")
        self.served = _Served()
        handler = type("Handler", (_Handler,), {"root": _bundle_root(), "served": self.served})
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.server.daemon_threads = True
        threading.Thread(target=self.server.serve_forever, name="graphs-page", daemon=True).start()
        env = dict(os.environ)
        # One libfaketime read per second instead of one per call: the page
        # reads the clock only for the Calendar (not read yet), and Chromium's
        # threads call it all the time (as in oracle/ankiconnect).
        env.pop("FAKETIME_NO_CACHE", None)
        env["FAKETIME_CACHE_DURATION"] = "1"
        env["HOME"] = os.path.join(os.environ.get("ORACLE_SCRATCH", "/scratch"), "graphs-page-home")
        os.makedirs(env["HOME"], exist_ok=True)
        self.proc = subprocess.Popen(
            ["python3", os.path.join(RUNNER_DIR, "runner.py")],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env=env, text=True, encoding="utf-8",
        )

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self.server.server_address[1]}/graphs"

    def read(self, backend, graphs: bytes, prefs: bytes, plan: dict) -> dict:
        with self.served.lock:
            self.served.backend = backend
            self.served.graphs = graphs
            self.served.prefs = prefs
            self.served.graph_calls = 0
            self.served.stray = []
        self.proc.stdin.write(json.dumps({"url": self.url, "plan": plan, "timeout_s": PAGE_TIMEOUT_S}) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            raise DriverError("internal_error", detail="the graphs-page runner exited")
        answer = json.loads(line)
        with self.served.lock:
            stray = list(self.served.stray)
        if stray:
            raise DriverError("internal_error", detail=f"page_call: the page called {stray}")
        if "error" in answer:
            code = answer["error"]
            if code in ("no_control", "control_not_set"):
                raise invalid(f"the page has no {plan['title']} radio {plan['range']!r} for this request ({code})")
            raise DriverError("internal_error", detail=f"graphs page: {code}")
        return answer["result"]


_PAGE: _Page | None = None


def _page() -> _Page:
    global _PAGE
    if _PAGE is None or _PAGE.proc.poll() is not None:
        _PAGE = _Page()
    return _PAGE


# --- the observation -----------------------------------------------------------


def _number(value: Any) -> Any:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise DriverError("internal_error", detail="graphs page: a bin value is not a number")
    if isinstance(value, float) and value.is_integer() and abs(value) < 2**53:
        return int(value)
    return value


def _text(value: Any) -> str:
    if not isinstance(value, str):
        raise DriverError("internal_error", detail="graphs page: a text is not a string")
    return value


def _page_value(kind: str, reading: dict) -> dict:
    page: dict[str, Any] = {"no_data": bool(reading["no_data"])}
    bins = []
    for b in reading["bins"]:
        bins.append({
            "x0": _number(b["x0"]),
            "x1": _number(b["x1"]),
            "value": _number(b["value"]),
            "drawn": bool(b["drawn"]),
            "tooltip": None if b["tooltip"] is None else [_text(line) for line in b["tooltip"]],
            "query": None if b["query"] is None else _text(b["query"]),
        })
    page["bins"] = bins
    page["table"] = [[_text(cell) for cell in row] for row in reading["table"]]
    if kind == "card_counts":
        sectors = [_number(v) for v in reading["sectors"]]
        if len(sectors) != 7:
            raise DriverError("internal_error", detail="graphs page: Card Counts has not seven sectors")
        page["sectors"] = sectors
    return page


@observation(KIND)
def stats_presentation(session, args):
    requests = _requests(args)
    col = session.col
    names = {entry.name for entry in col.decks.all_names_and_ids()}
    reasons = Reasons(col.tr)
    out = []
    for req in requests:
        kind, title, part = GRAPHS[req["graph"]]
        search = _search_text(col, names, req["scope"])
        failure = _search_failure(reasons, col, search)
        if failure is not None:
            raise DriverError("anki_error", detail=f"the scope's search fails: {failure['error']}")
        try:
            resp = col._backend.graphs(search=search, days=HISTORY_DAYS[req["history"]])
        except anki.errors.DBError as exc:
            raise DriverError("anki_error", detail="the graph data has an unreadable row") from exc
        prefs = col._backend.get_graph_preferences()
        controls = req["controls"]
        if "backlog" in controls:
            prefs.future_due_show_backlog = controls["backlog"]
        if "separate" in controls:
            prefs.card_counts_separate_inactive = controls["separate"]
        plan = {
            "kind": kind,
            "title": title,
            "history_label": HISTORY_LABELS[req["history"]],
            "range": controls.get("range"),
            "page_search": PAGE_SEARCH,
        }
        reading = _page().read(col._backend, resp.SerializeToString(), prefs.SerializeToString(), plan)
        out.append({"data": {part: _part(resp, part, req["history"])}, "page": _page_value(kind, reading)})
    return out
