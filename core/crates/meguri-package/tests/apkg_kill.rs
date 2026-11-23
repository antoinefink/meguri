//! R2003 for a deck package import (`specs/package/apkg-import.md` I14
//! Kill): the process ends inside the write phase or just after the commit,
//! and the reopened collection holds none of the import or all of it. Its
//! own test target: it runs this binary again by test name.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use meguri_engine::{CancelToken, OpSource, ProgressSink};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::faultpoint::{self, Fault};
use meguri_model::rng::SeededRandom;
use meguri_package::apkg::{self, ApkgContext, ImportOptions, UpdateCondition};
use meguri_package::{CollectionPackageWriteContext, Finalisation, write_collection_package_from_live};
use meguri_storage::atomic_file::Purpose;
use meguri_storage::integrity::integrity_check;
use meguri_storage::space::StatvfsSpace;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_testkit::bench::fixtures::open_engine_collection_with_backups;
use meguri_testkit::normal_support::{Fixture, NOW};

const NOTES: u64 = 40;
const OPTIONS: ImportOptions = ImportOptions {
    merge_notetypes: false,
    update_notes: UpdateCondition::IfNewer,
    update_notetypes: UpdateCondition::IfNewer,
    with_scheduling: true,
    with_deck_configs: true,
};

fn clock() -> Arc<StepClock> {
    Arc::new(StepClock::new(NOW + 60_000, UtcOffset::UTC, "UTC"))
}

/// A package of `NOTES` notes under a note type and a deck the target lacks.
fn package(dir: &Path) -> PathBuf {
    let source = Fixture::new(0);
    let deck = meguri_engine::decks::ops::add(&source.collection, "Quorvane".into()).unwrap().value.id;
    for at in 0..NOTES {
        meguri_engine::notes::add(
            &source.collection,
            meguri_engine::notes::AddNote {
                notetype_id: source.notetype,
                fields: vec![format!("front {at}"), format!("back {at}")],
                tags: vec![],
                deck_id: deck,
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    }
    let path = dir.join("deck.apkg");
    let lease = source.collection.reader(ReaderRole::Maintenance).unwrap();
    write_collection_package_from_live(
        &lease,
        &path,
        &CollectionPackageWriteContext {
            collection_directory: source.collection.path().parent().unwrap(),
            purpose: Purpose::Export,
            finalisation: Finalisation::Replace,
            clock: source.clock.as_ref(),
            random: source.random.as_ref(),
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    path
}

fn context(collection: &Path, clock: &Arc<StepClock>) -> ApkgContext {
    ApkgContext {
        collection_directory: collection.parent().unwrap().to_owned(),
        clock: clock.clone(),
        random: Arc::new(SeededRandom::new(211)),
        space: Arc::new(StatvfsSpace),
    }
}

/// The rows an import adds to: notes, cards, note types, decks.
fn counts(collection: &meguri_engine::Collection) -> [u64; 4] {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    [Table::Notes, Table::Cards, Table::Notetypes, Table::Decks].map(|table| reader.row_count(table).unwrap())
}

/// The child: imports with the fault armed, so it never returns.
#[test]
fn kill_worker() {
    let Some(root) = std::env::var_os("MEGURI_APKG_KILL_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let point = std::env::var("MEGURI_APKG_KILL_POINT").unwrap();
    let collection = root.join("collection").join("collection.anki2");
    let clock = clock();
    let host = open_engine_collection_with_backups(&collection, &root.join("backups"), clock.clone()).unwrap();
    let session = apkg::inspect(
        &root.join("deck.apkg"),
        "deck.apkg",
        &context(&collection, &clock),
        &CancelToken::new(),
        &ProgressSink::new(),
    )
    .unwrap();
    faultpoint::arm(&point, Fault::Abort);
    let outcome = apkg::apply(host.engine(), &session, OPTIONS, None, &CancelToken::new(), &ProgressSink::new());
    panic!("the import returned {:?} instead of ending the process", outcome.map(|result| result.counts));
}

fn killed_at(point: &str, imported: bool) {
    let root = tempfile::tempdir().unwrap();
    let collection = root.path().join("collection").join("collection.anki2");
    std::fs::create_dir_all(collection.parent().unwrap()).unwrap();
    let clock = clock();
    meguri_engine::bootstrap::create(&collection, clock.as_ref(), &SeededRandom::new(211)).unwrap();
    package(root.path());
    let before = {
        let host = open_engine_collection_with_backups(&collection, &root.path().join("backups"), clock.clone()).unwrap();
        let before = counts(&host.collection().unwrap());
        host.close().unwrap();
        before
    };

    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "kill_worker", "--nocapture"])
        .env("MEGURI_APKG_KILL_ROOT", root.path())
        .env("MEGURI_APKG_KILL_POINT", point)
        .status()
        .unwrap();
    assert!(!status.success(), "the worker was to be killed at {point}");
    assert_eq!(status.code(), None, "ended by a signal, not by a failed assertion: {status:?}");

    assert!(integrity_check(&collection).unwrap().is_ok());
    let host = open_engine_collection_with_backups(&collection, &root.path().join("backups"), clock.clone()).unwrap();
    let live = host.collection().unwrap();
    let after = counts(&live);
    if imported {
        // All of it: the notes, a card each, the note type and the deck.
        assert_eq!(after, [before[0] + NOTES, before[1] + NOTES, before[2] + 1, before[3] + 1]);
    } else {
        assert_eq!(after, before, "none of the import");
    }
    // The collection takes the same import afterwards: every note once.
    let session = apkg::inspect(
        &root.path().join("deck.apkg"),
        "deck.apkg",
        &context(&collection, &clock),
        &CancelToken::new(),
        &ProgressSink::new(),
    )
    .unwrap();
    let result = apkg::apply(host.engine(), &session, OPTIONS, None, &CancelToken::new(), &ProgressSink::new()).unwrap();
    assert_eq!(result.counts.notes_added, if imported { 0 } else { NOTES });
    assert_eq!(counts(&live), [before[0] + NOTES, before[1] + NOTES, before[2] + 1, before[3] + 1]);
    session.close();
    drop(live);
    host.close().unwrap();
    assert!(integrity_check(&collection).unwrap().is_ok());
}

#[test]
fn a_kill_inside_the_write_phase_leaves_none_of_the_import() {
    killed_at("package.apkg.write_phase", false);
}

#[test]
fn a_kill_after_the_commit_leaves_all_of_the_import() {
    killed_at("package.apkg.after_commit", true);
}
