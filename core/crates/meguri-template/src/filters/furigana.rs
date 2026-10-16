//! `furigana`, `kana` and `kanji` (`specs/render/filters.md` F4, R966).

use std::sync::LazyLock;

use regex::{Captures, Regex};

/// F4 step 2, one ruby group: an optional space, the shortest base of
/// characters other than space and `>`, `[`, the shortest reading of
/// characters other than LF, `]`. The `regex` crate's leftmost-first
/// semantics pick the earliest start and, there, the shortest base and
/// reading, as the spec describes.
static GROUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" ?([^ >]+?)\[(.+?)\]").expect("F4 ruby pattern"));

/// F4: every `&nbsp;` becomes a space; then each group whose reading does
/// not start with `sound:` becomes `markup(base, reading)`, its space
/// dropped. Sound groups stay as they are, space included.
fn replace_groups(text: &str, markup: impl Fn(&str, &str) -> String) -> String {
    let spaced = text.replace("&nbsp;", " ");
    GROUP
        .replace_all(&spaced, |group: &Captures<'_>| {
            let (base, reading) = (&group[1], &group[2]);
            if reading.starts_with("sound:") {
                group[0].to_owned()
            } else {
                markup(base, reading)
            }
        })
        .into_owned()
}

pub fn furigana(text: &str) -> String {
    replace_groups(text, |base, reading| {
        format!("<ruby><rb>{base}</rb><rt>{reading}</rt></ruby>")
    })
}

pub fn kana(text: &str) -> String {
    replace_groups(text, |_, reading| reading.to_owned())
}

pub fn kanji(text: &str) -> String {
    replace_groups(text, |base, _| base.to_owned())
}
