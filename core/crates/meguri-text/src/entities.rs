//! Entity decoding, all or nothing (`specs/text/strip-html.md` H3).

use std::borrow::Cow;

/// Decodes the entities of `text` when every `&` in it starts a valid one,
/// then turns each U+00A0 of the result into a space. A text with an invalid
/// entity, or without `&`, comes back exactly as given (H3).
pub fn decode_text(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    // htmlescape's decoder accepts exactly H3's entity grammar and fails on
    // the first invalid entity; a failure means "keep the text".
    htmlescape::decode_html(text).map_or(Cow::Borrowed(text), |decoded| {
        Cow::Owned(decoded.replace('\u{a0}', " "))
    })
}

#[cfg(test)]
mod tests {
    use super::decode_text;

    fn d(text: &str) -> String {
        decode_text(text).into_owned()
    }

    #[test]
    fn decodes_the_whole_text_when_every_entity_is_valid() {
        assert_eq!(d("salt &amp; pepper"), "salt & pepper");
        assert_eq!(d("&lt;b&gt;"), "<b>");
        assert_eq!(d("caf&eacute; &Eacute;t&eacute;"), "café Été");
        assert_eq!(d("&#65;&#x42;&#x63;"), "ABc");
        assert_eq!(d("&#00065;"), "A");
        assert_eq!(d("&#0;z"), "\u{0}z");
        assert_eq!(d("&#1114111;"), "\u{10FFFF}");
        assert_eq!(d("&#x1F600; &#128512;"), "\u{1F600} \u{1F600}");
        assert_eq!(d("&euro;&hellip;"), "€…");
        // One pass only.
        assert_eq!(d("&amp;nbsp;"), "&nbsp;");
        assert_eq!(d("&amp;amp;"), "&amp;");
    }

    #[test]
    fn a_text_without_an_ampersand_is_unchanged() {
        assert_eq!(d("rock & roll"), "rock & roll");
        // Even its literal U+00A0 stays.
        assert_eq!(d("raw\u{a0}nbsp"), "raw\u{a0}nbsp");
        assert!(matches!(decode_text("plain"), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn any_invalid_entity_leaves_the_text_as_given() {
        for text in [
            "ok &amp; then &unknown; later",
            "rock &amp roll",
            "rock &amp; roll &",
            "&AMP;",
            "&EAcute;",
            "& amp;",
            "&;",
            "&#;",
            "&#x;",
            "&#X41;",
            "&#55296;",
            "&#xD800;",
            "&#1114112;",
            "&#x110000;",
            "&#99999999999;",
            "&apos;",
            "&amp b;",
            "raw\u{a0}nbsp &bogus; x",
        ] {
            assert_eq!(d(text), text, "{text:?}");
        }
    }

    #[test]
    fn no_break_spaces_become_spaces_after_a_decode() {
        assert_eq!(d("&nbsp;"), " ");
        assert_eq!(d("a&nbsp;b&#160;c&#xa0;d"), "a b c d");
        assert_eq!(d("raw\u{a0}nbsp &amp; x"), "raw nbsp & x");
    }
}
