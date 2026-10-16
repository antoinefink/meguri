# SPDX-License-Identifier: AGPL-3.0-or-later
"""render [(#cN, ...)]: the card render pipeline's outputs for stored cards, in
review and browser mode, before any display step. Request, value and
normalization: specs/conformance/render-ops.md Q4; the behavior is
specs/render/card-render-pipeline.md K4-K5 and av-tags.md.

Value: {"#cN": {review, browser} | null}, null for a card whose row is gone
(its note was deleted).

Each card is rendered by the backend's existing-card render, partial for
Python as pylib's TemplateRenderContext asks for it, and composed here as
observe/template.py composes (no add-on hooks). card.question(), answer() and
render_output() are never called: they run pylib's card_did_render hooks
(LaTeX image generation and its localized errors).

  review   browser=False. The question nodes with FrontSide nothing; when the
           response is flagged empty, Anki's blank notice (the last question
           node) is dropped after checking it, and the card is blank:
           missing_cloze for a cloze note type, empty_front otherwise.
           extract_av_tags on the question (side q); unless blank, the answer
           composed with FrontSide = the extracted question text, then
           extract_av_tags on it (side a). A blank card has answer null and no
           answer entries.
  browser  browser=True: the browser formats where not empty, FrontSide
           nothing on both sides, no extraction. Only a cloze card can be
           flagged empty there (missing_cloze).

AV entries: {"sound": <the backend's name, entity-decoded, not reduced to a
base name>} or {"tts": <lang>}, in index order.

Template errors are classified as for the template kind, with the browser
side headers in browser mode; a message that fits no kind of Q2 is a run
error (anki_error). Where a TTS directive has no language, Anki writes a
localized message between brackets: every occurrence of exactly that text in
the value is reported as `[tts: lang not set]` (Q4.4), after both extractions
(so FrontSide carries Anki's own text into the answer, as in review).

Card and note rows are read when the request runs. Note types and decks come
through Anki's caches, which a db_exec write does not clear: scenarios do not
write those rows with db_exec before a render (Q4.3).
"""

from __future__ import annotations

from typing import Any

import anki.errors
from anki.consts import MODEL_CLOZE
from anki.utils import from_json_bytes

from ..errors import DriverError, invalid
from ..registry import observation
from . import template as _template

TTS_LANG_NOT_SET = "[tts: lang not set]"


def _targets(session, args: Any) -> list[tuple[str, int]]:
    """render-ops.md Q4.1: no argument (every card) or card references."""
    if args is not None and not isinstance(args, (str, list)):
        raise invalid("render takes card references or nothing (render-ops.md Q4.1)")
    targets = session.card_targets(args)
    refs = [ref for ref, _ in targets]
    if len(set(refs)) != len(refs):
        raise invalid("a render request names a card twice")
    return targets


def _entries(av_tags) -> list[dict]:
    out = []
    for tag in av_tags:
        which = tag.WhichOneof("value")
        if which == "sound_or_video":
            out.append({"sound": tag.sound_or_video})
        elif which == "tts":
            out.append({"tts": tag.tts.lang})
        else:
            raise DriverError("internal_error", detail=f"an AV tag of kind {which}")
    return out


def _error(col, exc: anki.errors.TemplateError, browser: bool) -> dict:
    classified = _template.classify(col.tr, str(exc), browser=browser)
    if classified is None:
        raise exc  # Q4.5: a run error (anki_error) until Q2's list grows
    kind, side = classified
    return {"error": {"kind": kind, "side": side}}


class _Card:
    """One stored card: its ordinal and, when needed, its note type's kind."""

    def __init__(self, col, card_id: int, card_ord: int, note_id: int) -> None:
        self.col, self.id, self.ord, self.note_id = col, card_id, card_ord, note_id
        self._cloze: bool | None = None

    @property
    def cloze(self) -> bool:
        if self._cloze is None:
            mid = self.col.db.scalar("select mid from notes where id = ?", self.note_id)
            self._cloze = from_json_bytes(self.col._backend.get_notetype_legacy(mid))["type"] == MODEL_CLOZE
        return self._cloze

    def render(self, browser: bool):
        return self.col._backend.render_existing_card(card_id=self.id, browser=browser, partial_render=True)


def _review(card: _Card) -> dict:
    col = card.col
    try:
        out = card.render(browser=False)
    except anki.errors.TemplateError as exc:
        return _error(col, exc, browser=False)
    qnodes = out.question_nodes
    if out.is_empty:
        qnodes = _template.without_blank_notice(col.tr, qnodes, card.cloze, card.ord)
    question = col._backend.extract_av_tags(text=_template.compose(qnodes, None), question_side=True)
    if out.is_empty:
        return {"question": question.text, "answer": None, "question_av": _entries(question.av_tags),
                "answer_av": [], "blank": "missing_cloze" if card.cloze else "empty_front"}
    answer = col._backend.extract_av_tags(text=_template.compose(out.answer_nodes, question.text), question_side=False)
    return {"question": question.text, "answer": answer.text, "question_av": _entries(question.av_tags),
            "answer_av": _entries(answer.av_tags), "blank": None}


def _browser(card: _Card) -> dict:
    col = card.col
    try:
        out = card.render(browser=True)
    except anki.errors.TemplateError as exc:
        return _error(col, exc, browser=True)
    if out.is_empty:
        if not card.cloze:
            raise DriverError("internal_error", detail="a standard card is blank in browser mode")
        qnodes = _template.without_blank_notice(col.tr, out.question_nodes, True, card.ord)
        return {"question": _template.compose(qnodes, None), "answer": None, "blank": "missing_cloze"}
    return {"question": _template.compose(out.question_nodes, None),
            "answer": _template.compose(out.answer_nodes, None), "blank": None}


def _normalize(value: Any, needle: str) -> Any:
    if isinstance(value, str):
        return value.replace(needle, TTS_LANG_NOT_SET)
    if isinstance(value, dict):
        return {k: _normalize(v, needle) for k, v in value.items()}
    if isinstance(value, list):
        return [_normalize(v, needle) for v in value]
    return value


@observation("render")
def render(session, args):
    targets = _targets(session, args)
    col = session.col
    tr = col.tr
    needle = "[" + tr.errors_bad_directive(directive="anki:tts", error=tr.errors_option_not_set(option="lang")) + "]"
    out = {}
    for ref, card_id in targets:
        row = col.db.first("select ord, nid from cards where id = ?", card_id)
        if row is None:
            out[ref] = None
            continue
        card = _Card(col, card_id, *row)
        out[ref] = _normalize({"review": _review(card), "browser": _browser(card)}, needle)
    return out
