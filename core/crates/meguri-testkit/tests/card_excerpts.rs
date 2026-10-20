//! Compare all eight excerpt scenarios with their recorded goldens.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario::{self, Scenario},
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}
fn play(name: &str) -> Value {
    let id = format!("browser/columns/question-deck/{name}");
    let loaded = scenario::load(&root().join(format!("scenarios/{id}.yaml"))).unwrap();
    let golden = parse_json(&std::fs::read_to_string(root().join(format!("goldens/{id}.json"))).unwrap()).unwrap();
    assert_eq!(
        golden["scenario_sha256"].as_str(),
        Some(loaded.sha256.as_str()),
        "{id}: stale golden"
    );
    let actual = MeguriExecutor::new().run(&loaded).unwrap();
    let comparison = diff::compare(&id, &golden["observations"], &actual, &CompareOptions::default());
    assert!(comparison.differences.is_empty(), "{id}: {:#?}", comparison.differences);
    actual
}

#[test]
fn browser_format() {
    play("browser-format");
}
#[test]
fn cloze() {
    play("cloze");
}
#[test]
fn deck() {
    play("deck");
}
#[test]
fn media_line() {
    play("media-line");
}
#[test]
fn read_only() {
    play("read-only");
}
#[test]
fn neutral_damaged_notetype() {
    let result = play("damaged-notetype");
    assert_eq!(
        result["steps"][2]["card_excerpts"],
        json!([
        {"error":"damaged"},{"error":"damaged"},{"question":"Q 101","deck":"ExcerptFixtureDeck"}])
    );
}
#[test]
fn neutral_damaged() {
    let result = play("damaged");
    assert_eq!(
        result["steps"][3]["card_excerpts"],
        json!([
        {"error":"damaged"},{"question":"Q 101","deck":"ExcerptFixtureDeck"},{"error":"damaged"},{"error":"damaged"}])
    );
}
#[test]
fn neutral_missing() {
    let result = play("missing");
    assert_eq!(
        result["steps"][1]["card_excerpts"],
        json!([
        {"question":"Q 102","deck":"Default"},{"question":"Q 101","deck":"Default"},{"error":"missing"}])
    );
}
fn input(observe: Value) -> Result<Scenario, scenario::FormatError> {
    Scenario::from_value(
        &json!({"id":"browser/excerpt-validation","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"observe":observe}]}),
    )
}
#[test]
fn request_validation_keeps_duplicates_but_rejects_empty_text_notes_and_repeated_requests() {
    let scenario = input(json!([{"card_excerpts":["#c2","#c1","#c2"]}])).unwrap();
    assert!(scenario.needs().observations.contains("card_excerpts"));
    assert!(scenario.needs().setup.contains("new_collection"));
    for invalid in [
        json!([{"card_excerpts":[]}]),
        json!(["card_excerpts(#c1)"]),
        json!(["card_excerpts"]),
        json!([{"card_excerpts":["#n1"]}]),
        json!([{"card_excerpts":{}}]),
        json!([{"card_excerpts":null}]),
        json!([{"card_excerpts":["#c1"]},{"card_excerpts":["#c1"]}]),
    ] {
        assert!(input(invalid).is_err());
    }
}

// Mutate only in-memory copies of neutral scenarios. These assert product
// failure contracts, and never create oracle expectations or recordings.
fn changed(name: &str, edit: impl FnOnce(&mut Value)) -> scenario::LoadedScenario {
    let mut loaded =
        scenario::load(&root().join(format!("scenarios/browser/columns/question-deck/{name}.yaml"))).unwrap();
    edit(&mut loaded.value);
    loaded.scenario = Scenario::from_value(&loaded.value).unwrap();
    loaded.sha256 = scenario::scenario_sha256(&loaded.value).unwrap();
    loaded
}
#[test]
fn repeated_references_remain_in_order_in_one_product_call() {
    let loaded = changed("missing", |value| {
        value["steps"][1]["observe"][0]["card_excerpts"] = json!(["#c2", "#c3", "#c2", "#c1"]);
    });
    let result = MeguriExecutor::new().run(&loaded).unwrap();
    assert_eq!(
        result["steps"][1]["card_excerpts"],
        json!([
        {"question":"Q 102","deck":"Default"},{"error":"missing"},
        {"question":"Q 102","deck":"Default"},{"question":"Q 101","deck":"Default"}])
    );
}
#[test]
fn unresolved_reference_is_a_run_error() {
    let loaded = changed("cloze", |value| {
        value["steps"][0]["observe"][0]["card_excerpts"] = json!(["#c99"]);
    });
    assert_eq!(MeguriExecutor::new().run(&loaded).unwrap_err().kind, "unknown_ref");
}
#[test]
fn missing_cloze_and_template_errors_are_unrecordable_typed_outcomes() {
    let blank = changed("cloze", |value| {
        value["steps"] = json!([
            {"db_exec":{"sql":"UPDATE cards SET ord=9"}},
            {"observe":[{"card_excerpts":["#c1"]}]}]);
    });
    let error = MeguriExecutor::new().run(&blank).unwrap_err();
    assert_eq!(error.kind, "unsupported_observation");
    assert!(error.detail.contains("MissingCloze"));
    for side in ["browser_front", "browser_back"] {
        let template = changed("browser-format", |value| {
            value["steps"] = json!([
                {"update_notetype":{"notetype":"Formats","edits":[{"template":{"name":"Browser Front"}}]}},
                {"observe":[{"card_excerpts":["#c1"]}]}]);
            value["steps"][0]["update_notetype"]["edits"][0]["template"][side] = json!("{{Absent}}");
        });
        let error = MeguriExecutor::new().run(&template).unwrap_err();
        assert_eq!(error.kind, "unsupported_observation");
        assert!(error.detail.contains("TemplateError"));
    }
}
#[test]
fn decoding_fault_aborts_the_observation_instead_of_recording_damaged_or_partial_results() {
    let loaded = changed("missing", |value| {
        value["steps"] = json!([
            {"db_exec":{"sql":"UPDATE cards SET queue=99 WHERE nid=(SELECT id FROM notes ORDER BY id LIMIT 1 OFFSET 1)"}},
            {"observe":[{"card_excerpts":["#c1","#c2"]}]}]);
    });
    let error = MeguriExecutor::new().run(&loaded).unwrap_err();
    assert_eq!(error.kind, "core_error");
    assert_eq!(error.step, Some(1));
}
