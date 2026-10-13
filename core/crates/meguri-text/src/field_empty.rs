//! Field emptiness for card generation and conditionals
//! (`specs/text/field-emptiness.md` E1). Apply it to the field as saved
//! (E2: `normalize::field_as_saved` first).

/// E1: whether the field holds only ASCII whitespace and spacer tags (`<br>`,
/// `</div>`, `<DIV />`, ...). Nothing is decoded: `&nbsp;` makes it non-empty.
pub fn counts_as_empty(field: &str) -> bool {
    let mut rest = field.as_bytes();
    while let Some((&first, after)) = rest.split_first() {
        rest = match first {
            b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ' => after,
            b'<' => match after_spacer_tag(after) {
                Some(after_tag) => after_tag,
                None => return false,
            },
            _ => return false,
        };
    }
    true
}

/// The bytes after a spacer tag whose `<` was just read: optional `/`, `br`
/// or `div` in any ASCII case, optional single space, optional `/`, `>`.
fn after_spacer_tag(bytes: &[u8]) -> Option<&[u8]> {
    let bytes = bytes.strip_prefix(b"/").unwrap_or(bytes);
    let name_len = [&b"br"[..], b"div"]
        .into_iter()
        .find(|name| bytes.get(..name.len()).is_some_and(|b| b.eq_ignore_ascii_case(name)))?
        .len();
    let bytes = &bytes[name_len..];
    let bytes = bytes.strip_prefix(b" ").unwrap_or(bytes);
    let bytes = bytes.strip_prefix(b"/").unwrap_or(bytes);
    bytes.strip_prefix(b">")
}

#[cfg(test)]
mod tests {
    use super::counts_as_empty;
    use crate::normalize::field_as_saved;

    #[test]
    fn whitespace_and_spacer_tags_are_empty() {
        for field in [
            "",
            " \t\n\u{b}\u{c}\r",
            "<br>",
            "<BR/>",
            "<br />",
            "</br />",
            "<Br >",
            "<div/>",
            "<div />",
            "</div>",
            "<DIV>",
            "<br/> <div></div>\n",
            "<dIv></DiV><bR>",
        ] {
            assert!(counts_as_empty(field), "{field:?}");
        }
    }

    #[test]
    fn anything_else_is_content() {
        for field in [
            "<br  >",
            "<br\t>",
            "<br\n>",
            "< br>",
            "<br/ >",
            "<br //>",
            "<div class=\"x\">",
            "&nbsp;",
            "&#32;",
            "\u{a0}",
            "\u{3000}",
            "<span></span>",
            "<p></p>",
            "<!-- -->",
            "<b",
            "<br",
            "<",
            "x",
            "<brr>",
            "<divv>",
        ] {
            assert!(!counts_as_empty(field), "{field:?}");
        }
    }

    #[test]
    fn saved_values_are_judged_after_normalization() {
        let saved = |v: &str| counts_as_empty(&field_as_saved(v, true));
        assert!(saved("\u{b}\u{c}\r"));
        assert!(saved("<b\rr>"));
        assert!(saved("<br>\u{1}"));
        assert!(!saved("&nbsp;"));
        assert!(!saved("\u{a0}"));
    }
}
