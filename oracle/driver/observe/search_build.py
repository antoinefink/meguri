# SPDX-License-Identifier: AGPL-3.0-or-later
"""search_build, search_join and search_replace (specs/conformance/search-ops.md
S5-S6; specs/search/join-and-replace.md J11).

Each item of the neutral form (S5) becomes Anki's own structured search item,
a SearchNode, and the backend writes the text:

search_build [<item>, ...] -> [{text} | parse error, ...]: the backend's
    search-string builder given the item's node (pylib build_search_string).
search_join [{existing, item, joiner, negate}, ...] -> [{text} | parse error]:
    the backend's join of the existing text, as one parsable node, with the
    item's node, wrapped in a negated node when negate is set (what the
    desktop browser's sidebar does for a modified click).
search_replace [{existing, item}, ...] -> [{text} | parse error]: the
    backend's replacement of the existing text's nodes of the item's kind.

`existing` and parsed-text items with field_text set are search-field text:
U+001C-U+001F become U+0020 first (join-and-replace.md J2), which Anki's search
field does and its backend does not. Parse errors are read as in search.py.
None of the three reads or changes the collection; the backend only parses
and writes.

Inputs join-and-replace.md J10 lists (Meguri writes another text, or refuses
them) are invalid_scenario here, as are J3's refusals: they stay out of the
oracle scenarios (J11). So are the J3 items without a structured counterpart
in Anki (note type by id, card and deck id lists, the two buried states),
which the neutral form does not have.
"""

from __future__ import annotations

import unicodedata
from typing import Any

import anki.errors
from anki import search_pb2

from ..errors import invalid
from ..registry import observation
from .search import Reasons, _search_error

Node = search_pb2.SearchNode

WORDS = {
    "whole_collection": lambda: Node(),
    "current_deck": lambda: Node(deck="current"),
    "any_deck": lambda: Node(deck="_*"),
    "any_tag": lambda: Node(tag="_*"),
    "untagged": lambda: Node(negated=Node(tag="_*")),
    "any_notetype": lambda: Node(note="_*"),
}
STATES = {
    "new": Node.CARD_STATE_NEW,
    "learn": Node.CARD_STATE_LEARN,
    "review": Node.CARD_STATE_REVIEW,
    "due": Node.CARD_STATE_DUE,
    "suspended": Node.CARD_STATE_SUSPENDED,
    "buried": Node.CARD_STATE_BURIED,
}
FLAGS = {"none": Node.FLAG_NONE, "any": Node.FLAG_ANY}
FLAG_COLORS = [Node.FLAG_RED, Node.FLAG_ORANGE, Node.FLAG_GREEN, Node.FLAG_BLUE,
               Node.FLAG_PINK, Node.FLAG_TURQUOISE, Node.FLAG_PURPLE]
RATINGS = [Node.RATING_ANY, Node.RATING_AGAIN, Node.RATING_HARD, Node.RATING_GOOD, Node.RATING_EASY]
JOINERS = {"and": Node.Group.Joiner.AND, "or": Node.Group.Joiner.OR}
# grammar.md G6: reserved keys (a field so named cannot be searched, J3.3).
RESERVED = {"deck", "note", "tag", "card", "flag", "resched", "prop", "added", "edited", "introduced", "rated",
            "is", "did", "mid", "nid", "cid", "re", "nc", "sc", "w", "dupe", "has-cd", "preset"}
U32 = 4294967295
I32 = 2147483648
SAFE = 9007199254740991
FIELD_SEPARATORS = {c: " " for c in range(0x1C, 0x20)}


def _field_text(text: str) -> str:
    """join-and-replace.md J2: the search field's rule."""
    return text.translate(FIELD_SEPARATORS)


def _other_space(s: str) -> bool:
    """White_Space other than U+0020, or U+001C-U+001F (Python's isspace is
    exactly White_Space plus those four)."""
    return any(c.isspace() and c != " " for c in s)


def _int(value: Any, lo: int, hi: int, what: str) -> int:
    if type(value) is not int or not lo <= value <= hi:
        raise invalid(f"{what} is an integer from {lo} to {hi}")
    return value


def _name(value: Any, what: str) -> str:
    """A non-empty name that J3's escape writes exactly and that is not the
    special value `_*` (J3.1, J10)."""
    if not isinstance(value, str) or not value:
        raise invalid(f"{what} is a non-empty string")
    if value == "_*":
        raise invalid(f"{what} `_*` is written differently by Meguri (join-and-replace.md J10)")
    return value


def _exact(value: str, what: str) -> None:
    """J3.5's note type rule: no other White_Space, no U+001C-U+001F, NFC."""
    if _other_space(value) or unicodedata.normalize("NFC", value) != value:
        raise invalid(f"{what} cannot be written exactly (join-and-replace.md J3.5, J10)")


def _deck_name(value: Any) -> str:
    name = _name(value, "a deck name")
    if name in ("current", "filtered"):
        raise invalid("a deck named current or filtered is written differently by Meguri (J3.1, J10)")
    _exact(name, "the deck name")
    for level in name.split("::"):
        if (
            not level
            or any(ord(c) < 0x20 or ord(c) == 0x7F for c in level)
            or level[0].isspace() or level[-1].isspace()
            or level[0] == ":" or level[-1] == ":"
        ):
            raise invalid("the deck name cannot be written exactly (join-and-replace.md J3.5, J10)")
    return name


def _text(value: Any, what: str) -> str:
    if not isinstance(value, str) or not value:
        raise invalid(f"{what} is a non-empty string")
    if _other_space(value):
        raise invalid(f"{what} holds white space Meguri writes as _ (join-and-replace.md J3, J10)")
    return value


def _field(value: Any) -> str:
    name = _text(_name(value, "a field name"), "a field name")
    if "".join(c.lower() if c.isascii() else c for c in name) in RESERVED:
        raise invalid("a field named like a reserved key is refused by Meguri (join-and-replace.md J3.3)")
    return name


def _one(item: Any, what: str) -> tuple[str, Any]:
    if not isinstance(item, dict) or len(item) != 1:
        raise invalid(f"{what} is one of the item words or a mapping with one key (search-ops.md S5)")
    return next(iter(item.items()))


def _node(item: Any) -> Any:
    """The SearchNode for a neutral item (search-ops.md S5)."""
    if isinstance(item, str):
        if item not in WORDS:
            raise invalid(f"unknown search item word {item!r}")
        return WORDS[item]()
    if isinstance(item, dict) and "text" in item:
        if set(item) - {"text", "field_text"} or not isinstance(item["text"], str) \
                or not isinstance(item.get("field_text", False), bool):
            raise invalid("a parsed-text item is {text: <search>, field_text?: <boolean>}")
        text = item["text"]
        return Node(parsable_text=_field_text(text) if item.get("field_text") else text)
    key, value = _one(item, "a search item")
    if key == "deck":
        return Node(deck=_deck_name(value))
    if key == "tag":
        name = _text(_name(value, "a tag name"), "a tag name")
        if name == "none":
            raise invalid("a tag named none is written differently by Meguri (J3.1, J10)")
        return Node(tag=name)
    if key == "notetype":
        name = _name(value, "a note type name")
        _exact(name, "the note type name")
        return Node(note=name)
    if key == "template":
        return Node(template=_int(value, 0, 65534, "a template ordinal"))
    if key == "field_present":
        return Node(field_name=_field(value))
    if key == "field":
        if not isinstance(value, dict) or set(value) != {"name", "text"}:
            raise invalid("a field item is {field: {name, text}}")
        text = value["text"]
        if not isinstance(text, str) or _other_space(text):
            raise invalid("a field text is a string without white space Meguri writes as _ (J3, J10)")
        return Node(field=Node.Field(field_name=_field(value["name"]), text=text, mode=Node.FIELD_SEARCH_MODE_NORMAL))
    if key == "literal":
        return Node(literal_text=_text(value, "a literal text"))
    if key == "flag":
        if isinstance(value, str) and value in FLAGS:
            return Node(flag=FLAGS[value])
        return Node(flag=FLAG_COLORS[_int(value, 1, 7, "a flag") - 1])
    if key == "state":
        if not isinstance(value, str) or value not in STATES:
            raise invalid(f"a state item is one of {sorted(STATES)}")
        return Node(card_state=STATES[value])
    if key in ("added", "edited", "introduced"):
        days = _int(value, 1, U32, f"{key} days")  # 0 is J10's
        return Node(**{f"{key}_in_days": days})
    if key == "answered":
        if not isinstance(value, dict) or "days" not in value or set(value) - {"days", "button"}:
            raise invalid("an answered item is {answered: {days, button?}}")
        button = _int(value["button"], 1, 4, "an answer button") if "button" in value else 0
        return Node(rated=Node.Rated(days=_int(value["days"], 1, U32, "answered days"), rating=RATINGS[button]))
    if key == "rescheduled":
        return Node(rated=Node.Rated(days=_int(value, 0, U32, "rescheduled days"), rating=Node.RATING_BY_RESCHEDULE))
    if key == "due_on":
        return Node(due_on_day=_int(value, -I32, I32 - 1, "a due day"))
    if key == "due_within":
        return Node(due_in_days=_int(value, -I32, I32 - 1, "a due day"))
    if key == "note_ids":
        if not isinstance(value, list) or not value:
            raise invalid("a note ids item is a non-empty list")
        return Node(nids=Node.IdList(ids=[_int(i, 0, SAFE, "a note id") for i in value]))
    if key == "dupe":
        if not isinstance(value, dict) or set(value) != {"notetype_id", "text"} or not isinstance(value["text"], str):
            raise invalid("a dupe item is {dupe: {notetype_id, text}}")
        if _other_space(value["text"]):
            raise invalid("dupe text with other white space is refused by Meguri (join-and-replace.md J3.4)")
        return Node(dupe=Node.Dupe(notetype_id=_int(value["notetype_id"], 0, SAFE, "a note type id"),
                                   first_field=value["text"]))
    if key == "group":
        if not isinstance(value, dict) or set(value) != {"joiner", "items"} or not isinstance(value["joiner"], str) \
                or value["joiner"] not in JOINERS \
                or not isinstance(value["items"], list) or not value["items"]:
            raise invalid("a group item is {group: {joiner: and | or, items: [<item>, ...]}} with one or more items")
        return Node(group=Node.Group(nodes=[_node(i) for i in value["items"]], joiner=JOINERS[value["joiner"]]))
    if key == "negated":
        return Node(negated=_node(value))
    raise invalid(f"unknown search item {key!r}")


def _cases(args: Any, kind: str, keys: set[str]) -> list[dict]:
    if not isinstance(args, list) or not args:
        raise invalid(f"{kind} takes a non-empty list")
    for case in args:
        if not isinstance(case, dict) or set(case) != keys or not isinstance(case["existing"], str):
            raise invalid(f"a {kind} case is {{{', '.join(sorted(keys))}}} with existing a search text")
    return args


def _run(session, nodes: list, write) -> list:
    col = session.col
    reasons = Reasons(col.tr)
    out = []
    for node in nodes:
        try:
            out.append({"text": write(col, node)})
        except anki.errors.SearchError as exc:
            out.append(_search_error(reasons, exc))
    return out


@observation("search_build")
def search_build(session, args):
    if not isinstance(args, list) or not args:
        raise invalid("search_build takes a non-empty list of search items")
    nodes = [_node(item) for item in args]
    return _run(session, nodes, lambda col, node: col._backend.build_search_string(node))


@observation("search_join")
def search_join(session, args):
    cases = _cases(args, "search_join", {"existing", "item", "joiner", "negate"})
    nodes = []
    for case in cases:
        if not isinstance(case["joiner"], str) or case["joiner"] not in JOINERS \
                or not isinstance(case["negate"], bool):
            raise invalid("a search_join case's joiner is and or or, and negate a boolean")
        item = _node(case["item"])
        nodes.append((Node(parsable_text=_field_text(case["existing"])),
                      Node(negated=item) if case["negate"] else item, JOINERS[case["joiner"]]))
    return _run(session, nodes, lambda col, n: col._backend.join_search_nodes(
        joiner=n[2], existing_node=n[0], additional_node=n[1]))


@observation("search_replace")
def search_replace(session, args):
    cases = _cases(args, "search_replace", {"existing", "item"})
    nodes = [(Node(parsable_text=_field_text(c["existing"])), _node(c["item"])) for c in cases]
    return _run(session, nodes, lambda col, n: col._backend.replace_search_node(
        existing_node=n[0], replacement_node=n[1]))
