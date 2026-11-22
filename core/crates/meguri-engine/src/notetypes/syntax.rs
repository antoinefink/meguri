//! The template editor's colouring of Front and Back (`notetypes/workspace`
//! W6): `render/template-syntax` T12's nodes give field replacements one
//! colour, section tags a second, comments and the alternate-delimiter
//! prefix their own; HTML tags in the text between them get the code
//! colour of a tag. Pure: the text is read, never changed.

use std::ops::Range;

use meguri_template::{NodeKind, ParsedTemplate};

/// What a coloured range of a template text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TemplateHighlight {
    /// A replacement, `{{Field}}` or `{{hint:Field}}`.
    Field,
    /// A section open or close, `{{#Field}}`, `{{^Field}}`, `{{/Field}}`.
    Section,
    /// `<!--…-->`.
    Comment,
    /// An HTML tag's characters, `<div class="x">`; a template tag inside
    /// it keeps its own colour.
    Tag,
    /// T2's leading alternate-delimiter directive.
    Directive,
}

/// The coloured ranges of `source`, in source order and never overlapping;
/// plain text has none.
pub fn template_highlights(source: &str) -> Vec<(Range<usize>, TemplateHighlight)> {
    let parsed = ParsedTemplate::parse(source);
    let mut spans = Vec::new();
    // An HTML tag can span template tags (`<img src="{{Image}}">`): its
    // state carries from one text node to the next.
    let mut tag = TagScan::default();
    for node in parsed.nodes() {
        let highlight = match &node.kind {
            NodeKind::Prefix => TemplateHighlight::Directive,
            NodeKind::Comment => TemplateHighlight::Comment,
            NodeKind::Replacement { .. } => TemplateHighlight::Field,
            NodeKind::Open { .. } | NodeKind::Close { .. } => TemplateHighlight::Section,
            NodeKind::Text => {
                tag.text(source, node.span.clone(), &mut spans);
                continue;
            }
        };
        spans.push((node.span.clone(), highlight));
    }
    spans
}

/// The HTML tag scan over text nodes: a tag starts at `<` followed by a
/// letter, `/`, `!` or `?`, and ends at the next `>` outside quotes.
#[derive(Default)]
struct TagScan {
    inside: bool,
    quote: Option<u8>,
    /// The last byte other than white space, inside the tag: a quote opens
    /// a value only right after `=` (an apostrophe in `title=it's` does not).
    last: u8,
}

impl TagScan {
    fn text(&mut self, source: &str, range: Range<usize>, spans: &mut Vec<(Range<usize>, TemplateHighlight)>) {
        let bytes = source.as_bytes();
        let mut start = self.inside.then_some(range.start);
        let mut at = range.start;
        while at < range.end {
            let byte = bytes[at];
            if self.inside {
                match self.quote {
                    Some(q) if byte == q => self.quote = None,
                    Some(_) => {}
                    None if (byte == b'"' || byte == b'\'') && self.last == b'=' => self.quote = Some(byte),
                    None if byte == b'>' => {
                        self.inside = false;
                        if let Some(s) = start.take() {
                            spans.push((s..at + 1, TemplateHighlight::Tag));
                        }
                    }
                    None => {}
                }
                if !byte.is_ascii_whitespace() {
                    self.last = byte;
                }
            } else if byte == b'<'
                && bytes
                    .get(at + 1)
                    .is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, b'/' | b'!' | b'?'))
            {
                self.inside = true;
                self.last = b'<';
                start = Some(at);
            }
            at += 1;
        }
        if let Some(s) = start
            && s < range.end
        {
            spans.push((s..range.end, TemplateHighlight::Tag));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TemplateHighlight::*;

    fn coloured(source: &str) -> Vec<(&str, TemplateHighlight)> {
        template_highlights(source)
            .into_iter()
            .map(|(range, h)| (&source[range], h))
            .collect()
    }

    #[test]
    fn nodes_and_tags_take_their_colours() {
        assert_eq!(
            coloured("<div class=\"w\">{{Word}}</div>\n{{#Reading}}{{hint:Reading}}{{/Reading}}<!-- {{x}} -->"),
            vec![
                ("<div class=\"w\">", Tag),
                ("{{Word}}", Field),
                ("</div>", Tag),
                ("{{#Reading}}", Section),
                ("{{hint:Reading}}", Field),
                ("{{/Reading}}", Section),
                ("<!-- {{x}} -->", Comment),
            ]
        );
    }

    #[test]
    fn a_tag_around_a_field_keeps_the_field_its_colour() {
        assert_eq!(
            coloured("<img src=\"{{Image}}\"> a < b"),
            vec![("<img src=\"", Tag), ("{{Image}}", Field), ("\">", Tag)]
        );
        assert_eq!(coloured("<span title='>'>x"), vec![("<span title='>'>", Tag)]);
        assert_eq!(coloured("<b title=it's>x</b> y'z"), vec![("<b title=it's>", Tag), ("</b>", Tag)]);
    }

    #[test]
    fn the_alternate_prefix_and_unclosed_text_are_read_as_written() {
        assert_eq!(
            coloured(" {{=<% %>=}}<%Front%> {{Back}}"),
            vec![(" {{=<% %>=}}", Directive), ("<%Front%>", Field)]
        );
        assert_eq!(coloured("{{Front"), vec![]);
        assert_eq!(coloured("<b"), vec![("<b", Tag)]);
        assert_eq!(coloured(""), vec![]);
    }
}
