//! Properties of the text primitives over the committed corpus and over
//! seeded generated text. A failing case prints its seed and input.

mod common;

use std::borrow::Cow;
use std::cmp::Ordering;

use common::{Rng, awkward_text, corpus, corpus_strings};
use meguri_text::media_refs::{MediaTag, is_remote, media_tags, sound_tags};
use meguri_text::{base91, field_empty, fold, html, iri, media_name, media_refs, normalize};

const SEED: u64 = 0x6d65_6775_7269_0020;

/// Overlapping occurrences of `needle` in `haystack`.
fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack
        .char_indices()
        .filter(|&(i, _)| haystack[i..].starts_with(needle))
        .count()
}

/// `%` followed by two hex digits.
fn has_percent_escape(name: &str) -> bool {
    name.as_bytes()
        .windows(3)
        .any(|w| w[0] == b'%' && w[1].is_ascii_hexdigit() && w[2].is_ascii_hexdigit())
}

/// media-references.md M8: the four conditions under which decoding the
/// encoding gives the text back.
fn meets_m8(text: &str) -> bool {
    let encoded = iri::encode_for_display(text);
    let before: Vec<MediaTag<'_>> = media_tags(text).collect();
    let after: Vec<MediaTag<'_>> = media_tags(&encoded).collect();
    // Condition 4: as many matches, the n-th naming the n-th's encoding.
    if before.len() != after.len() {
        return false;
    }
    for (old, new) in before.iter().zip(&after) {
        let remote = is_remote(old.raw_name);
        let encoded_name = if remote {
            Cow::Borrowed(old.raw_name)
        } else {
            iri::encode_name(old.raw_name)
        };
        if new.raw_name != encoded_name {
            return false;
        }
        if remote {
            continue;
        }
        // Conditions 1 to 3, for every local match.
        if has_percent_escape(old.raw_name) || occurrences(old.matched, old.raw_name) != 1 {
            return false;
        }
        let at_own_place = usize::from(encoded_name == old.raw_name);
        if occurrences(old.matched, &encoded_name) != at_own_place {
            return false;
        }
    }
    true
}

/// Media-heavy text: tags of every kind and quoting around names with
/// characters the encoding touches, mixed with awkward pieces.
fn media_text(rng: &mut Rng) -> String {
    const ELEMENTS: &[&str] = &["img", "IMG", "audio", "video", "source", "object", "imgx", "picture"];
    const ATTRIBUTES: &[&str] = &["src=", "data=", "data-src=", "SRC=", "srcset=", "src ="];
    const NAMES: &[&str] = &[
        "a b.png",
        "c#d.jpg",
        "e\"f.mp3",
        "g'h.ogg",
        "i<j>.png",
        "k`l",
        "日本 語.png",
        "m%20n.png",
        "100%.jpg",
        "o\tp",
        "https://e.org/q r.png",
        "HTTP://x/y z",
        "&amp; s.png",
        "t",
        " u.png",
        "v.png/",
        "[sound:w.mp3]",
        "x>y.png",
    ];
    const QUOTES: &[&str] = &["\"", "'", ""];
    const EXTRAS: &[&str] = &[
        "",
        " alt=\"a b\"",
        " title='t'",
        " class=c",
        " alt=\"q>r\"",
        " data-x=1",
    ];
    let mut text = String::new();
    for _ in 0..rng.below(5) {
        match rng.below(3) {
            0 => text.push_str(&awkward_text(rng, 4)),
            _ => {
                let quote = rng.pick(QUOTES);
                text.push_str(&format!(
                    "<{}{} {}{quote}{}{quote}{}>",
                    rng.pick(ELEMENTS),
                    rng.pick(EXTRAS),
                    rng.pick(ATTRIBUTES),
                    rng.pick(NAMES),
                    rng.pick(EXTRAS),
                ));
            }
        }
    }
    text
}

#[test]
fn decoding_the_encoding_gives_the_text_back_under_m8() {
    let mut rng = Rng::new(SEED);
    let mut texts = corpus_strings("iri_encode");
    texts.extend((0..30_000).map(|_| media_text(&mut rng)));
    let (mut held, mut renamed) = (0, 0);
    for text in &texts {
        if !meets_m8(text) {
            continue;
        }
        let encoded = iri::encode_for_display(text);
        assert_eq!(
            iri::decode_for_saving(&encoded),
            text.as_str(),
            "text {text:?}, encoded {encoded:?}"
        );
        held += 1;
        renamed += usize::from(encoded != text.as_str());
    }
    // The property must not hold vacuously.
    assert!(
        held >= 20_000 && renamed >= 5_000,
        "{held} texts met M8, {renamed} of them renamed"
    );
}

#[test]
fn texts_without_media_tags_are_never_rewritten() {
    let mut rng = Rng::new(SEED ^ 1);
    for _ in 0..20_000 {
        let text = awkward_text(&mut rng, 24);
        if media_tags(&text).next().is_some() {
            continue;
        }
        assert!(matches!(iri::encode_for_display(&text), Cow::Borrowed(_)), "{text:?}");
        assert!(matches!(iri::decode_for_saving(&text), Cow::Borrowed(_)), "{text:?}");
        assert_eq!(
            html::strip_markup_keep_media(&text),
            html::strip_markup(&text),
            "{text:?}"
        );
    }
}

#[test]
fn markup_removal_leaves_no_closed_tag() {
    let mut rng = Rng::new(SEED ^ 2);
    let mut texts = corpus_strings("strip_markup");
    texts.extend((0..30_000).map(|_| awkward_text(&mut rng, 30)));
    for text in &texts {
        // A `<` that stays has no `>` anywhere after it.
        let removed = html::remove_markup(text);
        if let Some(open) = removed.find('<') {
            assert!(!removed[open..].contains('>'), "{text:?} -> {removed:?}");
        }
        let line = html::text_line(text);
        assert_eq!(line.trim(), line, "{text:?}");
        let line = html::text_line_keep_media(text);
        assert_eq!(line.trim(), line, "{text:?}");
    }
}

#[test]
fn references_are_the_decoded_tag_names() {
    let mut rng = Rng::new(SEED ^ 3);
    for _ in 0..20_000 {
        let text = media_text(&mut rng);
        let raw: Vec<&str> = media_tags(&text)
            .map(|t| t.raw_name)
            .chain(sound_tags(&text).map(|s| s.raw_name))
            .collect();
        let decoded = media_refs::references(&text);
        assert_eq!(decoded.len(), raw.len(), "{text:?}");
        for (name, reference) in raw.iter().zip(&decoded) {
            assert_eq!(meguri_text::entities::decode_text(name), reference.as_str(), "{text:?}");
        }
        let local = media_refs::local_references(&text);
        assert_eq!(
            local.len(),
            raw.iter().filter(|name| !is_remote(name)).count(),
            "{text:?}"
        );
        for tag in media_tags(&text) {
            assert!(tag.matched.starts_with('<') && tag.matched.ends_with('>'), "{text:?}");
            assert_eq!(&text[tag.start..tag.start + tag.matched.len()], tag.matched);
        }
    }
}

#[test]
fn spacer_only_fields_are_empty_and_content_is_not() {
    const SPACERS: &[&str] = &[
        " ", "\t", "\n", "\r", "\u{b}", "\u{c}", "<br>", "<BR>", "<br/>", "<br />", "</br>", "</br />", "<Br >",
        "<div>", "</div>", "<DIV/>", "<div />", "</DiV >",
    ];
    const CONTENT: &[&str] = &[
        "x",
        "&nbsp;",
        "\u{a0}",
        "\u{3000}",
        "<p>",
        "<span>",
        "<br  >",
        "<br\t>",
        "< br>",
        "<br/ >",
        "<!-- -->",
        "<",
        ">",
        "0",
        "<brr>",
        "<div class=x>",
    ];
    let mut rng = Rng::new(SEED ^ 4);
    for _ in 0..20_000 {
        let pieces: Vec<&str> = (0..rng.below(12)).map(|_| *rng.pick(SPACERS)).collect();
        let empty = pieces.concat();
        assert!(field_empty::counts_as_empty(&empty), "{empty:?}");
        let at = rng.below(pieces.len() + 1);
        let content = [
            pieces[..at].concat(),
            (*rng.pick(CONTENT)).to_owned(),
            pieces[at..].concat(),
        ]
        .concat();
        assert!(!field_empty::counts_as_empty(&content), "{content:?}");
    }
}

#[test]
fn media_names_are_short_safe_and_stable() {
    let mut rng = Rng::new(SEED ^ 5);
    let mut names = corpus_strings("media_filename");
    names.extend((0..20_000).map(|_| awkward_text(&mut rng, 40)));
    names.extend((0..2_000).map(|_| "日本語😀é".repeat(rng.below(30)) + &awkward_text(&mut rng, 6)));
    for name in &names {
        let safe = media_name::safe_media_name(name);
        assert!(safe.len() <= 120, "{name:?} -> {safe:?}");
        assert!(!safe.ends_with([' ', '.']), "{name:?} -> {safe:?}");
        assert!(
            !safe.contains(|c: char| c.is_ascii_control() || "[]<>:\"/?*^\\|\u{a0}".contains(c)),
            "{name:?} -> {safe:?}"
        );
        // Only removal can leave a name that NFC would change (F2).
        if normalize::nfc(&safe) == safe.as_str() {
            assert_eq!(media_name::safe_media_name(&safe), safe, "{name:?}");
        }
    }
}

#[test]
fn guid_text_is_base91_of_the_value() {
    let decode = |text: &str| {
        text.bytes().fold(0u64, |value, digit| {
            let d = base91::DIGITS.iter().position(|&b| b == digit).unwrap() as u64;
            value * 91 + d
        })
    };
    let mut rng = Rng::new(SEED ^ 6);
    for _ in 0..50_000 {
        let value = rng.next_u64() >> rng.below(64);
        let text = base91::encode(value);
        assert_eq!(decode(&text), value, "{value} -> {text}");
        assert!(text.len() <= 10 && (value == 0) == text.is_empty(), "{value} -> {text}");
        assert!(!text.starts_with('a'), "{value} -> {text}: no padding");
    }
}

#[test]
fn unicase_comparison_is_a_consistent_order() {
    let mut pairs: Vec<(String, String)> = corpus()
        .into_iter()
        .filter(|r| r.fns.iter().any(|f| f == "unicase_compare"))
        .flat_map(|r| r.inputs)
        .map(|pair| {
            (
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(pairs.len() >= 2_000);
    let mut rng = Rng::new(SEED ^ 7);
    pairs.extend((0..20_000).map(|_| (awkward_text(&mut rng, 6), awkward_text(&mut rng, 6))));
    for (a, b) in &pairs {
        assert_eq!(
            fold::compare_folded(a, b),
            fold::compare_folded(b, a).reverse(),
            "{a:?} {b:?}"
        );
        assert_eq!(fold::compare_folded(a, a), Ordering::Equal, "{a:?}");
        let upper = a.to_ascii_uppercase();
        assert_eq!(fold::compare_folded(a, &upper), Ordering::Equal, "{a:?}");
    }
}
