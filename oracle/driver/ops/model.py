# SPDX-License-Identifier: AGPL-3.0-or-later
"""Schema-11 JSON of note types, decks and presets, and whether their blobs
decode (model-ops.md L1, L2, L4).

legacy_json (observation): {notetypes?, decks?, presets?: [name, ...]}
    -> {notetypes?: {name: object}, decks?: {...}, presets?: {...}}
legacy_update (op): {notetype | deck | preset: name, patch: object} -> none;
    rejection invalid_json
blob_decode (observation): {notetypes?, decks?, presets?: [name, ...]}
    -> {notetypes?: {name: "ok" | "invalid_blob"}, decks?: {...}, presets?: {...}}

Anki keeps note types and decks cached and raw SQL (db_exec) does not clear
those caches, so both reopen the collection before reading: the objects are
what the file holds. Names match exactly as stored; deck names in human form
("::" for the native separator U+001F). The objects come from the backend's
legacy getters and are parsed as JSON as they are, so integers stay ints and
numbers Anki wrote with a fraction or an exponent stay floats.

legacy_update applies `patch` as an RFC 7396 merge patch to that object and
saves the result the way sync merges a legacy record: note types with
add_or_update_notetype(preserve_usn_and_mtime, skip_checks), decks with
add_or_update_deck_legacy(preserve_usn_and_mtime), presets with
add_or_update_deck_config_legacy (stamped as a local change). Only a backend
JsonError is the rejection; pylib raises it as a plain BackendError, as it
does other kinds, so the kind is read from the backend's error message.

blob_decode finds each record by its name column (no blob decoded), reopens,
and reads the record with the same legacy getters: a backend ProtoError (any
protobuf decode failure of one of the record's blobs) is "invalid_blob", a
read that succeeds is "ok", and any other failure (a deck kind with no member,
a TEXT blob) stays a run error.
"""

from __future__ import annotations

import json
from typing import Any

from anki import backend_pb2
from anki._backend import backend_exception_to_pylib

from ..errors import DriverError, Rejected, invalid
from ..registry import observation, op

SECTIONS = ("notetypes", "decks", "presets")
TABLES = {"notetypes": "notetypes", "decks": "decks", "presets": "deck_config"}
GETTERS = {"notetypes": "get_notetype_legacy", "decks": "get_deck_legacy", "presets": "get_deck_config_legacy"}
UPDATE_KEYS = {"notetype": "notetypes", "deck": "decks", "preset": "presets"}
NATIVE_SEPARATOR = "\x1f"


def _name_list(kind: str, section: str, names: Any) -> list[str]:
    if not isinstance(names, list) or not names or not all(isinstance(n, str) and n for n in names):
        raise invalid(f"{kind} {section} is a non-empty list of names")
    return names


def _requested(kind: str, args: Any) -> dict[str, list[str]]:
    if not isinstance(args, dict) or not args:
        raise invalid(f"{kind} takes {{notetypes?, decks?, presets?}} with at least one list")
    unknown = sorted(set(args) - set(SECTIONS))
    if unknown:
        raise invalid(f"unknown {kind} keys: {', '.join(unknown)}")
    return {section: _name_list(kind, section, args[section]) for section in SECTIONS if section in args}


def _record_id(session, section: str, name: str) -> int:
    rows = session.col.db.all(f"select id, name from {TABLES[section]}")
    if section == "decks":
        rows = [(rid, stored.replace(NATIVE_SEPARATOR, "::")) for rid, stored in rows]
    ids = [rid for rid, stored in rows if stored == name]
    if not ids:
        raise DriverError("unknown_ref", ref=name)
    if len(ids) > 1:
        raise invalid(f"{len(ids)} {section} are named {name!r}")
    return ids[0]


def _unique_keys(pairs: list[tuple[str, Any]]) -> dict:
    obj = dict(pairs)
    if len(obj) != len(pairs):
        # The observation's JSON cannot hold both members.
        raise DriverError("internal_error", detail="Anki's legacy JSON repeats a member name")
    return obj


def _read(session, section: str, name: str) -> dict:
    rid = _record_id(session, section, name)
    raw = getattr(session.col._backend, GETTERS[section])(rid)
    return json.loads(raw, object_pairs_hook=_unique_keys)


@observation("legacy_json")
def legacy_json(session, args):
    requested = _requested("legacy_json", args)
    session.reopen()
    return {section: {name: _read(session, section, name) for name in names} for section, names in requested.items()}


class _InvalidBlob(Exception):
    pass


def _decode_status(session, section: str, name: str) -> str:
    rid = _record_id(session, section, name)
    backend = session.col._backend
    probe = KindProbe(backend, backend_pb2.BackendError.PROTO_ERROR, _InvalidBlob)
    try:
        getattr(type(backend), GETTERS[section])(probe, rid)
    except _InvalidBlob:
        return "invalid_blob"
    return "ok"


@observation("blob_decode")
def blob_decode(session, args):
    requested = _requested("blob_decode", args)
    session.reopen()
    return {
        section: {name: _decode_status(session, section, name) for name in names}
        for section, names in requested.items()
    }


def merge_patch(target: Any, patch: Any) -> Any:
    """RFC 7396: an object patch merges member by member (null removes the
    member); any other value, arrays included, replaces the target whole."""
    if not isinstance(patch, dict):
        return patch
    result = dict(target) if isinstance(target, dict) else {}
    for key, value in patch.items():
        if value is None:
            result.pop(key, None)
        else:
            result[key] = merge_patch(result.get(key), value)
    return result


class KindProbe:
    """Stands in for the RustBackend in one generated backend call: a backend
    error of `kind` raises `raises()`, any other error the exception pylib
    would raise."""

    def __init__(self, backend, kind: int, raises) -> None:
        self._rust = backend._backend
        self._kind = kind
        self._raises = raises

    def _run_command(self, service: int, method: int, data: bytes) -> bytes:
        try:
            return self._rust.command(service, method, data)
        except Exception as error:  # noqa: BLE001 - pylib's own handling, below
            error_bytes = bytes(error.args[0])
        err = backend_pb2.BackendError()
        err.ParseFromString(error_bytes)
        if err.kind == self._kind:
            raise self._raises()
        raise backend_exception_to_pylib(err)


def _save(session, section: str, obj: dict) -> None:
    backend = session.col._backend
    probe = KindProbe(backend, backend_pb2.BackendError.JSON_ERROR, lambda: Rejected("invalid_json"))
    data = json.dumps(obj, ensure_ascii=False).encode("utf-8")
    generated = type(backend)
    if section == "notetypes":
        generated.add_or_update_notetype(probe, json=data, preserve_usn_and_mtime=True, skip_checks=True)
    elif section == "decks":
        generated.add_or_update_deck_legacy(probe, deck=data, preserve_usn_and_mtime=True)
    else:
        generated.add_or_update_deck_config_legacy(probe, data)


@op("legacy_update", rejects=("invalid_json",))
def legacy_update(session, args):
    if not isinstance(args, dict):
        raise invalid("legacy_update takes {notetype | deck | preset: name, patch: object}")
    unknown = sorted(set(args) - set(UPDATE_KEYS) - {"patch"})
    if unknown:
        raise invalid(f"unknown legacy_update keys: {', '.join(unknown)}")
    targets = [key for key in UPDATE_KEYS if key in args]
    if len(targets) != 1:
        raise invalid("legacy_update names exactly one of notetype, deck and preset")
    name = args[targets[0]]
    if not isinstance(name, str) or not name:
        raise invalid(f"legacy_update {targets[0]} is a non-empty name")
    if not isinstance(args.get("patch"), dict):
        raise invalid("legacy_update patch is an object")
    section = UPDATE_KEYS[targets[0]]
    session.reopen()
    _save(session, section, merge_patch(_read(session, section, name), args["patch"]))
