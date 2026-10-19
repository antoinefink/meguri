# SPDX-License-Identifier: AGPL-3.0-or-later
"""type_answer and review_html: the type-in comparison, and the HTML review
shows for a stored card (render area, U64). Requests, values and errors:
specs/conformance/type-answer-ops.md K1-K3; the behavior is
specs/render/type-answer.md and specs/render/media-display.md.

type_answer {fns, inputs}   the collection's compare_answer(expected, typed,
    combining) for every [expected, typed] pair: combining on for `compare`,
    off for `compare_nc`. It runs on one scratch collection per worker process
    (as text.py), since the call reads and writes nothing; the scenario's
    collection is never touched, so the kind works in any observe step.

review_html [{card, typed?}]   per case, on the scenario's collection: the card
    read fresh (no render or note kept from an earlier case), its question()
    and answer() (the render the reviewer uses), each side through the media
    folder's escape_media_filenames in the encoding direction, then the
    reviewer's question and answer type-in filters, in that order. Play
    buttons are not added (render/type-answer#A3 step 3).

    The image holds Anki's library, not its desktop app, so _Reviewer carries
    the two filters and their cloze helper from qt/aqt/reviewer.py at tag
    26.09.3 with executable bodies unchanged. K3's three changes are made by what
    the bodies call: `tr` returns the warning tokens of render/type-answer#A8
    instead of the localized texts (inserted through re.sub exactly as Anki
    inserts its texts), `gui_hooks` passes the two comparison hooks' values
    through, and `mw.col` is the scenario's collection. typedAnswer is the
    case's typed text ("" by default).
"""

from __future__ import annotations

import os
import re
import tempfile
from re import Match
from typing import Any

from anki.collection import Collection

from ..errors import DriverError, invalid
from ..registry import observation, scenario_check
from ..session import parse_observation
from . import render as _render

FNS = ("compare", "compare_nc")
CASE_KEYS = {"card", "typed"}
CARD_REF = re.compile(r"#c[1-9][0-9]*")


# --- type_answer ---------------------------------------------------------------


class _Scratch:
    """The worker's collection for compare_answer: a new, empty collection."""

    def __init__(self, root: str) -> None:
        self.dir = tempfile.mkdtemp(prefix="type-answer-", dir=root)
        self.col = Collection(os.path.join(self.dir, "collection.anki2"))


_scratch: _Scratch | None = None


def _get_scratch(session) -> _Scratch:
    global _scratch
    if _scratch is None or not os.path.isdir(_scratch.dir):
        _scratch = _Scratch(os.path.dirname(session.dir))
    return _scratch


def _check_type_answer(args: Any) -> tuple[list[str], list[list[str]]]:
    if not isinstance(args, dict) or set(args) != {"fns", "inputs"}:
        raise invalid("type_answer takes {fns, inputs}")
    fns, inputs = args["fns"], args["inputs"]
    if not isinstance(fns, list) or not fns or not all(isinstance(f, str) for f in fns):
        raise invalid("type_answer fns is a non-empty list of function names")
    unknown = sorted(set(fns) - set(FNS))
    if unknown:
        raise invalid(f"unknown type_answer functions: {', '.join(unknown)}")
    if len(set(fns)) != len(fns):
        raise invalid("a type_answer function is listed twice")
    if not isinstance(inputs, list):
        raise invalid("type_answer inputs is a list")
    for item in inputs:
        if not (isinstance(item, list) and len(item) == 2 and all(isinstance(s, str) for s in item)):
            raise invalid("type_answer inputs are [expected, typed] pairs of strings")
    return fns, inputs


@observation("type_answer")
def type_answer(session, args):
    fns, inputs = _check_type_answer(args)
    col = _get_scratch(session).col
    combining = {"compare": True, "compare_nc": False}
    return {fn: [col.compare_answer(expected, typed, combining[fn]) for expected, typed in inputs] for fn in fns}


# --- review_html: the reviewer's type-in filters --------------------------------


class _Tokens:
    """`tr` for the carried filters: the two warnings as the observation tokens
    of render/type-answer#A8, never Anki's localized text (R1887)."""

    @staticmethod
    def studying_please_run_toolsempty_cards() -> str:
        return "[[warning:empty_cards]]"

    @staticmethod
    def studying_type_answer_unknown_field(val: str) -> str:
        return f"[[warning:unknown_field:{val}]]"


class _NoAddons:
    """`gui_hooks` for the carried filters: the comparison hooks with nothing
    registered return their first argument."""

    @staticmethod
    def reviewer_will_compare_answer(expected_provided_tuple: tuple[str, str], type_pattern: str) -> tuple[str, str]:
        return expected_provided_tuple

    @staticmethod
    def reviewer_will_render_compared_answer(
        output: str, initial_expected: str, initial_provided: str, type_pattern: str
    ) -> str:
        return output


tr = _Tokens()
gui_hooks = _NoAddons()


class _Main:
    """`mw` for the carried filters: only its collection is used."""

    def __init__(self, col: Collection) -> None:
        self.col = col


class _Reviewer:
    """One case's reviewer: the state Reviewer.__init__ and _showQuestion set
    before the filters run, then the filters themselves."""

    def __init__(self, col: Collection, card, typed: str) -> None:
        self.mw = _Main(col)
        self.card = card
        self._combining: bool = True
        self.typeCorrect: str | None = None
        self.typedAnswer: str | None = typed

    # --- carried from qt/aqt/reviewer.py (class Reviewer) at tag 26.09.3 ------
    # The bodies below are unchanged; do not edit them (type-answer-ops.md K3).

    typeAnsPat = r"\[\[type:(.+?)\]\]"

    def typeAnsQuestionFilter(self, buf: str) -> str:
        self._combining = True
        self.typeCorrect = None
        clozeIdx = None
        m = re.search(self.typeAnsPat, buf)
        if not m:
            return buf
        fld = m.group(1)
        # if it's a cloze, extract data
        if fld.startswith("cloze:"):
            # get field and cloze position
            clozeIdx = self.card.ord + 1
            fld = fld.split(":")[1]
        if fld.startswith("nc:"):
            self._combining = False
            fld = fld.split(":")[1]
        # loop through fields for a match
        for f in self.card.note_type()["flds"]:
            if f["name"] == fld:
                self.typeCorrect = self.card.note()[f["name"]]
                if clozeIdx:
                    # narrow to cloze
                    self.typeCorrect = self._contentForCloze(self.typeCorrect, clozeIdx)
                self.typeFont = f["font"]
                self.typeSize = f["size"]
                break
        if not self.typeCorrect:
            if self.typeCorrect is None:
                if clozeIdx:
                    warn = tr.studying_please_run_toolsempty_cards()
                else:
                    warn = tr.studying_type_answer_unknown_field(val=fld)
                return re.sub(self.typeAnsPat, warn, buf)
            else:
                # empty field, remove type answer pattern
                return re.sub(self.typeAnsPat, "", buf)
        return re.sub(
            self.typeAnsPat,
            f"""
<center>
<input type=text id=typeans onkeypress="_typeAnsPress();"
   style="font-family: '{self.typeFont}'; font-size: {self.typeSize}px;">
</center>
""",
            buf,
        )

    def typeAnsAnswerFilter(self, buf: str) -> str:
        if not self.typeCorrect:
            return re.sub(self.typeAnsPat, "", buf)
        m = re.search(self.typeAnsPat, buf)
        type_pattern = m.group(1) if m else ""
        orig = buf
        origSize = len(buf)
        buf = buf.replace("<hr id=answer>", "")
        hadHR = len(buf) != origSize
        initial_expected = self.typeCorrect
        initial_provided = self.typedAnswer
        expected, provided = gui_hooks.reviewer_will_compare_answer(
            (initial_expected, initial_provided), type_pattern
        )

        output = self.mw.col.compare_answer(expected, provided, self._combining)
        output = gui_hooks.reviewer_will_render_compared_answer(
            output,
            initial_expected,
            initial_provided,
            type_pattern,
        )

        # and update the type answer area
        def repl(match: Match) -> str:
            # can't pass a string in directly, and can't use re.escape as it
            # escapes too much
            s = """
<div style="font-family: '{}'; font-size: {}px">{}</div>""".format(
                self.typeFont,
                self.typeSize,
                output,
            )
            if hadHR:
                # Keep the separator before comparison content when FrontSide is used.
                s = f"<hr id=answer>{s}"
            return s

        if hadHR and not re.search(self.typeAnsPat, buf):
            return orig

        return re.sub(self.typeAnsPat, repl, buf)

    def _contentForCloze(self, txt: str, idx: int) -> str | None:
        return self.mw.col.extract_cloze_for_typing(txt, idx) or None

    # --- end of the carried code -----------------------------------------------


# --- review_html: the observation ------------------------------------------------


def check_review_request(args: Any) -> list[tuple[str, str]]:
    """K2's request, without the collection: [(card ref, typed text)]."""
    if not isinstance(args, list) or not args:
        raise invalid("review_html takes a non-empty list of cases")
    cases = []
    for case in args:
        if not isinstance(case, dict) or set(case) - CASE_KEYS or "card" not in case:
            raise invalid("a review_html case is {card, typed?}")
        ref, typed = case["card"], case.get("typed", "")
        if not isinstance(ref, str) or not CARD_REF.fullmatch(ref):
            raise invalid(f"{ref!r} is not a #c<N> reference")
        if not isinstance(typed, str):
            raise invalid("a review_html case's typed is a string")
        cases.append((ref, typed))
    return cases


@scenario_check
def check_review_html_requests(scenario: dict) -> None:
    """At most one review_html request per observe step (O2), each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe":
            continue
        requests = [args for kind, args in map(parse_observation, specs) if kind == "review_html"]
        if len(requests) > 1:
            raise invalid("an observe step has at most one review_html request", step=index)
        for args in requests:
            try:
                check_review_request(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


def _review(col: Collection, card_id: int, typed: str) -> dict:
    card = col.get_card(card_id)
    reviewer = _Reviewer(col, card, typed)
    pipeline = _render._review(_render._Card(col, card.id, card.ord, card.nid))
    if "error" in pipeline or pipeline["blank"] is not None:
        raise invalid("review_html requires a nonblank card without template errors")
    prefix = "<style>" + card.note_type()["css"] + "</style>"
    try:
        question = reviewer.typeAnsQuestionFilter(col.media.escape_media_filenames(prefix + pipeline["question"]))
    except (re.error, IndexError):
        return {"error": "invalid_type_answer_replacement"}
    answer = reviewer.typeAnsAnswerFilter(col.media.escape_media_filenames(prefix + pipeline["answer"]))
    return {"question": question, "answer": answer}


@observation("review_html")
def review_html(session, args):
    cases = check_review_request(args)
    card_ids = [session.resolve(ref, "c") for ref, _ in cases]
    return [_review(session.col, cid, typed) for cid, (_, typed) in zip(card_ids, cases)]
