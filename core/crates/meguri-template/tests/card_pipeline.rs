use meguri_template::{
    CardSpecials, FieldMap, ParsedTemplate,
    av_tags::AvTag,
    card::{self, BlankKind, Outcome, RenderMode},
};
fn fields(values: &[(&str, &str)], ord: u32) -> FieldMap {
    FieldMap::new(
        values.iter().copied(),
        &CardSpecials {
            ord,
            notetype_name: "Custom",
            template_name: "Card",
            deck_name: Some("Parent::Child"),
            flags: 9,
            card_id: 17,
            tags: &[],
        },
    )
}
#[test]
fn front_side_contains_question_markers_without_replaying_its_av() {
    let fields = fields(&[("Front", "one[sound:q1][sound:q2]"), ("Back", "two[sound:a]")], 0);
    let output = card::render(
        &ParsedTemplate::parse("{{Front}}"),
        &ParsedTemplate::parse("{{FrontSide}}<hr>{{Back}}"),
        &fields,
        false,
        RenderMode::Review,
    );
    assert_eq!(output.outcome, Outcome::Rendered);
    assert_eq!(output.question.as_deref(), Some("one[anki:play:q:0][anki:play:q:1]"));
    assert_eq!(
        output.answer.as_deref(),
        Some("one[anki:play:q:0][anki:play:q:1]<hr>two[anki:play:a:0]")
    );
    assert_eq!(output.question_av.len(), 2);
    assert_eq!(output.answer_av, vec![AvTag::Sound { name: "a".into() }]);
}
#[test]
fn blank_front_keeps_question_audio_and_hides_answer_errors_outside_browser() {
    let fields = fields(&[("Extra", "")], 0);
    let front = ParsedTemplate::parse("Listen [sound:x]{{#Extra}}{{Extra}}{{/Extra}}");
    let back = ParsedTemplate::parse("{{NoSuchField}}");
    let output = card::render(&front, &back, &fields, false, RenderMode::Review);
    assert_eq!(output.outcome, Outcome::Blank(BlankKind::EmptyFront));
    assert_eq!(output.question.as_deref(), Some("Listen [anki:play:q:0]"));
    assert_eq!(output.question_av.len(), 1);
    assert!(output.answer.is_none());
    assert!(matches!(
        card::render(&front, &back, &fields, false, RenderMode::Browser).outcome,
        Outcome::Error { .. }
    ));
}
#[test]
fn browser_keeps_audio_text_and_has_empty_front_side_but_cloze_blank_metadata() {
    let values = fields(&[("Front", "[sound:a]"), ("Text", "{{c2::other}}")], 0);
    let result = card::render(
        &ParsedTemplate::parse("{{Front}}"),
        &ParsedTemplate::parse("{{FrontSide}}tail"),
        &values,
        false,
        RenderMode::Browser,
    );
    assert_eq!(result.question.as_deref(), Some("[sound:a]"));
    assert_eq!(result.answer.as_deref(), Some("tail"));
    assert!(result.question_av.is_empty());
    let result = card::render(
        &ParsedTemplate::parse("{{cloze:Text}}"),
        &ParsedTemplate::parse("{{NoSuchField}}"),
        &values,
        true,
        RenderMode::Browser,
    );
    assert_eq!(result.outcome, Outcome::Blank(BlankKind::MissingCloze));
    assert!(result.answer.is_none());
}
