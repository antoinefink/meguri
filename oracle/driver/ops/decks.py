# SPDX-License-Identifier: AGPL-3.0-or-later
"""Presets (deck options) and decks.

setup.presets: [{name, <option>: <value>, ...}]   options below; a preset named
                 "Default" edits the collection's default preset
setup.decks:   [{name, preset?}]   "::" nests decks; preset defaults to Default

Options are written through the schema-11 deck-config JSON, the persisted
format. Steps are durations ('30s', '10m', '1d').
"""

from __future__ import annotations

from typing import Any, Callable

from ..clock import parse_duration_minutes
from ..errors import invalid
from ..registry import setup_section

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


def deck_id(session, name: Any) -> int:
    """A deck named in setup.decks, or the default deck when name is None."""
    if name is None:
        return 1
    if name not in session.decks:
        raise invalid(f"deck {name!r} is not in setup.decks")
    return session.decks[name]
