# SPDX-License-Identifier: AGPL-3.0-or-later
"""template {notetype?, cases}: the template engine on given template texts
(render area). Request, value and error kinds: specs/conformance/render-ops.md
Q1-Q2; the behavior is specs/render/template-syntax.md, filters.md and
special-fields.md.

Value: [result, ...], one per case in case order:
  {question, answer, empty: false}   both sides rendered
  {question, answer: null, empty: true}   a blank question (T10)
  {error: {kind, side}}               a template error (T9)

Every case is the backend's legacy uncommitted render (the card layout
screen's), partial for Python, fill_empty off, with a schema-11 template object
built from the case. The case's templates are never saved (a save validates
them).

  new-note case  on one scratch collection per worker process (as text.py),
                 never the scenario's: the request's note type is saved there
                 with exactly the given name, kind and fields and one valid
                 dummy template, any note type of that name (stock ones too)
                 removed first, and removed again after the request. The note
                 is new and unsaved; template {name, ord: null, qfmt, afmt}
                 and the case's ord, so the card is a new one (no deck, flag 0,
                 id 0).
  card case      on the scenario's collection: the stored note and the card's
                 ord; template {name: the stored template's (the first for
                 cloze), ord: the card's ord, ...}, so the stored card is
                 found by ordinal.

Nodes are composed as pylib composes them with no add-on hooks: text as it is,
a replacement's current text (filters left over after an unknown one never
run), FrontSide nothing on the question side and the composed question on the
answer side. No AV tag extraction. When the response is flagged empty, the last
question node is Anki's blank notice: it is dropped (after checking it is that
notice) and the answer is not reported.

Template errors arrive as one localized HTML message: the side's problem
header, the escaped details and a help link. Both are read through the
backend's own translations rendered with sentinel arguments, never against
literal text (R1887); the kind never carries message text. Anki's unknown-field
message serves both unknown_field and unknown_conditional: the latter exactly
when the quoted tag is {{# or {{^, at least one more character, then }}. A
message that matches no kind of Q2 is a run error (the TemplateError reaches
the session's guard as anki_error).
"""

from __future__ import annotations

import html
import os
import re
import tempfile
import unicodedata
from dataclasses import dataclass, field
from typing import Any

import anki.errors
from anki.collection import Collection
from anki.consts import MODEL_CLOZE, MODEL_STD
from anki.utils import from_json_bytes, to_json_bytes

from ..errors import DriverError, invalid
from ..registry import observation, scenario_check
from ..session import parse_observation

NOTETYPE_KEYS = {"name", "kind", "fields"}
NEW_NOTE_KEYS = {"front", "back", "ord", "name", "fields", "tags"}
CARD_KEYS = {"card", "front", "back"}
KINDS = ("standard", "cloze")
MAX_ORD = 499
CARD_REF = re.compile(r"#c[1-9][0-9]*")
# Unicode White_Space (PropList.txt).
WHITE_SPACE = frozenset(
    "\t\n\x0b\x0c\r \x85\xa0\u1680" + "".join(map(chr, range(0x2000, 0x200B))) + "\u2028\u2029\u202f\u205f\u3000"
)
FIELD_NAME_BAD_CHARS = frozenset(':{}"')
FIELD_NAME_BAD_STARTS = frozenset("#/^")

# Q2 kind -> the translation method of Anki's message and its arguments.
# no_closing_brackets is Anki's too but no kind of Q2: matched so that it is
# told apart from the others, then reported as a run error.
MESSAGES = (
    ("unclosed_conditional", "card_template_rendering_conditional_not_closed", ("missing",)),
    ("mismatched_conditional", "card_template_rendering_wrong_conditional_closed", ("found", "expected")),
    ("unopened_conditional", "card_template_rendering_conditional_not_open", ("found", "missing1", "missing2")),
    ("unknown_field", "card_template_rendering_no_such_field", ("found", "field")),
    (None, "card_template_rendering_no_closing_brackets", ("missing", "tag")),
)
SIDES = (
    ("question", "card_template_rendering_front_side_problem"),
    ("answer", "card_template_rendering_back_side_problem"),
)
# Browser mode (the render kind, render-ops.md Q5) has its own side headers.
BROWSER_SIDES = (
    ("question", "card_template_rendering_browser_front_side_problem"),
    ("answer", "card_template_rendering_browser_back_side_problem"),
)
SENTINEL = "MEGURI-ARGUMENT-{}-MEGURI"
_SENTINEL = re.compile(r"MEGURI-ARGUMENT-([a-z0-9]+)-MEGURI")
# A tag Anki quotes in the unknown-field message names a section exactly when
# it is {{#...}} or {{^...}} with something after the sign (render-ops.md Q2).
CONDITIONAL_TAG = re.compile(r"\{\{[#^].+\}\}", re.DOTALL)


# --- the request -------------------------------------------------------------


@dataclass
class Notetype:
    name: str
    cloze: bool
    fields: list[str]


@dataclass
class Case:
    front: str
    back: str
    card: str | None = None
    ord: int = 0
    name: str = ""
    values: list[str] = field(default_factory=list)
    tags: list[str] = field(default_factory=list)


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def _is_nfc(s: str) -> bool:
    return unicodedata.is_normalized("NFC", s)


def _check_notetype(nt: Any) -> Notetype:
    if not isinstance(nt, dict) or set(nt) - NOTETYPE_KEYS or not {"name", "fields"} <= set(nt):
        raise invalid("template notetype is {name, kind?, fields}")
    name, kind, fields = nt["name"], nt.get("kind", "standard"), nt["fields"]
    if not isinstance(name, str) or not name or not _is_nfc(name) or '"' in name:
        raise invalid('the notetype name is a non-empty NFC string without "')
    if kind not in KINDS:
        raise invalid("the notetype kind is standard or cloze")
    if not isinstance(fields, list) or not fields:
        raise invalid("the notetype fields are a non-empty list of names")
    for f in fields:
        if (
            not isinstance(f, str)
            or not f
            or not _is_nfc(f)
            or FIELD_NAME_BAD_CHARS & set(f)
            or f[0] in FIELD_NAME_BAD_STARTS
            or f[0] in WHITE_SPACE
            or f[-1] in WHITE_SPACE
        ):
            raise invalid(f"{f!r} is not a field name a note type save keeps unchanged (render-ops.md Q1.1)")
    if len(set(fields)) != len(fields):
        # Names equal only under unicase are refused when the note type is
        # saved (Anki renames one of them).
        raise invalid("a field name is listed twice")
    return Notetype(name, kind == "cloze", list(fields))


def _check_strings(value: Any, what: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(s, str) for s in value):
        raise invalid(f"{what} is a list of strings")
    return list(value)


def _texts(case: dict) -> tuple[str, str]:
    front, back = case.get("front"), case.get("back", "")
    if not isinstance(front, str) or not isinstance(back, str):
        raise invalid("a case's front is a string, and so is back when given")
    return front, back


def _check_case(case: Any, nt: Notetype | None) -> Case:
    if not isinstance(case, dict):
        raise invalid("a template case is a mapping")
    if "card" in case:
        if set(case) - CARD_KEYS:
            raise invalid("a card case is {card, front, back?}")
        ref = case["card"]
        if not isinstance(ref, str) or not CARD_REF.fullmatch(ref):
            raise invalid(f"{ref!r} is not a #c<N> reference")
        return Case(*_texts(case), card=ref)
    if set(case) - NEW_NOTE_KEYS:
        raise invalid("a new-note case is {front, back?, ord?, name?, fields?, tags?}")
    front, back = _texts(case)
    ord_ = case.get("ord", 0)
    if not _is_int(ord_) or not 0 <= ord_ <= MAX_ORD:
        raise invalid(f"ord is an integer from 0 to {MAX_ORD}")
    name = case.get("name", f"Card {ord_ + 1}")
    if not isinstance(name, str) or not name:
        raise invalid("a case's template name is a non-empty string")
    given = case.get("fields", [])
    if isinstance(given, list):
        values = _check_strings(given, "a case's field values")
        if len(values) > len(nt.fields):
            raise invalid("a case has more field values than its note type has fields")
        values += [""] * (len(nt.fields) - len(values))
    elif isinstance(given, dict):
        unknown = [k for k in given if k not in nt.fields]
        if unknown or not all(isinstance(v, str) for v in given.values()):
            raise invalid("a case's field mapping names fields of its note type and gives strings")
        values = [given.get(f, "") for f in nt.fields]
    else:
        raise invalid("a case's fields are a list or a mapping")
    tags = _check_strings(case.get("tags", []), "a case's tags")
    return Case(front, back, ord=ord_, name=name, values=values, tags=tags)


def check_request(args: Any) -> tuple[Notetype | None, list[Case]]:
    """render-ops.md Q1.1-Q1.3 and Q1.6, without the collection."""
    if not isinstance(args, dict) or set(args) - {"notetype", "cases"} or "cases" not in args:
        raise invalid("template takes {notetype?, cases}")
    cases = args["cases"]
    if not isinstance(cases, list) or not cases:
        raise invalid("template cases are a non-empty list")
    new_note = any(not (isinstance(c, dict) and "card" in c) for c in cases)
    if new_note != ("notetype" in args):
        raise invalid("template needs a notetype exactly when a case is a new-note case")
    nt = _check_notetype(args["notetype"]) if new_note else None
    return nt, [_check_case(c, nt) for c in cases]


@scenario_check
def check_template_requests(scenario: dict) -> None:
    """At most one template request per observe step (O2), each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe":
            continue
        requests = [args for kind, args in map(parse_observation, specs) if kind == "template"]
        if len(requests) > 1:
            raise invalid("an observe step has at most one template request", step=index)
        for args in requests:
            try:
                check_request(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


# --- the scratch collection ----------------------------------------------------


class _Scratch:
    """The worker's collection for new-note cases. Between requests it holds
    only what a new collection holds, less the stock note types requests
    named."""

    def __init__(self, root: str) -> None:
        self.dir = tempfile.mkdtemp(prefix="template-", dir=root)
        self.col = Collection(os.path.join(self.dir, "collection.anki2"))

    def save(self, nt: Notetype) -> dict:
        models = self.col.models
        while (old := models.id_for_name(nt.name)) is not None:
            models.remove(old)
        d = models.new(nt.name)
        d["type"] = MODEL_CLOZE if nt.cloze else MODEL_STD
        for name in nt.fields:
            models.add_field(d, models.new_field(name))
        t = models.new_template("Card 1")
        t["qfmt"] = t["afmt"] = ("{{cloze:%s}}" if nt.cloze else "{{%s}}") % nt.fields[0]
        models.add_template(d, t)
        ntid = models.add_dict(d).id
        saved = from_json_bytes(self.col._backend.get_notetype_legacy(ntid))
        if saved["name"] != nt.name or [f["name"] for f in saved["flds"]] != nt.fields:
            models.remove(ntid)
            raise invalid("the note type's name or field names change when Anki saves it (render-ops.md Q1.1)")
        return saved


_scratch: _Scratch | None = None


def _get_scratch(session) -> _Scratch:
    global _scratch
    if _scratch is None or not os.path.isdir(_scratch.dir):
        _scratch = _Scratch(os.path.dirname(session.dir))
    return _scratch


# --- rendering ----------------------------------------------------------------


def without_blank_notice(tr, question_nodes, cloze: bool, card_ord: int) -> list:
    """The question nodes of a response flagged empty, less Anki's blank
    notice (the last node), after checking it is that notice."""
    nodes = list(question_nodes)
    notice = tr.card_template_rendering_missing_cloze(number=card_ord + 1) if cloze else (
        tr.card_template_rendering_empty_front()
    )
    if not nodes or nodes[-1].WhichOneof("value") != "text" or not nodes[-1].text.startswith(f"<div>{notice}<br>"):
        raise DriverError("internal_error", detail="a blank question does not end with Anki's blank notice")
    return nodes[:-1]


def compose(nodes, front_side: str | None) -> str:
    """Partially rendered nodes as pylib composes them with no add-on hooks:
    text as it is, a replacement's current text, and FrontSide's front_side
    when given (its current text, empty, otherwise)."""
    out = []
    for node in nodes:
        if node.WhichOneof("value") == "text":
            out.append(node.text)
        elif node.replacement.field_name == "FrontSide" and front_side is not None:
            out.append(front_side)
        else:
            out.append(node.replacement.current_text)
    return "".join(out)


def _render(col, note, card_ord: int, template: dict, cloze: bool) -> dict:
    try:
        out = col._backend.render_uncommitted_card_legacy(
            note=note._to_backend_note(),
            card_ord=card_ord,
            template=to_json_bytes(template),
            fill_empty=False,
            partial_render=True,
        )
    except anki.errors.TemplateError as exc:
        classified = classify(col.tr, str(exc))
        if classified is None:
            raise  # Q2: a run error (anki_error) until the list grows
        kind, side = classified
        return {"error": {"kind": kind, "side": side}}
    if out.is_empty:
        return {"question": compose(without_blank_notice(col.tr, out.question_nodes, cloze, card_ord), None),
                "answer": None, "empty": True}
    question = compose(out.question_nodes, None)
    return {"question": question, "answer": compose(out.answer_nodes, question), "empty": False}


def _new_note_case(scratch: _Scratch, saved: dict, nt: Notetype, case: Case) -> dict:
    note = scratch.col.new_note(saved)
    note.fields = list(case.values)
    note.tags = list(case.tags)
    template = {"name": case.name, "ord": None, "qfmt": case.front, "afmt": case.back}
    return _render(scratch.col, note, case.ord, template, nt.cloze)


def _card_case(col, card_id: int, case: Case) -> dict:
    card = col.get_card(card_id)
    note = card.note()
    nt = from_json_bytes(col._backend.get_notetype_legacy(note.mid))
    cloze = nt["type"] == MODEL_CLOZE
    stored = nt["tmpls"][:1] if cloze else [t for t in nt["tmpls"] if t["ord"] == card.ord]
    if not stored:
        raise DriverError("anki_error", op="observe", detail="the card's ordinal has no template in its note type")
    template = {"name": stored[0]["name"], "ord": card.ord, "qfmt": case.front, "afmt": case.back}
    return _render(col, note, card.ord, template, cloze)


# --- errors -------------------------------------------------------------------


def _pattern(render, args: tuple[str, ...]) -> tuple[re.Pattern, list[str]]:
    """The message rendered with sentinel arguments as a full-match pattern,
    and the argument behind each group, in the message's order."""
    text = render(**{a: SENTINEL.format(a) for a in args})
    regex, order, pos = "", [], 0
    for m in _SENTINEL.finditer(text):
        regex += re.escape(text[pos:m.start()]) + "(.*)"
        order.append(m.group(1))
        pos = m.end()
    if sorted(order) != sorted(args):
        raise DriverError("internal_error", detail=f"{render.__name__} does not show each argument once")
    return re.compile(regex + re.escape(text[pos:]), re.DOTALL), order


def classify(tr, message: str, browser: bool = False) -> tuple[str, str] | None:
    """(kind, side) of a template error message, or None when it is not a
    side header (the browser-mode ones when browser is set, the others
    otherwise), escaped details and a help link whose details are one of Q2's
    kinds (no_closing_brackets is not).

    Arguments are free text, so details can fit two messages ("Missing '}}'
    in '{{A'" also fits "Missing '<anything>'"): the message whose fixed text
    covers more of the details wins, and a tie is an internal_error."""
    more_info = tr.card_template_rendering_more_info()
    for side, method in BROWSER_SIDES if browser else SIDES:
        prefix = getattr(tr, method)() + "<br>"
        if message.startswith(prefix):
            break
    else:
        return None
    # The details are HTML-escaped, so the first <br> ends them.
    escaped, sep, link = message[len(prefix):].partition("<br>")
    if not sep or not link.startswith("<a ") or not link.endswith(f">{more_info}</a>"):
        return None
    details = html.unescape(escaped)
    found = []
    for kind, method, args in MESSAGES:
        pattern, order = _pattern(getattr(tr, method), args)
        m = pattern.fullmatch(details)
        if m:
            found.append((sum(map(len, m.groups())), kind, dict(zip(order, m.groups()))))
    if not found:
        return None
    found.sort(key=lambda f: f[0])
    if len(found) > 1 and found[0][0] == found[1][0]:
        raise DriverError("internal_error", detail="a template error fits two messages equally")
    _, kind, values = found[0]
    if kind is None:
        return None
    if kind == "unknown_field" and CONDITIONAL_TAG.fullmatch(values["found"]):
        kind = "unknown_conditional"
    return kind, side


# --- the observation --------------------------------------------------------


@observation("template")
def template(session, args):
    nt, cases = check_request(args)
    card_ids = {c.card: session.resolve(c.card, "c") for c in cases if c.card is not None}
    scratch = saved = None
    if nt is not None:
        scratch = _get_scratch(session)
        saved = scratch.save(nt)
    try:
        return [
            _card_case(session.col, card_ids[c.card], c) if c.card is not None else _new_note_case(scratch, saved, nt, c)
            for c in cases
        ]
    finally:
        if saved is not None:
            scratch.col.models.remove(saved["id"])
