//! R1852 for a large deck package (`specs/package/apkg-import.md` I6, I15):
//! an apply pages every text, so its peak memory does not grow with what the
//! package's rows store, only with their number, which `MAX_ROWS` caps. Its
//! own test target: the counting allocator would serve every test of a
//! shared binary.
use std::io::Write;
use std::sync::Arc;

use meguri_engine::{CancelToken, ProgressSink};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::clock::StepClock;
use meguri_model::ids::{CardId, DeckId, NoteId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::note::Note;
use meguri_model::rng::SeededRandom;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_package::apkg::{self, ApkgContext, ImportOptions, UpdateCondition};
use meguri_storage::space::StatvfsSpace;
use meguri_storage::{ReadRows, ReaderRole, Record, StampMode, Table};
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::normal_support::Fixture;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const MIB: usize = 1024 * 1024;
const BASE: i64 = 1_600_000_000_000;

/// A legacy 2 package written straight into a collection file: `notes`
/// notes of one card each under `notetype`, whose second field holds `text`
/// bytes, and `revlog` review-log rows.
fn build(
    dir: &std::path::Path,
    notetype: &meguri_model::notetype::Notetype,
    (notes, text, revlog): (i64, usize, i64),
    clock: &StepClock,
    random: &SeededRandom,
) -> std::path::PathBuf {
    let path = dir.join("large.anki2");
    meguri_engine::bootstrap::create(&path, clock, random).unwrap();
    let opened = meguri_storage::open_collection(&path, &meguri_storage::OpenOptions::new(clock, random)).unwrap();
    {
        let mut scope = opened.writer().begin_write(StampMode::Raw, clock, None).unwrap();
        scope.put_notetype(notetype).unwrap();
        let filler = "x".repeat(text);
        for start in (0..notes).step_by(5000) {
            let (mut note_rows, mut card_rows) = (Vec::new(), Vec::new());
            for at in start..(start + 5000).min(notes) {
                let first = format!("n{at}");
                note_rows.push(
                    Note {
                        id: NoteId(BASE + at),
                        guid: format!("large-{at}"),
                        notetype_id: notetype.id,
                        mtime: EpochSecs(100),
                        usn: Usn(0),
                        tags: String::new(),
                        fields: format!("{first}\u{1f}{filler}"),
                        sort_field: first,
                        checksum: 0,
                        flags: 0,
                        data: String::new(),
                    }
                    .to_cells(),
                );
                card_rows.push(
                    Card {
                        id: CardId(BASE + at),
                        note_id: NoteId(BASE + at),
                        deck_id: DeckId(1),
                        ord: 0,
                        mtime: EpochSecs(100),
                        usn: Usn(0),
                        card_type: CardType::New,
                        queue: CardQueue::New,
                        due: at as i32,
                        interval: 0,
                        ease_factor: 0,
                        reps: 0,
                        lapses: 0,
                        remaining_steps: 0,
                        original_due: 0,
                        original_deck_id: DeckId(0),
                        flags: 0,
                        data: CardData::parse("{}"),
                    }
                    .to_cells(),
                );
            }
            scope.insert_rows(Table::Notes, &note_rows).unwrap();
            scope.insert_rows(Table::Cards, &card_rows).unwrap();
        }
        for start in (0..revlog).step_by(20_000) {
            let rows: Vec<_> = (start..(start + 20_000).min(revlog))
                .map(|at| {
                    RevlogEntry {
                        id: RevlogId(BASE + at),
                        card_id: CardId(BASE),
                        usn: Usn(0),
                        button: 3,
                        interval: 1,
                        last_interval: 0,
                        ease_factor: 2500,
                        answer_millis: 1000,
                        kind: RevlogKind::Review,
                    }
                    .to_cells()
                })
                .collect();
            scope.insert_rows(Table::Revlog, &rows).unwrap();
        }
        scope.commit().unwrap();
    }
    opened.close().unwrap();
    // Zipped as a stream: this test never holds the collection either.
    let package = dir.join("large.apkg");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    archive.start_file("collection.anki21", options).unwrap();
    std::io::copy(&mut std::fs::File::open(&path).unwrap(), &mut archive).unwrap();
    archive.finish().unwrap().flush().unwrap();
    std::fs::remove_file(&path).unwrap();
    package
}

const OPTIONS: ImportOptions = ImportOptions {
    merge_notetypes: false,
    update_notes: UpdateCondition::IfNewer,
    update_notetypes: UpdateCondition::Never,
    with_scheduling: false,
    with_deck_configs: false,
};

fn context(target: &Fixture) -> ApkgContext {
    ApkgContext {
        collection_directory: target.collection.path().parent().unwrap().to_owned(),
        clock: target.clock.clone(),
        random: target.random.clone(),
        space: Arc::new(StatvfsSpace),
    }
}

/// Imports a package of `notes` notes of `text` bytes into a fresh target:
/// the most bytes the inspect and then the whole import held at once, over
/// what was live before it.
fn peaks(notes: i64, text: usize) -> (usize, usize) {
    // The package's notes use the target fixture's note type, so the import
    // routes them without a copy.
    let target = Fixture::new(0);
    let scratch = tempfile::tempdir().unwrap();
    let notetype = target.collection.reader(ReaderRole::Query).unwrap().notetype(target.notetype).unwrap().unwrap();
    let package = build(scratch.path(), &notetype, (notes, text, 0), target.clock.as_ref(), target.random.as_ref());
    assert!(std::fs::metadata(&package).unwrap().len() > (notes as u64) * (text as u64));
    PeakAlloc::reset_peak();
    let baseline = PeakAlloc::current();
    let session =
        apkg::inspect(&package, "large.apkg", &context(&target), &CancelToken::new(), &ProgressSink::new()).unwrap();
    assert_eq!(session.facts().notes, notes as u64);
    let inspected = PeakAlloc::peak().saturating_sub(baseline);
    let result =
        apkg::apply(target.host.engine(), &session, OPTIONS, None, &CancelToken::new(), &ProgressSink::new()).unwrap();
    let peak = PeakAlloc::peak().saturating_sub(baseline);
    assert_eq!(result.counts.notes_added, notes as u64);
    assert_eq!(result.counts.cards_added, notes as u64);
    assert_eq!(target.collection.reader(ReaderRole::Query).unwrap().row_count(Table::Notes).unwrap(), notes as u64);
    eprintln!(
        "apkg_large: {notes} notes of {text} bytes: inspect peak {} MiB, import peak {} MiB",
        inspected / MIB,
        peak / MIB
    );
    (inspected, peak)
}

#[test]
fn an_import_s_memory_grows_with_its_rows_and_not_with_their_text() {
    assert!(PeakAlloc::is_installed());
    // The same rows with ten times the text: 18 MiB against 180 MiB.
    let (_, narrow) = peaks(30_000, 600);
    let (inspected, wide) = peaks(30_000, 6000);
    assert!(inspected < 32 * MIB, "the inspect held {} MiB", inspected / MIB);
    assert!(wide < narrow + 16 * MIB, "{} MiB with the wide text, {} MiB with the narrow", wide / MIB, narrow / MIB);
    assert!(wide < 30_000 * 6000 / 3, "{} MiB for 171 MiB of text", wide / MIB);

    // 300,000 notes and as many cards: under 640 bytes per written row, of
    // which the op's undo journal is the larger part.
    let (inspected, peak) = peaks(300_000, 600);
    assert!(inspected < 32 * MIB, "the inspect held {} MiB", inspected / MIB);
    assert!(peak < 600_000 * 640, "the import held {} MiB for 600,000 rows", peak / MIB);
}

#[test]
fn a_package_past_the_row_limit_is_refused_before_the_plan() {
    let target = Fixture::new(0);
    let scratch = tempfile::tempdir().unwrap();
    let notetype = target.collection.reader(ReaderRole::Query).unwrap().notetype(target.notetype).unwrap().unwrap();
    // One note and more review-log rows than the limit.
    let revlog = apkg::MAX_ROWS as i64 + 1;
    let package = build(scratch.path(), &notetype, (1, 4, revlog), target.clock.as_ref(), target.random.as_ref());
    let session =
        apkg::inspect(&package, "large.apkg", &context(&target), &CancelToken::new(), &ProgressSink::new()).unwrap();
    let scheduling = ImportOptions {
        with_scheduling: true,
        ..OPTIONS
    };
    PeakAlloc::reset_peak();
    let baseline = PeakAlloc::current();
    let refusal = apkg::apply(target.host.engine(), &session, scheduling, None, &CancelToken::new(), &ProgressSink::new());
    let Err(apkg::ApkgImportError::TooLarge { what: "rows", found: rows, limit }) = refusal else {
        panic!("a size refusal, not {refusal:?}");
    };
    assert_eq!((rows, limit), (apkg::MAX_ROWS + 3, apkg::MAX_ROWS));
    assert!(PeakAlloc::peak().saturating_sub(baseline) < 8 * MIB, "counted, never read");
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.row_count(Table::Revlog).unwrap(), 0);
    drop(reader);
    // Without its review log the same package is within the limit.
    let result =
        apkg::apply(target.host.engine(), &session, OPTIONS, None, &CancelToken::new(), &ProgressSink::new()).unwrap();
    assert_eq!(result.counts.notes_added, 1);
}
