# SPDX-License-Identifier: AGPL-3.0-or-later
"""cloze {fns: [name, ...], inputs: [input, ...]}: pure cloze functions (render area).

Value: {name: [output for each input, in input order]} for every requested
function. The meaning of each function is specs/render/cloze.md; the request
and value shapes are specs/conformance/cloze-ops.md (Z1, Z2).

The scenario's collection is never read or written. Everything runs on one
scratch collection per worker process, created on first use in the worker's
scratch directory; nothing is ever added to it after its note type, so no
output depends on an earlier one.

  numbers                         ClozeNumbersInNote on a note whose only field
                                  is the text, sorted ascending
  typing                          ExtractClozeForTyping(text, card)
  front, back                     RenderUncommittedCardLegacy of a new note of a
                                  standard note type with fields Front and Back,
                                  card_ord = card - 1, qfmt = afmt =
                                  {{cloze:Front}}{{Back}}: question nodes for
                                  front, answer nodes for back
  only_front, only_back           the same with {{cloze-only:Front}}{{Back}}

Back holds a sentinel the driver removes from the end of each side. The
template therefore always references a non-empty field, which keeps the
renderer's blank-front message (added when a standard template's fields all
count as empty) away from texts that count as empty: "", white space, <br>.
"""

from __future__ import annotations

import json
import os
import tempfile

from anki import notes_pb2
from anki.collection import Collection

from ..errors import DriverError, invalid
from ..registry import observation

PAIR_FNS = ("front", "back", "only_front", "only_back", "typing")
FNS = ("numbers",) + PAIR_FNS
MAX_CARD = 65535

# Plain ASCII: the renderer copies a field without filters verbatim.
_SENTINEL = "|meguri-cloze-sentinel|"
_FIELDS = ("Front", "Back")


class _Scratch:
    """The worker's scratch collection with one standard note type (fields Front
    and Back), defined here so no stock note type is involved (R1959)."""

    def __init__(self, root: str) -> None:
        self.dir = tempfile.mkdtemp(prefix="cloze-", dir=root)
        self.col = Collection(os.path.join(self.dir, "collection.anki2"))
        models = self.col.models
        nt = models.new("Cloze probe")
        for name in _FIELDS:
            models.add_field(nt, models.new_field(name))
        t = models.new_template("Card 1")
        t["qfmt"] = "{{Front}}"
        t["afmt"] = "{{Back}}"
        models.add_template(nt, t)
        self.notetype = models.get(models.add_dict(nt).id)
        self.backend = self.col._backend

    def render(self, text: str, card: int, filt: str) -> tuple[str, str]:
        """(question, answer) output of {{<filt>:Front}} on card `card`."""
        fmt = "{{%s:Front}}{{Back}}" % filt
        # ord null: the renderer synthesizes a card with template_idx card_ord
        # instead of looking for a saved one.
        template = {"name": "Card 1", "ord": None, "qfmt": fmt, "afmt": fmt}
        note = self.col.new_note(self.notetype)
        note.fields[0] = text
        note.fields[1] = _SENTINEL
        out = self.backend.render_uncommitted_card_legacy(
            note=note._to_backend_note(),
            card_ord=card - 1,
            template=json.dumps(template).encode("utf-8"),
            fill_empty=False,
            partial_render=False,
        )
        if out.is_empty:
            raise DriverError("internal_error", detail="the renderer reported an empty front")
        return _side(out.question_nodes), _side(out.answer_nodes)

    def numbers(self, text: str) -> list[int]:
        return sorted(self.backend.cloze_numbers_in_note(notes_pb2.Note(fields=[text])))

    def typing(self, text: str, card: int) -> str:
        return self.backend.extract_cloze_for_typing(text=text, ordinal=card)


def _side(nodes) -> str:
    parts = []
    for node in nodes:
        if node.WhichOneof("value") != "text":
            raise DriverError("internal_error", detail="the renderer left a replacement node")
        parts.append(node.text)
    joined = "".join(parts)
    if not joined.endswith(_SENTINEL):
        raise DriverError("internal_error", detail="the rendered side does not end with the sentinel")
    return joined[: -len(_SENTINEL)]


_scratch: _Scratch | None = None


def _get_scratch(session) -> _Scratch:
    global _scratch
    if _scratch is None:
        _scratch = _Scratch(os.path.dirname(session.dir))
    return _scratch


def _is_card(value) -> bool:
    # bool is an int subclass in Python; a scenario's true is not a card number.
    return type(value) is int and 1 <= value <= MAX_CARD


def _check(args) -> tuple[list[str], list]:
    if not isinstance(args, dict) or set(args) != {"fns", "inputs"}:
        raise invalid("cloze takes {fns, inputs}")
    fns, inputs = args["fns"], args["inputs"]
    if not isinstance(fns, list) or not fns or not all(isinstance(f, str) for f in fns):
        raise invalid("cloze fns is a non-empty list of function names")
    unknown = sorted(set(fns) - set(FNS))
    if unknown:
        raise invalid(f"unknown cloze functions: {', '.join(unknown)}")
    if len(set(fns)) != len(fns):
        raise invalid("a cloze function is listed twice")
    if not isinstance(inputs, list):
        raise invalid("cloze inputs is a list")
    if "numbers" in fns:
        if len(fns) != 1:
            raise invalid("numbers takes strings and cannot share a request with other cloze functions")
        if not all(isinstance(s, str) for s in inputs):
            raise invalid("numbers inputs are strings")
    else:
        for item in inputs:
            if not (isinstance(item, list) and len(item) == 2 and isinstance(item[0], str)):
                raise invalid("cloze inputs are [text, card] pairs")
            if not _is_card(item[1]):
                raise invalid(f"a card number is an integer from 1 to {MAX_CARD}")
    return fns, inputs


@observation("cloze")
def cloze(session, args):
    fns, inputs = _check(args)
    scratch = _get_scratch(session)
    if fns == ["numbers"]:
        return {"numbers": [scratch.numbers(s) for s in inputs]}

    rendered: dict[tuple[str, int, str], tuple[str, str]] = {}

    def side(text: str, card: int, filt: str, question: bool) -> str:
        key = (text, card, filt)
        if key not in rendered:
            rendered[key] = scratch.render(text, card, filt)
        return rendered[key][0 if question else 1]

    impl = {
        "front": lambda t, c: side(t, c, "cloze", True),
        "back": lambda t, c: side(t, c, "cloze", False),
        "only_front": lambda t, c: side(t, c, "cloze-only", True),
        "only_back": lambda t, c: side(t, c, "cloze-only", False),
        "typing": lambda t, c: scratch.typing(t, c),
    }
    return {fn: [impl[fn](t, c) for t, c in inputs] for fn in fns}
