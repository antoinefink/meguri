//! Retained normal-bidirectional's reviewed headless fixture acknowledgment.
use crate::{PrepareForwardStage, Result, fixture, need, output};
use meguri_engine::{Collection, sched::fsrs_policy::enable_fsrs};
use meguri_host::CoreHost;
use meguri_model::clock::Clock;
use serde_json::{Value, json};

pub fn prepare(host: &CoreHost, clock: &dyn Clock, stage: &mut PrepareForwardStage) -> Result<Value> {
    prepare_with(host, clock, stage, output, |collection| {
        // Exactly one public operation; propagate every refusal/error.
        enable_fsrs(collection, &Default::default())?;
        Ok(())
    })
}

// Private seam: tests can observe capture/order and inject a refusal, without
// a runtime selector or a substituted successful scheduler/transport.
fn prepare_with(host: &CoreHost, clock: &dyn Clock, stage: &mut PrepareForwardStage,
    mut capture: impl FnMut(&str, &Value) -> Result<String>,
    enable: impl FnOnce(&Collection) -> Result<()>) -> Result<Value> {
    *stage = PrepareForwardStage::FsrsBeforeSnapshot;
    let before = fixture::snapshot(host)?;
    let before_sha = capture("fsrs-before.json", &before)?;
    fixed_input(&before)?;
    let collection = host.collection()?;
    *stage = PrepareForwardStage::FsrsEnable;
    let started = clock.now_ms();
    enable(&collection)?;
    let finished = clock.now_ms();
    *stage = PrepareForwardStage::FsrsAfterSnapshot;
    let after = fixture::snapshot(host)?;
    let after_sha = capture("fsrs-after.json", &after)?;
    *stage = PrepareForwardStage::FsrsEffects;
    // E2 computes zero cards at this fixed point. The parent additionally
    // checks every table/media byte and the two allowed stamp boundaries on
    // these original captures before admitting any preparation authority.
    need(before["tables"]["cards"] == after["tables"]["cards"], "fsrs-card-preservation")?;
    need(before["tables"]["revlog"] == after["tables"]["revlog"], "fsrs-history-preservation")?;
    Ok(json!({"format":"meguri-retained-fsrs-prerequisite-v1", "case":"normal-bidirectional",
        "action":"prepare_forward", "acknowledgment":"reviewed_headless_fsrs_only_and_synchronized_effects",
        "operation":"enable_fsrs", "before_sha256":before_sha, "after_sha256":after_sha,
        "eligible_card_ids":[],
        "started_ms":started, "finished_ms":finished, "case_credit":false}))
}

fn fixed_input(before: &Value) -> Result<()> {
    let cards = before["tables"]["cards"]["rows"].as_array().ok_or("fsrs-cards")?;
    need(cards.len() == 3 && cards.iter().all(|row| row[6] == json!({"integer":"0"})),
        "fsrs-requires-three-new-cards")?;
    need(before["tables"]["revlog"]["rows"].as_array().ok_or("fsrs-history")?.is_empty(),
        "fsrs-requires-no-history")?;
    need(before["tables"]["config"]["rows"].as_array().ok_or("fsrs-config")?
        .iter().all(|row| row[0] != json!({"text_hex":"66737273"})), "fsrs-requires-absent-key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_host::HostConfig;
    use meguri_model::{clock::{StepClock, UtcOffset}, rng::SeededRandom};
    use meguri_storage::space::StatvfsSpace;
    use std::{cell::Cell, path::PathBuf, sync::{Arc, atomic::{AtomicU64, Ordering}}};

    struct HostFixture { host: Option<CoreHost>, directory: PathBuf, clock: Arc<StepClock> }
    impl HostFixture {
        fn new(populate: bool) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!("meguri-retained-fsrs-{}-{}",
                std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&directory).unwrap();
            let collection = directory.join("collection");
            std::fs::create_dir(&collection).unwrap();
            let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
            let random = Arc::new(SeededRandom::new(19));
            meguri_engine::bootstrap::create(&collection.join("collection.anki2"), clock.as_ref(), random.as_ref()).unwrap();
            let host = CoreHost::open(HostConfig::new(collection, directory.join("backups"),
                directory.join("widget"), clock.clone(), random, Arc::new(StatvfsSpace))).unwrap();
            if populate { fixture::initialize(&host, false).unwrap(); }
            Self { host: Some(host), directory, clock }
        }
    }
    impl Drop for HostFixture {
        fn drop(&mut self) {
            if let Some(host) = self.host.take() { host.close().unwrap(); }
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    #[test]
    fn captures_bracket_one_real_public_enable_before_forward_mutations() {
        let f = HostFixture::new(true);
        let calls = Cell::new(0);
        let captures = Cell::new(0);
        let mut stage = PrepareForwardStage::HostAdmission;
        let proof = prepare_with(f.host.as_ref().unwrap(), f.clock.as_ref(), &mut stage, |name, value| {
            let n = captures.get();
            assert_eq!(name, ["fsrs-before.json", "fsrs-after.json"][n]);
            assert_eq!(calls.get(), n);
            if n == 0 { fixed_input(value)?; }
            captures.set(n + 1);
            Ok(crate::sha(&serde_json::to_vec(value)?))
        }, |collection| {
            assert_eq!(captures.get(), 1);
            calls.set(calls.get() + 1);
            enable_fsrs(collection, &Default::default())?;
            Ok(())
        }).unwrap();
        assert_eq!((calls.get(), captures.get()), (1, 2));
        assert_eq!(proof["operation"], "enable_fsrs");
        assert_eq!(proof["eligible_card_ids"], json!([]));
        assert_eq!(proof["case_credit"], false);
    }

    #[test]
    fn failed_enable_stops_after_before_capture_without_authority_or_retry() {
        let f = HostFixture::new(true);
        let host = f.host.as_ref().unwrap();
        let original = fixture::snapshot(host).unwrap();
        let calls = Cell::new(0);
        let mut captures = Vec::new();
        let mut stage = PrepareForwardStage::HostAdmission;
        let result = prepare_with(host, f.clock.as_ref(), &mut stage, |name, _| {
            captures.push(name.to_owned());
            Ok("a".repeat(64))
        }, |_| {
            calls.set(calls.get() + 1);
            Err("injected operation refusal; no successful SDK substitute".into())
        });
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
        assert_eq!(captures, ["fsrs-before.json"]);
        assert!(matches!(stage, PrepareForwardStage::FsrsEnable));
        assert_eq!(fixture::snapshot(host).unwrap(), original);
    }

    #[test]
    fn empty_bootstrap_cannot_call_enable_or_supply_authority() {
        let f = HostFixture::new(false);
        let calls = Cell::new(0);
        let mut stage = PrepareForwardStage::HostAdmission;
        let result = prepare_with(f.host.as_ref().unwrap(), f.clock.as_ref(), &mut stage,
            |_, _| Ok("a".repeat(64)), |_| { calls.set(calls.get() + 1); Err("unexpected enable".into()) });
        assert!(result.is_err());
        assert_eq!(calls.get(), 0);
        assert!(matches!(stage, PrepareForwardStage::FsrsBeforeSnapshot));
    }

    #[test]
    fn fixed_input_rejects_nonnew_history_and_present_key_without_parsing_data() {
        let f = HostFixture::new(true);
        let original = fixture::snapshot(f.host.as_ref().unwrap()).unwrap();
        fixed_input(&original).unwrap();
        let mut changed = original.clone();
        changed["tables"]["cards"]["rows"][0][6] = json!({"integer":"2"});
        assert!(fixed_input(&changed).is_err());
        let mut changed = original.clone();
        changed["tables"]["revlog"]["rows"].as_array_mut().unwrap().push(json!([]));
        assert!(fixed_input(&changed).is_err());
        let mut changed = original.clone();
        changed["tables"]["config"]["rows"].as_array_mut().unwrap()
            .push(json!([{"text_hex":"66737273"}, {"integer":"0"}, {"integer":"0"}, {"blob_hex":"66616c7365"}]));
        assert!(fixed_input(&changed).is_err());
        let mut changed = original;
        changed["tables"]["cards"]["rows"][0][17] = json!({"text_hex":"6f7061717565"});
        fixed_input(&changed).unwrap();
    }
}
