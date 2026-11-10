# SPDX-License-Identifier: AGPL-3.0-or-later
"""FSRS optimizer ops (specs/conformance/optimizer-ops.md P1-P4).

compute_fsrs_params: {preset, relearning_steps, form?}
    -> {items, reviews, kept, health, eval}; rejections invalid_cutoff,
    invalid_search
    One ComputeFsrsParams request for the preset found by its exact stored
    name: the search of `form` (P1.2), the parameter list in use as
    current_params (fsrs_params_6, else _5, else _4), the stored cutoff text
    in epoch milliseconds (P1.3), relearning_steps as given and health_check
    as Anki's deck options screen reads fsrsHealthCheck. `reviews` is the
    review count of the last progress report the call leaves (read before any
    other backend call starts a new one), 0 with no item; `eval` is the
    log_loss of EvaluateParamsLegacy of the returned list with the same search
    and cutoff (its rmse_bins varies in the last bits between calls), null
    with no item. Writes nothing. The cutoff is checked first
    (Anki's ignored-before count reads the text), then the search (a card
    search), so a rejection runs no training.
optimize_all_presets: {deck}
    The deck options screen's "Save and optimize all presets" on the deck:
    one UpdateDeckConfigs request in compute-all mode with the deck's preset
    as stored, FSRS on, rescheduling off, the deck's limits as the screen
    leaves them and the other collection-wide values as read. Anki prints each
    preset's result to stdout; fd 1 points at /dev/null for the call, so the
    text reaches no log. A refused save is a run error.
optimizer_run: {}
    The oracle's side of a live chain's Meguri optimizer run: the same save on
    the default deck (id 1).
fsrs_model_check (observation): {presets: [name, ...]}
    -> {name: "ok" | "invalid_params"}
    Anki's legacy evaluation of the preset's list in use on the search
    `cid:0`, which finds no card: the backend builds the model first, so
    "invalid parameters" is invalid_params and "not enough history" is ok
    (both arrive as the invalid-input kind and are told apart by the backend's
    own translations). Anything else is internal_error. Read only.
"""

from __future__ import annotations

import os
import re
from contextlib import contextmanager
from typing import Any

import anki.errors
from anki.deck_config_pb2 import UpdateDeckConfigsMode

from ..errors import DriverError, Rejected, invalid
from ..numbers import f32_value
from ..registry import observation, op
from .deck_options import _limits, _search_name
from .decks import find_deck, find_preset

DEFAULT_DECK_ID = 1
U32_MAX = 2**32 - 1
FORMS = ("one_preset", "all_presets")
COMPUTE_ARGS = {"preset", "relearning_steps", "form"}
# Unicode White_Space: what Rust's str::trim removes (Python's isspace also
# takes U+001C-U+001F, which it does not).
WHITE_SPACE = (
    "\t\n\x0b\x0c\r \x85\xa0        "
    "        　"
)
# Anki's search writer quotes a term holding these (its reserved-word and
# leading-hyphen cases cannot start with "preset:").
_NEEDS_QUOTES = re.compile("[ 　()]")
# sched/fsrs-items#I1.3's accepted form, with each number's leading white space.
_WS = "[" + re.escape(WHITE_SPACE) + "]*"
_DATE = re.compile(rf"^{_WS}([+-][0-9]+|[0-9]{{1,4}})-{_WS}([0-9]{{1,2}})-{_WS}([0-9]{{1,2}})$")
YEAR_RANGE = (-262143, 262142)
NO_CARD_SEARCH = "cid:0"


def _days_from_civil(y: int, m: int, d: int) -> int:
    """Days from 1970-01-01 to a proleptic Gregorian date."""
    y -= m <= 2
    era = (y if y >= 0 else y - 399) // 400
    yoe = y - era * 400
    doy = (153 * (m + (-3 if m > 2 else 9)) + 2) // 5 + d - 1
    doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
    return era * 146097 + doe - 719468


def _leap(y: int) -> bool:
    return y % 4 == 0 and (y % 100 != 0 or y % 400 == 0)


def _date_ms(text: str) -> int | None:
    """00:00 UTC of the date in epoch ms, or None when I1.3 refuses the text."""
    m = _DATE.match(text)
    if not m:
        return None
    y, mo, d = (int(g) for g in m.groups())
    if not YEAR_RANGE[0] <= y <= YEAR_RANGE[1] or not 1 <= mo <= 12:
        return None
    length = [31, 29 if _leap(y) else 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][mo - 1]
    if not 1 <= d <= length:
        return None
    return _days_from_civil(y, mo, d) * 86_400_000


def _cutoff_ms(backend, text: str) -> int:
    if text == "":
        return 0
    try:
        backend.get_ignored_before_count(ignore_revlogs_before_date=text, search=NO_CARD_SEARCH)
    except anki.errors.InvalidInput:
        raise Rejected("invalid_cutoff") from None
    ms = _date_ms(text)
    if ms is None:
        raise DriverError("internal_error", detail="Anki reads a cutoff text that the I1.3 reading refuses")
    return ms


def _default_search(name: str, form: str) -> str:
    if form == "one_preset":
        return f'preset:"{_search_name(name)}" -is:suspended'
    term = ("preset:" + name).replace('"', '\\"')
    if _NEEDS_QUOTES.search(term):
        term = f'"{term}"'
    return f"({term} -is:suspended)"


def _search(config, form: str) -> str:
    stored = config.config.param_search
    blank = stored == "" if form == "one_preset" else stored.strip(WHITE_SPACE) == ""
    return _default_search(config.name, form) if blank else stored


def params_in_use(config) -> list[float]:
    """sched/presets#V6: the first non-empty of fsrs_params_6, _5 and _4."""
    inner = config.config
    for field in ("fsrs_params_6", "fsrs_params_5", "fsrs_params_4"):
        values = list(getattr(inner, field))
        if values:
            return values
    return []


def _count(v: Any) -> int:
    if not isinstance(v, int) or isinstance(v, bool) or not 0 <= v <= U32_MAX:
        raise invalid("compute_fsrs_params relearning_steps is an integer from 0 to 4294967295")
    return v


@op("compute_fsrs_params", rejects=("invalid_cutoff", "invalid_search"))
def compute_fsrs_params(session, args):
    if not isinstance(args, dict) or not {"preset", "relearning_steps"} <= set(args) or set(args) - COMPUTE_ARGS:
        raise invalid("compute_fsrs_params takes {preset, relearning_steps, form?}")
    steps = _count(args["relearning_steps"])
    form = args.get("form", "one_preset")
    if form not in FORMS:
        raise invalid("compute_fsrs_params form is one_preset or all_presets")
    col = session.col
    backend = col._backend
    config = backend.get_deck_config(find_preset(session, args["preset"]))
    ms = _cutoff_ms(backend, config.config.ignore_revlogs_before_date)
    search = _search(config, form)
    try:
        col.find_cards(search)
    except anki.errors.SearchError:
        raise Rejected("invalid_search") from None
    current = params_in_use(config)
    health_check = backend.get_deck_configs_for_update(DEFAULT_DECK_ID).fsrs_health_check
    resp = backend.compute_fsrs_params(
        search=search,
        current_params=current,
        ignore_revlogs_before_ms=ms,
        num_of_relearning_steps=steps,
        health_check=health_check,
    )
    items = resp.fsrs_items
    reviews = 0
    if items:
        progress = col.latest_progress()
        if progress.WhichOneof("value") != "compute_params":
            raise DriverError("internal_error", detail="the computation left no parameter progress report")
        reviews = progress.compute_params.reviews
    returned = list(resp.params)
    evaluation = None
    if items:
        ev = backend.evaluate_params_legacy(params=returned, search=search, ignore_revlogs_before_ms=ms)
        # rmse_bins is left out: the crate sums its bins in hash-map order, so
        # its last bits differ between two calls on the same items.
        evaluation = {"log_loss": f32_value(ev.log_loss)}
    health = None
    if resp.HasField("health_check_passed"):
        health = "good" if resp.health_check_passed else "bad"
    return {
        "items": items,
        "reviews": reviews,
        "kept": returned == current,
        "health": health,
        "eval": evaluation,
    }


@contextmanager
def _stdout_discarded():
    saved = os.dup(1)
    devnull = os.open(os.devnull, os.O_WRONLY)
    try:
        os.dup2(devnull, 1)
        yield
    finally:
        os.dup2(saved, 1)
        os.close(saved)
        os.close(devnull)


def _optimize_all(session, deck_id: int) -> None:
    backend = session.col._backend
    screen = backend.get_deck_configs_for_update(deck_id)
    stored = backend.get_deck_config(screen.current_deck.config_id)
    with _stdout_discarded():
        backend.update_deck_configs(
            target_deck_id=deck_id,
            configs=[stored],
            removed_config_ids=[],
            mode=UpdateDeckConfigsMode.UPDATE_DECK_CONFIGS_MODE_COMPUTE_ALL_PARAMS,
            card_state_customizer=screen.card_state_customizer,
            limits=_limits(screen, {}, {}),
            new_cards_ignore_review_limit=screen.new_cards_ignore_review_limit,
            apply_all_parent_limits=screen.apply_all_parent_limits,
            fsrs=True,
            fsrs_reschedule=False,
            fsrs_health_check=screen.fsrs_health_check,
        )


@op("optimize_all_presets")
def optimize_all_presets(session, args):
    if not isinstance(args, dict) or set(args) != {"deck"}:
        raise invalid("optimize_all_presets takes {deck}")
    _optimize_all(session, find_deck(session, args["deck"]))


@op("optimizer_run")
def optimizer_run(session, args):
    if not isinstance(args, dict) or args:
        raise invalid("optimizer_run takes {}")
    _optimize_all(session, DEFAULT_DECK_ID)


@observation("fsrs_model_check")
def fsrs_model_check(session, args):
    if not isinstance(args, dict) or set(args) != {"presets"}:
        raise invalid("fsrs_model_check takes {presets}")
    names = args["presets"]
    if not isinstance(names, list) or not names or not all(isinstance(n, str) and n for n in names):
        raise invalid("fsrs_model_check presets is a non-empty list of preset names")
    if len(set(names)) != len(names):
        raise invalid("fsrs_model_check names a preset twice")
    col = session.col
    tr = col.tr
    out = {}
    for name in names:
        config = col._backend.get_deck_config(find_preset(session, name))
        try:
            col._backend.evaluate_params_legacy(
                params=params_in_use(config), search=NO_CARD_SEARCH, ignore_revlogs_before_ms=0
            )
        except anki.errors.InvalidInput as exc:
            message = str(exc)
            if message == tr.deck_config_invalid_parameters():
                out[name] = "invalid_params"
                continue
            if message == tr.deck_config_not_enough_history():
                out[name] = "ok"
                continue
            raise DriverError("internal_error", detail="the model check failed with another invalid input") from None
        raise DriverError("internal_error", detail="an evaluation on no card did not fail")
    return out
