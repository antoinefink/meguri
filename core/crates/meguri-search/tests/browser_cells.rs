//! C10 audio display is applied before H6; C7.4 uses complete deck names.
use meguri_search::browser::{
    deck_cell,
    qa::{self, Question},
};
use meguri_template::card::{CardRender, Outcome};
#[test]
fn question_uses_the_shared_av_grammar_and_decodes_entities_only_in_the_text_step() {
    for (html, expected) in [
        ("<b>猫</b><br>[sound:neko.mp3]", "猫 🔉neko.mp3🔉"),
        ("[sound:a&amp;lt;b.mp3]", "🔉a&lt;b.mp3🔉"),
        ("[anki:tts]猫[/anki:tts]", "💬猫💬"),
        ("[anki:tts lang=\"\"][/anki:tts]", "💬💬"),
        ("[anki:TTS lang=ja]猫[/anki:TTS]", "[anki:TTS lang=ja]猫[/anki:TTS]"),
        (
            "[anki:other  x=\"a b\" x=last]猫[/anki:other]",
            "[anki:other x=last]猫[/anki:other]",
        ),
        ("<div>犬</div><img src=\"inu.jpg\">[[type:Back]]", "犬 inu.jpg"),
    ] {
        let rendered = CardRender {
            question: Some(html.into()),
            answer: Some(String::new()),
            question_av: vec![],
            answer_av: vec![],
            outcome: Outcome::Rendered,
        };
        assert_eq!(qa::question(&rendered), Question::Text(expected.into()));
    }
}
#[test]
fn cards_deck_cell_has_no_elision_or_state_dependent_home_filter() {
    assert_eq!(deck_cell::cards("Japanese::Mining", None), "Japanese::Mining");
    assert_eq!(
        deck_cell::cards("Filtered 1", Some("Japanese::Mining")),
        "Filtered 1 (Japanese::Mining)"
    );
    assert_eq!(
        deck_cell::cards("Ordinary::Current", Some("Home::Full")),
        "Ordinary::Current (Home::Full)"
    );
}
