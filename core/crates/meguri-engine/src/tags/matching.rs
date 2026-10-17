//! Matching tags against names (`specs/tags/tags.md` T5): a tag matches a
//! name when it equals it or begins with it followed by `::`, characters
//! compared by Unicode 16.0 simple case folding (the `regex` crate's
//! case-insensitive mode) and every character literal.

use regex::{Regex, RegexBuilder};

use super::canonical::{LEVEL, is_separator, split_text};

/// The names an operation acts on, in list order.
#[derive(Clone, Debug)]
pub struct Matcher {
    /// One anchored, case-insensitive literal per name.
    names: Vec<Regex>,
    /// Any name as a whole tag or a tag's leading components, anywhere in a
    /// note's stored text: the scan's test.
    stored: Option<Regex>,
}

fn literal(name: &str) -> String {
    regex::escape(name)
}

fn build(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .unicode(true)
        // Hundreds of names to delete make one large alternation.
        .size_limit(256 << 20)
        .build()
        .expect("an escaped literal pattern compiles")
}

impl Matcher {
    /// The names in list order. A name that is empty or holds a separator
    /// matches no tag: a stored tag holds no separator, and no caller
    /// passes such a name (`specs/conformance/tag-ops.md`).
    pub fn new<S: AsRef<str>>(names: impl IntoIterator<Item = S>) -> Matcher {
        let names: Vec<String> = names.into_iter().map(|n| n.as_ref().to_owned()).collect();
        let usable = |name: &String| !name.is_empty() && !name.chars().any(is_separator);
        let compiled = names
            .iter()
            .map(|name| {
                if usable(name) {
                    build(&format!("^{}", literal(name)))
                } else {
                    // Matches nothing: a tag never holds a separator.
                    build("^ ")
                }
            })
            .collect();
        let alternatives: Vec<String> = names.iter().filter(|n| usable(n)).map(|n| literal(n)).collect();
        let stored = (!alternatives.is_empty()).then(|| {
            build(&format!(
                "(?:^|[ \u{3000}])(?:{})(?:{}|[ \u{3000}]|$)",
                alternatives.join("|"),
                literal(LEVEL)
            ))
        });
        Matcher {
            names: compiled,
            stored,
        }
    }

    /// Whether no name can match.
    pub fn is_empty(&self) -> bool {
        self.stored.is_none()
    }

    /// The first name in list order that `tag` matches, and the byte length
    /// of the tag's part that matched it: the rest of the tag is empty or
    /// begins with `::`.
    pub fn first_match(&self, tag: &str) -> Option<(usize, usize)> {
        self.names.iter().enumerate().find_map(|(index, name)| {
            let found = name.find(tag)?;
            let rest = &tag[found.end()..];
            (rest.is_empty() || rest.starts_with(LEVEL)).then_some((index, found.end()))
        })
    }

    /// Whether `tag` matches some name.
    pub fn matches(&self, tag: &str) -> bool {
        self.first_match(tag).is_some()
    }

    /// Whether a note with this stored `tags` text matches (T5): one of its
    /// tags matches some name.
    pub fn matches_stored(&self, stored: &str) -> bool {
        self.stored.as_ref().is_some_and(|any| any.is_match(stored))
    }

    /// The tags of a stored text that match, in stored order.
    pub fn matching_tags<'t>(&self, stored: &'t str) -> impl Iterator<Item = &'t str> {
        split_text(stored).filter(|tag| self.matches(tag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_matches_itself_and_its_descendants_in_any_case() {
        let m = Matcher::new(["foo"]);
        for tag in ["foo", "FOO::kid", "foo::a::b", "Foo:::x"] {
            assert!(m.matches(tag), "{tag}");
        }
        for tag in ["foobar", "bar::foo", "foo:", "fo"] {
            assert!(!m.matches(tag), "{tag}");
        }
        assert_eq!(m.first_match("FOO::kid"), Some((0, 3)));
    }

    #[test]
    fn simple_folding_without_full_folding() {
        assert!(Matcher::new(["\u{e9}"]).matches("\u{c9}"));
        assert!(Matcher::new(["k"]).matches("\u{212a}"));
        assert!(Matcher::new(["k"]).matches("K::x"));
        assert!(!Matcher::new(["ss"]).matches("\u{df}"));
        assert!(!Matcher::new(["STRASSE"]).matches("Stra\u{df}e"));
        // The matched part's length is the tag's, not the name's.
        assert_eq!(Matcher::new(["k"]).first_match("\u{212a}::x"), Some((0, 3)));
    }

    #[test]
    fn every_character_is_literal() {
        let m = Matcher::new(["c++", "mix*"]);
        assert!(m.matches("c++") && m.matches("C++::x") && m.matches("mix*"));
        assert!(!m.matches("c+") && !m.matches("mix") && !m.matches("mixed") && !m.matches("ccc"));
    }

    #[test]
    fn the_first_name_in_list_order_decides() {
        let m = Matcher::new(["foo", "foo::bar"]);
        assert_eq!(m.first_match("foo::bar::x"), Some((0, 3)));
        let m = Matcher::new(["foo::bar", "foo"]);
        assert_eq!(m.first_match("foo::bar::x"), Some((0, 8)));
        assert_eq!(m.first_match("foo::baz"), Some((1, 3)));
    }

    #[test]
    fn stored_texts_match_through_one_of_their_tags() {
        let m = Matcher::new(["a", "b c"]);
        assert!(m.matches_stored(" x a "));
        assert!(m.matches_stored(" A::y "));
        assert!(m.matches_stored("\u{3000}a\u{3000}"));
        assert!(!m.matches_stored(" ab ba "));
        assert!(!m.matches_stored(" b c "), "a name with a space matches nothing");
        assert!(Matcher::new([""]).is_empty());
        assert_eq!(m.matching_tags(" a q A::z ").collect::<Vec<_>>(), ["a", "A::z"]);
    }
}
