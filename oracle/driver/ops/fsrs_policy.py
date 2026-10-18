# SPDX-License-Identifier: AGPL-3.0-or-later
"""FSRS policy ops (specs/conformance/fsrs-policy-ops.md F1-F3).

enable_fsrs: {}   Anki's deck options save that turns FSRS on, as made from the
    Default deck's screen (F2): only that deck's preset as stored (the backend's
    preset read; the screen's for-update loading pre-fills an empty FSRS-6 list
    from the older ones), normal mode, FSRS on, rescheduling off, and the
    global options and the deck's limits sent back as the screen reads them.
    The F2 check then compares every row of every table except cards, the
    config row fsrs and col.mod with the state before the step; any difference
    is invalid_scenario. Its detail ends with the rows that differ, appeared or
    disappeared, as "<table>:<row key>" (db_dump's row keys) joined by ", "
    (the protocol's error object has no other field for them).
enable_v3: {}     Anki's scheduler update as the desktop makes it (F3): the
    collection's v3 setter, which writes sched2021 true without an undo entry
    and loads the current scheduler (adding creationOffset when missing); on
    the current scheduler it writes nothing.

Neither op has rejection kinds; Anki's failures (a preset the fsrs crate cannot
build, a v1 collection) are run errors.
"""

from __future__ import annotations

from typing import Any

from anki.decks import UpdateDeckConfigs, UpdateDeckConfigsMode

from ..errors import invalid
from ..observe import db_dump
from ..registry import op

DEFAULT_DECK_ID = 1


def _empty_args(name: str, args: Any) -> None:
    if not isinstance(args, dict) or args:
        raise invalid(f"{name} takes {{}}")


def _rows(col, table: str) -> dict[str, Any]:
    if table in db_dump.KEYS:
        return db_dump.dump_table(col, table, raw=True)
    # A table outside schema 18's twelve: its rows as an ordered list.
    return {str(i): row for i, row in enumerate(sorted(repr(r) for r in col.db.all(f'select * from "{table}"')))}


def _checked_state(col) -> dict[str, dict[str, Any]]:
    """Every row of every table except cards, without the fsrs config row and
    col.mod (F2)."""
    tables = col.db.list("select name from sqlite_master where type = 'table' order by name")
    state = {t: _rows(col, t) for t in tables if t != "cards"}
    state.get("config", {}).pop("fsrs", None)
    for row in state.get("col", {}).values():
        row.pop("mod", None)
    return state


def _changed(before: dict, after: dict) -> list[str]:
    out = []
    for table in sorted(before.keys() | after.keys()):
        b, a = before.get(table, {}), after.get(table, {})
        out += [f"{table}:{key}" for key in sorted(b.keys() | a.keys()) if b.get(key) != a.get(key)]
    return out


@op("enable_fsrs")
def enable_fsrs(session, args):
    _empty_args("enable_fsrs", args)
    col = session.col
    before = _checked_state(col)
    screen = col.decks.get_deck_configs_for_update(DEFAULT_DECK_ID)
    stored = col._backend.get_deck_config(screen.current_deck.config_id)
    col.decks.update_deck_configs(
        UpdateDeckConfigs(
            target_deck_id=DEFAULT_DECK_ID,
            configs=[stored],
            removed_config_ids=[],
            mode=UpdateDeckConfigsMode.UPDATE_DECK_CONFIGS_MODE_NORMAL,
            card_state_customizer=screen.card_state_customizer,
            limits=screen.current_deck.limits,
            new_cards_ignore_review_limit=screen.new_cards_ignore_review_limit,
            apply_all_parent_limits=screen.apply_all_parent_limits,
            fsrs=True,
            fsrs_reschedule=False,
            fsrs_health_check=screen.fsrs_health_check,
        )
    )
    changed = _changed(before, _checked_state(col))
    if changed:
        rows = ", ".join(changed)
        raise invalid(f"the scenario does not meet fsrs-policy-ops.md F2.1; Anki's save also changed: {rows}")


@op("enable_v3")
def enable_v3(session, args):
    _empty_args("enable_v3", args)
    session.col.set_v3_scheduler(True)
