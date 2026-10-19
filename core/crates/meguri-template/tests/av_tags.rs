use meguri_template::av_tags::{AvSide, AvTag, extract};
#[test]
fn audio_is_extracted_in_script_attributes_and_side_order() {
    let output = extract(
        "<script>'[sound:a&amp;b.mp3]'</script><img alt='[sound:y]'>",
        AvSide::Question,
    );
    assert_eq!(
        output.text,
        "<script>'[anki:play:q:0]'</script><img alt='[anki:play:q:1]'>"
    );
    assert_eq!(
        output.av,
        vec![
            AvTag::Sound { name: "a&b.mp3".into() },
            AvTag::Sound { name: "y".into() }
        ]
    );
    let answer = extract("[anki:play:q:0][sound:z]", AvSide::Answer);
    assert_eq!(answer.text, "[anki:play:q:0][anki:play:a:0]");
    assert_eq!(answer.av.len(), 1);
}
#[test]
fn tts_is_opaque_keeps_indexes_and_requires_last_language() {
    let output = extract(
        "[anki:tts lang=en lang=ja]猫[sound:inside][/anki:tts][sound:next][anki:tts]bad[/anki:tts]",
        AvSide::Question,
    );
    assert_eq!(output.text, "[anki:play:q:0][anki:play:q:1][tts: lang not set]");
    assert_eq!(
        output.av[0],
        AvTag::Tts {
            language: "ja".into(),
            content: "猫[sound:inside]".into()
        }
    );
    assert_eq!(output.av.len(), 2);
}

#[test]
fn malformed_items_normal_form_and_entities_follow_the_whole_side_grammar() {
    for (input, expected) in [
        ("[sound:]] [sound:] [sound:a]b]", "[sound:]] [sound:] [anki:play:q:0]b]"),
        ("[anki:foo  a=1\n]x[/anki:foo]", "[anki:foo a=1]x[/anki:foo]"),
        (r#"[anki:foo a="x y" a=z]x[/anki:foo]"#, "[anki:foo a=z]x[/anki:foo]"),
        ("[anki:TTS lang=en]up[/anki:TTS]", "[anki:TTS lang=en]up[/anki:TTS]"),
        (
            "[anki:tts lang= ja][sound:x][/anki:tts]",
            "[anki:tts lang= ja][anki:play:q:0][/anki:tts]",
        ),
    ] {
        assert_eq!(extract(input, AvSide::Question).text, expected, "{input}");
    }
    let output = extract(
        "[sound:&nbsp;x][sound:a&b&c][anki:foo][sound:nested][/anki:foo]",
        AvSide::Question,
    );
    assert_eq!(
        output.av,
        vec![
            AvTag::Sound { name: " x".into() },
            AvTag::Sound { name: "a&b&c".into() }
        ]
    );
    assert!(output.text.ends_with("[anki:foo][sound:nested][/anki:foo]"));
}
#[test]
fn an_unclosed_quote_uses_the_unquoted_value_fallback() {
    assert_eq!(
        extract(r#"[anki:foo a="b=c]x[/anki:foo]"#, AvSide::Question).text,
        r#"[anki:foo a= "b=c]x[/anki:foo]"#
    );
}
#[test]
fn overlapping_close_names_still_find_the_exact_inner_close_text() {
    let output = extract("[anki:x][sound:z][/anki:foo[/anki:x]", AvSide::Question);
    assert!(output.av.is_empty());
    assert_eq!(output.text, "[anki:x][sound:z][/anki:foo[/anki:x]");
}
#[test]
fn many_unclosed_directives_of_distinct_names_preserve_the_text() {
    let mut text = String::new();
    for n in 0..20_000 {
        text.push_str(&format!("[anki:unclosed{n}]text"));
    }
    // An unrelated closer prevents the trivial no-closing-tags fast path.
    text.push_str("[/anki:other]");
    let output = extract(&text, AvSide::Question);
    assert_eq!(output.text, text);
    assert!(output.av.is_empty());
}
