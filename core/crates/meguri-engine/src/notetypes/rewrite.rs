//! Template references after field renames and removals
//! (`notetypes/structure-changes` S4, `notetypes/field-rename` F3): one pass
//! over U60's parsed nodes and spans per text. Each tag's key is looked up
//! once in the map, so swapped names never cascade and a rename to a removed
//! field's old name edits only the tags that named the renamed field. Tags
//! that name no mapped field, comments and text stay byte for byte
//! (deviation `template-rewrite-keeps-text`).

use std::collections::HashMap;

use meguri_template::{NodeKind, ParsedTemplate};

/// What a field's base name becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target<'a> {
    /// Renamed to this final name (F3).
    Renamed(&'a str),
    /// Removed by the save (S4.2).
    Removed,
}

/// Base field name to its target, compared by exact code points.
pub(crate) type Map<'a> = HashMap<&'a str, Target<'a>>;

/// Which of a template's four texts is rewritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Front,
    Back,
    BrowserFront,
    BrowserBack,
}

impl Side {
    fn is_front(self) -> bool {
        matches!(self, Side::Front | Side::BrowserFront)
    }

    fn is_browser(self) -> bool {
        matches!(self, Side::BrowserFront | Side::BrowserBack)
    }
}

/// S4 on one text. An empty browser text and a text with a structural error
/// come back unchanged (F3's skips). `first` is the saved note type's first
/// field (S4.3), `cloze` whether the note type is a cloze note type.
pub(crate) fn rewrite(source: &str, map: &Map<'_>, first: &str, cloze: bool, side: Side) -> String {
    if side.is_browser() && source.is_empty() {
        return String::new();
    }
    let parsed = ParsedTemplate::parse(source);
    if parsed.structure_error().is_some() {
        return source.to_owned();
    }
    let delimiters = parsed.delimiters();
    let (open, close) = (delimiters.open(), delimiters.close());
    let mut output = String::with_capacity(source.len());
    let mut through = 0;
    // Whether a replacement (of any key) and a cloze replacement remain.
    let mut replacement = false;
    let mut cloze_replacement = false;
    for node in parsed.nodes() {
        let written = match &node.kind {
            NodeKind::Replacement { key, filters } => {
                let target = map.get(key.as_str()).copied();
                if target != Some(Target::Removed) {
                    replacement = true;
                    cloze_replacement |= filters.iter().any(|filter| filter == "cloze");
                }
                // S4.1: the canonical replacement, filter pieces in source
                // order, each followed by `:`.
                target.map(|target| match target {
                    Target::Renamed(name) => {
                        let mut tag = String::from(open);
                        for filter in filters {
                            tag.push_str(filter);
                            tag.push(':');
                        }
                        tag.push_str(name);
                        tag.push_str(close);
                        tag
                    }
                    Target::Removed => String::new(),
                })
            }
            NodeKind::Open { key, negated } => {
                let marker = if *negated { "^" } else { "#" };
                section_tag(map, key, open, marker, close)
            }
            NodeKind::Close { key } => section_tag(map, key, open, "/", close),
            _ => continue,
        };
        if let Some(written) = written {
            output.push_str(&source[through..node.span.start]);
            output.push_str(&written);
            through = node.span.end;
        }
    }
    output.push_str(&source[through..]);
    // S4.3: the first field appended where nothing would show it, in the
    // text's own delimiters and with nothing else added.
    if (cloze && !cloze_replacement) || (!cloze && side.is_front() && !replacement) {
        output.push_str(open);
        if cloze {
            output.push_str("cloze:");
        }
        output.push_str(first);
        output.push_str(close);
    }
    output
}

/// A section's open or close tag rewritten (S4.1) or deleted (S4.2): in a
/// text without structural errors an open and its matching close name the
/// same key, so a removed field loses both tags and keeps their contents.
/// `None` for a key the map does not hold.
fn section_tag(map: &Map<'_>, key: &str, open: &str, marker: &str, close: &str) -> Option<String> {
    map.get(key).map(|target| match target {
        Target::Renamed(name) => format!("{open}{marker}{name}{close}"),
        Target::Removed => String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renamed<'a>(pairs: &[(&'a str, &'a str)]) -> Map<'a> {
        pairs.iter().map(|(from, to)| (*from, Target::Renamed(to))).collect()
    }

    #[test]
    fn s4_4_examples() {
        let front = Side::Front;
        let map = renamed(&[("Front", "Question")]);
        assert_eq!(
            rewrite("{{Front}}<br>{{text:Front}}", &map, "Question", false, front),
            "{{Question}}<br>{{text:Question}}"
        );
        assert_eq!(
            rewrite("{{#Front}}{{Back}}{{/Front}}{{^Front}}-{{/Front}}", &map, "Question", false, front),
            "{{#Question}}{{Back}}{{/Question}}{{^Question}}-{{/Question}}"
        );
        assert_eq!(
            rewrite("{{FrontSide}}<!-- {{Front}} -->", &map, "Question", false, Side::Back),
            "{{FrontSide}}<!-- {{Front}} -->"
        );
        let map = renamed(&[("Extra", "Note")]);
        assert_eq!(
            rewrite("{{ Front }} {{{Back}}} {{ text:Extra }}", &map, "Front", false, front),
            "{{ Front }} {{{Back}}} {{text:Note}}"
        );
        assert_eq!(
            rewrite("{{=<% %>=}}<%Back%> {{x}} <% Extra %>", &map, "Front", false, front),
            "{{=<% %>=}}<%Back%> {{x}} <%Note%>"
        );
        let map = renamed(&[("Front", "Back"), ("Back", "Front")]);
        assert_eq!(rewrite("{{Front}} {{Back}}", &map, "Back", false, front), "{{Back}} {{Front}}");

        let removed: Map<'_> = [("Front", Target::Removed)].into_iter().collect();
        assert_eq!(rewrite("{{Front}}", &removed, "Back", false, front), "{{Back}}");
        assert_eq!(
            rewrite("{{Back}}{{#Front}} ({{Front}}){{/Front}}", &removed, "Back", false, front),
            "{{Back}} ()"
        );
        let removed: Map<'_> = [("Back", Target::Removed)].into_iter().collect();
        assert_eq!(
            rewrite("{{#Extra}}hint{{/Extra}}{{Back}}", &removed, "Front", false, front),
            "{{#Extra}}hint{{/Extra}}{{Front}}"
        );
        let map = renamed(&[("Extra", "Note")]);
        assert_eq!(
            rewrite("{{#Back}}hello{{/Back}}{{Tags}}", &map, "Front", false, front),
            "{{#Back}}hello{{/Back}}{{Tags}}"
        );
        let map = renamed(&[("Front", "Q")]);
        assert_eq!(
            rewrite("{{#Back}}{{Back}} {{ Front }}", &map, "Q", false, Side::Back),
            "{{#Back}}{{Back}} {{ Front }}"
        );
        assert_eq!(rewrite("", &map, "Q", false, Side::BrowserFront), "");

        let removed: Map<'_> = [("Text", Target::Removed)].into_iter().collect();
        assert_eq!(
            rewrite("{{Intro}}<br>{{cloze:Text}}", &removed, "Intro", true, front),
            "{{Intro}}<br>{{cloze:Intro}}"
        );
        assert_eq!(
            rewrite("{{cloze:Text}}<br>{{Extra}}", &removed, "Intro", true, Side::Back),
            "<br>{{Extra}}{{cloze:Intro}}"
        );
    }

    #[test]
    fn a_rename_to_a_removed_name_rewrites_only_the_renamed_references() {
        let map: Map<'_> = [("A", Target::Renamed("B")), ("B", Target::Removed)].into_iter().collect();
        assert_eq!(
            rewrite("{{A}} {{B}} {{#B}}x{{/B}}", &map, "B", false, Side::Front),
            "{{B}}  x"
        );
    }

    #[test]
    fn extra_braces_and_spaces_go_only_from_rewritten_tags() {
        let map = renamed(&[("Front", "Q")]);
        assert_eq!(
            rewrite("{{{Front}}} {{# Front }}x{{/ Front }} {{text: Front}}", &map, "Q", false, Side::Front),
            "{{Q}}} {{#Q}}x{{/Q}} {{text: Front}}"
        );
    }
}
