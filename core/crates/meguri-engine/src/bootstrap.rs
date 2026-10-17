//! New collection defaults (specs/storage/new-collection.md B1-B8).
use crate::EngineError;
use meguri_model::{
    clock::Clock,
    col::Col,
    config::ConfigEntry,
    ids::DeckId,
    rng::RandomSource,
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use std::path::Path;

pub fn create(path: &Path, clock: &dyn Clock, random: &dyn RandomSource) -> Result<(), EngineError> {
    let now = clock.now_ms();
    let frozen = meguri_model::clock::FixedClock::new(now, clock.utc_offset_at(now));
    let timing = meguri_sched::timing::new_collection_timing(now.div_euclid(1000), clock);
    let col = Col {
        version: Col::SCHEMA_VERSION,
        created: EpochSecs(timing.crt),
        modified: EpochMillis(0),
        schema_modified: EpochMillis(now),
        last_sync: EpochMillis(0),
        usn: Usn(0),
        dirty: 0,
        legacy: Default::default(),
    };
    let mut config = Vec::new();
    for (key, value) in [
        ("activeDecks", "[1]"),
        ("addToCur", "true"),
        ("collapseTime", "1200"),
        ("curDeck", "1"),
        ("dayLearnFirst", "false"),
        ("dueCounts", "true"),
        ("estTimes", "true"),
        ("newSpread", "0"),
        ("nextPos", "1"),
        ("sched2021", "true"),
        ("schedVer", "2"),
        ("sortBackwards", "false"),
        ("sortType", "\"noteFld\""),
        ("timeLim", "0"),
    ] {
        config.push(ConfigEntry {
            key: key.into(),
            value: value.as_bytes().to_vec(),
            usn: Usn(0),
            mtime: EpochSecs(0),
        });
    }
    config.push(ConfigEntry {
        key: "creationOffset".into(),
        value: timing.creation_offset.to_string().into_bytes(),
        usn: Usn(0),
        mtime: EpochSecs(0),
    });
    meguri_storage::bootstrap::create(path, &col, &config, &frozen, |scope| {
        scope.put(&crate::deck_config::defaults())?;
        let mut deck = crate::decks::new_deck("Default".into());
        deck.id = DeckId(1);
        scope.put(&deck)?;
        crate::notetypes::stock::add_to_new_collection(scope, random)?;
        Ok(())
    })
}
