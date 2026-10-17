# SPDX-License-Identifier: AGPL-3.0-or-later
"""Note types (specs/conformance/notetype-ops.md). Scenarios define their own
(R1959); Anki's stock note types appear only in scenarios about interop with
them, and their template text and CSS never reach an observation.

setup.notetypes: [{name, kind?: standard|cloze, fields: [names],
                   templates: [{name, front?, back?}], css?: text}]
    each added in list order as add_notetype adds one (dsl.md D6.4).
add_notetype {name, kind?, fields, templates, css?} -> {name}   (T2)
add_stock_notetype {kind, name?} -> {name}   (T3)
clone_notetype {notetype, name?} -> {name}   (T4)
update_notetype {notetype, edits, skip_checks?} -> {name}   (T5)
ensure_io_notetype {} -> {name}   (T6)
    The adds and the update record {error: <T7 kind>} when the save refuses.
notetype_structure (observation): every note type, or {notetypes: [name]}   (T8)

Names are the stored ones, compared exactly (T1). The adds go through the
backend's legacy add called directly: the Python model manager's add renames
a colliding name with a checksum before the backend sees it. The update saves
the edited legacy object with the backend's legacy update (the call Anki's
field and card layout editors make), skip_checks as given; kept fields and
templates keep their `ord`, which is how the save tells them apart. Each op
reopens the collection first, so note type rows a db_exec wrote are what it
reads (the backend caches note types). The observation reads through the
protobuf getter, which reads the file, and deck names with SQL.

Refusals: an invalid-input error of the save is invalid_notetype; a card type
error maps by its help page, and the template-error page, which parse errors
and unknown fields share, by the message's detail compared with the backend's
own translations (never literal text, which is localized, R1887).
"""

from __future__ import annotations

import json
from typing import Any

from anki import backend_pb2, links_pb2, notetypes_pb2
from anki._backend import backend_exception_to_pylib

from ..errors import DriverError, Rejected, invalid
from ..registry import observation, op, setup_section
from .model import NATIVE_SEPARATOR, _record_id

NOTETYPE_KEYS = {"name", "kind", "fields", "templates", "css"}
TEMPLATE_KEYS = {"name", "front", "back"}
KINDS = {"standard": 0, "cloze": 1}
KIND_NAMES = {0: "standard", 1: "cloze"}
STOCK = notetypes_pb2.StockNotetype.Kind
STOCK_KINDS = {
    "basic": STOCK.KIND_BASIC,
    "basic_and_reversed": STOCK.KIND_BASIC_AND_REVERSED,
    "basic_optional_reversed": STOCK.KIND_BASIC_OPTIONAL_REVERSED,
    "basic_typing": STOCK.KIND_BASIC_TYPING,
    "cloze": STOCK.KIND_CLOZE,
    "image_occlusion": STOCK.KIND_IMAGE_OCCLUSION,
}
IO_STOCK_KIND = notetypes_pb2.StockNotetype.OriginalStockKind.ORIGINAL_STOCK_KIND_IMAGE_OCCLUSION
IO_MIN_FIELDS = 4
REQUIREMENT_KINDS = {0: "none", 1: "any", 2: "all"}
REJECTS = ("invalid_notetype", "template_parse_error", "duplicate_front", "no_front_field", "unknown_field", "missing_cloze")
HELP = links_pb2.HelpPageLinkRequest.HelpPage
HELP_PAGE_KINDS = {
    HELP.CARD_TYPE_DUPLICATE: "duplicate_front",
    HELP.CARD_TYPE_NO_FRONT_FIELD: "no_front_field",
    HELP.CARD_TYPE_MISSING_CLOZE: "missing_cloze",
}
SENTINEL = "\x00"
DEFAULT_DECK = ("Default", 1)

# option -> (legacy key, value type) for the field_options and template edits.
FIELD_OPTIONS = {
    "sticky": ("sticky", "bool"),
    "rtl": ("rtl", "bool"),
    "font_name": ("font", "str"),
    "font_size": ("size", "uint"),
    "description": ("description", "str"),
    "plain_text": ("plainText", "bool"),
    "collapsed": ("collapsed", "bool"),
    "exclude_from_search": ("excludeFromSearch", "bool"),
}
TEMPLATE_VALUES = {
    "front": ("qfmt", "str"),
    "back": ("afmt", "str"),
    "browser_front": ("bqfmt", "str"),
    "browser_back": ("bafmt", "str"),
    "browser_font_name": ("bfont", "str"),
    "browser_font_size": ("bsize", "uint"),
}
TYPE_NAMES = {"bool": "a boolean", "str": "a string", "uint": "a non-negative integer"}


def _is_uint(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool) and v >= 0


def _checked(value: Any, kind: str, what: str) -> Any:
    ok = {"bool": isinstance(value, bool), "str": isinstance(value, str), "uint": _is_uint(value)}[kind]
    if not ok:
        raise invalid(f"{what} is {TYPE_NAMES[kind]}")
    return value


def _mapping(args: Any, keys: set[str], required: set[str], what: str) -> dict:
    if not isinstance(args, dict):
        raise invalid(f"{what} is a mapping")
    unknown = sorted(set(args) - keys)
    if unknown:
        raise invalid(f"unknown {what} keys: {', '.join(unknown)}")
    missing = sorted(required - set(args))
    if missing:
        raise invalid(f"{what} needs {', '.join(missing)}")
    return args


def _optional_name(args: dict, what: str) -> None:
    if "name" in args and (not isinstance(args["name"], str) or not args["name"]):
        raise invalid(f"{what} name is a non-empty string")


# --- reading -------------------------------------------------------------------


def notetype_id(session, name: Any) -> int:
    """The note type stored under exactly this name (T1), or unknown_ref."""
    if not isinstance(name, str) or not name:
        raise invalid("a note type is named by a non-empty string")
    return _record_id(session, "notetypes", name)


def _stored_name(session, ntid: int) -> str:
    return session.col.db.scalar("select name from notetypes where id = ?", ntid)


def _legacy(session, ntid: int) -> dict:
    return json.loads(session.col._backend.get_notetype_legacy(ntid))


# --- saving --------------------------------------------------------------------


class _SaveProbe:
    """Stands in for the RustBackend in one generated backend call: a refusal
    of the save (T7) raises Rejected(kind), any other error what pylib raises."""

    def __init__(self, col) -> None:
        self._rust = col._backend._backend
        self._tr = col.tr

    def _run_command(self, service: int, method: int, data: bytes) -> bytes:
        try:
            return self._rust.command(service, method, data)
        except Exception as error:  # noqa: BLE001 - pylib's own handling, below
            error_bytes = bytes(error.args[0])
        err = backend_pb2.BackendError()
        err.ParseFromString(error_bytes)
        kind = _refusal(err, self._tr)
        if kind is not None:
            raise Rejected(kind)
        raise backend_exception_to_pylib(err)


def _refusal(err, tr) -> str | None:
    """The T7 kind of a backend error of a note type save, or None."""
    if err.kind == backend_pb2.BackendError.INVALID_INPUT:
        return "invalid_notetype"
    if err.kind != backend_pb2.BackendError.CARD_TYPE_ERROR or not err.HasField("help_page"):
        return None
    if err.help_page in HELP_PAGE_KINDS:
        return HELP_PAGE_KINDS[err.help_page]
    if err.help_page != HELP.CARD_TYPE_TEMPLATE_ERROR:
        return None
    # "<header><br><detail>": the see-preview text for a parse error, the
    # field-not-found text for an unknown field.
    if err.message.endswith("<br>" + tr.card_templates_see_preview()):
        return "template_parse_error"
    before, sep, after = tr.card_templates_field_not_found(field=SENTINEL).partition(SENTINEL)
    if sep and "<br>" + before in err.message and err.message.endswith(after):
        return "unknown_field"
    return None


def _encode(obj: dict) -> bytes:
    return json.dumps(obj, ensure_ascii=False).encode("utf-8")


def _add(session, nt: dict, refusals: bool = True) -> int:
    """A legacy note type object added with the backend's legacy add (a
    checked save); its new id. With refusals, a T7 refusal raises Rejected;
    without, it stays the backend's error (a run error)."""
    backend = session.col._backend
    if not refusals:
        return backend.add_notetype_legacy(_encode(nt)).id
    return type(backend).add_notetype_legacy(_SaveProbe(session.col), _encode(nt)).id


def _added(session, nt: dict) -> dict:
    return {"name": _stored_name(session, _add(session, nt))}


# --- building ------------------------------------------------------------------


def _new_template(session, tmpl: dict) -> dict:
    t = session.col.models.new_template(tmpl["name"])
    t["qfmt"] = tmpl.get("front", "")
    t["afmt"] = tmpl.get("back", "")
    return t


def _from_spec(session, item: Any, what: str) -> dict:
    """A setup.notetypes entry or add_notetype argument as a new legacy note
    type: Anki's new note type with its new fields and templates (T2)."""
    _mapping(item, NOTETYPE_KEYS, {"name", "fields", "templates"}, what)
    _checked(item["name"], "str", f"{what} name")
    kind = item.get("kind", "standard")
    if kind not in KINDS:
        raise invalid("note type kind is standard or cloze")
    fields, templates = item["fields"], item["templates"]
    if not isinstance(fields, list) or not fields or not all(isinstance(f, str) for f in fields):
        raise invalid("fields is a non-empty list of names")
    if not isinstance(templates, list) or not templates:
        raise invalid("templates is a non-empty list")
    _checked(item.get("css", ""), "str", "css")
    for tmpl in templates:
        if not isinstance(tmpl, dict) or set(tmpl) - TEMPLATE_KEYS or "name" not in tmpl:
            raise invalid("a template is {name, front, back}")
        if not all(isinstance(tmpl.get(k, ""), str) for k in TEMPLATE_KEYS):
            raise invalid("a template's name, front and back are strings")
    models = session.col.models
    nt = models.new(item["name"])
    nt["type"] = KINDS[kind]
    nt["css"] = item.get("css", "")
    for name in fields:
        models.add_field(nt, models.new_field(name))
    for tmpl in templates:
        models.add_template(nt, _new_template(session, tmpl))
    return nt


@setup_section("notetypes", order=40)
def notetypes(session, items):
    if not isinstance(items, list):
        raise invalid("setup.notetypes is a list")
    for item in items:
        if not isinstance(item, dict) or not isinstance(item.get("name"), str):
            raise invalid("a note type is a mapping with a name")
        session.notetypes[item["name"]] = _add(session, _from_spec(session, item, "note type"), refusals=False)


# --- ops -----------------------------------------------------------------------


@op("add_notetype", rejects=REJECTS)
def add_notetype(session, args):
    session.reopen()
    return _added(session, _from_spec(session, args, "add_notetype"))


@op("add_stock_notetype", rejects=REJECTS)
def add_stock_notetype(session, args):
    _mapping(args, {"kind", "name"}, {"kind"}, "add_stock_notetype")
    if args["kind"] not in STOCK_KINDS:
        raise invalid(f"add_stock_notetype kind is one of {', '.join(STOCK_KINDS)}")
    _optional_name(args, "add_stock_notetype")
    session.reopen()
    nt = json.loads(session.col._backend.get_stock_notetype_legacy(STOCK_KINDS[args["kind"]]))
    if "name" in args:
        nt["name"] = args["name"]
    return _added(session, nt)


@op("clone_notetype", rejects=REJECTS)
def clone_notetype(session, args):
    _mapping(args, {"notetype", "name"}, {"notetype"}, "clone_notetype")
    _optional_name(args, "clone_notetype")
    session.reopen()
    source = args["notetype"]
    clone = session.col.models.copy(_legacy(session, notetype_id(session, source)), add=False)
    clone["name"] = args.get("name", f"{source} copy")
    return _added(session, clone)


@op("ensure_io_notetype")
def ensure_io_notetype(session, args):
    if args != {}:
        raise invalid("ensure_io_notetype takes {}")
    session.reopen()
    backend = session.col._backend
    backend.add_image_occlusion_notetype()
    for ntid in session.col.db.list("select id from notetypes order by id"):
        nt = backend.get_notetype(ntid)
        if nt.config.original_stock_kind == IO_STOCK_KIND and len(nt.fields) >= IO_MIN_FIELDS:
            return {"name": nt.name}
    raise DriverError("internal_error", detail="no Image Occlusion note type after Anki's on-demand add")


# --- update_notetype -----------------------------------------------------------


def _named(items: list[dict], name: Any, what: str) -> dict:
    """The draft's field or template with exactly this name, or unknown_ref."""
    _checked(name, "str", f"a {what} name")
    for item in items:
        if item["name"] == name:
            return item
    raise DriverError("unknown_ref", ref=name)


def _move(items: list[dict], item: dict, to: Any, what: str) -> None:
    if not _is_uint(to) or to >= len(items):
        raise invalid(f"{what} to is a position of the draft's list (0 to {len(items) - 1})")
    items.remove(item)
    items.insert(to, item)


def _rename_pair(value: Any, what: str) -> tuple[Any, str]:
    _mapping(value, {"from", "to"}, {"from", "to"}, what)
    return value["from"], _checked(value["to"], "str", f"{what} to")


def _set_values(target: dict, value: dict, table: dict, what: str) -> None:
    for key, (legacy_key, kind) in table.items():
        if key in value:
            target[legacy_key] = _checked(value[key], kind, f"{what} {key}")


def _deck(session, name: Any) -> int | None:
    """A template's deck override: the default deck, a setup.decks deck, or
    None to clear it."""
    if name is None:
        return None
    _checked(name, "str", "template deck")
    if name in session.decks:
        return session.decks[name]
    if name == DEFAULT_DECK[0]:
        return DEFAULT_DECK[1]
    raise invalid(f"deck {name!r} is not the default deck or in setup.decks")


def _apply(session, draft: dict, edit: Any) -> None:
    """One T5 edit on the draft legacy object."""
    if not isinstance(edit, dict) or len(edit) != 1:
        raise invalid("an edit is a mapping with exactly one key")
    ((key, value),) = edit.items()
    fields, templates = draft["flds"], draft["tmpls"]
    if key == "rename":
        draft["name"] = _checked(value, "str", "rename")
    elif key == "css":
        draft["css"] = _checked(value, "str", "css")
    elif key == "sort_field":
        field = _named(fields, value, "field")
        if field.get("ord") is None:
            raise invalid("sort_field is a field of the stored note type, not one this update added")
        draft["sortf"] = field["ord"]
    elif key == "add_field":
        fields.append(session.col.models.new_field(_checked(value, "str", "add_field")))
    elif key == "remove_field":
        fields.remove(_named(fields, value, "field"))
    elif key == "rename_field":
        old, new = _rename_pair(value, "rename_field")
        _named(fields, old, "field")["name"] = new
    elif key == "move_field":
        _mapping(value, {"field", "to"}, {"field", "to"}, "move_field")
        _move(fields, _named(fields, value["field"], "field"), value["to"], "move_field")
    elif key == "field_options":
        _mapping(value, {"field"} | set(FIELD_OPTIONS), {"field"}, "field_options")
        _set_values(_named(fields, value["field"], "field"), value, FIELD_OPTIONS, "field_options")
    elif key == "add_template":
        _mapping(value, TEMPLATE_KEYS, {"name"}, "add_template")
        for k in TEMPLATE_KEYS & set(value):
            _checked(value[k], "str", f"add_template {k}")
        templates.append(_new_template(session, value))
    elif key == "remove_template":
        templates.remove(_named(templates, value, "template"))
    elif key == "rename_template":
        old, new = _rename_pair(value, "rename_template")
        _named(templates, old, "template")["name"] = new
    elif key == "move_template":
        _mapping(value, {"template", "to"}, {"template", "to"}, "move_template")
        _move(templates, _named(templates, value["template"], "template"), value["to"], "move_template")
    elif key == "template":
        _mapping(value, {"name", "deck"} | set(TEMPLATE_VALUES), {"name"}, "template")
        template = _named(templates, value["name"], "template")
        _set_values(template, value, TEMPLATE_VALUES, "template")
        if "deck" in value:
            template["did"] = _deck(session, value["deck"])
    else:
        raise invalid(f"unknown note type edit {key!r}")


@op("update_notetype", rejects=REJECTS)
def update_notetype(session, args):
    _mapping(args, {"notetype", "edits", "skip_checks"}, {"notetype", "edits"}, "update_notetype")
    if not isinstance(args["edits"], list):
        raise invalid("update_notetype edits is a list")
    skip_checks = _checked(args.get("skip_checks", False), "bool", "update_notetype skip_checks")
    session.reopen()
    ntid = notetype_id(session, args["notetype"])
    draft = _legacy(session, ntid)
    for edit in args["edits"]:
        _apply(session, draft, edit)
    backend = session.col._backend
    type(backend).update_notetype_legacy(_SaveProbe(session.col), json=_encode(draft), skip_checks=skip_checks)
    return {"name": _stored_name(session, ntid)}


# --- notetype_structure --------------------------------------------------------


def _deck_value(session, did: int) -> str | int | None:
    if not did:
        return None
    name = session.col.db.scalar("select name from decks where id = ?", did)
    return did if name is None else name.replace(NATIVE_SEPARATOR, "::")


def _structure(session, ntid: int) -> dict:
    nt = session.col._backend.get_notetype(ntid)
    config = nt.config
    reqs: dict[int, dict] = {}
    for req in config.reqs:
        reqs.setdefault(req.card_ord, {"kind": REQUIREMENT_KINDS[req.kind], "fields": list(req.field_ords)})
    return {
        "kind": KIND_NAMES[config.kind],
        "original_stock_kind": config.original_stock_kind,
        "original_id": config.original_id if config.HasField("original_id") else None,
        "sort_field": config.sort_field_idx,
        "fields": [
            {
                "name": f.name,
                "sticky": f.config.sticky,
                "rtl": f.config.rtl,
                "font_name": f.config.font_name,
                "font_size": f.config.font_size,
                "description": f.config.description,
                "plain_text": f.config.plain_text,
                "collapsed": f.config.collapsed,
                "exclude_from_search": f.config.exclude_from_search,
                "tag": f.config.tag if f.config.HasField("tag") else None,
                "prevent_deletion": f.config.prevent_deletion,
            }
            for f in nt.fields
        ],
        "templates": [
            {"name": t.name, "deck": _deck_value(session, t.config.target_deck_id), "requirement": reqs.get(i)}
            for i, t in enumerate(nt.templates)
        ],
    }


@observation("notetype_structure")
def notetype_structure(session, args):
    if args is None or args == []:
        rows = session.col.db.all("select id, name from notetypes order by id")
    else:
        _mapping(args, {"notetypes"}, {"notetypes"}, "notetype_structure")
        names = args["notetypes"]
        if not isinstance(names, list) or not names or not all(isinstance(n, str) and n for n in names):
            raise invalid("notetype_structure notetypes is a non-empty list of names")
        rows = [(notetype_id(session, name), name) for name in names]
    return {name: _structure(session, ntid) for ntid, name in rows}
