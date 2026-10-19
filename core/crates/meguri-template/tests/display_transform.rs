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
