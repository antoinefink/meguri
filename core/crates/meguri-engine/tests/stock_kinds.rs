//! Meguri's stock note types (`specs/notetypes/stock-kinds.md`): their
//! structure as Anki has it (K1, K3, K4), what a new collection holds (K2),
//! "Add: <stock kind>" (K8), and Meguri's own template text and styling
//! checked against the rules of K5 to K7 (K9: text and CSS are never
//! compared with Anki's).

mod common;

use common::Harness;
use meguri_engine::notetypes::{self, StockKind, template_failures};
use meguri_engine::undo::names::ADD_NOTE_TYPE;
use meguri_engine::{EngineError, OpSpec};
use meguri_model::changes::OpSource;
use meguri_model::clock::Clock;
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::{Notetype, NotetypeTemplate};
use meguri_model::rng::SeededRandom;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, ReaderRole, StampMode};
use meguri_template::{NodeKind, ParsedTemplate};

/// K4: fields as (name, tag, protected), templates by name, and the card
/// requirements as (kind, field ordinals), kind 1 any and 2 all.
struct Expected {
    name: &'static str,
    stored: i32,
    cloze: bool,
    fields: &'static [(&'static str, Option<u32>, bool)],
    templates: &'static [&'static str],
    reqs: &'static [(i32, &'static [u32])],
}

const EXPECTED: [Expected; 6] = [
    Expected {
        name: "Basic",
        stored: 1,
        cloze: false,
        fields: &[("Front", None, false), ("Back", None, false)],
        templates: &["Card 1"],
        reqs: &[(1, &[0])],
    },
    Expected {
        name: "Basic (and reversed card)",
        stored: 2,
        cloze: false,
        fields: &[("Front", None, false), ("Back", None, false)],
        templates: &["Card 1", "Card 2"],
        reqs: &[(1, &[0]), (1, &[1])],
    },
    Expected {
        name: "Basic (optional reversed card)",
        stored: 3,
        cloze: false,
        fields: &[
            ("Front", None, false),
            ("Back", None, false),
            ("Add Reverse", None, false),
        ],
        templates: &["Card 1", "Card 2"],
        reqs: &[(1, &[0]), (2, &[1, 2])],
    },
    Expected {
        name: "Basic (type in the answer)",
        stored: 4,
        cloze: false,
        fields: &[("Front", None, false), ("Back", None, false)],
        templates: &["Card 1"],
        reqs: &[(1, &[0, 1])],
    },
    Expected {
        name: "Cloze",
        stored: 5,
        cloze: true,
        fields: &[("Text", Some(0), true), ("Back Extra", Some(1), false)],
        templates: &["Cloze"],
        reqs: &[(1, &[0])],
    },
    Expected {
        name: "Image Occlusion",
        stored: 6,
        cloze: true,
        fields: &[
            ("Occlusion", Some(0), true),
            ("Image", Some(1), true),
            ("Header", Some(2), true),
            ("Back Extra", Some(3), true),
            ("Comments", Some(4), false),
        ],
        templates: &["Image Occlusion"],
        reqs: &[(1, &[0, 1, 2])],
    },
];

fn read(h: &Harness, id: NotetypeId) -> Notetype {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(id)
        .unwrap()
        .expect("the note type exists")
}

/// K1, K3, K4 on a stored note type of `kind` named `name`.
fn assert_structure(notetype: &Notetype, expected: &Expected, name: &str) {
    assert_eq!(notetype.name, name);
    let config = &notetype.config;
    assert_eq!(config.kind, i32::from(expected.cloze), "{name}");
    assert_eq!(config.original_stock_kind, expected.stored, "{name}");
    assert_eq!(config.original_id, None);
    assert_eq!(config.sort_field_idx, 0);
    assert_eq!(config.latex_pre, notetypes::DEFAULT_LATEX_PRE);
    assert_eq!(config.latex_post, notetypes::DEFAULT_LATEX_POST);
    assert!(!config.latex_svg);
    assert_eq!(config.target_deck_id_unused, 0);
    assert!(config.other().is_empty());
    let fields: Vec<(&str, Option<u32>, bool)> = notetype
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.config.tag, f.config.prevent_deletion))
        .collect();
    assert_eq!(fields, expected.fields, "{name}");
    for field in &notetype.fields {
        let c = &field.config;
        assert_eq!((c.font_name.as_str(), c.font_size), ("Arial", 20));
        assert!(!(c.sticky || c.rtl || c.plain_text || c.collapsed || c.exclude_from_search));
        assert!(c.description.is_empty() && c.id.is_some());
    }
    let templates: Vec<&str> = notetype.templates.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(templates, expected.templates, "{name}");
    for template in &notetype.templates {
        let c = &template.config;
        assert!(c.q_format_browser.is_empty() && c.a_format_browser.is_empty());
        assert_eq!((c.target_deck_id, c.browser_font_size), (0, 0));
        assert!(c.id.is_some());
    }
    let reqs: Vec<(u32, i32, Vec<u32>)> = config
        .reqs
        .iter()
        .map(|r| (r.card_ord, r.kind, r.field_ords.clone()))
        .collect();
    let want: Vec<(u32, i32, Vec<u32>)> = expected
        .reqs
        .iter()
        .enumerate()
        .map(|(ord, (kind, fields))| (u32::try_from(ord).unwrap(), *kind, fields.to_vec()))
        .collect();
    assert_eq!(reqs, want, "{name}");
}

/// Removes every note type of the fixture, as before a new collection's
/// bootstrap.
fn no_notetypes(h: &Harness) {
    h.collection
        .run(
            OpSpec::named("test_clear_notetypes", OpSource::UserNonUndoable).skip_undo(),
            |ctx| {
                for id in notetypes::read::ids(ctx.scope())? {
                    ctx.scope().remove_notetype(id)?;
                }
                Ok(())
            },
        )
        .unwrap();
}

/// K2 in a scope that writes rows as given, as U28's bootstrap runs it.
fn bootstrap(h: &Harness, mode: StampMode) -> Result<Vec<NotetypeId>, EngineError> {
    h.collection
        .run(
            OpSpec::named("test_new_collection", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(mode),
            |ctx| {
                let (scope, random) = ctx.scope_and_random();
                notetypes::stock::add_to_new_collection(scope, random)
            },
        )
        .map(|r| r.value)
}

#[test]
fn a_new_collection_holds_the_six_kinds() {
    // K2 (R913): K1's order, ids now to now + 5, written as part of the new
    // collection (stamps 0), curModel Basic's id as a local change.
    let h = Harness::open();
    no_notetypes(&h);
    let now = h.clock.now_ms();
    let ids = bootstrap(&h, StampMode::CallerSupplied).unwrap();
    assert_eq!(ids, (0..6).map(|i| NotetypeId(now + i)).collect::<Vec<_>>());
    for ((id, expected), kind) in ids.iter().zip(&EXPECTED).zip(StockKind::ALL) {
        let notetype = read(&h, *id);
        assert_eq!(kind.name(), expected.name);
        assert_structure(&notetype, expected, expected.name);
        assert_eq!((notetype.mtime, notetype.usn), (EpochSecs(0), Usn(0)));
        assert!(
            notetype
                .templates
                .iter()
                .all(|t| (t.mtime, t.usn) == (EpochSecs(0), Usn(0)))
        );
    }
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let current = reader.config_entry("curModel").unwrap().unwrap().entry;
    assert_eq!(current.value, now.to_string().into_bytes());
    assert_eq!((current.usn, current.mtime.0), (Usn(-1), now / 1000));
}

#[test]
fn the_bootstrap_refuses_a_scope_that_stamps() {
    let h = Harness::open();
    no_notetypes(&h);
    assert!(matches!(
        bootstrap(&h, StampMode::Local),
        Err(EngineError::InvalidArgument { .. })
    ));
}

#[test]
fn a_stock_kind_added_later_is_a_local_add_under_a_free_name() {
    // K8 (R916): local stamps, N4 step 7, curModel set, no schema change.
    let h = Harness::open();
    no_notetypes(&h);
    bootstrap(&h, StampMode::CallerSupplied).unwrap();
    h.clock.step_ms(60_000);
    let scm = || {
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .col()
            .unwrap()
            .schema_modified
    };
    let schema = scm();
    for (kind, expected) in StockKind::ALL.into_iter().zip(&EXPECTED) {
        let added = notetypes::add_stock(&h.collection, kind, None).unwrap();
        assert_eq!(added.undo.map(|u| u.name), Some(ADD_NOTE_TYPE));
        let added = added.value;
        let name = format!("{}+", kind.name());
        assert_eq!(added.name, name);
        let notetype = read(&h, added.id);
        assert_structure(&notetype, expected, &name);
        assert_eq!((notetype.mtime.0, notetype.usn), (h.clock.now_ms() / 1000, Usn(-1)));
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let current = reader.config_entry("curModel").unwrap().unwrap().entry;
        assert_eq!(current.value, added.id.0.to_string().into_bytes());
    }
    let named = notetypes::add_stock(
        &h.collection,
        StockKind::BasicTyping,
        Some("Typing \"Drill\"".to_owned()),
    )
    .unwrap()
    .value;
    assert_eq!(named.name, "Typing Drill");
    assert_eq!(scm(), schema);
}

/// A stock kind's note type, outside any collection.
fn stock(kind: StockKind) -> Notetype {
    kind.notetype(&SeededRandom::new(1))
}

fn template<'n>(notetype: &'n Notetype, name: &str) -> &'n NotetypeTemplate {
    notetype
        .templates
        .iter()
        .find(|t| t.name == name)
        .expect("the template exists")
}

/// A side's replacements as (key, filters), in order, comments left out.
fn replacements(text: &str) -> Vec<(String, Vec<String>)> {
    ParsedTemplate::parse(text)
        .nodes()
        .iter()
        .filter_map(|node| match &node.kind {
            NodeKind::Replacement { key, filters } => Some((key.clone(), filters.clone())),
            _ => None,
        })
        .collect()
}

fn references(text: &str) -> Vec<String> {
    ParsedTemplate::parse(text)
        .field_references()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// R910: the back starts with `question`, then an `<hr id=answer>` line,
/// then `rest`.
fn assert_question_hr_then(back: &str, question: &str, rest: &str) {
    let q = back.find(question).expect("the question");
    let hr = back.find("<hr id=answer>").expect("the answer line");
    let r = back.rfind(rest).expect("the answer");
    assert!(q < hr && hr < r, "{back}");
    assert!(
        back.lines().any(|line| line.trim() == "<hr id=answer>"),
        "the answer line stands alone: {back}"
    );
}

#[test]
fn the_standard_kinds_reference_exactly_their_fields() {
    // K5.
    let basic = stock(StockKind::Basic);
    let card1 = template(&basic, "Card 1");
    assert_eq!(references(&card1.config.q_format), ["Front"]);
    assert_eq!(references(&card1.config.a_format), ["FrontSide", "Back"]);
    assert!(card1.config.a_format.trim_start().starts_with("{{FrontSide}}"));
    assert_question_hr_then(&card1.config.a_format, "{{FrontSide}}", "{{Back}}");

    let reversed = stock(StockKind::BasicAndReversed);
    let same = template(&reversed, "Card 1");
    assert_eq!(
        (&same.config.q_format, &same.config.a_format),
        (&card1.config.q_format, &card1.config.a_format)
    );
    let card2 = template(&reversed, "Card 2");
    assert_eq!(references(&card2.config.q_format), ["Back"]);
    assert_eq!(references(&card2.config.a_format), ["FrontSide", "Front"]);
    assert!(card2.config.a_format.trim_start().starts_with("{{FrontSide}}"));
    assert_question_hr_then(&card2.config.a_format, "{{FrontSide}}", "{{Front}}");

    let optional = stock(StockKind::BasicOptionalReversed);
    let card2 = template(&optional, "Card 2");
    let front = card2.config.q_format.trim();
    assert!(
        front.starts_with("{{#Add Reverse}}") && front.ends_with("{{/Add Reverse}}"),
        "{front}"
    );
    let parsed = ParsedTemplate::parse(front);
    let opens = parsed
        .nodes()
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Open { .. }))
        .count();
    assert_eq!(opens, 1, "one section holds the whole front");
    assert_eq!(references(front), ["Add Reverse", "Back"]);
    assert_eq!(references(&card2.config.a_format), ["FrontSide", "Front"]);

    let typing = stock(StockKind::BasicTyping);
    let card1 = template(&typing, "Card 1");
    assert_eq!(
        replacements(&card1.config.q_format),
        [
            ("Front".to_owned(), vec![]),
            ("Back".to_owned(), vec!["type".to_owned()])
        ]
    );
    let back = &card1.config.a_format;
    assert_eq!(
        replacements(back),
        [
            ("Front".to_owned(), vec![]),
            ("Back".to_owned(), vec!["type".to_owned()])
        ]
    );
    assert!(!back.contains("FrontSide"));
    assert_question_hr_then(back, "{{Front}}", "{{type:Back}}");
}

#[test]
fn the_cloze_kind_shows_its_cloze_on_both_sides_and_highlights_it() {
    // K5, K6 (R911).
    let cloze = stock(StockKind::Cloze);
    let card = template(&cloze, "Cloze");
    assert_eq!(
        replacements(&card.config.q_format),
        [("Text".to_owned(), vec!["cloze".to_owned()])]
    );
    let back = replacements(&card.config.a_format);
    assert_eq!(back[0], ("Text".to_owned(), vec!["cloze".to_owned()]));
    assert!(
        back[1..]
            .iter()
            .all(|(key, filters)| key == "Back Extra" && filters.is_empty())
    );
    assert!(references(&card.config.a_format).contains(&"Back Extra".to_owned()));
    let css = &cloze.config.css;
    assert!(css.contains(".cloze {"), "{css}");
    assert!(css.contains(".nightMode .cloze {"), "{css}");
}

#[test]
fn every_kind_uses_the_base_card_css() {
    // K6: `.card` in light and dark appearance.
    for kind in StockKind::ALL {
        let css = stock(kind).config.css.clone();
        assert!(css.starts_with(notetypes::BASE_CARD_CSS), "{kind:?}");
    }
    let base = notetypes::BASE_CARD_CSS;
    assert!(base.contains(".card {"));
    assert!(base.contains(".nightMode .card"));
}

#[test]
fn the_image_occlusion_templates_follow_the_runtime_contract() {
    // K7 (R912).
    let io = stock(StockKind::ImageOcclusion);
    let card = template(&io, "Image Occlusion");
    for (side, text) in [("front", &card.config.q_format), ("back", &card.config.a_format)] {
        // The header only when non-empty.
        assert!(text.contains("{{#Header}}") && text.contains("{{/Header}}"), "{side}");
        let header = &text[text.find("{{#Header}}").unwrap()..text.find("{{/Header}}").unwrap()];
        assert!(header.contains("{{Header}}"), "{side}");
        // A display: none element holding the cloze.
        let cloze = text.find("{{cloze:Occlusion}}").expect("the masks' cloze");
        let element = &text[text[..cloze].rfind('<').unwrap()..cloze];
        assert!(element.contains("display: none"), "{side}: {element}");
        // The container with the image and the canvas.
        let container = text.find("id=\"image-occlusion-container\"").expect("the container");
        let image = text.find("{{Image}}").expect("the image");
        let canvas = text.find("<canvas id=\"image-occlusion-canvas\"").expect("the canvas");
        let closed = container + text[container..].find("</div>").unwrap();
        assert!(container < image && image < canvas && canvas < closed, "{side}");
        // The setup call, with Meguri's message when it throws.
        let script = &text[text.find("<script>").unwrap()..text.find("</script>").unwrap()];
        assert!(script.contains("anki.imageOcclusion.setup()"), "{side}");
        assert!(script.contains("try") && script.contains("catch"), "{side}");
        assert!(script.contains("textContent = \""), "{side}: a message of Meguri's own");
        // Comments is on no template.
        assert!(!references(text).contains(&"Comments".to_owned()), "{side}");
    }
    let front = &card.config.q_format;
    assert!(!front.contains("id=\"toggle\""));
    assert!(!references(front).contains(&"Back Extra".to_owned()));
    let back = &card.config.a_format;
    let toggle = back.find("<button id=\"toggle\">").expect("the toggle button");
    let extra = back.find("{{#Back Extra}}").expect("Back Extra when non-empty");
    assert!(back.find("</script>").unwrap() < toggle && toggle < extra);
    assert!(back[extra..].contains("{{Back Extra}}") && back[extra..].contains("{{/Back Extra}}"));
    // The six mask properties, in light and dark appearance.
    let css = &io.config.css;
    let light = &css[css.find("#image-occlusion-canvas {").expect("light")..];
    let light = &light[..light.find('}').unwrap()];
    let dark = &css[css.find(".nightMode #image-occlusion-canvas {").expect("dark")..];
    let dark = &dark[..dark.find('}').unwrap()];
    for rules in [light, dark] {
        for state in ["inactive", "active", "highlight"] {
            assert!(rules.contains(&format!("--{state}-shape-color:")), "{state}");
            let border = rules
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("--{state}-shape-border:")))
                .expect("a border");
            let value = border.split(':').nth(1).unwrap().trim().trim_end_matches(';');
            let parts: Vec<&str> = value.split_whitespace().collect();
            assert_eq!(parts.len(), 2, "a width and a colour: {value}");
            assert!(parts[0].ends_with("px"), "{value}");
        }
    }
}

#[test]
fn every_stock_template_parses_and_passes_the_checks() {
    // K5: both sides parse and pass validation V3.
    for kind in StockKind::ALL {
        let notetype = stock(kind);
        for t in &notetype.templates {
            assert!(ParsedTemplate::parse(&t.config.q_format).structure_error().is_none());
            assert!(ParsedTemplate::parse(&t.config.a_format).structure_error().is_none());
        }
        assert_eq!(template_failures(&notetype), [], "{kind:?}");
    }
}

#[test]
fn stock_field_and_template_ids_are_fresh_per_note_type() {
    // R905: a fresh random 64-bit id for each field and template.
    let random = SeededRandom::new(9);
    let a = StockKind::Basic.notetype(&random);
    let b = StockKind::Basic.notetype(&random);
    let ids = |n: &Notetype| {
        n.fields
            .iter()
            .map(|f| f.config.id)
            .chain(n.templates.iter().map(|t| t.config.id))
            .collect::<Vec<_>>()
    };
    assert_ne!(ids(&a), ids(&b));
}
