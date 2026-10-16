# SPDX-License-Identifier: AGPL-3.0-or-later
"""Browser and search config keys (specs/conformance/browser-config-ops.md B1, B2).

browser_config_set: {key, value}   one of B1's 22 keys and a value of that key's
    JSON type (a boolean is not an integer, an array holds only strings, an
    object only string members), else invalid_scenario. Written with the
    collection's typed boolean or string setter where Anki has one, else its
    config setter; all three default to no undo entry (the history is kept).
    No output.
observe browser_config: B2's 23 keys as Anki's own reads give them. The
    typed boolean and string reads carry their defaults; the other eight keys
    go through the generic config read with the default Anki's browser passes,
    which gives the stored JSON value whatever its shape, or the default when
    the key is missing, not a BLOB or not JSON. The browser's column loaders
    are not called: they also set the backend's active columns.

Values reach Anki as the scenario gave them and come back from its JSON
decoder, so 1.0 and -0.0 stay floats and integers stay integers.
"""

from __future__ import annotations

import copy
from typing import Any

from anki.browser import BrowserDefaults
from anki.config import Config

from ..errors import invalid
from ..registry import observation, op

_B = Config.Bool
_S = Config.String

# Keys Anki reads and writes with its typed boolean accessors.
TYPED_BOOLS = {
    "browserTableShowNotesMode": _B.BROWSER_TABLE_SHOW_NOTES_MODE,
    "collapseSavedSearches": _B.COLLAPSE_SAVED_SEARCHES,
    "collapseToday": _B.COLLAPSE_TODAY,
    "collapseFlags": _B.COLLAPSE_FLAGS,
    "collapseCardState": _B.COLLAPSE_CARD_STATE,
    "collapseDecks": _B.COLLAPSE_DECKS,
    "collapseNotetypes": _B.COLLAPSE_NOTETYPES,
    "collapseTags": _B.COLLAPSE_TAGS,
    "previewBothSides": _B.PREVIEW_BOTH_SIDES,
    "ignoreAccentsInSearch": _B.IGNORE_ACCENTS_IN_SEARCH,
    "restorePositionBrowser": _B.RESTORE_POSITION_BROWSER,
    "resetCountsBrowser": _B.RESET_COUNTS_BROWSER,
}
# Read by the kind, never written by browser_config_set (scenarios use set_config).
READ_ONLY_BOOLS = {"normalize_note_text": _B.NORMALIZE_NOTE_TEXT}
TYPED_STRINGS = {
    "setDueBrowser": _S.SET_DUE_BROWSER,
    "defaultSearchText": _S.DEFAULT_SEARCH_TEXT,
}
# Keys with no typed accessor: the shape browser_config_set takes and the
# default the generic read is given.
UNTYPED: dict[str, tuple[str, Any]] = {
    "activeCols": ("strings", BrowserDefaults.CARD_COLUMNS),
    "activeNoteCols": ("strings", BrowserDefaults.NOTE_COLUMNS),
    "sortType": ("string", None),
    "noteSortType": ("string", None),
    "sortBackwards": ("boolean", False),
    "browserNoteSortBackwards": ("boolean", False),
    "savedFilters": ("object", {}),
    "flagLabels": ("object", {}),
}

SHAPES: dict[str, str] = {
    **{key: "boolean" for key in TYPED_BOOLS},
    **{key: "string" for key in TYPED_STRINGS},
    **{key: shape for key, (shape, _) in UNTYPED.items()},
}
_SHAPE_TEXT = {
    "boolean": "a boolean",
    "string": "a string",
    "strings": "an array of strings",
    "object": "an object whose members are strings",
}


def _fits(shape: str, value: Any) -> bool:
    if shape == "boolean":
        return isinstance(value, bool)
    if shape == "string":
        return isinstance(value, str)
    if shape == "strings":
        return isinstance(value, list) and all(isinstance(v, str) for v in value)
    return isinstance(value, dict) and all(isinstance(k, str) and isinstance(v, str) for k, v in value.items())


@op("browser_config_set")
def browser_config_set(session, args):
    if not isinstance(args, dict) or set(args) != {"key", "value"}:
        raise invalid("browser_config_set takes {key, value}")
    key, value = args["key"], args["value"]
    shape = SHAPES.get(key) if isinstance(key, str) else None
    if shape is None:
        raise invalid("browser_config_set key is one of the 22 browser keys of browser-config-ops.md B1")
    if not _fits(shape, value):
        raise invalid(f"browser_config_set {key} takes {_SHAPE_TEXT[shape]}")
    col = session.col
    if key in TYPED_BOOLS:
        col.set_config_bool(TYPED_BOOLS[key], value)
    elif key in TYPED_STRINGS:
        col.set_config_string(TYPED_STRINGS[key], value)
    else:
        col.set_config(key, value)


@observation("browser_config")
def browser_config(session, args):
    if args is not None:
        raise invalid("browser_config takes no arguments")
    col = session.col
    out: dict[str, Any] = {}
    for key, enum in {**TYPED_BOOLS, **READ_ONLY_BOOLS}.items():
        out[key] = col.get_config_bool(enum)
    for key, enum in TYPED_STRINGS.items():
        out[key] = col.get_config_string(enum)
    for key, (_, default) in UNTYPED.items():
        out[key] = col.get_config(key, copy.deepcopy(default))
    return out
