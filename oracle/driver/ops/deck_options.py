# SPDX-License-Identifier: AGPL-3.0-or-later
"""The deck options screen's save and its ignore-date count
(specs/conformance/deck-options-ops.md E1, E2).

save_deck_options: {deck, preset?, options?, deck_desired_retention?} -> none
    Reads the screen's state for the deck (GetDeckConfigsForUpdate), takes the
    chosen preset's config from it (the deck's own preset when `preset` is
    missing), applies `options` with the setup.presets conversions
    (decks.PRESET_OPTIONS) plus historical_retention and ignore_reviews_before,
    and sends one UpdateDeckConfigs request as the screen's Save does: that
    one config, the deck as target, mode normal, the deck's limits as read
    with desired_retention set (a number), cleared (null) or kept (missing),
    the collection-wide values as read (fsrs, card state customizer, new
    cards ignore review limit, apply all parent limits, health check) and
    fsrs_reschedule false. Anki then writes the preset and the deck, reorders
    new cards, remaps learning steps and recomputes memory states, as one
    "Deck Options" undo step. A save Anki refuses is a run error (no
    rejection kinds).
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

from ..errors import DriverError, invalid
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


# option -> (DeckConfig.Config field, converter): setup.presets' options with
# their conversions (decks.PRESET_OPTIONS, dsl.md D6.2), then the two only the
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
}
ARGS = {"deck", "preset", "options", "deck_desired_retention"}


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


@op("save_deck_options")
def save_deck_options(session, args):
    if not isinstance(args, dict) or "deck" not in args or set(args) - ARGS:
        raise invalid("save_deck_options takes {deck, preset?, options?, deck_desired_retention?}")
    options = args.get("options", {})
    if not isinstance(options, dict):
        raise invalid("save_deck_options options is a mapping of options to values")
    unknown = sorted(set(options) - set(OPTIONS))
    if unknown:
        raise invalid(f"unknown save_deck_options options: {', '.join(unknown)}")
    changes = {OPTIONS[name][0]: OPTIONS[name][1](options[name]) for name in sorted(options)}
    retention = args.get("deck_desired_retention")
    if retention is not None:
        retention = _num(retention)
    deck_id = _find(session, "decks", args["deck"], "deck")
    preset_id = _find(session, "deck_config", args["preset"], "preset") if "preset" in args else None

    backend = session.col._backend
    screen = backend.get_deck_configs_for_update(deck_id)
    if preset_id is None:
        preset_id = screen.current_deck.config_id
    shown = [entry.config for entry in screen.all_config if entry.config.id == preset_id]
    if len(shown) != 1:
        raise DriverError("internal_error", detail="the deck options screen does not list the preset")
    config = DeckConfig()
    config.CopyFrom(shown[0])
    for field, value in changes.items():
        _set(config.config, field, value)
    limits = DeckConfigsForUpdate.CurrentDeck.Limits()
    limits.CopyFrom(screen.current_deck.limits)
    if "deck_desired_retention" in args:
        if retention is None:
            limits.ClearField("desired_retention")
        else:
            limits.desired_retention = retention
    backend.update_deck_configs(
        target_deck_id=deck_id,
        configs=[config],
        removed_config_ids=[],
        mode=UpdateDeckConfigsMode.UPDATE_DECK_CONFIGS_MODE_NORMAL,
        card_state_customizer=screen.card_state_customizer,
        limits=limits,
        new_cards_ignore_review_limit=screen.new_cards_ignore_review_limit,
        fsrs=screen.fsrs,
        apply_all_parent_limits=screen.apply_all_parent_limits,
        fsrs_reschedule=False,
        fsrs_health_check=screen.fsrs_health_check,
    )


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
