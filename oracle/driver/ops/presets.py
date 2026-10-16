# SPDX-License-Identifier: AGPL-3.0-or-later
"""Preset ops (specs/conformance/deck-ops.md K8-K10). Presets and decks are
named as K1 says (ops/decks.py).

add_preset: {name}   the schema-11 defaults of the backend with the FSRS-6
    parameters the deck options screen gives a new preset (its `defaults`),
    saved through the deck-config storage call (stamped, new id)
clone_preset: {preset, name}   the source's schema-11 object with id 0 and the
    new name, saved the same way
rename_preset: {preset, name}   the stored object with the new name, saved the
    same way; nothing is called when the stored name already is name
remove_preset: {preset, fallback}   confirmed: Anki's preset removal (refused
    for the default preset, rejection default_preset; it sets scm), then every
    normal deck whose preset is the removed one or names no preset gets the
    fallback through Anki's deck update
set_deck_preset: {deck, preset, subtree?}   the deck, and with subtree its
    normal descendants (Anki's deck-and-children list), get the preset through
    Anki's deck update, which writes nothing for a deck that already has it;
    rejection filtered_deck when deck is filtered

None of them goes through the deck options save, which also writes
collection-wide keys and deck limits (specs/decks/deck-presets.md P12). The
deck-config storage call stamps a preset even when it is unchanged; the deck
options screen does nothing for a rename to the current name and its save skips
an unchanged preset, hence rename_preset's check. The removal and the deck
update run in Anki's order: removal first, so a refused removal writes nothing.
"""

from __future__ import annotations

import json
from typing import Any

import anki.errors

from ..errors import Rejected, invalid
from ..numbers import f32_value
from ..registry import op, scenario_check
from .decks import find_deck, find_preset, op_args

DEFAULT_PRESET_ID = 1


def _name(op_name: str, value: Any) -> str:
    if not isinstance(value, str):
        raise invalid(f"{op_name} name is a string")
    return value


def _save_legacy(session, conf: dict) -> int:
    data = json.dumps(conf, ensure_ascii=False).encode("utf-8")
    return session.col._backend.add_or_update_deck_config_legacy(json=data)


def _legacy(session, conf_id: int) -> dict:
    return json.loads(session.col._backend.get_deck_config_legacy(conf_id))


def _set_deck_preset(session, did: int, conf_id: int) -> None:
    """Anki's deck update with the normal deck's config_id set."""
    deck = session.col._backend.get_deck(did)
    deck.normal.config_id = conf_id
    session.col._backend.update_deck(deck)


def _is_normal(deck) -> bool:
    return deck.WhichOneof("kind") == "normal"


@op("add_preset")
def add_preset(session, args):
    name = _name("add_preset", op_args("add_preset", args, {"name"})["name"])
    defaults = session.col.decks.get_deck_configs_for_update(DEFAULT_PRESET_ID).defaults
    conf = json.loads(session.col._backend.new_deck_config_legacy())
    conf["name"] = name
    conf["fsrsParams6"] = [f32_value(x) for x in defaults.config.fsrs_params_6]
    _save_legacy(session, conf)


@op("clone_preset")
def clone_preset(session, args):
    args = op_args("clone_preset", args, {"preset", "name"})
    source = find_preset(session, args["preset"])
    name = _name("clone_preset", args["name"])
    conf = _legacy(session, source)
    conf["id"] = 0
    conf["name"] = name
    _save_legacy(session, conf)


@op("rename_preset")
def rename_preset(session, args):
    args = op_args("rename_preset", args, {"preset", "name"})
    conf_id = find_preset(session, args["preset"])
    name = _name("rename_preset", args["name"])
    if session.col.db.scalar("select name from deck_config where id = ?", conf_id) == name:
        return
    conf = _legacy(session, conf_id)
    conf["name"] = name
    _save_legacy(session, conf)


@scenario_check
def check_remove_preset_steps(scenario: dict) -> None:
    for index, step in enumerate(scenario.get("steps", [])):
        if isinstance(step, dict) and len(step) == 1 and "remove_preset" in step:
            args = step["remove_preset"]
            if isinstance(args, dict) and "preset" in args and args.get("preset") == args.get("fallback"):
                raise invalid("remove_preset preset and fallback differ", step=index)


@op("remove_preset", rejects=("default_preset",))
def remove_preset(session, args):
    args = op_args("remove_preset", args, {"preset", "fallback"})
    removed = find_preset(session, args["preset"])
    fallback = find_preset(session, args["fallback"], "fallback")
    if removed == fallback:
        raise invalid("remove_preset preset and fallback differ")
    backend = session.col._backend
    try:
        backend.remove_deck_config(removed)
    except anki.errors.InvalidInput as exc:
        if removed != DEFAULT_PRESET_ID:
            raise
        raise Rejected("default_preset") from exc
    remaining = set(session.col.db.list("select id from deck_config"))
    for did in session.col.db.list("select id from decks order by id"):
        deck = backend.get_deck(did)
        if _is_normal(deck) and (deck.normal.config_id == removed or deck.normal.config_id not in remaining):
            _set_deck_preset(session, did, fallback)


@op("set_deck_preset", rejects=("filtered_deck",))
def set_deck_preset(session, args):
    args = op_args("set_deck_preset", args, {"deck", "preset"}, {"subtree"})
    did = find_deck(session, args["deck"])
    conf_id = find_preset(session, args["preset"])
    subtree = args.get("subtree", False)
    if not isinstance(subtree, bool):
        raise invalid("set_deck_preset subtree is a boolean")
    if not _is_normal(session.col._backend.get_deck(did)):
        raise Rejected("filtered_deck")
    targets = session.col.decks.deck_and_child_ids(did) if subtree else [did]
    for target in targets:
        if _is_normal(session.col._backend.get_deck(target)):
            _set_deck_preset(session, target, conf_id)
