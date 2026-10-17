# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tag management and card flags (specs/conformance/tag-ops.md G1-G8, G10).

add_note_tags:     {notes: [#nN, ...], tags: <text>} -> {count}   bulk tag add
remove_note_tags:  {notes: [#nN, ...], tags: <text>} -> {count}   bulk tag removal
delete_tags:       {tags: <text>} -> {count}   tag removal from every note and the registry
rename_tag:        {from: <name>, to: <text>} -> {count}; rejection invalid_name
reparent_tags:     {tags: [<name>, ...], parent: <name or "">} -> {count}
clear_unused_tags: {} -> {count}
set_tag_collapsed: {tags: [<name>, ...], collapsed: <bool>} -> none
set_flag:          {cards: [#cN, ...], flag: <int>} -> {count}; rejection invalid_flag

Each op is one call of the collection's own operation; counts are the
operation's count as Anki returns it. The bulk ops pass the listed notes' ids
as given, deleted notes included. set_tag_collapsed runs Anki's set-collapsed
operation once per name, in order. rename_tag and set_flag reject on the
backend's invalid-input kind, read from the backend error (never the
exception class); Anki refuses both before it writes anything.

reparent_tags trap (tag-ops.md G5): when a source the reparent skips is
listed with a source it moves and a note uses the skipped one, Anki panics.
Scenarios avoid that case; the op does not detect it (oracle/AGENTS.md).
"""

from __future__ import annotations

from typing import Any

from anki import backend_pb2

from ..errors import Rejected, invalid
from ..registry import op
from .model import KindProbe

INVALID_INPUT = backend_pb2.BackendError.INVALID_INPUT
# The flag travels in a uint32 protobuf field.
FLAG_LIMIT = 2**32


def _args(name: str, args: Any, keys: set[str], shape: str) -> dict:
    if not isinstance(args, dict) or set(args) != keys:
        raise invalid(f"{name} takes {shape}")
    return args


def _text(name: str, key: str, value: Any) -> str:
    if not isinstance(value, str):
        raise invalid(f"{name} {key} is a string; quote numbers, booleans and null")
    return value


def _names(name: str, key: str, value: Any) -> list[str]:
    if not isinstance(value, list) or not value or not all(isinstance(v, str) and v for v in value):
        raise invalid(f"{name} {key} is a list of at least one non-empty string")
    return value


def _refs(session, name: str, key: str, value: Any, kind: str) -> list[int]:
    if not isinstance(value, list) or not value:
        raise invalid(f"{name} {key} is a list of at least one reference")
    return [session.resolve(ref, kind) for ref in value]


def _count(changes) -> dict:
    return {"count": changes.count}


def _rejecting(session, method: str, rejection: str, **kwargs):
    """One generated backend call; the backend's invalid-input kind raises
    Rejected(rejection), any other error what pylib raises."""
    backend = session.col._backend
    probe = KindProbe(backend, INVALID_INPUT, lambda: Rejected(rejection))
    return getattr(type(backend), method)(probe, **kwargs)


def _note_tags(session, name: str, args: Any) -> tuple[list[int], str]:
    args = _args(name, args, {"notes", "tags"}, "{notes: [#nN, ...], tags: <text>}")
    tags = _text(name, "tags", args["tags"])
    return _refs(session, name, "notes", args["notes"], "n"), tags


@op("add_note_tags")
def add_note_tags(session, args):
    note_ids, tags = _note_tags(session, "add_note_tags", args)
    return _count(session.col.tags.bulk_add(note_ids, tags))


@op("remove_note_tags")
def remove_note_tags(session, args):
    note_ids, tags = _note_tags(session, "remove_note_tags", args)
    return _count(session.col.tags.bulk_remove(note_ids, tags))


@op("delete_tags")
def delete_tags(session, args):
    args = _args("delete_tags", args, {"tags"}, "{tags: <text>}")
    return _count(session.col.tags.remove(_text("delete_tags", "tags", args["tags"])))


@op("rename_tag", rejects=("invalid_name",))
def rename_tag(session, args):
    args = _args("rename_tag", args, {"from", "to"}, "{from: <name>, to: <text>}")
    old = _text("rename_tag", "from", args["from"])
    new = _text("rename_tag", "to", args["to"])
    if not old or " " in old or "　" in old:
        raise invalid("rename_tag from is one non-empty name without U+0020 or U+3000")
    return _count(_rejecting(session, "rename_tags", "invalid_name", current_prefix=old, new_prefix=new))


@op("reparent_tags")
def reparent_tags(session, args):
    args = _args("reparent_tags", args, {"tags", "parent"}, "{tags: [<name>, ...], parent: <name or empty>}")
    tags = _names("reparent_tags", "tags", args["tags"])
    parent = _text("reparent_tags", "parent", args["parent"])
    return _count(session.col.tags.reparent(tags, parent))


@op("clear_unused_tags")
def clear_unused_tags(session, args):
    if args != {}:
        raise invalid("clear_unused_tags takes {}")
    return _count(session.col.tags.clear_unused_tags())


@op("set_tag_collapsed")
def set_tag_collapsed(session, args):
    args = _args("set_tag_collapsed", args, {"tags", "collapsed"}, "{tags: [<name>, ...], collapsed: <boolean>}")
    names = _names("set_tag_collapsed", "tags", args["tags"])
    collapsed = args["collapsed"]
    if not isinstance(collapsed, bool):
        raise invalid("set_tag_collapsed collapsed is a boolean")
    for name in names:
        session.col.tags.set_collapsed(tag=name, collapsed=collapsed)


@op("set_flag", rejects=("invalid_flag",))
def set_flag(session, args):
    args = _args("set_flag", args, {"cards", "flag"}, "{cards: [#cN, ...], flag: <integer>}")
    flag = args["flag"]
    if isinstance(flag, bool) or not isinstance(flag, int) or flag < 0:
        raise invalid("set_flag flag is a non-negative integer")
    if flag >= FLAG_LIMIT:
        raise invalid(f"set_flag flag is below {FLAG_LIMIT}: Anki's call carries no larger value")
    card_ids = _refs(session, "set_flag", "cards", args["cards"], "c")
    return _count(_rejecting(session, "set_flag", "invalid_flag", card_ids=card_ids, flag=flag))
