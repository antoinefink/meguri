//! Search strings (`specs/fixtures/anonymizer.md` A13): filtered-deck
//! searches, saved searches and the default search, rewritten term by term
//! so that deck and tag terms still select what they selected.

use regex::Regex;

use meguri_text::fold;

use super::keys::Stream;
use super::rewrite::{self, CharCounts};
use super::text::Names;

/// A16 `searches`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchCounts {
    pub inexact: u64,
    pub rewritten: u64,
    pub unparsed: u64,
}

/// What deck and tag terms are expanded against (the source collection).
pub struct Targets {
    /// Every deck: its human name (`::` between components) and its new one.
    pub decks: Vec<(String, String)>,
    /// The distinct tags of notes and of the `tags` table, and their new
    /// names.
    pub tags: Vec<(String, String)>,
}

/// A13.2: keys whose terms are kept.
const KEPT_KEYS: &[&str] = &[
    "note",
    "card",
    "flag",
    "resched",
    "prop",
    "added",
    "edited",
    "introduced",
    "rated",
    "is",
    "did",
    "mid",
    "nid",
    "cid",
    "preset",
    "has-cd",
];

/// One item of a search string (A13.1).
enum Item {
    /// Whitespace, a parenthesis, a `-` before a term, an operator.
    Keep(usize, usize),
    Term(Term),
}

/// A term: its span, and where its text lies inside it.
struct Term {
    span: (usize, usize),
    text: (usize, usize),
}

/// A13.1: the items of `s`, or `None` when it does not read as a search.
fn read(s: &str) -> Option<Vec<Item>> {
    let bytes = s.as_bytes();
    let mut items = Vec::new();
    let mut depth = 0i64;
    let mut p = 0;
    while p < s.len() {
        let c = s[p..].chars().next()?;
        if c.is_whitespace() {
            items.push(Item::Keep(p, p + c.len_utf8()));
            p += c.len_utf8();
            continue;
        }
        match c {
            '(' => {
                depth += 1;
                items.push(Item::Keep(p, p + 1));
                p += 1;
                continue;
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                items.push(Item::Keep(p, p + 1));
                p += 1;
                continue;
            }
            '-' => {
                if let Some(next) = s[p + 1..].chars().next()
                    && !next.is_whitespace()
                    && next != ')'
                {
                    items.push(Item::Keep(p, p + 1));
                    p += 1;
                    continue;
                }
            }
            _ => {}
        }
        if c == '"' {
            let close = quoted_end(s, p)?;
            if close == p + 1 {
                return None;
            }
            items.push(Item::Term(Term {
                span: (p, close + 1),
                text: (p + 1, close),
            }));
            p = close + 1;
            continue;
        }
        // A partly quoted term: a key, `:`, a quoted part.
        let key_len = s[p..]
            .find(|c: char| c == '"' || c == '(' || c == ')' || c == ':' || c.is_whitespace())
            .unwrap_or(s.len() - p);
        if key_len > 0 && bytes.get(p + key_len) == Some(&b':') && bytes.get(p + key_len + 1) == Some(&b'"') {
            let open = p + key_len + 1;
            let close = quoted_end(s, open)?;
            items.push(Item::Term(Term {
                span: (p, close + 1),
                text: (p, close),
            }));
            p = close + 1;
            continue;
        }
        // An unquoted term.
        let mut i = p;
        while i < s.len() {
            let c = s[i..].chars().next()?;
            if c.is_whitespace() || c == '"' || c == '(' || c == ')' {
                break;
            }
            if c == '\\' {
                let next = s[i + 1..].chars().next();
                match next {
                    Some(n) if !n.is_whitespace() => i += 1 + n.len_utf8(),
                    // An odd `\` at the end of a term.
                    _ => return None,
                }
                continue;
            }
            i += c.len_utf8();
        }
        let text = &s[p..i];
        if text.eq_ignore_ascii_case("and") || text.eq_ignore_ascii_case("or") {
            items.push(Item::Keep(p, i));
        } else {
            items.push(Item::Term(Term {
                span: (p, i),
                text: (p, i),
            }));
        }
        p = i;
    }
    if depth != 0 {
        return None;
    }
    // A term whose text starts with `:`.
    let starts_with_colon = items
        .iter()
        .any(|item| matches!(item, Item::Term(t) if s[t.text.0..t.text.1].starts_with(':')));
    (!starts_with_colon).then_some(items)
}

/// The closing quote of the quoted part opening at `open`.
fn quoted_end(s: &str, open: usize) -> Option<usize> {
    let mut i = open + 1;
    while i < s.len() {
        let c = s[i..].chars().next()?;
        match c {
            '\\' => {
                let next = s[i + 1..].chars().next()?;
                i += 1 + next.len_utf8();
            }
            '"' => return Some(i),
            _ => i += c.len_utf8(),
        }
    }
    None
}

/// The position of a term text's first unescaped `:`.
fn key_colon(text: &str) -> Option<usize> {
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next()?;
        match c {
            '\\' => i += 1 + text[i + 1..].chars().next().map_or(0, char::len_utf8),
            ':' => return Some(i),
            _ => i += c.len_utf8(),
        }
    }
    None
}

/// A13.3: a deck or tag value unescaped into pattern pieces.
enum Piece {
    Char(char),
    Any,
    One,
}

fn pattern(value: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(next) => out.push(Piece::Char(next)),
                None => out.push(Piece::Char('\\')),
            },
            '*' => out.push(Piece::Any),
            '_' => out.push(Piece::One),
            c => out.push(Piece::Char(c)),
        }
    }
    out
}

/// A regex that holds for a name when the pattern matches it from its start
/// and the match ends at its end or right before a `::`.
fn matcher(pieces: &[Piece], any: &str, one: &str) -> Regex {
    let mut re = String::from("(?is)\\A(?:");
    for piece in pieces {
        match piece {
            Piece::Char(c) => re.push_str(&regex::escape(&c.to_string())),
            Piece::Any => re.push_str(any),
            Piece::One => re.push_str(one),
        }
    }
    re.push_str(")(?:::.*)?\\z");
    Regex::new(&re).expect("an escaped pattern compiles")
}

/// The `::` ancestors of a name, nearest first.
fn ancestors(name: &str) -> impl Iterator<Item = &str> {
    name.match_indices(super::names::SEPARATOR)
        .map(|(i, _)| &name[..i])
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
}

/// A13.3, A13.4: the expansion of a deck or tag term over `targets`.
fn expand(key: &str, matcher: &Regex, targets: &[(String, String)]) -> String {
    let folds: std::collections::HashSet<String> = targets.iter().map(|(old, _)| fold::folded(old)).collect();
    let matched: std::collections::HashSet<String> = targets
        .iter()
        .filter(|(old, _)| matcher.is_match(old))
        .map(|(old, _)| fold::folded(old))
        .collect();
    let mut chosen: Vec<(String, &str)> = targets
        .iter()
        .filter(|(old, _)| matcher.is_match(old))
        .filter(|(old, _)| {
            !ancestors(old).any(|a| {
                let f = fold::folded(a);
                folds.contains(&f) && matched.contains(&f)
            })
        })
        .map(|(_, new)| (fold::folded(new), new.as_str()))
        .collect();
    chosen.sort_by(|a, b| a.0.cmp(&b.0));
    chosen.dedup_by(|b, a| a.0 == b.0);
    match chosen.as_slice() {
        [] => "did:0".to_owned(),
        [(_, one)] => format!("{key}:{one}"),
        many => format!(
            "({})",
            many.iter()
                .map(|(_, new)| format!("{key}:{new}"))
                .collect::<Vec<_>>()
                .join(" OR ")
        ),
    }
}

pub struct SearchRewriter<'a, N: Names> {
    pub targets: &'a Targets,
    pub names: &'a mut N,
    pub stream: &'a mut Stream,
    pub chars: &'a mut CharCounts,
    pub counts: &'a mut SearchCounts,
}

impl<N: Names> SearchRewriter<'_, N> {
    fn text(&mut self, s: &str) -> String {
        rewrite::plain(s, self.names, self.stream, self.chars)
    }

    /// A13: one search string.
    pub fn rewrite(&mut self, s: &str) -> String {
        let Some(items) = read(s) else {
            self.counts.unparsed += 1;
            return self.text(s);
        };
        let mut out = String::with_capacity(s.len());
        for item in items {
            match item {
                Item::Keep(a, b) => out.push_str(&s[a..b]),
                Item::Term(term) => {
                    let rewritten = self.term(s, &term);
                    out.push_str(&rewritten);
                }
            }
        }
        out
    }

    fn term(&mut self, s: &str, term: &Term) -> String {
        let (t0, t1) = term.text;
        let text = &s[t0..t1];
        let before = &s[term.span.0..t0];
        let after = &s[t1..term.span.1];
        let Some(colon) = key_colon(text) else {
            return format!("{before}{}{after}", self.text(text));
        };
        let key = &text[..colon];
        // In a partly quoted term the value starts after the opening quote.
        let raw_value = &text[colon + 1..];
        let (value_prefix, value) = match raw_value.strip_prefix('"') {
            Some(v) if before.is_empty() && !after.is_empty() => ("\"", v),
            _ => ("", raw_value),
        };
        let head = format!("{before}{key}:{value_prefix}");
        let lower = key.to_ascii_lowercase();
        if KEPT_KEYS.contains(&lower.as_str()) {
            return s[term.span.0..term.span.1].to_owned();
        }
        match lower.as_str() {
            "deck" => {
                if matches!(value, "*" | "filtered" | "current") {
                    return s[term.span.0..term.span.1].to_owned();
                }
                self.counts.rewritten += 1;
                let re = matcher(&pattern(value), ".*", ".");
                expand("deck", &re, &self.targets.decks)
            }
            "tag" => {
                if matches!(value, "none" | "*") {
                    return s[term.span.0..term.span.1].to_owned();
                }
                if value.starts_with("re:") || value.starts_with("nc:") {
                    self.counts.inexact += 1;
                    return format!("{head}{}{}{after}", &value[..3], self.text(&value[3..]));
                }
                self.counts.rewritten += 1;
                if value.contains(' ') {
                    return "did:0".to_owned();
                }
                let re = matcher(&pattern(value), "\\S*", "\\S");
                expand("tag", &re, &self.targets.tags)
            }
            "dupe" => match value.find(',') {
                Some(comma) => format!("{head}{}{}{after}", &value[..=comma], self.text(&value[comma + 1..])),
                None => s[term.span.0..term.span.1].to_owned(),
            },
            _ => format!("{head}{}{after}", self.text(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_that_do_not_read_are_refused() {
        for s in ["\"open", "\"\"", "(a", "a)", "x\\", ":x", "\":x\"", "deck:\"open"] {
            assert!(read(s).is_none(), "{s:?}");
        }
        for s in [
            "a b",
            "(a OR b) -c",
            "deck:\"A B\" tag:x",
            "\"a b\"",
            "a\\(b",
            "-(a)",
            "front:*x*",
        ] {
            assert!(read(s).is_some(), "{s:?}");
        }
    }

    #[test]
    fn deck_patterns_match_names_and_their_descendants() {
        let re = matcher(&pattern("Japanese"), ".*", ".");
        assert!(re.is_match("Japanese"));
        assert!(re.is_match("japanese::Audio"));
        assert!(!re.is_match("Japanese2"));
        let re = matcher(&pattern("*a*"), ".*", ".");
        assert!(re.is_match("Default"));
        assert!(!re.is_match("x::y"));
        let re = matcher(&pattern("j_p"), ".*", ".");
        assert!(re.is_match("jap"));
        let re = matcher(&pattern("a\\*"), ".*", ".");
        assert!(re.is_match("a*"));
        assert!(!re.is_match("ab"));
    }
}
