# SPDX-License-Identifier: AGPL-3.0-or-later
"""Collection packages (specs/conformance/package-ops.md E1, E2).

export, collection package form {format: colpkg, name} (ops/package.py
    dispatches here) -> none. Anki's collection package export, media and the
    option for older clients off, to <session dir>/<name>, replacing a file of
    that name. Anki's export closes the collection itself; the driver reopens
    it afterwards, as the desktop does (also after a failure). Session.hand_off
    copies the session directory, so the package crosses a handoff.
import, {format: colpkg, fixture} or {format: colpkg, name} -> none;
    rejections too_new, corrupt. The driver closes the collection, runs
    Anki's collection package import into the session's collection.anki2 with
    the session's media folder and media database, and opens collection.anki2
    again with the same backend (the desktop's restore), after a refusal too.
    Other import forms are the package area's drafts: unsupported_op before
    any collection exists.

The import's refusals are read from the backend error (pylib raises import
and protobuf errors as the same plain BackendError, so a probe reads the kind):
too_new is the import error whose message is the backend's own "too new"
translation; corrupt is the import error whose message is its "not a valid
file" translation, a protobuf decode error, a zip error (Anki raises those as
sync errors) and an I/O error, which are what the import raises before it
replaces the collection (E2.3). Any other failure stays a run error. No
message leaves the driver (R1887).

E2.4: a scenario with a collection package import has no setup, opens first,
adds no note and names no logical id anywhere; checked before any collection.
"""

from __future__ import annotations

import os
import re
from typing import Any

from anki import backend_pb2
from anki._backend import backend_exception_to_pylib
from anki.collection import Collection
from anki.media import media_paths_from_col_path

from ..errors import DriverError, Rejected, invalid
from ..registry import op, scenario_check
from .storage_open import fixture_path, fixture_rel

# conformance/schema/scenario.schema.json colpkg_name; fullmatch, since `$`
# also matches before a final newline.
COLPKG_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*\.colpkg")
IMPORT_FORMS = ({"format", "fixture"}, {"format", "name"})
_LOGICAL_ID = re.compile(r"#[nc][1-9]")
_ERR = backend_pb2.BackendError
# Backend error kinds Anki's collection package import raises for a package
# it cannot read before the replace (E2.3), besides its import errors.
_CORRUPT_KINDS = frozenset({_ERR.PROTO_ERROR, _ERR.SYNC_OTHER_ERROR, _ERR.IO_ERROR})


def colpkg_name(value: Any, what: str) -> str:
    if not isinstance(value, str) or not COLPKG_NAME.fullmatch(value):
        raise invalid(f"{what} is a file name of ASCII letters, digits, '.', '_' and '-' that ends in .colpkg")
    return value


def import_source(args: Any) -> tuple[str, str]:
    """('fixture', path) or ('name', file name) of a collection package import;
    unsupported_op for the other (draft) forms."""
    if not isinstance(args, dict):
        raise invalid("import takes {format, ...}")
    if args.get("format") != "colpkg":
        raise DriverError("unsupported_op", op="import")
    if set(args) not in IMPORT_FORMS:
        raise invalid("the collection package import is exactly {format: colpkg, fixture} or {format: colpkg, name}")
    if "fixture" in args:
        return "fixture", fixture_rel(args["fixture"], (".colpkg",), "import fixture")
    return "name", colpkg_name(args["name"], "import name")


def _strings(value: Any):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for k, v in value.items():
            yield from _strings(k)
            yield from _strings(v)
    elif isinstance(value, list):
        for v in value:
            yield from _strings(v)


@scenario_check
def check_import_steps(scenario: dict) -> None:
    steps = scenario.get("steps", [])
    imports = []
    for index, step in enumerate(steps):
        ((name, args),) = step.items()
        if name == "import":
            try:
                import_source(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise
            imports.append(index)
    if not imports:
        return
    if scenario.get("setup") or "open" not in steps[0]:
        raise invalid("a scenario with a collection package import has no setup and opens first (package-ops.md E2.4)",
                      step=imports[0])
    for index, step in enumerate(steps):
        if "add_note" in step or any(_LOGICAL_ID.search(s) for s in _strings(step)):
            raise invalid("a scenario with a collection package import creates and names no logical id "
                          "(package-ops.md E2.4)", step=index)


def export(session, name: str) -> None:
    col = session.col
    try:
        col.export_collection_package(os.path.join(session.dir, name), include_media=False, legacy=False)
    finally:
        col.reopen()


class _Failed(Exception):
    def __init__(self, error: backend_pb2.BackendError) -> None:
        super().__init__(error.kind)
        self.error = error


class _ErrorProbe:
    """Stands in for the RustBackend in one generated backend call: a backend
    error raises _Failed with the decoded error (kind and message)."""

    def __init__(self, backend) -> None:
        self._rust = backend._backend

    def _run_command(self, service: int, method: int, data: bytes) -> bytes:
        try:
            return self._rust.command(service, method, data)
        except Exception as error:  # noqa: BLE001 - decoded as pylib does
            error_bytes = bytes(error.args[0])
        err = backend_pb2.BackendError()
        err.ParseFromString(error_bytes)
        raise _Failed(err)


def _refusal(tr, error: backend_pb2.BackendError) -> str | None:
    if error.kind == _ERR.IMPORT_ERROR:
        if error.message == tr.errors_collection_too_new():
            return "too_new"
        if error.message == tr.importing_the_provided_file_is_not_a():
            return "corrupt"
        return None
    return "corrupt" if error.kind in _CORRUPT_KINDS else None


@op("import", rejects=("too_new", "corrupt"))
def import_package(session, args):
    kind, value = import_source(args)
    if kind == "fixture":
        package = fixture_path(value, (".colpkg",), "import fixture")
    else:
        package = os.path.join(session.dir, value)
        if not os.path.isfile(package):
            raise DriverError("unknown_ref", ref=value)
    backend = session.col._backend
    media_folder, media_db = media_paths_from_col_path(session.path)
    session.close_collection()
    failed = None
    try:
        type(backend).import_collection_package(
            _ErrorProbe(backend), col_path=session.path, backup_path=package, media_folder=media_folder, media_db=media_db
        )
    except _Failed as exc:
        failed = exc.error
    finally:
        session.col = Collection(session.path, backend=backend)
    if failed is not None:
        refusal = _refusal(session.col.tr, failed)
        if refusal is None:
            raise backend_exception_to_pylib(failed)
        raise Rejected(refusal)
