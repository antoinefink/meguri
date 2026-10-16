//! Step names (`specs/conformance/undo-ops.md` V4; R370, R719): Anki's
//! English name of every undo step. An op names the step it records with one
//! of these through `OpSpec::undo_step`, exactly as written here; the UI
//! shows them as "Undo <name>" and "Redo <name>".

pub const ADD_DECK: &str = "Add Deck";
pub const ADD_NOTE: &str = "Add Note";
pub const ADD_NOTE_TYPE: &str = "Add Note Type";
pub const ANSWER_CARD: &str = "Answer Card";
/// Building and rebuilding a filtered deck.
pub const BUILD_DECK: &str = "Build Deck";
pub const BURY: &str = "Bury";
pub const CHANGE_DECK: &str = "Change Deck";
pub const CHANGE_NOTE_TYPE: &str = "Change Note Type";
pub const CLEAR_UNUSED_TAGS: &str = "Clear Unused Tags";
pub const CUSTOM_STUDY: &str = "Custom Study";
pub const DECK_OPTIONS: &str = "Deck Options";
pub const DELETE_DECK: &str = "Delete Deck";
pub const DELETE_NOTE: &str = "Delete Note";
/// Emptying a filtered deck.
pub const EMPTY: &str = "Empty";
pub const EMPTY_CARDS: &str = "Empty Cards";
pub const FIND_AND_REPLACE: &str = "Find and Replace";
pub const GRADE_NOW: &str = "Grade Now";
pub const IMAGE_OCCLUSION: &str = "Image Occlusion";
pub const IMPORT: &str = "Import";
pub const PREFERENCES: &str = "Preferences";
pub const REMOVE_NOTE_TYPE: &str = "Remove Note Type";
pub const REMOVE_TAG: &str = "Remove Tag";
/// Renaming and moving a deck.
pub const RENAME_DECK: &str = "Rename Deck";
/// Renaming and moving a tag.
pub const RENAME_TAG: &str = "Rename Tag";
pub const REPOSITION: &str = "Reposition";
pub const RESET_CARD: &str = "Reset Card";
/// Anki's step for selecting a deck. Meguri records none (R42): selecting a
/// deck is a skip-undo op (M2).
pub const SELECT_DECK: &str = "Select Deck";
pub const SET_DUE_DATE: &str = "Set Due Date";
pub const SET_FLAG: &str = "Set Flag";
pub const SUSPEND: &str = "Suspend";
pub const TOGGLE_LOAD_BALANCER: &str = "Toggle Load Balancer";
pub const UNBURY_UNSUSPEND: &str = "Unbury/Unsuspend";
pub const UPDATE_CARD: &str = "Update Card";
/// A config write or removal that records a step (`undo-ops.md` V3).
pub const UPDATE_CONFIG: &str = "Update Config";
pub const UPDATE_DECK: &str = "Update Deck";
/// A note update; merges with the top step under `engine/undo` A3.
pub const UPDATE_NOTE: &str = "Update Note";
pub const UPDATE_NOTE_TYPE: &str = "Update Note Type";
pub const UPDATE_TAG: &str = "Update Tag";

/// Meguri's own step for the tag sidebar's collapsed state
/// (`specs/tags/tags.md` T12): Anki records none. Not in V4, which lists
/// Anki's steps; no scenario observes it.
pub const EXPAND_COLLAPSE: &str = "Expand/Collapse";
/// Meguri's own step for renaming a flag or restoring its default name
/// (`specs/tags/flags-and-marked.md` F4): Anki records none. Not in V4.
pub const RENAME_FLAG: &str = "Rename Flag";

/// Every name, in V4's order.
pub const ALL: [&str; 38] = [
    ADD_DECK,
    ADD_NOTE,
    ADD_NOTE_TYPE,
    ANSWER_CARD,
    BUILD_DECK,
    BURY,
    CHANGE_DECK,
    CHANGE_NOTE_TYPE,
    CLEAR_UNUSED_TAGS,
    CUSTOM_STUDY,
    DECK_OPTIONS,
    DELETE_DECK,
    DELETE_NOTE,
    EMPTY,
    EMPTY_CARDS,
    FIND_AND_REPLACE,
    GRADE_NOW,
    IMAGE_OCCLUSION,
    IMPORT,
    PREFERENCES,
    REMOVE_NOTE_TYPE,
    REMOVE_TAG,
    RENAME_DECK,
    RENAME_TAG,
    REPOSITION,
    RESET_CARD,
    SELECT_DECK,
    SET_DUE_DATE,
    SET_FLAG,
    SUSPEND,
    TOGGLE_LOAD_BALANCER,
    UNBURY_UNSUSPEND,
    UPDATE_CARD,
    UPDATE_CONFIG,
    UPDATE_DECK,
    UPDATE_NOTE,
    UPDATE_NOTE_TYPE,
    UPDATE_TAG,
];
