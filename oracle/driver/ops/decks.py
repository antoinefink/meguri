# SPDX-License-Identifier: AGPL-3.0-or-later
"""Presets (deck options) and decks: setup sections, name lookups and the deck
ops of specs/conformance/deck-ops.md K2-K7 (the preset ops are presets.py).

setup.presets: [{name, <option>: <value>, ...}]   options below; a preset named
                 "Default" edits the collection's default preset
setup.decks:   [{name, preset?}]   "::" nests decks; preset defaults to Default

Options are written through the schema-11 deck-config JSON, the persisted
format. Steps are durations ('30s', '10m', '1d').

Steps name decks and presets as K1 says: a deck by its stored name in human
form ("::" for U+001F), a preset by its stored name, both compared exactly and
read from the collection file when the step runs (db_exec rows included);
unknown_ref when nothing matches, invalid_scenario when several do. The typed
name of add_deck and rename_deck reaches Anki as given.

add_deck: {name} -> {name}   the desktop's Add Deck (an existing name returns
    that deck); rejections filtered_parent, too_deep
rename_deck: {deck, name} -> {name}   rejections filtered_parent, too_deep
reparent_decks: {decks, parent} -> {count}   the drop of the deck list; parent
    "" is the top level; rejection filtered_parent
delete_decks: {decks} -> {count}   cards removed
set_deck_collapsed: {decks, scope: study|browser, collapsed}   Anki's collapse
    once per deck, in order
set_current_deck: {deck}

Refusals: Anki refuses a filtered nearest parent with its FilteredDeckError and
a name nested too deep (12 or more components, the 11 nearest ancestors all
missing) with InvalidInput, rolling the op back. An InvalidInput for a name of
fewer components is not that refusal and stays a run error.
"""

from __future__ import annotations

from typing import Any, Callable

import anki.errors
from anki.decks import DeckCollapseScope

from ..clock import parse_duration_minutes
from ..errors import DriverError, Rejected, invalid
from ..registry import op, setup_section

DEFAULT_PRESET_ID = 1


def _steps(v: Any) -> list[float]:
    if not isinstance(v, list):
        raise invalid("steps are a list of durations")
    return [parse_duration_minutes(s) for s in v]


def _int(v: Any) -> int:
    if isinstance(v, bool) or not isinstance(v, int):
        raise invalid(f"{v!r} is not an integer")
    return v


def _num(v: Any) -> float:
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        raise invalid(f"{v!r} is not a number")
    return float(v)


def _bool(v: Any) -> bool:
    if not isinstance(v, bool):
        raise invalid(f"{v!r} is not a boolean")
    return v


def _leech_action(v: Any) -> int:
    actions = {"suspend": 0, "tag_only": 1}
    if v not in actions:
        raise invalid("leech_action is suspend or tag_only")
    return actions[v]


def _params(v: Any) -> list[float]:
    if not isinstance(v, list):
        raise invalid("fsrs_params is a list of numbers")
    return [_num(x) for x in v]


# option -> (path in the schema-11 deck-config JSON, converter)
PRESET_OPTIONS: dict[str, tuple[tuple, Callable[[Any], Any]]] = {
    "learn_steps": (("new", "delays"), _steps),
    "graduating_interval_days": (("new", "ints", 0), _int),
    "easy_interval_days": (("new", "ints", 1), _int),
    "new_per_day": (("new", "perDay"), _int),
    "starting_ease": (("new", "initialFactor"), lambda v: round(_num(v) * 1000)),
    "bury_new": (("new", "bury"), _bool),
    "reviews_per_day": (("rev", "perDay"), _int),
    "max_interval_days": (("rev", "maxIvl"), _int),
    "easy_bonus": (("rev", "ease4"), _num),
    "hard_interval": (("rev", "hardFactor"), _num),
    "interval_modifier": (("rev", "ivlFct"), _num),
    "bury_reviews": (("rev", "bury"), _bool),
    "relearn_steps": (("lapse", "delays"), _steps),
    "lapse_new_interval": (("lapse", "mult"), _num),
    "min_lapse_interval_days": (("lapse", "minInt"), _int),
    "leech_threshold": (("lapse", "leechFails"), _int),
    "leech_action": (("lapse", "leechAction"), _leech_action),
    "bury_interday_learning": (("buryInterdayLearning",), _bool),
    "desired_retention": (("desiredRetention",), _num),
    "fsrs_params": (("fsrsParams6",), _params),
}


def _put(conf: dict, path: tuple, value: Any) -> None:
    *parents, last = path
    target = conf
    for key in parents:
        target = target[key]
    target[last] = value


@setup_section("presets", order=20)
def presets(session, items):
    if not isinstance(items, list):
        raise invalid("setup.presets is a list")
    decks = session.col.decks
    for item in items:
        if not isinstance(item, dict) or not isinstance(item.get("name"), str):
            raise invalid("a preset is a mapping with a name")
        name = item["name"]
        if name in session.presets:
            raise invalid(f"preset {name!r} is defined twice")
        unknown = sorted(set(item) - set(PRESET_OPTIONS) - {"name"})
        if unknown:
            raise invalid(f"unknown preset options: {', '.join(unknown)}")
        conf_id = DEFAULT_PRESET_ID if name == "Default" else decks.add_config_returning_id(name)
        conf = decks.get_config(conf_id)
        for option in sorted(set(item) - {"name"}):
            path, convert = PRESET_OPTIONS[option]
            _put(conf, path, convert(item[option]))
        decks.update_config(conf)
        session.presets[name] = conf_id


@setup_section("decks", order=30)
def decks_section(session, items):
    if not isinstance(items, list):
        raise invalid("setup.decks is a list")
    decks = session.col.decks
    for item in items:
        if isinstance(item, str):
            item = {"name": item}
        if not isinstance(item, dict) or not isinstance(item.get("name"), str):
            raise invalid("a deck is a name or a mapping with a name")
        unknown = sorted(set(item) - {"name", "preset"})
        if unknown:
            raise invalid(f"unknown deck keys: {', '.join(unknown)}")
        deck_id = decks.id(item["name"])
        preset = item.get("preset")
        if preset is not None:
            if preset not in session.presets and preset != "Default":
                raise invalid(f"deck {item['name']!r} uses undefined preset {preset!r}")
            deck = decks.get(deck_id)
            deck["conf"] = session.presets.get(preset, DEFAULT_PRESET_ID)
            decks.save(deck)
        session.decks[item["name"]] = deck_id


NATIVE_SEPARATOR = "\x1f"
NESTING_LIMIT = 12  # components from which Anki can refuse a name (too_deep)
SCOPES = {"study": DeckCollapseScope.REVIEWER, "browser": DeckCollapseScope.BROWSER}


def human_name(native: str) -> str:
    return native.replace(NATIVE_SEPARATOR, "::")


def _matching_ids(session, table: str, name: str, human: bool) -> list[int]:
    rows = session.col.db.all(f"select id, name from {table}")
    return [rid for rid, stored in rows if (human_name(stored) if human else stored) == name]


def find_deck(session, name: Any, what: str = "deck") -> int:
    """K1: the deck whose stored human name is exactly name."""
    if not isinstance(name, str) or not name:
        raise invalid(f"{what} is a deck's stored name")
    ids = _matching_ids(session, "decks", name, human=True)
    if not ids:
        raise DriverError("unknown_ref", ref=name)
    if len(ids) > 1:
        raise invalid(f"{len(ids)} decks are named {name!r}")
    return ids[0]


def find_preset(session, name: Any, what: str = "preset") -> int:
    """K1: the preset whose stored name is exactly name."""
    if not isinstance(name, str):
        raise invalid(f"{what} is a preset's stored name")
    ids = _matching_ids(session, "deck_config", name, human=False)
    if not ids:
        raise DriverError("unknown_ref", ref=name)
    if len(ids) > 1:
        raise invalid(f"{len(ids)} presets are named {name!r}; give presets distinct names")
    return ids[0]


def stored_name(session, did: int) -> str:
    native = session.col.db.scalar("select name from decks where id = ?", did)
    if native is None:
        raise DriverError("internal_error", detail="the deck the op returned is not in the collection")
    return human_name(native)


def deck_id(session, name: Any) -> int:
    """add_note's deck (K1): the default deck when name is None; else the deck
    whose stored human name is name, else the deck setup.decks gave that name."""
    if name is None:
        return 1
    if not isinstance(name, str) or not name:
        raise invalid("a note's deck is a deck's stored name")
    ids = _matching_ids(session, "decks", name, human=True)
    if len(ids) > 1:
        raise invalid(f"{len(ids)} decks are named {name!r}")
    if ids:
        return ids[0]
    if name in session.decks:
        return session.decks[name]
    raise DriverError("unknown_ref", ref=name)


def op_args(op_name: str, args: Any, required: set[str], optional: set[str] = frozenset()) -> dict:
    if not isinstance(args, dict) or not required <= set(args) or set(args) - required - optional:
        keys = ", ".join(sorted(required) + [f"{k}?" for k in sorted(optional)])
        raise invalid(f"{op_name} takes {{{keys}}}")
    return args


def _deck_list(op_name: str, value: Any) -> list[str]:
    if not isinstance(value, list) or not value:
        raise invalid(f"{op_name} decks is a non-empty list of deck names")
    return value


def _typed_name(op_name: str, value: Any) -> str:
    if not isinstance(value, str):
        raise invalid(f"{op_name} name is a string")
    return value


def _refusal(exc: Exception, typed: str):
    """The rejection kind of an Anki refusal of a deck name, or None."""
    if isinstance(exc, anki.errors.FilteredDeckError):
        return "filtered_parent"
    if isinstance(exc, anki.errors.InvalidInput) and len(typed.split("::")) >= NESTING_LIMIT:
        return "too_deep"
    return None


@op("add_deck", rejects=("filtered_parent", "too_deep"))
def add_deck(session, args):
    name = _typed_name("add_deck", op_args("add_deck", args, {"name"})["name"])
    try:
        out = session.col.decks.add_normal_deck_with_name(name)
    except (anki.errors.FilteredDeckError, anki.errors.InvalidInput) as exc:
        kind = _refusal(exc, name)
        if kind is None:
            raise
        raise Rejected(kind) from exc
    return {"name": stored_name(session, out.id)}


@op("rename_deck", rejects=("filtered_parent", "too_deep"))
def rename_deck(session, args):
    args = op_args("rename_deck", args, {"deck", "name"})
    did = find_deck(session, args["deck"])
    name = _typed_name("rename_deck", args["name"])
    try:
        session.col.decks.rename(did, name)
    except (anki.errors.FilteredDeckError, anki.errors.InvalidInput) as exc:
        kind = _refusal(exc, name)
        if kind is None:
            raise
        raise Rejected(kind) from exc
    return {"name": stored_name(session, did)}


@op("reparent_decks", rejects=("filtered_parent",))
def reparent_decks(session, args):
    args = op_args("reparent_decks", args, {"decks", "parent"})
    dids = [find_deck(session, name) for name in _deck_list("reparent_decks", args["decks"])]
    parent = args["parent"]
    if not isinstance(parent, str):
        raise invalid("reparent_decks parent is a deck name, or empty for the top level")
    target = 0 if parent == "" else find_deck(session, parent, "parent")
    try:
        out = session.col.decks.reparent(dids, target)
    except anki.errors.FilteredDeckError as exc:
        raise Rejected("filtered_parent") from exc
    return {"count": out.count}


@op("delete_decks")
def delete_decks(session, args):
    args = op_args("delete_decks", args, {"decks"})
    dids = [find_deck(session, name) for name in _deck_list("delete_decks", args["decks"])]
    return {"count": session.col.decks.remove(dids).count}


@op("set_deck_collapsed")
def set_deck_collapsed(session, args):
    args = op_args("set_deck_collapsed", args, {"decks", "scope", "collapsed"})
    if args["scope"] not in SCOPES:
        raise invalid("set_deck_collapsed scope is study or browser")
    if not isinstance(args["collapsed"], bool):
        raise invalid("set_deck_collapsed collapsed is a boolean")
    dids = [find_deck(session, name) for name in _deck_list("set_deck_collapsed", args["decks"])]
    for did in dids:
        session.col.decks.set_collapsed(did, args["collapsed"], SCOPES[args["scope"]])


@op("set_current_deck")
def set_current_deck(session, args):
    did = find_deck(session, op_args("set_current_deck", args, {"deck"})["deck"])
    session.col.decks.set_current(did)
