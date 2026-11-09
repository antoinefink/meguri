use meguri_template::{
    av_tags::AvTag,
    display::{self, Platform, RenderHost},
};
#[test]
fn visible_marker_markup_is_platform_specific_and_keeps_literal_command_digits() {
    let input = "[anki:play:q:00][anki:play:x:1][anki:play:q:١]";
    for (platform, action) in [
        (Platform::Mac, "pycmd('play:q:00')"),
        (
            Platform::Ios,
            "window.webkit.messageHandlers.meguri.postMessage('play:q:00')",
        ),
    ] {
        let html = display::markers(
            input,
            &[],
            &[],
            RenderHost {
                platform,
                hide_play_buttons: false,
            },
        );
        assert!(html.contains(action));
        assert!(html.contains("<svg class=\"playImage\""));
        assert!(html.ends_with("[anki:play:x:1][anki:play:q:١]"));
        assert!(!html.contains('`'));
        assert!(!html.contains("${"));
        assert!(!html.starts_with('\n'));
    }
}
#[test]
fn tts_content_is_inserted_once_and_hidden_buttons_do_not_touch_lists() {
    let question = vec![
        AvTag::Tts {
            language: "ja".into(),
            content: "<b>猫</b>[anki:play:q:9]".into(),
        },
        AvTag::Sound { name: "a".into() },
    ];
    let host = RenderHost {
        platform: Platform::Ios,
        hide_play_buttons: true,
    };
    assert_eq!(
        display::markers("[anki:play:q:0][anki:play:q:1]", &question, &[], host),
        "<b>猫</b>[anki:play:q:9]"
    );
    assert_eq!(question.len(), 2);
    let mut visible = host;
    visible.hide_play_buttons = false;
    let html = display::markers("[anki:play:q:0][anki:play:q:1]", &question, &[], visible);
    assert!(html.starts_with("<b>猫</b>[anki:play:q:9]"));
    assert!(html.contains("postMessage('play:q:1')"));
}

#[test]
fn media_encoding_precedes_play_and_type_transforms_but_keeps_av_filenames_raw() {
    use meguri_template::{
        ParsedTemplate,
        card::{CardRender, Outcome},
        type_answer::{self, Field, WarningForm},
    };
    let card = CardRender {
        question: Some(
            "<img src=\"my file#1.jpg\"><img src=\"https://example.org/a b.png\">[anki:play:q:0][anki:play:q:1]".into(),
        ),
        answer: Some("[[type:F]]".into()),
        question_av: vec![
            AvTag::Sound {
                name: "voice clip.mp3".into(),
            },
            AvTag::Tts {
                language: "en".into(),
                content: "<img src=\"tts image#2.jpg\">[anki:play:q:9]".into(),
            },
        ],
        answer_av: vec![],
        outcome: Outcome::Rendered,
    };
    let sides = display::compose(
        &card,
        "<img src=\"css logo.png\">[[type:F]]",
        0,
        RenderHost {
            platform: Platform::Mac,
            hide_play_buttons: false,
        },
    );
    assert!(sides.question.contains("css%20logo.png"));
    assert!(sides.question.contains("my%20file%231.jpg"));
    assert!(sides.question.contains("https://example.org/a b.png"));
    assert!(sides.question.contains("tts%20image%232.jpg"));
    assert!(
        sides.question.contains("[anki:play:q:9]"),
        "inserted content is never rescanned for play markers"
    );
    let (question, descriptor) = type_answer::question(
        &sides.question,
        &ParsedTemplate::parse("{{type:F}}"),
        &[Field {
            name: "F",
            value: "<img src=\"raw image.jpg\">ka",
            font: "Arial",
            size: 20,
        }],
        1,
        WarningForm::Display,
        true,
    )
    .unwrap();
    assert!(question.contains("spellcheck=\"false\""));
    assert_eq!(descriptor.unwrap().expected, "<img src=\"raw image.jpg\">ka");
    assert_eq!(
        card.question_av[0],
        AvTag::Sound {
            name: "voice clip.mp3".into()
        }
    );
}

#[test]
fn generated_replay_controls_have_names_and_unique_deterministic_ids() {
    use meguri_template::{
        CardSpecials, FieldMap, ParsedTemplate,
        card::{self, RenderMode},
    };
    let fields = FieldMap::new(
        [("Front", "[sound:one][sound:two][anki:play:q:00][anki:play:q:00]")],
        &CardSpecials {
            ord: 0,
            notetype_name: "Synthetic",
            template_name: "Card",
            deck_name: None,
            flags: 0,
            card_id: 1,
            tags: &[],
        },
    );
    let authored = r#"<a ID='review.card.replay.q&#48;' class="replay-button soundLink" data-meguri-replay="true" aria-label="Authored" style="color:purple">own</a><i id=review.card.replay.a0></i>"#;
    let card = card::render(
        &ParsedTemplate::parse(&format!("{authored}{{{{Front}}}}")),
        &ParsedTemplate::parse("{{FrontSide}}{{FrontSide}}[sound:answer][anki:play:a:000]"),
        &fields,
        false,
        RenderMode::Review,
    );
    let comparable = card.clone();
    for platform in [Platform::Mac, Platform::Ios] {
        let host = RenderHost {
            platform,
            hide_play_buttons: false,
        };
        let output = display::compose(&card, ".card { color: navy; }", 0, host);
        assert_eq!(output, display::compose(&card, ".card { color: navy; }", 0, host));
        assert_eq!(
            output.question_replay_ids,
            (1..5).map(|n| format!("review.card.replay.q{n}")).collect::<Vec<_>>()
        );
        assert_eq!(
            output.answer_replay_ids,
            (1..11).map(|n| format!("review.card.replay.a{n}")).collect::<Vec<_>>()
        );
        for (html, ids, expected_commands) in [
            (
                &output.question,
                &output.question_replay_ids,
                vec!["play:q:0", "play:q:1", "play:q:00", "play:q:00"],
            ),
            (
                &output.answer,
                &output.answer_replay_ids,
                vec![
                    "play:q:0",
                    "play:q:1",
                    "play:q:00",
                    "play:q:00",
                    "play:q:0",
                    "play:q:1",
                    "play:q:00",
                    "play:q:00",
                    "play:a:0",
                    "play:a:000",
                ],
            ),
        ] {
            assert_eq!(html.matches("aria-label=\"Play audio\"").count(), ids.len());
            assert_eq!(
                html.matches("aria-hidden=\"true\" focusable=\"false\"").count(),
                ids.len()
            );
            let mut rest = html.as_str();
            for (id, command) in ids.iter().zip(expected_commands) {
                let attr = format!("id=\"{id}\" data-meguri-replay=\"true\" aria-label=\"Play audio\"");
                assert_eq!(html.matches(&attr).count(), 1);
                let (_, tail) = rest.split_once(&attr).expect("IDs in document order");
                let action = match platform {
                    Platform::Mac => format!("pycmd('{command}')"),
                    Platform::Ios => format!("window.webkit.messageHandlers.meguri.postMessage('{command}')"),
                };
                assert!(tail.starts_with(&format!(" href=# onclick=\"{action}; return false;\"")));
                rest = tail;
            }
            assert!(!html.contains('`'));
            assert!(!html.contains("${"));
        }
        assert!(output.question.contains(authored));
        assert_eq!(output.answer.matches(authored).count(), 2);
    }
    assert_eq!(
        card, comparable,
        "display must leave comparable output and AV lists intact"
    );
}

#[test]
fn generated_replay_accessibility_preserves_display_contract() {
    use meguri_template::card::{CardRender, Outcome};
    let authored = r#"<a id="review.card.replay.q0" class="replay-button soundLink" data-meguri-replay="true" aria-label="Own &amp; label" style="color:purple">own</a>"#;
    let tts = r#"<b id="review.card.replay.q1">猫</b>[anki:play:q:99]"#;
    let card = CardRender {
        question: Some(format!("{authored}[anki:play:q:0][anki:play:q:01]")),
        answer: Some("[anki:play:q:0]".into()),
        question_av: vec![
            AvTag::Tts {
                language: "ja".into(),
                content: tts.into(),
            },
            AvTag::Sound {
                name: "raw clip.mp3".into(),
            },
        ],
        answer_av: vec![],
        outcome: Outcome::Rendered,
    };
    let original = card.clone();
    let css = ".soundLink { color: purple; }";
    for platform in [Platform::Mac, Platform::Ios] {
        let output = display::compose(
            &card,
            css,
            0,
            RenderHost {
                platform,
                hide_play_buttons: false,
            },
        );
        assert_eq!(output.question_replay_ids, ["review.card.replay.q2"]);
        assert!(output.answer_replay_ids.is_empty());
        assert_eq!(output.answer, format!("<style>{css}</style>{tts}"));
        // Removing exactly the newly specified attributes recovers the old
        // original graphic and action byte-for-byte; no style/geometry update.
        let added = " id=\"review.card.replay.q2\" data-meguri-replay=\"true\" aria-label=\"Play audio\"";
        let legacy = output
            .question
            .replace(added, "")
            .replace(" aria-hidden=\"true\" focusable=\"false\"", "");
        let action = match platform {
            Platform::Mac => "pycmd('play:q:01')",
            Platform::Ios => "window.webkit.messageHandlers.meguri.postMessage('play:q:01')",
        };
        let control = format!(
            r##"<a class="replay-button soundLink" href=# onclick="{action}; return false;" draggable="false"><svg class="playImage" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>"##
        );
        assert_eq!(legacy, format!("<style>{css}</style>{authored}{tts}{control}"));
        let hidden = display::compose(
            &card,
            css,
            0,
            RenderHost {
                platform,
                hide_play_buttons: true,
            },
        );
        assert_eq!(hidden.question, format!("<style>{css}</style>{authored}{tts}"));
        assert!(hidden.question_replay_ids.is_empty());
        assert!(hidden.answer_replay_ids.is_empty());
    }
    assert_eq!(card, original);
}
