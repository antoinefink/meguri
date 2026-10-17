# SPDX-License-Identifier: AGPL-3.0-or-later
"""Undo and redo, config removal and the undo status
(specs/conformance/undo-ops.md V1-V4).

undo: {}   the collection's undo; Anki's "undo empty" error (UndoEmpty) is the
    rejection nothing_to_undo. Any other error stays a run error.
redo: {}   the collection's redo; UndoEmpty is the rejection nothing_to_redo.
    changed_externally is Meguri's only kind (V1): the driver never raises it.
remove_config: {key}   the collection's config removal, always one undoable
    "Update Config" operation (V3.2).
(set_config's undoable, V3.1, is in ops/storage.py beside the op.)

undo_status (observation, no arguments) -> {undo: <id> | null, redo: <id> | null}
    The collection's undo status names each step in the backend's language.
    Each name is mapped back to its V4 id by comparing it with the backend's
    own translation of that id's name (the methods Anki's operations describe
    their steps with), never with English text in the driver. An empty name
    is null; a name no id translates to (an add-on's custom step) is a run
    error until V4 lists it. Two ids translating to one text is a driver
    error, never a guess. The names never leave the driver (R1887).

pylib's undo and redo drop its note type cache themselves when note types
changed.
"""

from __future__ import annotations

import anki.errors

from ..errors import DriverError, Rejected, invalid
from ..registry import observation, op

# V4 id -> the translation method of its step name, in V4's order.
STEP_NAMES = {
    "add_deck": "actions_add_deck",
    "add_note": "actions_add_note",
    "add_note_type": "actions_add_notetype",
    "answer_card": "actions_answer_card",
    "build_deck": "actions_build_filtered_deck",
    "bury": "studying_bury",
    "change_deck": "browsing_change_deck",
    "change_note_type": "browsing_change_notetype",
    "clear_unused_tags": "browsing_clear_unused_tags",
    "custom_study": "actions_custom_study",
    "deck_options": "deck_config_title",
    "delete_deck": "decks_delete_deck",
    "delete_note": "studying_delete_note",
    "empty": "studying_empty",
    "empty_cards": "actions_empty_cards",
    "find_and_replace": "browsing_find_and_replace",
    "grade_now": "actions_grade_now",
    "image_occlusion": "notetypes_image_occlusion_name",
    "import": "actions_import",
    "preferences": "preferences_preferences",
    "remove_note_type": "actions_remove_notetype",
    "remove_tag": "actions_remove_tag",
    "rename_deck": "actions_rename_deck",
    "rename_tag": "actions_rename_tag",
    "reposition": "actions_reposition",
    "reset_card": "actions_forget_card",
    "select_deck": "browsing_select_deck",
    "set_due_date": "actions_set_due_date",
    "set_flag": "actions_set_flag",
    "suspend": "studying_suspend",
    "toggle_load_balancer": "actions_toggle_load_balancer",
    "unbury_unsuspend": "actions_unbury_unsuspend",
    "update_card": "actions_update_card",
    "update_config": "actions_update_config",
    "update_deck": "actions_update_deck",
    "update_note": "actions_update_note",
    "update_note_type": "actions_update_notetype",
    "update_tag": "actions_update_tag",
}


def step_ids(tr) -> dict[str, str]:
    """The backend's step name -> its V4 id, from `tr`, a collection's
    translations."""
    by_name: dict[str, str] = {}
    for step_id, method in STEP_NAMES.items():
        name = getattr(tr, method)()
        if not name or name in by_name:
            raise DriverError("internal_error", detail=f"undo step {step_id} has no name of its own in the backend's language")
        by_name[name] = step_id
    return by_name


def _step_id(name: str, by_name: dict[str, str]) -> str | None:
    if name == "":
        return None
    step_id = by_name.get(name)
    if step_id is None:
        raise DriverError("internal_error", detail="the undo status names a step that undo-ops.md V4 does not list")
    return step_id


def _no_args(name: str, args) -> None:
    if args != {}:
        raise invalid(f"{name} takes {{}}")


@op("undo", rejects=("nothing_to_undo",))
def undo(session, args):
    _no_args("undo", args)
    try:
        session.col.undo()
    except anki.errors.UndoEmpty as exc:
        raise Rejected("nothing_to_undo") from exc


@op("redo", rejects=("nothing_to_redo",))
def redo(session, args):
    _no_args("redo", args)
    try:
        session.col.redo()
    except anki.errors.UndoEmpty as exc:
        raise Rejected("nothing_to_redo") from exc


@op("remove_config")
def remove_config(session, args):
    if not isinstance(args, dict) or set(args) != {"key"}:
        raise invalid("remove_config takes {key}")
    key = args["key"]
    if not isinstance(key, str) or not key:
        raise invalid("remove_config key is a non-empty string")
    session.col.remove_config(key)


@observation("undo_status")
def undo_status(session, args):
    if args is not None:
        raise invalid("undo_status takes no arguments")
    status = session.col.undo_status()
    by_name = step_ids(session.col.tr)
    return {"undo": _step_id(status.undo, by_name), "redo": _step_id(status.redo, by_name)}
