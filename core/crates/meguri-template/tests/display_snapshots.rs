//! Product-contract HTML snapshots, never oracle goldens.
use meguri_template::{
    CardSpecials, FieldMap, ParsedTemplate,
    card::{self, RenderMode},
    display::{self, Platform, RenderHost},
};
#[test]
fn display_outcomes_on_every_host_and_button_setting() {
    let fields = FieldMap::new(
        [
            ("Front", "猫[sound:q1][sound:q2]"),
            ("Back", "answer[sound:a]"),
            ("Text", "{{c2::猫}}"),
            ("Blank", ""),
            ("Speech", "猫"),
        ],
        &CardSpecials {
            ord: 0,
            notetype_name: "Custom",
            template_name: "Card",
            deck_name: Some("Deck"),
            flags: 0,
            card_id: 1,
            tags: &[],
        },
    );
    let mut failures = Vec::new();
    for (name, front, back, cloze) in [
        ("sounds", "{{Front}}", "{{FrontSide}}<hr>{{Back}}", false),
        (
            "tts",
            "[anki:tts lang=ja_JP]{{Speech}}[/anki:tts][sound:after]",
            "{{FrontSide}}",
            false,
        ),
        (
            "literal_markers",
            "{{Front}}[anki:play:x:1][anki:play:q:١]",
            "{{Back}}",
            false,
        ),
        ("blank_front", "{{Blank}}", "{{NoSuchField}}", false),
        ("missing_cloze", "{{cloze:Text}}", "{{NoSuchField}}", true),
        ("front_error", "{{NoSuchField}}", "{{Back}}", false),
        ("back_error", "{{Front}}", "{{NoSuchField}}", false),
        ("escaped_error", "{{<x&'\">}}", "{{Back}}", false),
    ] {
        let card = card::render(
            &ParsedTemplate::parse(front),
            &ParsedTemplate::parse(back),
            &fields,
            cloze,
            RenderMode::Review,
        );
        let av_before = (card.question_av.clone(), card.answer_av.clone());
        for platform in [Platform::Mac, Platform::Ios] {
            for hide_play_buttons in [false, true] {
                let output = display::compose(
                    &card,
                    ".card { color: navy; }",
                    0,
                    RenderHost {
                        platform,
                        hide_play_buttons,
                    },
                );
                let text = format!("QUESTION\n{}\nANSWER\n{}", output.question, output.answer);
                let suffix = format!("{platform:?}-hidden-{hide_play_buttons}");
                // Collect every proposed baseline in one run. A missing or
                // changed snapshot still fails the test after the full matrix.
                if std::panic::catch_unwind(
                    || insta::with_settings!({snapshot_suffix=>suffix.clone()},{insta::assert_snapshot!(name,text);}),
                )
                .is_err()
                {
                    failures.push(format!("{name}/{suffix}"));
                }
            }
        }
        assert_eq!(
            (card.question_av, card.answer_av),
            av_before,
            "display never changes AV lists"
        );
    }
    assert!(
        failures.is_empty(),
        "display snapshots need review: {}",
        failures.join(", ")
    );
}
