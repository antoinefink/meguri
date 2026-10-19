# SPDX-License-Identifier: AGPL-3.0-or-later
"""image_occlusion {fns: [name, ...], inputs: [input, ...]}: pure
image-occlusion functions (render area).

Value: {name: [output for each input, in input order]} for every requested
function. Request, value and meanings: specs/conformance/image-occlusion-ops.md
J1-J3; the behavior is specs/render/image-occlusion-format.md F2, F4, F5.

The scenario's collection is never read or written. Everything runs on one
scratch collection per worker process, created on first use in the worker's
scratch directory, holding one cloze note type the driver defines (no stock
note type, R1959). Every note and note type a function adds there is removed
again before it returns, so no output depends on an earlier one.

  parse    a note of the probe note type (four untagged fields, so the field
           roles are positions 0-3) with the input in its first field, read
           with GetImageOcclusionNote, then removed. The stored field must equal
           the input. Anki lists the numbers in hash-map order: they are sorted
           ascending here; shapes and properties keep Anki's order.
  fields   a cloze note type with one field per tag (legacy JSON `tag`, absent
           for null), read with GetImageOcclusionFields, then removed. The
           stored tags must equal the input. Anki's error for a note type
           without field roles (DatabaseCheckRequired: kind InvalidInput with
           the "please check database" message, read through the backend's own
           translation) is null; any other error stays a run error.
"""

from __future__ import annotations

import os
import tempfile
import unicodedata

from anki import backend_pb2
from anki._backend import backend_exception_to_pylib
from anki.collection import Collection
from anki.consts import MODEL_CLOZE
from anki.utils import from_json_bytes

from ..errors import DriverError, invalid
from ..registry import observation

FNS = ("parse", "fields")
MAX_TAG = 0xFFFFFFFF

_DEFAULT_DECK = 1
_PROBE_FIELDS = ("Occlusion", "Image", "Header", "Back Extra")
_FIELDS_PROBE = "IO fields probe"
# Below U+0020, a note save keeps only TAB and LF (image-occlusion-ops.md J1).
_KEPT_CONTROLS = frozenset("\t\n")


def _cloze_notetype(col: Collection, name: str, fields: list[tuple[str, int | None]]) -> int:
    """Add a cloze note type with these (name, tag) fields and one template
    showing {{cloze:<first field>}} on both sides; return its id."""
    models = col.models
    nt = models.new(name)
    nt["type"] = MODEL_CLOZE
    for field_name, tag in fields:
        fld = models.new_field(field_name)
        if tag is not None:
            fld["tag"] = tag
        models.add_field(nt, fld)
    t = models.new_template("Card 1")
    t["qfmt"] = t["afmt"] = "{{cloze:%s}}" % fields[0][0]
    models.add_template(nt, t)
    return models.add_dict(nt).id


class _NoRoles(Exception):
    pass


class _RolesProbe:
    """Stands in for the RustBackend in one generated backend call (as
    ops/model.py's _KindProbe): Anki's DatabaseCheckRequired raises _NoRoles,
    any other error the exception pylib would raise."""

    def __init__(self, col: Collection) -> None:
        self._rust = col._backend._backend
        self._message = col.tr.errors_please_check_database()

    def _run_command(self, service: int, method: int, data: bytes) -> bytes:
        try:
            return self._rust.command(service, method, data)
        except Exception as error:  # noqa: BLE001 - pylib's own handling, below
            error_bytes = bytes(error.args[0])
        err = backend_pb2.BackendError()
        err.ParseFromString(error_bytes)
        if err.kind == backend_pb2.BackendError.INVALID_INPUT and err.message == self._message:
            raise _NoRoles()
        raise backend_exception_to_pylib(err)


class _Scratch:
    """The worker's scratch collection with the parse probe note type."""

    def __init__(self, root: str) -> None:
        self.dir = tempfile.mkdtemp(prefix="image-occlusion-", dir=root)
        self.col = Collection(os.path.join(self.dir, "collection.anki2"))
        ntid = _cloze_notetype(self.col, "IO parse probe", [(name, None) for name in _PROBE_FIELDS])
        self.notetype = self.col.models.get(ntid)

    def parse(self, text: str) -> dict:
        col = self.col
        note = col.new_note(self.notetype)
        note.fields[0] = text
        col.add_note(note, _DEFAULT_DECK)
        try:
            if col.get_note(note.id).fields[0] != text:
                raise DriverError("internal_error", detail="the stored Occlusion field differs from the input")
            response = col._backend.get_image_occlusion_note(note_id=note.id)
        finally:
            col.remove_notes([note.id])
        if response.WhichOneof("value") != "note":
            raise DriverError("internal_error", detail="the image-occlusion note query answered with an error")
        io = response.note
        numbers = [
            {
                "number": occlusion.ordinal,
                "shapes": [
                    {"shape": shape.shape, "props": [[p.name, p.value] for p in shape.properties]}
                    for shape in occlusion.shapes
                ],
            }
            for occlusion in sorted(io.occlusions, key=lambda o: o.ordinal)
        ]
        return {"hide_all": io.occlude_inactive, "numbers": numbers}

    def fields(self, tags: list) -> dict | None:
        col = self.col
        ntid =_cloze_notetype(col, _FIELDS_PROBE, [(f"F{i + 1}", tag) for i, tag in enumerate(tags)])
        try:
            saved = from_json_bytes(col._backend.get_notetype_legacy(ntid))
            if [f.get("tag") for f in saved["flds"]] != tags:
                raise DriverError("internal_error", detail="the stored field tags differ from the input")
            try:
                idx = type(col._backend).get_image_occlusion_fields(_RolesProbe(col), notetype_id=ntid)
            except _NoRoles:
                return None
        finally:
            col.models.remove(ntid)
        return {
            "occlusions": idx.occlusions,
            "image": idx.image,
            "header": idx.header,
            "back_extra": idx.back_extra,
        }


_scratch: _Scratch | None = None


def _get_scratch(session) -> _Scratch:
    global _scratch
    if _scratch is None:
        _scratch = _Scratch(os.path.dirname(session.dir))
    return _scratch


def _is_tag(value) -> bool:
    # bool is an int subclass in Python; a scenario's true is not a tag.
    return value is None or (type(value) is int and 0 <= value <= MAX_TAG)


def _is_field_text(value) -> bool:
    return (
        isinstance(value, str)
        and unicodedata.is_normalized("NFC", value)
        and not any(c < " " and c not in _KEPT_CONTROLS for c in value)
    )


def _check(args) -> tuple[list[str], list]:
    if not isinstance(args, dict) or set(args) != {"fns", "inputs"}:
        raise invalid("image_occlusion takes {fns, inputs}")
    fns, inputs = args["fns"], args["inputs"]
    if not isinstance(fns, list) or not fns or not all(isinstance(f, str) for f in fns):
        raise invalid("image_occlusion fns is a non-empty list of function names")
    unknown = sorted(set(fns) - set(FNS))
    if unknown:
        raise invalid(f"unknown image_occlusion functions: {', '.join(unknown)}")
    if len(set(fns)) != len(fns):
        raise invalid("an image_occlusion function is listed twice")
    if not isinstance(inputs, list):
        raise invalid("image_occlusion inputs is a list")
    if "fields" in fns:
        if len(fns) != 1:
            raise invalid("fields takes tag lists and cannot share a request with other image_occlusion functions")
        for item in inputs:
            if not isinstance(item, list) or not item:
                raise invalid("fields inputs are non-empty lists of field tags")
            if not all(_is_tag(tag) for tag in item):
                raise invalid(f"a field tag is an integer from 0 to {MAX_TAG} or null")
    elif not all(_is_field_text(s) for s in inputs):
        raise invalid("parse inputs are NFC strings with no control character but TAB and LF")
    return fns, inputs


@observation("image_occlusion")
def image_occlusion(session, args):
    fns, inputs = _check(args)
    scratch = _get_scratch(session)
    impl = {"parse": scratch.parse, "fields": scratch.fields}
    return {fn: [impl[fn](x) for x in inputs] for fn in fns}
