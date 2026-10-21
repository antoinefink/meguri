//! Production port harness; no E2 held-queue adapter.
#![allow(dead_code)]
use meguri_engine::{
    Collection, Engine, EngineConfig, OpSource, OpSpec, OpenConfig, Ports, SessionId,
    sched::queue::QueueProvider,
    session::{Head, ReviewService, StartResult},
};
use meguri_model::{
    clock::{StepClock, UtcOffset},
    ids::{CardId, DeckId},
    rng::SeededRandom,
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};
use std::{path::Path, sync::Arc};
pub const NOW: i64 = 1_894_701_600_000;
pub struct Harness {
    pub dir: tempfile::TempDir,
    pub engine: Engine,
    pub collection: Collection,
    pub review: Arc<ReviewService>,
    pub clock: Arc<StepClock>,
    pub cards: Vec<CardId>,
}
pub fn open(path: &Path, clock: Arc<StepClock>) -> (Engine, Collection, Arc<ReviewService>) {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let queues = Arc::new(QueueProvider::default());
    let review = ReviewService::new(&engine, queues.clone()).unwrap();
    let ports = Ports::new()
        .observer(queues.clone())
        .generation_listener(queues)
        .open_step(review.clone())
        .observer(review.clone())
        .generation_listener(review.clone())
        .suspend_participant(review.clone());
    let c = engine
        .open_raw(
            path,
            OpenConfig::new(clock, Arc::new(SeededRandom::new(43))).ports(ports),
        )
        .unwrap();
    (engine, c, review)
}
impl Harness {
    pub fn new(count: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
        meguri_engine::bootstrap::create(&path, clock.as_ref(), &SeededRandom::new(43)).unwrap();
        let (engine, c, review) = open(&path, clock.clone());
        let nt = meguri_engine::notetypes::add(
            &c,
            meguri_engine::notetypes::NewNotetype {
                name: "Production session".into(),
                kind: meguri_engine::notetypes::NotetypeKind::Standard,
                fields: vec!["Front".into(), "Back".into()],
                templates: vec![meguri_engine::notetypes::NewTemplate {
                    name: "Face".into(),
                    front: "{{Front}}<img src=\"study.png\">[sound:question.mp3]".into(),
                    back: "{{FrontSide}}/{{Back}}[sound:answer.mp3]".into(),
                    ..Default::default()
                }],
                css: Some(".card { color: black; }".into()),
            },
        )
        .unwrap()
        .value
        .id;
        c.run(OpSpec::named("session fixture policy", OpSource::Maintenance), |ctx| {
            ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
            ctx.scope()
                .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
            let mut preset = ctx.scope().deck_config(meguri_model::ids::DeckConfigId(1))?.unwrap();
            let mut fields = (*preset.config).clone();
            fields.new_per_day = 9999;
            fields.reviews_per_day = 9999;
            preset.config = meguri_model::proto::ProtoBlob::new(fields);
            ctx.scope().put(&preset)?;
            Ok(())
        })
        .unwrap();
        let mut cards = Vec::new();
        for i in 0..count {
            let note = meguri_engine::notes::add(
                &c,
                meguri_engine::notes::AddNote {
                    notetype_id: nt,
                    fields: vec![format!("front {i}"), "answer".into()],
                    tags: Vec::new(),
                    deck_id: DeckId(1),
                    guid: None,
                    update_add_defaults: false,
                },
                OpSource::UserUndoable,
            )
            .unwrap()
            .value;
            cards.push(c.reader(ReaderRole::Query).unwrap().cards_of_note(note.id).unwrap()[0].id);
        }
        c.clear_undo(meguri_engine::ClearCause::CheckDatabase).unwrap();
        Self {
            dir,
            engine,
            collection: c,
            review,
            clock,
            cards,
        }
    }
    pub fn start(&self) -> (SessionId, Head) {
        match self.review.start(DeckId(1), None).unwrap() {
            StartResult::Started { session_id, head, .. } => (session_id, head),
            other => panic!("start: {other:?}"),
        }
    }
    pub fn logs(&self) -> Vec<i64> {
        self.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .ids_after(meguri_storage::Table::Revlog, None, 100000)
            .unwrap()
    }
    pub fn settle(&self) {
        self.review.flush_for_background().unwrap();
    }
}
