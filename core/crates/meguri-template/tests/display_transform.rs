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
