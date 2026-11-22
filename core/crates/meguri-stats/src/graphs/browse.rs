//! `stats_browse_scope` (`stats/browse-queries` Q1, Q2): the scope's part of
//! the text a click on a graph opens in the Browser. The core writes it from
//! the request's snapshot; only `StatsBrowseLink` joins it with an item
//! (Q3). Never canonical text.

use std::collections::HashSet;

/// The scope text of a Data result (Q1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseScope {
    /// The Collection scope: the item is used alone (Q2.1).
    None,
    /// The text S (Q2.2, Q2.3).
    Text(String),
    /// No item of the scope is clickable (Q2.2's negative ids).
    Unavailable,
}

/// Q2.3: the Search text with leading and trailing White_Space removed;
/// none once empty.
pub(crate) fn search_text(text: &str) -> BrowseScope {
    let trimmed = text.trim_matches(char::is_whitespace);
    if trimmed.is_empty() {
        BrowseScope::None
    } else {
        BrowseScope::Text(trimmed.to_owned())
    }
}

/// Q2.2 for the deck whose stored name is `name` and whose scope decks
/// (G2.1) are `scope`.
pub(crate) fn deck_text(name: &str, scope: &HashSet<i64>) -> BrowseScope {
    let levels: Vec<&str> = name.split('\u{1f}').collect();
    let full = levels.join("::");
    if full == "current" || full == "filtered" {
        let mut text = String::from("deck:\"");
        text.push_str(if full == "current" { "Current" } else { "Filtered" });
        text.push('"');
        return BrowseScope::Text(text);
    }
    if levels.iter().any(|level| needs_ids(level)) {
        let mut ids: Vec<i64> = scope.iter().copied().collect();
        if ids.iter().any(|id| *id < 0) {
            return BrowseScope::Unavailable;
        }
        ids.sort_unstable();
        let list: Vec<String> = ids.iter().map(i64::to_string).collect();
        return BrowseScope::Text(format!("did:{}", list.join(",")));
    }
    let mut text = String::from("deck:\"");
    for c in full.chars() {
        match c {
            '\\' => text.push_str("\\\\"),
            '"' => text.push_str("\\\""),
            '*' => text.push_str("\\*"),
            '_' => text.push_str("\\_"),
            other => text.push(other),
        }
    }
    text.push('"');
    BrowseScope::Text(text)
}

/// Q2.2: a level no `deck:` text gives back as stored.
fn needs_ids(level: &str) -> bool {
    level.is_empty()
        || level.contains("::")
        || level.chars().any(|c| (c.is_whitespace() && c != ' ') || c.is_ascii_control())
        || level.starts_with(|c: char| c.is_whitespace() || c == ':')
        || level.ends_with(|c: char| c.is_whitespace() || c == ':')
        || meguri_text::normalize::nfc(level) != level
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(name: &str, scope: &[i64]) -> BrowseScope {
        deck_text(name, &scope.iter().copied().collect())
    }

    fn written(text: &str) -> BrowseScope {
        BrowseScope::Text(text.to_owned())
    }

    #[test]
    fn q2_2_escapes_the_four_characters_and_joins_levels_with_colons() {
        assert_eq!(text("Japanese", &[5]), written("deck:\"Japanese\""));
        assert_eq!(text("a_b", &[5]), written("deck:\"a\\_b\""));
        assert_eq!(text("a*b", &[5]), written("deck:\"a\\*b\""));
        assert_eq!(text("a\"b", &[5]), written("deck:\"a\\\"b\""));
        assert_eq!(text("a\\b", &[5]), written("deck:\"a\\\\b\""));
        assert_eq!(text("日本語\u{1f}子", &[5]), written("deck:\"日本語::子\""));
        assert_eq!(text("x\u{1f}current", &[5]), written("deck:\"x::current\""));
        assert_eq!(text("a b", &[5]), written("deck:\"a b\""));
        assert_eq!(text("(x)", &[5]), written("deck:\"(x)\""));
        assert_eq!(text("-x", &[5]), written("deck:\"-x\""));
        assert_eq!(text("_*", &[5]), written("deck:\"\\_\\*\""));
    }

    #[test]
    fn q2_2_current_and_filtered_take_the_capitalized_name() {
        assert_eq!(text("current", &[5, 6]), written("deck:\"Current\""));
        assert_eq!(text("filtered", &[5]), written("deck:\"Filtered\""));
        assert_eq!(text("Filtered", &[5]), written("deck:\"Filtered\""));
        assert_eq!(text("Current", &[5]), written("deck:\"Current\""));
    }

    #[test]
    fn q2_2_names_deck_cannot_give_back_take_the_ids() {
        assert_eq!(text("a\u{a0}b", &[9, 3, 12]), written("did:3,9,12"));
        assert_eq!(text("a\u{1f}", &[9]), written("did:9"));
        assert_eq!(text("a::b", &[9]), written("did:9"));
        assert_eq!(text(" a", &[9]), written("did:9"));
        assert_eq!(text("a:", &[9]), written("did:9"));
        assert_eq!(text("a\tb", &[9]), written("did:9"));
        assert_eq!(text("a\u{7f}b", &[9]), written("did:9"));
        // An e followed by U+0301 is not NFC; U+00E9 is.
        assert_eq!(text("e\u{301}", &[9]), written("did:9"));
        assert_eq!(text("\u{e9}", &[9]), written("deck:\"\u{e9}\""));
    }

    #[test]
    fn q2_2_a_negative_id_in_the_ids_form_is_unavailable() {
        assert_eq!(text("a\u{a0}b", &[9, -4]), BrowseScope::Unavailable);
        // A name deck: matches keeps its text whatever the ids.
        assert_eq!(text("plain", &[-4, 9]), written("deck:\"plain\""));
    }

    #[test]
    fn q2_3_trims_white_space_only() {
        assert_eq!(search_text("  tag:a or tag:b \u{3000}"), written("tag:a or tag:b"));
        assert_eq!(search_text(" \u{a0} "), BrowseScope::None);
        assert_eq!(search_text("deck:*"), written("deck:*"));
    }
}
