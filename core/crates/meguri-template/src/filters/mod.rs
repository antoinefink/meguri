//! Filter chains (`specs/render/filters.md`).
//!
//! A replacement's filter names are kept in source order; the chain applies
//! them from the one nearest the key outward (F1). After the type rewrite,
//! an empty name changes nothing, a built-in name (F2) replaces the text
//! with its output, and any other name ends the chain.

pub mod furigana;
pub mod hint;
pub mod text;
pub mod tts;

use std::borrow::Cow;

/// F7: the three type-in markers; each discards the text it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeMarker {
    /// `type`: `[[type:KEY]]`.
    Plain,
    /// `type-nc`: `[[type:nc:KEY]]`.
    NoCombining,
    /// `type-cloze`: `[[type:cloze:KEY]]`.
    Cloze,
}

impl TypeMarker {
    pub fn marker(self, key: &str) -> String {
        let modifier = match self {
            TypeMarker::Plain => "",
            TypeMarker::NoCombining => "nc:",
            TypeMarker::Cloze => "cloze:",
        };
        format!("[[type:{modifier}{key}]]")
    }
}

/// F2: the built-in filters, by exact name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Builtin<'a> {
    Text,
    Furigana,
    Kana,
    Kanji,
    Hint,
    /// `tts ` followed by its options, kept exactly.
    Tts(&'a str),
    Type(TypeMarker),
    /// `cloze` and `cloze-only` (F8): built in, so they never end a chain.
    /// Their output is U61's (`specs/render/cloze.md`); until it lands they
    /// leave the text unchanged.
    Cloze,
}

impl<'a> Builtin<'a> {
    fn named(name: &'a str) -> Option<Builtin<'a>> {
        Some(match name {
            "text" => Builtin::Text,
            "furigana" => Builtin::Furigana,
            "kana" => Builtin::Kana,
            "kanji" => Builtin::Kanji,
            "hint" => Builtin::Hint,
            "type" => Builtin::Type(TypeMarker::Plain),
            "type-nc" => Builtin::Type(TypeMarker::NoCombining),
            "type-cloze" => Builtin::Type(TypeMarker::Cloze),
            "cloze" | "cloze-only" => Builtin::Cloze,
            _ => Builtin::Tts(name.strip_prefix("tts ")?),
        })
    }

    fn apply(self, text: &str, key: &str) -> String {
        match self {
            Builtin::Text => text::text(text),
            Builtin::Furigana => furigana::furigana(text),
            Builtin::Kana => furigana::kana(text),
            Builtin::Kanji => furigana::kanji(text),
            Builtin::Hint => hint::hint(text, key),
            Builtin::Tts(options) => tts::tts(text, options),
            Builtin::Type(marker) => marker.marker(key),
            Builtin::Cloze => text.to_owned(),
        }
    }
}

/// F1 step 1: when the outermost filter is exactly `type`, the whole chain
/// becomes one marker filter: `type:cloze` and `type:nc` keep their
/// modifier, anything else is the plain marker.
fn type_rewrite(filters: &[String]) -> Option<TypeMarker> {
    match filters {
        [outer, rest @ ..] if outer == "type" => Some(match rest {
            [only] if only == "cloze" => TypeMarker::Cloze,
            [only] if only == "nc" => TypeMarker::NoCombining,
            _ => TypeMarker::Plain,
        }),
        _ => None,
    }
}

/// F1: the field's text through a replacement's filters, given in source
/// order, for the replacement's key as written.
pub fn apply<'t>(value: &'t str, key: &str, filters: &[String]) -> Cow<'t, str> {
    if let Some(marker) = type_rewrite(filters) {
        return Cow::Owned(marker.marker(key));
    }
    let mut text = Cow::Borrowed(value);
    for name in filters.iter().rev() {
        if name.is_empty() {
            continue;
        }
        let Some(filter) = Builtin::named(name) else {
            break;
        };
        text = Cow::Owned(filter.apply(&text, key));
    }
    text
}
