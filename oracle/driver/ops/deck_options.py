# SPDX-License-Identifier: AGPL-3.0-or-later
"""The deck options screen's save, the scheduling preferences' save and the
ignore-date count (specs/conformance/deck-options-ops.md E1, E2, E4).

save_deck_options: {deck, preset?, options?, deck_desired_retention?, limits?,
                    globals?, remove?, subtree?} -> none; rejection default_preset
    Reads the screen's state for the deck (GetDeckConfigsForUpdate), takes the
    chosen preset's config from it (the deck's own preset when `preset` is
    missing), applies `options` with the setup.presets conversions
    (decks.PRESET_OPTIONS) plus historical_retention, ignore_reviews_before
    and insertion_order, and sends one UpdateDeckConfigs request as the
    screen's Save does: that one config, the presets of `remove` as removed
    ids, the deck as target, mode apply-to-children with `subtree` (else
    normal), the deck's limits as the screen leaves them (below) with
    `limits` and desired_retention set (a number), cleared (null) or kept
    (missing), `globals` over the two limit options as read, the other
    collection-wide values as read (fsrs, card state customizer, health
    check) and fsrs_reschedule false. Anki then removes the presets (scm set),
    writes the preset and the decks, reorders new cards, remaps learning steps
    and recomputes memory states, as one "Deck Options" undo step.
    Limits the argument does not name: the This-deck ones as read; a
    Today-only one as read while its stored day is today (the screen's Today
    tab is the one in use), else cleared (the screen's other tab clears it).
    Anki's save refuses to remove the default preset (id 1) with an invalid
    input and rolls back: that is the rejection default_preset (the screen
    never sends it; the refusal also empties Anki's undo history, as any
    backend op that fails does). Removing the chosen preset is
    invalid_scenario. Any other save Anki refuses is a run error.
set_scheduling_prefs: {rollover?, learn_ahead_minutes?, load_balancer?} -> none
    Anki's preferences read (GetPreferences), rollover, learn_ahead_secs
    (minutes x 60) and load_balancer_enabled replaced, and saved back with
    SetPreferences, as the desktop preferences dialog does: one "Preferences"
    undo step that writes the other scheduling, reviewing, editing and backup
    keys with the values read, and drops the cached day when the hour
    changes.
ignored_before_count (observation): {preset, dates: [text, ...]}
    -> [{included, total} | {error: "invalid_date"}], one entry per date.
    GetIgnoredBeforeCount with the screen's search: the preset's param_search
    when not empty, else `preset:"<stored name>" -is:suspended` with `\\` and
    `"` escaped as the screen escapes them. A backend InvalidInput (the date
    parse) is invalid_date; any other failure is a run error. Read only.

Decks and presets are found by their stored names (human deck names, "::" for
the native separator), so `Default` is the default deck or preset. The
collection is not reopened: that would clear the undo history the save adds
to.
"""

from __future__ import annotations

from typing import Any, Callable

import anki.errors
from anki.deck_config_pb2 import DeckConfig, DeckConfigsForUpdate, UpdateDeckConfigsMode

from ..errors import DriverError, Rejected, invalid
from ..registry import observation, op
from .decks import _bool, _int, _leech_action, _num, _params, _steps

NATIVE_SEPARATOR = "\x1f"
U32_MAX = 2**32 - 1


def _u32(v: Any) -> int:
    v = _int(v)
    if not 0 <= v <= U32_MAX:
        raise invalid(f"{v} does not fit the option's unsigned 32-bit field")
    return v


def _text(v: Any) -> str:
    if not isinstance(v, str):
        raise invalid(f"{v!r} is not a string")
    return v


def _ease(v: Any) -> float:
    # setup.presets stores round(v * 1000) in initialFactor, which Anki reads
    # back as initialFactor / 1000: the same value here.
    return round(_num(v) * 1000) / 1000


def _insertion_order(v: Any) -> int:
    # NewCardInsertOrder: DUE (sequential) 0, RANDOM 1.
    orders = {"sequential": 0, "random": 1}
    if v not in orders:
        raise invalid("insertion_order is sequential or random")
    return orders[v]


# option -> (DeckConfig.Config field, converter): setup.presets' options with
# their conversions (decks.PRESET_OPTIONS, dsl.md D6.2), then the three only the
# save takes (deck-options-ops.md E1).
OPTIONS: dict[str, tuple[str, Callable[[Any], Any]]] = {
    "learn_steps": ("learn_steps", _steps),
    "relearn_steps": ("relearn_steps", _steps),
    "graduating_interval_days": ("graduating_interval_good", _u32),
    "easy_interval_days": ("graduating_interval_easy", _u32),
    "new_per_day": ("new_per_day", _u32),
    "reviews_per_day": ("reviews_per_day", _u32),
    "max_interval_days": ("maximum_review_interval", _u32),
    "min_lapse_interval_days": ("minimum_lapse_interval", _u32),
    "starting_ease": ("initial_ease", _ease),
    "easy_bonus": ("easy_multiplier", _num),
    "hard_interval": ("hard_multiplier", _num),
    "interval_modifier": ("interval_multiplier", _num),
    "lapse_new_interval": ("lapse_multiplier", _num),
    "leech_threshold": ("leech_threshold", _u32),
    "leech_action": ("leech_action", _leech_action),
    "bury_new": ("bury_new", _bool),
    "bury_reviews": ("bury_reviews", _bool),
    "bury_interday_learning": ("bury_interday_learning", _bool),
    "desired_retention": ("desired_retention", _num),
    "fsrs_params": ("fsrs_params_6", _params),
    "historical_retention": ("historical_retention", _num),
    "ignore_reviews_before": ("ignore_revlogs_before_date", _text),
    "insertion_order": ("new_card_insert_order", _insertion_order),
}
ARGS = {"deck", "preset", "options", "deck_desired_retention", "limits", "globals", "remove", "subtree"}
# limits member -> (Limits field, the Limits field saying its stored day is
# today, for a Today-only limit).
LIMITS: dict[str, tuple[str, str | None]] = {
    "new": ("new", None),
    "review": ("review", None),
    "new_today": ("new_today", "new_today_active"),
    "review_today": ("review_today", "review_today_active"),
}
LIMIT_MAX = 9999
# globals member -> DeckConfigsForUpdate field (and UpdateDeckConfigs argument).
GLOBALS = {
    "new_cards_ignore_review_limit": "new_cards_ignore_review_limit",
    "apply_all_parent_limits": "apply_all_parent_limits",
}
DEFAULT_PRESET_ID = 1


def _limit(v: Any) -> int | None:
    if v is None:
        return None
    v = _int(v)
    if not 0 <= v <= LIMIT_MAX:
        raise invalid(f"a deck limit is an integer from 0 to {LIMIT_MAX} or null, not {v}")
    return v


def _mapping(args: dict, key: str, members) -> dict:
    value = args.get(key, {})
    if not isinstance(value, dict):
        raise invalid(f"save_deck_options {key} is a mapping")
    unknown = sorted(set(value) - set(members))
    if unknown:
        raise invalid(f"unknown save_deck_options {key}: {', '.join(unknown)}")
    return value


def _names(v: Any) -> list[str]:
    if not isinstance(v, list) or not v or not all(isinstance(n, str) and n for n in v):
        raise invalid("save_deck_options remove is a non-empty list of preset names")
    if len(set(v)) != len(v):
        raise invalid("save_deck_options remove names a preset twice")
    return v


def _find(session, table: str, name: Any, what: str) -> int:
    if not isinstance(name, str) or not name:
        raise invalid(f"{what} is a non-empty name")
    rows = session.col.db.all(f"select id, name from {table}")
    if table == "decks":
        rows = [(rid, stored.replace(NATIVE_SEPARATOR, "::")) for rid, stored in rows]
    ids = [rid for rid, stored in rows if stored == name]
    if not ids:
        raise DriverError("unknown_ref", ref=name)
    if len(ids) > 1:
        raise invalid(f"{len(ids)} {what}s are named {name!r}")
    return ids[0]


def _set(inner, field: str, value: Any) -> None:
    if isinstance(value, list):
        inner.ClearField(field)
        getattr(inner, field).extend(value)
    else:
        setattr(inner, field, value)


def _limits(screen, args: dict, given: dict):
    """The Limits the screen sends: as read, a Today-only limit cleared unless
    its stored day is today, then the argument's values."""
    limits = DeckConfigsForUpdate.CurrentDeck.Limits()
    limits.CopyFrom(screen.current_deck.limits)
    for name, (field, active) in LIMITS.items():
        if name in given:
            value = given[name]
        elif active is not None and not getattr(limits, active):
            value = None
        else:
            continue
        if value is None:
            limits.ClearField(field)
        else:
            setattr(limits, field, value)
    if "deck_desired_retention" in args:
        retention = args["deck_desired_retention"]
        if retention is None:
            limits.ClearField("desired_retention")
        else:
            limits.desired_retention = _num(retention)
    return limits


@op("save_deck_options", rejects=("default_preset",))
def save_deck_options(session, args):
    if not isinstance(args, dict) or "deck" not in args or set(args) - ARGS:
        raise invalid(
            "save_deck_options takes {deck, preset?, options?, deck_desired_retention?, limits?, globals?, "
            "remove?, subtree?}"
        )
    options = _mapping(args, "options", OPTIONS)
    changes = {OPTIONS[name][0]: OPTIONS[name][1](options[name]) for name in sorted(options)}
    given_limits = {name: _limit(v) for name, v in _mapping(args, "limits", LIMITS).items()}
    given_globals = {GLOBALS[name]: _bool(v) for name, v in _mapping(args, "globals", GLOBALS).items()}
    if args.get("deck_desired_retention") is not None:
        _num(args["deck_desired_retention"])
    subtree = _bool(args.get("subtree", False))
    removed_names = _names(args["remove"]) if "remove" in args else []
    deck_id = _find(session, "decks", args["deck"], "deck")
    preset_id = _find(session, "deck_config", args["preset"], "preset") if "preset" in args else None
    removed = [_find(session, "deck_config", name, "preset") for name in removed_names]

    backend = session.col._backend
    screen = backend.get_deck_configs_for_update(deck_id)
    if preset_id is None:
        preset_id = screen.current_deck.config_id
    if preset_id in removed:
        raise invalid("save_deck_options removes the preset the deck is switched to")
    shown = [entry.config for entry in screen.all_config if entry.config.id == preset_id]
    if len(shown) != 1:
        raise DriverError("internal_error", detail="the deck options screen does not list the preset")
    config = DeckConfig()
    config.CopyFrom(shown[0])
    for field, value in changes.items():
        _set(config.config, field, value)
    collection_wide = {field: getattr(screen, field) for field in GLOBALS.values()}
    collection_wide.update(given_globals)
    mode = (
        UpdateDeckConfigsMode.UPDATE_DECK_CONFIGS_MODE_APPLY_TO_CHILDREN
        if subtree
        else UpdateDeckConfigsMode.UPDATE_DECK_CONFIGS_MODE_NORMAL
    )
    try:
        backend.update_deck_configs(
            target_deck_id=deck_id,
            configs=[config],
            removed_config_ids=removed,
            mode=mode,
            card_state_customizer=screen.card_state_customizer,
            limits=_limits(screen, args, given_limits),
            fsrs=screen.fsrs,
            fsrs_reschedule=False,
            fsrs_health_check=screen.fsrs_health_check,
            **collection_wide,
        )
    except anki.errors.InvalidInput:
        # Removals run first; Anki refuses the default one and rolls back.
        if DEFAULT_PRESET_ID in removed:
            raise Rejected("default_preset") from None
        raise


PREFS = {"rollover", "learn_ahead_minutes", "load_balancer"}


def _bounded(name: str, v: Any, high: int) -> int:
    v = _int(v)
    if not 0 <= v <= high:
        raise invalid(f"set_scheduling_prefs {name} is an integer from 0 to {high}, not {v}")
    return v


@op("set_scheduling_prefs")
def set_scheduling_prefs(session, args):
    if not isinstance(args, dict) or not args or set(args) - PREFS:
        raise invalid("set_scheduling_prefs takes {rollover?, learn_ahead_minutes?, load_balancer?}, at least one")
    rollover = _bounded("rollover", args["rollover"], 23) if "rollover" in args else None
    minutes = None
    if "learn_ahead_minutes" in args:
        minutes = _bounded("learn_ahead_minutes", args["learn_ahead_minutes"], 999)
    balancer = _bool(args["load_balancer"]) if "load_balancer" in args else None
    prefs = session.col.get_preferences()
    if rollover is not None:
        prefs.scheduling.rollover = rollover
    if minutes is not None:
        prefs.scheduling.learn_ahead_secs = minutes * 60
    if balancer is not None:
        prefs.reviewing.load_balancer_enabled = balancer
    session.col.set_preferences(prefs)


def _search_name(name: str) -> str:
    return name.replace("\\", "\\\\").replace('"', '\\"')


@observation("ignored_before_count")
def ignored_before_count(session, args):
    if not isinstance(args, dict) or set(args) != {"preset", "dates"}:
        raise invalid("ignored_before_count takes {preset, dates}")
    dates = args["dates"]
    if not isinstance(dates, list) or not dates or not all(isinstance(d, str) for d in dates):
        raise invalid("ignored_before_count dates is a non-empty list of strings")
    backend = session.col._backend
    preset = backend.get_deck_config(_find(session, "deck_config", args["preset"], "preset"))
    search = preset.config.param_search or f'preset:"{_search_name(preset.name)}" -is:suspended'
    out = []
    for date in dates:
        try:
            counts = backend.get_ignored_before_count(ignore_revlogs_before_date=date, search=search)
        except anki.errors.InvalidInput:
            out.append({"error": "invalid_date"})
        else:
            out.append({"included": counts.included, "total": counts.total})
    return out
