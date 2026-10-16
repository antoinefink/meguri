# SPDX-License-Identifier: AGPL-3.0-or-later
"""Answer button labels (specs/conformance/observations.md O10).

answer_labels [(#cN, ...)]: per card {again, hard, good, easy}, each label as
  {amount, less_than, unit}, from the same states as next_states at the same
  instant. Anki's English text is split here: the isolation marks U+2068 and
  U+2069 dropped, `(end)` the end label, otherwise an optional `<`, the amount
  and the suffix s|m|h|d|mo|y. Text that does not split is a run error
  (anki_error), never a value; no label text reaches the error (R1887).

The labels are English with a full stop as decimal separator because the
collection's backend is English (`anki.lang.current_lang`, which the driver
never changes); a backend in another language is refused as a driver bug.
"""

from __future__ import annotations

import json
import re

from ..errors import DriverError
from ..registry import observation

BUTTONS = ("again", "hard", "good", "easy")
UNITS = {"s": "seconds", "m": "minutes", "h": "hours", "d": "days", "mo": "months", "y": "years"}
_LABEL = re.compile(r"(<?)([0-9]+(?:\.[0-9]+)?)(s|m|h|d|mo|y)")
_MARKS = str.maketrans("", "", "⁨⁩")
_END = "(end)"


def split_label(text: str) -> dict | None:
    """Anki's label text -> {amount, less_than, unit}, or None when it does not split."""
    text = text.translate(_MARKS)
    if text == _END:
        return {"amount": "", "less_than": False, "unit": "end"}
    m = _LABEL.fullmatch(text)
    if m is None:
        return None
    return {"amount": m.group(2), "less_than": m.group(1) == "<", "unit": UNITS[m.group(3)]}


def _check_english(backend) -> None:
    langs = json.loads(backend.i18n_resources(modules=["scheduling"]))["langs"]
    if not langs or any(lang.split("-")[0] != "en" for lang in langs):
        raise DriverError("internal_error", op="observe", observation="answer_labels",
                          detail="the collection's backend is not English")


@observation("answer_labels")
def answer_labels(session, args):
    backend = session.col._backend
    _check_english(backend)
    out = {}
    for ref, cid in session.card_targets(args):
        texts = backend.describe_next_states(backend.get_scheduling_states(cid))
        labels = {}
        for button, text in zip(BUTTONS, texts, strict=True):
            parts = split_label(text)
            if parts is None:
                raise DriverError("anki_error", op="observe", observation="answer_labels",
                                  detail=f"the {button} label of {ref} does not split into amount and unit")
            labels[button] = parts
        out[ref] = labels
    return out
