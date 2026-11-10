# SPDX-License-Identifier: AGPL-3.0-or-later
"""heatmap_totals and heatmap_report (specs/conformance/stats-ops.md H1, H11):
the Review Heatmap add-on 1.0.1's own computation, run headless.

    heatmap_totals                    -> {home: {YYYY-MM-DD: count}}
    {heatmap_totals: {decks: [name]}} -> {home: ..., decks: {name: ...}}
    heatmap_report                    -> {home: <report>}
    {heatmap_report: {decks: [name]}} -> {home: ..., decks: {name: <report>}}

    <report> = {today, history, forecast, last_forecast_day,
                today_cell: {kind, count}, streaks}

Only the add-on image (oracle/heatmap/Dockerfile) loads this module: it lists
it in ORACLE_EXTRA_MODULES and installs the add-on's public tag v1.0.1 at
$ORACLE_HEATMAP_ADDON (the repository's LICENSE and src/review_heatmap/).
Importing it checks the installed files against heatmap_addon.sha256 (the pin:
a digest over every file's sha256 and path) and refuses any other file set.

How the add-on runs here:
- Only its computation modules are imported (consts, config, activity and the
  libaddon modules they pull in), as the package `1771074083` (the folder
  name Anki installs it under), without the package's __init__, which builds
  the GUI. Their `from aqt import mw` gets a stub main window: the add-ons
  folder for libaddon's platform module, and, during an observation, a view
  of the collection for the config manager. `aqt` leaves sys.modules again
  once they are imported.
- Each observation loads the add-on's synced config again
  (ConfigManager.load), so the `heatmap` key is read from the collection as
  it is then; the add-on reads the rollover itself at each computation. The
  config manager reads the collection's config through an overlay: what the
  add-on stores there (its defaults for an absent key, a merged copy for a
  key whose version is missing or below 1.0.1) stays in the overlay and is
  dropped after the observation, so the collection never changes (H1, read
  only). Values read back from the overlay are the ones the add-on stored,
  as Anki's config would return them.
- Home is the main screen's computation (current_deck_only False). A deck is
  the deck screen's (current_deck_only True) with that deck as the current
  deck, given through a deck-manager view, so the collection's current deck
  does not change either.
- The parts run as the add-on's get_report runs them: its time limits, the
  history query, the forecast query from its today, then _get_activity for
  the activity dict, the bounds and the streak stats. heatmap_totals stops
  after the history query (H1: before the forecast and today's rule).
- With an empty history the add-on returns no report and draws "no data"
  (get_report returns None after both queries). heatmap_report then gives
  the history {}, the forecast the add-on's query returned, streaks null, the
  last forecast day as _get_activity takes it (the last forecast key) and
  today's cell from the forecast alone, as _get_activity's merge would.
- Both queries key a count by 00:00 UTC of its date, in epoch seconds. The
  add-on's web view moves each key by the zone's offset at that instant and
  the calendar draws it on the local date of the moved instant, adding counts
  that land on one day (stats/day-buckets B4.6). `drawn_date` applies that
  step to the history and forecast keys (H1, drawn date); today, today's
  cell, the last forecast day and the streaks are written as the add-on
  computes them (their keys read as UTC dates).
- Forecast counts are the query's negated counts (the add-on negates them for
  its legend), reported positive. In the activity dict a negative value is a
  forecast count: today's cell is `future` then, else `today` (0 when absent).
"""

from __future__ import annotations

import hashlib
import importlib
import json
import os
import sys
import types
from datetime import date, datetime, timezone
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

from ..errors import DriverError, invalid
from ..registry import observation

PACKAGE = "1771074083"
PACKAGE_DIR = "src/review_heatmap"
PINS = Path(__file__).with_name("heatmap_addon.sha256")


# --- the pinned add-on ---------------------------------------------------------


def pinned_tree() -> tuple[str, int]:
    """(sha256 of the pinned files' sha256sum listing, their number)."""
    lines = [l for l in PINS.read_text(encoding="utf-8").split("\n") if l and not l.startswith("#")]
    digest, count = lines[0].split("  ")
    return digest, int(count)


def tree(root: Path) -> tuple[str, int]:
    """(digest, count) of root as the pin computes them: every regular file
    (byte-code caches aside), one "<sha256>  <path>" line each, sorted by
    path bytes."""
    paths = sorted(
        p.relative_to(root).as_posix()
        for p in root.rglob("*")
        if p.is_file() and "__pycache__" not in p.relative_to(root).parts
    )
    listing = "".join(f"{hashlib.sha256((root / p).read_bytes()).hexdigest()}  {p}\n" for p in paths)
    return hashlib.sha256(listing.encode("utf-8")).hexdigest(), len(paths)


def differences(root: Path) -> list[str]:
    """Empty when root holds exactly the pinned files; else what differs.
    The pin names no paths (some upstream file names match the private
    scan's tokens), so a difference names only the digest and count."""
    found, pinned = tree(root), pinned_tree()
    return [] if found == pinned else [f"tree {found[0]} of {found[1]} files is not the pinned {pinned[0]} of {pinned[1]}"]


# --- headless loading ----------------------------------------------------------


class _ProfileManager:
    def __init__(self, addons: str) -> None:
        self._addons = addons

    def addonFolder(self) -> str:  # noqa: N802 - Anki's name
        return self._addons


class _MainWindow:
    """What the add-on's computation modules use of Anki's main window."""

    def __init__(self, addons: str) -> None:
        self.pm = _ProfileManager(addons)
        self.col: _CollectionView | None = None


class _Stored(dict):
    """A dict value of the overlay, stored back when changed and dropped, as
    pylib's config does with the dicts it hands out."""

    def __init__(self, overlay: "_ConfigOverlay", key: str, value: dict) -> None:
        super().__init__(value)
        self._overlay = overlay
        self._key = key
        self._orig = json.loads(json.dumps(value))

    def __del__(self) -> None:
        current = dict(self)
        if current != self._orig:
            self._overlay[self._key] = current


class _ConfigOverlay:
    """The collection's config as the add-on's config manager sees it: reads
    come from the collection at each access, writes stay here."""

    def __init__(self, col) -> None:
        self._col = col
        self.stored: dict[str, str] = {}  # key -> JSON the add-on stored

    def _value(self, key: str) -> Any:
        if key in self.stored:
            return json.loads(self.stored[key])
        return self._col.conf.get_immutable(key)  # KeyError when absent

    def __contains__(self, key: str) -> bool:
        try:
            self._value(key)
        except KeyError:
            return False
        return True

    def __getitem__(self, key: str) -> Any:
        value = self._value(key)
        return _Stored(self, key, value) if isinstance(value, dict) else value

    def __setitem__(self, key: str, value: Any) -> None:
        self.stored[key] = json.dumps(value)

    def get(self, key: str, default: Any = None) -> Any:
        try:
            return self[key]
        except KeyError:
            return default


class _CollectionView:
    """mw.col for the add-on's config manager during one observation."""

    def __init__(self, col) -> None:
        self.conf = _ConfigOverlay(col)

    def setMod(self) -> None:  # noqa: N802 - Anki's name; a no-op there too
        pass


class _Decks:
    """The collection's deck manager with `did` as the current deck."""

    def __init__(self, decks, did: int) -> None:
        self._decks = decks
        self._did = did

    def get_current_id(self) -> int:
        return self._did

    def __getattr__(self, name: str) -> Any:
        return getattr(self._decks, name)


class _CurrentDeck:
    """The collection as the deck screen of `did` sees it."""

    def __init__(self, col, did: int) -> None:
        self._col = col
        self.decks = _Decks(col.decks, did)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._col, name)


def _load(package_dir: Path) -> tuple[_MainWindow, Any, Any]:
    """Import the add-on's computation modules under a stub main window:
    (the stub, the add-on's config manager, its activity module)."""
    mw = _MainWindow(str(package_dir.parent))
    aqt = types.ModuleType("aqt")
    aqt.mw = mw
    package = types.ModuleType(PACKAGE)
    package.__path__ = [str(package_dir)]
    saved = sys.modules.get("aqt")
    sys.modules["aqt"] = aqt
    sys.modules[PACKAGE] = package
    try:
        consts = importlib.import_module(f"{PACKAGE}.consts")
        # As the add-on's initialize_addon does first.
        importlib.import_module(f"{PACKAGE}.libaddon.consts").set_addon_properties(consts.ADDON)
        config = importlib.import_module(f"{PACKAGE}.config")
        activity = importlib.import_module(f"{PACKAGE}.activity")
    finally:
        if saved is None:
            del sys.modules["aqt"]
        else:
            sys.modules["aqt"] = saved
    return mw, config.config, activity


def _check_and_load() -> tuple[_MainWindow, Any, Any]:
    root = Path(os.environ.get("ORACLE_HEATMAP_ADDON", ""))
    if not root.is_absolute() or not root.is_dir():
        print("oracle: ORACLE_HEATMAP_ADDON names no add-on directory", file=sys.stderr)
        raise RuntimeError("no Review Heatmap add-on")
    diffs = differences(root)
    if diffs:
        print(f"oracle: the add-on at {root} is not the pinned one: {', '.join(diffs[:5])}", file=sys.stderr)
        raise RuntimeError("the Review Heatmap add-on is not the pinned one")
    return _load(root / PACKAGE_DIR)


_MW, _CONFIG, _ACTIVITY = _check_and_load()


# --- dates -----------------------------------------------------------------------


def key_date(key: int) -> date:
    """The date the add-on means by a key: 00:00 UTC of that date."""
    return datetime.fromtimestamp(key, timezone.utc).date()


def drawn_date(key: int, tz: ZoneInfo) -> date:
    """The day the add-on's calendar draws a key on. The web view adds the
    zone's offset at the key (getTimezoneOffset, minutes behind UTC) and the
    calendar takes the local date of the result."""
    moved = key - int(datetime.fromtimestamp(key, tz).utcoffset().total_seconds())
    return datetime.fromtimestamp(moved, tz).date()


def _drawn(rows, tz: ZoneInfo, sign: int = 1) -> dict[str, int]:
    days: dict[str, int] = {}
    for key, count in rows:
        count = sign * int(count)
        if count < 1:
            raise RuntimeError("the add-on returned a count below 1")
        day = drawn_date(int(key), tz).isoformat()
        days[day] = days.get(day, 0) + count
    return dict(sorted(days.items()))


# --- the observations ------------------------------------------------------------


def _reporter(col, did: int | None):
    return _ACTIVITY.ActivityReporter(col if did is None else _CurrentDeck(col, did), _CONFIG)


def _totals(col, did: int | None, tz: ZoneInfo) -> dict[str, int]:
    reporter = _reporter(col, did)
    history_start, _ = reporter._get_time_limits(None, None)
    history = reporter._cards_done(start=history_start, current_deck_only=did is not None)
    return _drawn(history, tz)


def _report(col, did: int | None, tz: ZoneInfo) -> dict[str, Any]:
    reporter = _reporter(col, did)
    deck_only = did is not None
    # As get_report(limhist=None, limfcst=None, current_deck_only=deck_only).
    history_start, forecast_stop = reporter._get_time_limits(None, None)
    history = reporter._cards_done(start=history_start, current_deck_only=deck_only)
    today = int(reporter._today)
    forecast = reporter._cards_due(start=today, stop=forecast_stop, current_deck_only=deck_only)
    if history:
        report = reporter._get_activity(history=history, forecast=forecast)
        if report.today != today * 1000:
            raise RuntimeError("the add-on's today moved during one observation")
        activity = report.activity
        stop = report.stop // 1000 if report.stop else None
        stats = report.stats
        streaks = {
            "daily_average": int(stats.activity_daily_avg.value),
            "days_learned": int(stats.pct_days_active.value),
            "longest_streak": int(stats.streak_max.value),
            "current_streak": int(stats.streak_cur.value),
        }
    else:
        activity = dict(forecast)
        stop = int(forecast[-1][0]) if forecast else None
        streaks = None
    cell = int(activity.get(today, 0))
    return {
        "today": key_date(today).isoformat(),
        "history": _drawn(history, tz),
        "forecast": _drawn(forecast, tz, sign=-1),
        "last_forecast_day": key_date(stop).isoformat() if stop else None,
        "today_cell": {"kind": "future", "count": -cell} if cell < 0 else {"kind": "today", "count": cell},
        "streaks": streaks,
    }


def _deck_names(kind: str, args: Any) -> list[str] | None:
    if args is None:
        return None
    if not isinstance(args, dict) or set(args) != {"decks"}:
        raise invalid(f"{kind} takes nothing or {{decks: [name, ...]}}")
    names = args["decks"]
    if (
        not isinstance(names, list)
        or not names
        or not all(isinstance(n, str) and n for n in names)
        or len(set(names)) != len(names)
    ):
        raise invalid(f"{kind} decks is a non-empty list of distinct deck names")
    return names


def _observe(session, kind: str, args: Any, scope) -> dict[str, Any]:
    names = _deck_names(kind, args)
    col = session.col
    dids: dict[str, int] = {}
    if names is not None:
        known = {entry.name: entry.id for entry in col.decks.all_names_and_ids()}
        for name in names:
            if name not in known:
                raise DriverError("unknown_ref", ref=name)
            dids[name] = known[name]
    _MW.col = _CollectionView(col)
    try:
        _CONFIG.load(storage_name="synced")
        out: dict[str, Any] = {"home": scope(col, None, session.tz)}
        if names is not None:
            out["decks"] = {name: scope(col, dids[name], session.tz) for name in names}
    finally:
        _MW.col = None
    return out


@observation("heatmap_totals")
def heatmap_totals(session, args):
    return _observe(session, "heatmap_totals", args, _totals)


@observation("heatmap_report")
def heatmap_report(session, args):
    return _observe(session, "heatmap_report", args, _report)
