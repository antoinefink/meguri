//! The engine against the oracle goldens of the two render scenarios that
//! Meguri's executor cannot run yet: `render/template/legacy-req` needs the
//! `legacy_update` and `legacy_json` adapters and the note type save (U29,
//! U36), `render/special-fields/cards` needs notes, cards and decks in the
//! collection (U27, U28, U29) and the cloze blank check (U61). Their engine
//! part is checked here; T0 runs them once those units land. Inputs come from
//! the scenarios and goldens, expected values from the goldens only.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_template::{CardSpecials, FieldMap, ParsedTemplate, Side, TemplateError, legacy_requirement};
use serde_json::{Value, json};

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn read(rel: &str) -> String {
    let path = conformance_dir().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A scenario by id, parsed (YAML 1.2 core schema), after checking that the
/// golden was recorded from it as it is now.
fn scenario(id: &str, golden: &Value) -> Value {
    let scenario = meguri_testkit::scenario::parse_yaml(&read(&format!("scenarios/{id}.yaml"))).unwrap();
    let sha = meguri_testkit::scenario::scenario_sha256(&scenario).unwrap();
    assert_eq!(golden["scenario_sha256"], sha.as_str(), "{id}: the golden is stale");
    scenario
}

fn golden(id: &str) -> Value {
    serde_json::from_str(&read(&format!("goldens/{id}.json"))).unwrap()
}

/// The golden's observations of the first step that observes `kind`.
fn observed<'a>(golden: &'a Value, kind: &str) -> &'a Value {
    golden["observations"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|step| step.get(kind))
        .unwrap_or_else(|| panic!("no {kind} observation"))
}

// template-syntax.md T11.3: the req Anki stores for every template of the
// probe note type, against the requirement computed from its question.
#[test]
fn legacy_requirements_match_the_oracle() {
    let golden = golden("render/template/legacy-req");
    let notetypes = observed(&golden, "legacy_json")["notetypes"].as_object().unwrap();
    assert!(!notetypes.is_empty());
    for (name, notetype) in notetypes {
        let mut fields: Vec<(u64, &str)> = notetype["flds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["ord"].as_u64().unwrap(), f["name"].as_str().unwrap()))
            .collect();
        fields.sort_unstable();
        let names: Vec<&str> = fields.iter().map(|(_, name)| *name).collect();
        let reqs = notetype["req"].as_array().unwrap();
        let templates = notetype["tmpls"].as_array().unwrap();
        assert_eq!(reqs.len(), templates.len(), "{name}: one req per template");
        for template in templates {
            let ord = template["ord"].as_u64().unwrap();
            let question = template["qfmt"].as_str().unwrap();
            let expected = reqs
                .iter()
                .find(|req| req[0] == ord)
                .unwrap_or_else(|| panic!("{name}: no req for template {ord}"));
            let req = legacy_requirement(&ParsedTemplate::parse(question), &names);
            assert_eq!(&json!([ord, req.kind.name(), req.ords]), expected, "{name} {question}");
        }
    }
}

/// A card of `render/special-fields/cards` as its db_exec steps leave it,
/// worked out by hand from the setup and the SQL. Cards are numbered in the
/// order they were added: the two cards of each Vocab note, then the cloze
/// note's single card, cloze 2.
struct Card {
    name: &'static str,
    /// Index into `setup.notes`.
    note: usize,
    ord: u32,
    /// The deck special-fields.md P3 names: `odid`'s when it is not 0, else
    /// `did`'s; `None` when that deck does not exist.
    deck: Option<&'static str>,
    flags: i64,
    /// Template-syntax.md T10 for a cloze card, whose check is U61's.
    cloze_blank: Option<bool>,
}

const VOCAB: Option<&str> = Some("Lang::Japanese::Vocab");

#[rustfmt::skip]
const CARDS: [Card; 5] = [
    Card { name: "#c1", note: 0, ord: 0, deck: VOCAB, flags: 0, cloze_blank: None },
    // flags = 3.
    Card { name: "#c2", note: 0, ord: 1, deck: VOCAB, flags: 3, cloze_blank: None },
    // flags = 7, odid = its own deck (Other), did = #c1's deck: the original
    // deck shows.
    Card { name: "#c3", note: 1, ord: 0, deck: Some("Other"), flags: 7, cloze_blank: None },
    // flags = 9, did = a deck id that does not exist.
    Card { name: "#c4", note: 1, ord: 1, deck: None, flags: 9, cloze_blank: None },
    // odid = a deck id that does not exist. The note holds c2, the card's
    // ordinal + 1, so it is not blank.
    Card { name: "#c5", note: 2, ord: 1, deck: None, flags: 0, cloze_blank: Some(false) },
];

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| list.iter().map(|v| v.as_str().unwrap().to_owned()).collect())
        .unwrap_or_default()
}

/// render-ops.md Q1.5: an error result.
fn failed(error: &TemplateError, side: &str) -> Value {
    json!({"error": {"kind": error.kind.name(), "side": side}})
}

// special-fields.md P2-P5 on stored cards: render-ops.md Q1.3's field map,
// evaluated as Q1.4 says, for every case of the scenario.
#[test]
fn stored_card_specials_match_the_oracle() {
    let id = "render/special-fields/cards";
    let golden = golden(id);
    let scenario = scenario(id, &golden);
    let ids = observed(&golden, "ids");
    let setup = &scenario["setup"];
    let notes = setup["notes"].as_array().unwrap();
    let notetypes = setup["notetypes"].as_array().unwrap();
    let cases = scenario["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|step| step.get("observe"))
        .flat_map(|kinds| kinds.as_array().unwrap())
        .find_map(|kind| kind.get("template"))
        .expect("a template observation")["cases"]
        .as_array()
        .unwrap();
    let expected = observed(&golden, "template").as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        let card_ref = case["card"].as_str().unwrap();
        let card = CARDS
            .iter()
            .find(|card| card.name == card_ref)
            .unwrap_or_else(|| panic!("no card {card_ref}"));
        let note = &notes[card.note];
        let notetype = notetypes.iter().find(|nt| nt["name"] == note["notetype"]).unwrap();
        let cloze = notetype["kind"] == "cloze";
        let templates = notetype["templates"].as_array().unwrap();
        let template = &templates[if cloze { 0 } else { card.ord as usize }];
        let field_names = strings(&notetype["fields"]);
        let field_values = strings(&note["fields"]);
        // Stored tags are in canonical order, which these already are.
        let tags = strings(&note["tags"]);
        let specials = CardSpecials {
            ord: card.ord,
            notetype_name: notetype["name"].as_str().unwrap(),
            template_name: template["name"].as_str().unwrap(),
            deck_name: card.deck,
            flags: card.flags,
            card_id: ids[card_ref].as_i64().unwrap(),
            tags: &tags,
        };
        let values = field_names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), field_values.get(i).map_or("", String::as_str)));
        let fields = FieldMap::new(values, &specials);
        let front = ParsedTemplate::parse(case["front"].as_str().unwrap());
        let back = ParsedTemplate::parse(case["back"].as_str().unwrap_or(""));
        let result = match front.render(&fields, Side::Question) {
            Err(error) => failed(&error, "question"),
            Ok(question) => {
                let blank = card.cloze_blank.unwrap_or_else(|| front.is_blank_question(&fields));
                if blank {
                    json!({"question": question, "answer": null, "empty": true})
                } else {
                    match back.render(&fields, Side::Answer { front_side: &question }) {
                        Ok(answer) => json!({"question": question, "answer": answer, "empty": false}),
                        Err(error) => failed(&error, "answer"),
                    }
                }
            }
        };
        assert_eq!(&result, expected, "{case}");
    }
}
