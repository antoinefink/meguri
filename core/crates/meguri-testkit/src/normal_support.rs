//! Real hosted collection support for normal-sync integration tests. Every
//! round goes through the production JobRegistry entry and transport.
use meguri_engine::{Collection, OpSource, OpSpec};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{Clock, StepClock, UtcOffset},
    ids::{CardId, DeckId, NotetypeId},
    rng::SeededRandom,
};
pub use meguri_sched::answer::Rating;
use meguri_storage::{ReadRows, ReaderRole};
use std::sync::{Arc, Mutex};
pub const NOW: i64 = 1_894_701_600_000;
pub struct Fixture {
    pub host: CoreHost,
    pub clock: Arc<StepClock>,
    pub random: Arc<SeededRandom>,
    pub memory: Arc<Mutex<meguri_sync::StatusMemory>>,
    pub collection: Collection,
    pub notetype: NotetypeId,
    pub cards: Vec<CardId>,
    pub vfs: meguri_storage::counting_vfs::CountingVfs,
    pub dir: tempfile::TempDir,
}
impl Fixture {
    pub fn new(count: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let collection_dir = dir.path().join("collection");
        std::fs::create_dir(&collection_dir).unwrap();
        let path = collection_dir.join("collection.anki2");
        let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
        let random = Arc::new(SeededRandom::new(107));
        meguri_engine::bootstrap::create(&path, clock.as_ref(), random.as_ref()).unwrap();
        let vfs = meguri_storage::counting_vfs::CountingVfs::for_path(&path);
        let config = HostConfig::new(
            collection_dir,
            dir.path().join("backups"),
            dir.path().join("widget"),
            clock.clone(),
            random.clone(),
            Arc::new(meguri_storage::space::StatvfsSpace),
        );
        let host = CoreHost::open(config).unwrap();
        let collection = host.collection().unwrap();
        let notetype = meguri_engine::notetypes::add(
            &collection,
            meguri_engine::notetypes::NewNotetype {
                name: "Normal fixture".into(),
                kind: meguri_engine::notetypes::NotetypeKind::Standard,
                fields: vec!["Front".into(), "Back".into()],
                templates: vec![meguri_engine::notetypes::NewTemplate {
                    name: "Face".into(),
                    front: "{{Front}}".into(),
                    back: "{{FrontSide}}/{{Back}}".into(),
                    ..Default::default()
                }],
                css: None,
            },
        )
        .unwrap()
        .value
        .id;
        collection
            .run(OpSpec::named("normal fixture policy", OpSource::Maintenance), |ctx| {
                ctx.scope().set_config("fsrs", &true.into())?;
                ctx.scope().set_config("loadBalancerEnabled", &false.into())?;
                let mut preset = ctx.scope().deck_config(meguri_model::ids::DeckConfigId(1))?.unwrap();
                let mut config = (*preset.config).clone();
                config.new_per_day = 200000;
                config.reviews_per_day = 200000;
                preset.config = meguri_model::proto::ProtoBlob::new(config);
                ctx.scope().put(&preset)?;
                Ok(())
            })
            .unwrap();
        let mut cards = Vec::new();
        // Bounded construction avoids a fixture-sized image vector too.
        for start in (0..count).step_by(250) {
            let batch = (start..(start + 250).min(count))
                .map(|i| meguri_engine::notes::AddNote {
                    notetype_id: notetype,
                    fields: vec![format!("front {i}"), "back".into()],
                    tags: vec![],
                    deck_id: DeckId(1),
                    guid: None,
                    update_add_defaults: false,
                })
                .collect();
            for added in meguri_engine::notes::bulk_add(&collection, batch, OpSource::UserUndoable)
                .unwrap()
                .value
            {
                cards.extend(added.cards);
            }
        }
        collection.clear_undo(meguri_engine::ClearCause::CheckDatabase).unwrap();
        Self {
            host,
            clock,
            random,
            memory: Arc::new(Mutex::new(Default::default())),
            collection,
            notetype,
            cards,
            vfs,
            dir,
        }
    }
    pub fn start(&self, url: &str) -> meguri_sync::normal::Task {
        meguri_sync::normal::start(
            self.host.engine(),
            self.collection.clone(),
            meguri_sync::JobOptions::new(
                meguri_sync::Endpoint::parse(url).unwrap(),
                meguri_sync::ServerKind::SelfHosted,
            ),
            meguri_sync::HostKey::new("synthetic-normal-key"),
            self.clock.clone(),
            self.random.clone(),
            self.memory.clone(),
            None,
        )
        .unwrap()
    }
    pub fn stub(&self) -> crate::sync_stub::SyncStub {
        let schema = self
            .collection
            .reader(ReaderRole::Query)
            .unwrap()
            .col()
            .unwrap()
            .schema_modified
            .0;
        let clock = self.clock.clone();
        crate::sync_stub::SyncStub::serve(move |request| default_reply(request.path(), schema, clock.now_ms()))
    }
    pub fn answer(&self, card: CardId, session: Option<meguri_engine::SessionId>) {
        let prediction =
            meguri_engine::sched::answer::predict(&self.collection, card, &mut Default::default(), None).unwrap();
        let mut request = prediction.answer(Rating::Good, 3000);
        request.session = session;
        meguri_engine::sched::answer::answer(&self.collection, request).unwrap();
    }
    pub fn images(&self) -> Vec<(meguri_storage::Table, meguri_storage::RowImage)> {
        let r = self.collection.reader(ReaderRole::Query).unwrap();
        let snapshot = r.snapshot().unwrap();
        let mut out = Vec::new();
        for table in meguri_storage::Table::ALL {
            let mut after = None;
            loop {
                let page = snapshot.row_page(table, after.as_ref()).unwrap();
                if page.is_empty() {
                    break;
                }
                after = page
                    .last()
                    .map(|r| meguri_storage::normal_stage::NormalStage::row_key(table, r));
                out.extend(page.into_iter().map(|r| (table, r)));
            }
        }
        out
    }
}
pub fn default_reply(path: &str, schema: i64, now: i64) -> crate::sync_stub::Reply {
    use crate::sync_stub::Reply;
    match path.rsplit('/').next().unwrap() {
        "meta" => Reply::ok(
            format!(
                r#"{{"mod":{},"scm":{schema},"usn":5,"ts":{},"msg":"","cont":true,"empty":false,"media_usn":0}}"#,
                now - 60000,
                now / 1000
            )
            .as_bytes(),
        ),
        "start" => Reply::ok(br#"{"cards":[],"notes":[],"decks":[]}"#),
        "applyChanges" => Reply::ok(br#"{"models":[],"decks":[[],[]],"tags":[]}"#),
        "chunk" => Reply::ok(br#"{"done":true}"#),
        "sanityCheck2" => Reply::ok(br#"{"status":"ok"}"#),
        "finish" => Reply::ok(now.to_string().as_bytes()),
        _ => Reply::ok(b"null"),
    }
}
