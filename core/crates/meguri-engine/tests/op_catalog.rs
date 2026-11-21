//! The op catalog's undo classes and every op source's undo effect
//! (`specs/engine/undo.md` M2, M11; R2020).

use std::collections::BTreeSet;

use meguri_engine::executor::catalog::{self, CATALOG};
use meguri_engine::{DisabledReason, SourceEffect, UndoClass, source_effect};
use meguri_model::changes::OpSource;

#[test]
fn every_catalog_op_declares_an_undo_class_and_destructive_ones_confirmation_and_backup() {
    let mut names = BTreeSet::new();
    for def in CATALOG {
        assert!(names.insert(def.name()), "{} is listed twice", def.name());
        let class = def
            .undo_class()
            .unwrap_or_else(|| panic!("{} declares no undo class", def.name()));
        match class {
            UndoClass::DestructiveNonUndoable => {
                assert!(
                    class.requires_confirmation() && class.requires_backup(),
                    "{} is destructive and not undoable: it needs confirmation and the pre-op backup",
                    def.name()
                );
            }
            UndoClass::Undoable | UndoClass::NonDestructive => {
                assert!(
                    !class.requires_confirmation() && !class.requires_backup(),
                    "{}",
                    def.name()
                );
            }
        }
        if def.is_maintenance_task() {
            assert_eq!(
                class,
                UndoClass::NonDestructive,
                "{}: a maintenance task changes no row",
                def.name()
            );
        }
    }
    // The classes the spec gives (op-model O9, undo M11).
    let class = |name: &str| catalog::lookup(name).and_then(|def| def.undo_class());
    assert_eq!(class("check_database"), Some(UndoClass::DestructiveNonUndoable));
    assert_eq!(class("apkg_apply"), Some(UndoClass::Undoable));
    assert_eq!(class("notetype_structure_change"), Some(UndoClass::Undoable));
    assert_eq!(class("change_notetype"), Some(UndoClass::Undoable));
    assert!(catalog::CHANGE_NOTETYPE.may_declare_long_write() && !catalog::CHANGE_NOTETYPE.cancellable_write());
    assert_eq!(class("bulk_op"), Some(UndoClass::Undoable));
    assert_eq!(class("memory_recompute"), Some(UndoClass::Undoable));
    assert!(catalog::MEMORY_RECOMPUTE.cancellable_write());
    assert_eq!(class("enable_fsrs"), Some(UndoClass::NonDestructive));
    assert!(catalog::ENABLE_FSRS.cancellable_write());
    assert_eq!(class("sync_incoming_merge"), Some(UndoClass::NonDestructive));
    assert_eq!(class("cloud_incoming_apply"), Some(UndoClass::NonDestructive));
    assert!(catalog::CLOUD_INCOMING_APPLY.may_declare_long_write());
    assert!(catalog::CLOUD_INCOMING_APPLY.cancellable_write());
    for name in ["undo", "redo", "undo_long_write", "redo_long_write"] {
        assert_eq!(class(name), Some(UndoClass::Undoable), "{name}");
    }
    // Plain undo and redo stay short, so the writer syncs no pending WAL
    // before them; the long variants may declare the long write of the step
    // they restore.
    assert!(!catalog::UNDO.may_declare_long_write() && !catalog::REDO.may_declare_long_write());
    assert!(catalog::UNDO_LONG_WRITE.may_declare_long_write() && catalog::REDO_LONG_WRITE.may_declare_long_write());
    assert_eq!(
        meguri_engine::OpDef::new("plain").undo_class(),
        None,
        "a plain op is no catalog op"
    );
}

#[test]
fn every_op_source_declares_its_undo_effect() {
    let effects: Vec<(OpSource, SourceEffect)> = OpSource::ALL.iter().map(|s| (*s, source_effect(*s))).collect();
    assert_eq!(
        effects,
        [
            (OpSource::UserUndoable, SourceEffect::Recorded),
            (
                OpSource::UserNonUndoable,
                SourceEffect::ClearAll(DisabledReason::ClearedByNonUndoableOp)
            ),
            (OpSource::SyncIncoming, SourceEffect::SyncMerge),
            (OpSource::LocalApi, SourceEffect::TruncateIntersecting),
            (
                OpSource::Optimizer,
                SourceEffect::ClearAll(DisabledReason::ClearedByNonUndoableOp)
            ),
            (OpSource::Rollover, SourceEffect::Kept),
            (
                OpSource::Maintenance,
                SourceEffect::ClearAll(DisabledReason::ClearedByNonUndoableOp)
            ),
            (OpSource::CloudIncoming, SourceEffect::CloudTargetedDrop),
            (OpSource::CloudRekey, SourceEffect::CloudTargetedDrop),
        ]
    );
}

mod common;
#[path = "op_catalog/structure_reports.rs"]
mod structure_reports;
