//! Public disposable native-import fixture; not CloudKit acceptance evidence.
//! Contracts: package/file-export E7, package/container P16, engine/host H1/H6,
//! media/media-files M3 and sched/review-session C2.
use meguri_engine::{
    CancelToken, EventKind, JobKind, OpSource, OpSpec, ProgressSink,
    session::{AnswerOutcome, StartResult},
};
use meguri_host::{CoreHost, HostConfig, ProcessCore};
use meguri_model::{
    card::{CardQueue, CardType},
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_package::entry;
use meguri_sched::answer::Rating;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, Table, space::StatvfsSpace};
use meguri_testkit::generate::{media::png, rng::{Rng, Stream}};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path, sync::{Arc, mpsc}, time::Duration};

const NOW: i64 = 1_767_268_800_000; // 2026-01-01 12:00 UTC; never advance it.
const IMAGE: &str = "media-review-pattern.png";
const AUDIO: &str = "media-review-tone.wav";
const PACKAGE: &str = "media-review-check.colpkg";
const MANIFEST: &str = "media-review-check.manifest.json";
const WAIT: Duration = Duration::from_secs(20);

fn config(root: &Path) -> HostConfig {
    let directory = root.join("collection");
    fs::create_dir_all(&directory).unwrap();
    HostConfig::new(
        directory, root.join("backups"), root.join("widget"),
        Arc::new(FixedClock::new(NOW, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(418)), Arc::new(StatvfsSpace),
    )
}

fn tone() -> Vec<u8> {
    // Half a second of 440 Hz, mono signed 16-bit PCM at 16 kHz, with fades.
    let mut samples = Vec::new();
    for index in 0..8_000i32 {
        let envelope = (index.min(7_999 - index) as f64 / 320.0).min(1.0);
        let phase = std::f64::consts::TAU * 440.0 * index as f64 / 16_000.0;
        samples.extend_from_slice(&((phase.sin() * envelope * 8_000.0) as i16).to_le_bytes());
    }
    let length = u32::try_from(samples.len()).unwrap();
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36 + length).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(b"\x02\0\x10\0data");
    wav.extend_from_slice(&length.to_le_bytes());
    wav.extend_from_slice(&samples);
    wav
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn public_media_review_collection_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let options = config(&root.path().join("source"));
    meguri_engine::bootstrap::create(
        &options.collection_directory.join("collection.anki2"),
        options.clock.as_ref(), options.random.as_ref(),
    ).unwrap();
    let source = CoreHost::open(options).unwrap();
    assert!(source.derived().wait_idle(WAIT));
    let collection = source.collection().unwrap();
    // This new synthetic collection must be studyable after native import.
    collection.run(OpSpec::named("media review fixture policy", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
        ctx.scope().set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
        Ok(())
    }).unwrap();
    let kind = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        reader.ids_after(Table::Notetypes, None, 20).unwrap().into_iter()
            .map(|id| reader.notetype(meguri_model::ids::NotetypeId(id)).unwrap().unwrap())
            .find(|kind| kind.name == "Basic").expect("bootstrap native Basic").id
    };
    let deck = meguri_engine::decks::add(&collection, "Media Review Check".into()).unwrap().value.id;
    let assets = [(IMAGE, png(96, 64, &mut Rng::stream(418, Stream::Media, 0))), (AUDIO, tone())];
    {
        let media = source.media().unwrap();
        for (name, bytes) in &assets {
            assert_eq!(media.add(name, bytes).unwrap(), *name);
        }
    }
    let media_answer = format!(
        "A generated color pattern and a short 440 Hz tone.<br><img src=\"{IMAGE}\"><br>[sound:{AUDIO}]"
    );
    let fields = [
        ("What image and sound belong to this media check?", media_answer.clone()),
        ("Can two cards reuse the same picture and tone?", format!("Yes. {media_answer}")),
        ("How many sides does a triangle have?", "Three sides.".into()),
        ("What do we call water after it freezes?", "Ice.".into()),
    ];
    for (question, answer) in &fields {
        meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
            notetype_id: kind, fields: vec![(*question).into(), answer.clone()], tags: vec![],
            deck_id: deck, guid: None, update_add_defaults: false,
        }, OpSource::UserUndoable).unwrap();
    }
    assert!(source.derived().wait_idle(WAIT));
    let cancel = CancelToken::new();
    let output = source.write_collection_export(&collection, &cancel, &ProgressSink::new()).unwrap();
    output.validate_sources(&cancel).unwrap();
    let package = fs::read(&output.path).unwrap();
    drop(collection);
    source.close().unwrap(); // Closes the engine/media before joining derived workers.

    let destination = config(&root.path().join("imported"));
    let process = ProcessCore::new(destination.engine.clone()).unwrap();
    let inspection = entry::inspect_entry(
        process.engine(), &output.path, &destination.entry_inputs(), CancelToken::new(), &ProgressSink::new(),
    ).unwrap();
    assert!(inspection.is_initial());
    assert_eq!(inspection.facts().source.notes, 4);
    assert_eq!(inspection.facts().media_count, 2);
    assert_eq!(inspection.facts().source_sha256, <[u8; 32]>::from(Sha256::digest(&package)));
    let importer = process.clone();
    let events = process.engine().stream().subscribe();
    let (tx, rx) = mpsc::channel();
    let job = process.engine().start_job(JobKind::ColpkgImport, move |ctx| {
        tx.send(importer.import_initial(destination, inspection, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)).ok();
        Ok(())
    }).unwrap();
    let completed = rx.recv_timeout(WAIT).unwrap().unwrap();
    let imported = completed.host; // Retain ownership before checking postactivation outcome.
    loop {
        let event = events.recv_timeout(WAIT).unwrap();
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == job.id) { break; }
    }
    assert!(completed.outcome.unwrap().backup.is_none());
    assert!(matches!(imported.engine().state(), meguri_engine::LifecycleState::Ready { .. }));
    assert!(imported.derived().wait_idle(WAIT));
    {
        let c = imported.collection().unwrap();
        let reader = c.reader(ReaderRole::Query).unwrap();
        let cards = reader.cards_after(None, 10).unwrap();
        assert_eq!(cards.len(), 4);
        assert!(cards.iter().all(|card| card.card_type == CardType::New
            && card.queue == CardQueue::New && card.reps == 0 && card.lapses == 0 && card.deck_id == deck));
        assert!(reader.revlog_after(None, 10).unwrap().is_empty());
        assert_eq!(reader.config_value("fsrs").unwrap(), Some(ConfigValue::Bool(true)));
        assert_eq!(reader.config_value("loadBalancerEnabled").unwrap(), Some(ConfigValue::Bool(false)));
        assert_eq!(reader.deck(deck).unwrap().unwrap().human_name(), "Media Review Check");
        let notes = reader.notes_after(None, 10).unwrap();
        assert_eq!(notes.len(), 4);
        for (question, answer) in &fields {
            assert!(notes.iter().any(|note| note.notetype_id == kind
                && note.field_values().eq([*question, answer.as_str()])));
        }
    }
    {
        let media = imported.media().unwrap();
        let mut names: Vec<_> = fs::read_dir(media.store().media_dir()).unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, [IMAGE, AUDIO]);
        for (name, bytes) in &assets {
            assert_eq!(&fs::read(media.store().media_dir().join(name)).unwrap(), bytes);
        }
    }
    let StartResult::Started { session_id, head, .. } = imported.review().start(deck, None).unwrap() else {
        panic!("imported fixture review must start");
    };
    let card = head.items.first().expect("imported fixture has a new card to study");
    assert!(matches!(
        imported.review().answer(
            session_id, card.card_id, Rating::Easy, card.state_token, EpochMillis(NOW), 123,
        ).unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    imported.review().end(session_id).unwrap();
    {
        let c = imported.collection().unwrap();
        let reader = c.reader(ReaderRole::Query).unwrap();
        let reviews = reader.revlog_after(None, 10).unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].card_id, card.card_id);
        let cards = reader.cards_after(None, 10).unwrap();
        assert_eq!(cards.len(), 4);
        assert_eq!(cards.iter().map(|card| card.reps).sum::<u32>(), 1);
        assert_eq!(cards.iter().find(|row| row.id == card.card_id).unwrap().reps, 1);
        assert!(cards.iter().filter(|row| row.id != card.card_id).all(|row| row.reps == 0));
    }
    imported.close().unwrap();
    assert_eq!(fs::read(&output.path).unwrap(), package);

    // The MG-owned run directory deliberately receives only these public artifacts.
    if let Some(run) = std::env::var_os("MG_RUN_DIR") {
        let run = Path::new(&run);
        let manifest = serde_json::json!({
            "purpose": "public disposable native import; not CloudKit acceptance",
            "package": PACKAGE, "sha256": hash(&package), "bytes": package.len(), "clock_ms": NOW,
            "notes": 4, "cards": 4, "new_cards": 4, "reviews": 0,
            "media": assets.iter().map(|(name, bytes)| serde_json::json!({
                "name": name, "bytes": bytes.len(), "sha256": hash(bytes)
            })).collect::<Vec<_>>()
        });
        fs::OpenOptions::new().write(true).create_new(true).open(run.join(PACKAGE)).unwrap()
            .write_all(&package).unwrap();
        fs::OpenOptions::new().write(true).create_new(true).open(run.join(MANIFEST)).unwrap()
            .write_all(&serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }
}
