//! C6-C9 and C11: the `cloze:` and `cloze-only:` filters.

use std::fmt::Write as _;
use std::sync::LazyLock;

use meguri_text::html;
use regex::{Captures, Regex};

use super::Side;
use super::tokenizer::{self, Cloze, Item};

/// C8: a backslash and `(` or `[`, the shortest run of any characters, a
/// backslash and `)` or `]`; leftmost-first (`text/strip-html#H1`).
static MATHJAX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\\[(\[](.*?)\\[)\]]").expect("C8 MathJax pattern"));

/// C9: the hint shown for a cloze that has none.
const NO_HINT: &str = "...";

/// The `cloze:` filter (C6, C8): `text` on card `card` (the template ordinal
/// plus one) for `side`. Empty when no cloze is active on that card (R975).
pub fn render(text: &str, card: u32, side: Side) -> String {
    let items = tokenizer::parse(text);
    if !items.iter().any(|item| decides_output(item, card)) {
        return String::new();
    }
    let mut out = String::new();
    for item in &items {
        render_item(item, card, side, &mut out);
    }
    mathjax(&out)
}

/// The `cloze-only:` filter (C9): the active clozes' hints (front) or plain
/// contents (back), joined by `, `. Nothing is encoded and C8 does not apply.
pub fn render_only(text: &str, card: u32, side: Side) -> String {
    entries(text, card, side).join(", ")
}

/// C9's entries: one per active cloze, a cloze before the clozes nested in
/// it. On the back they are also C10's.
pub(super) fn entries(text: &str, card: u32, side: Side) -> Vec<String> {
    let mut entries = Vec::new();
    tokenizer::for_each_cloze(&tokenizer::parse(text), &mut |cloze| {
        if cloze.is_active(card) {
            entries.push(match side {
                Side::Front => cloze.hint.unwrap_or(NO_HINT).to_owned(),
                Side::Back => cloze.plain_content(),
            });
        }
    });
    entries
}

/// C6 step 1: whether this item makes the output non-empty: an active
/// cloze, or one nested in a cloze that is not an image-occlusion cloze.
fn decides_output(item: &Item<'_>, card: u32) -> bool {
    match item {
        Item::Text(_) => false,
        Item::Cloze(cloze) => {
            cloze.is_active(card)
                || (cloze.image_occlusion_shape().is_none()
                    && cloze.items.iter().any(|child| decides_output(child, card)))
        }
    }
}

/// C6 step 2: one item by the C6 table.
fn render_item(item: &Item<'_>, card: u32, side: Side, out: &mut String) {
    let cloze = match item {
        Item::Text(text) => {
            out.push_str(text);
            return;
        }
        Item::Cloze(cloze) => cloze,
    };
    let list = cloze.number_list();
    let active = cloze.is_active(card);
    if let Some(shape) = cloze.image_occlusion_shape() {
        out.push_str(&image_occlusion_element(
            image_occlusion_class(cloze, active, side),
            &list,
            shape,
        ));
        return;
    }
    let mut content = String::new();
    for child in &cloze.items {
        render_item(child, card, side, &mut content);
    }
    // Writing to a String cannot fail.
    let _ = match (side, active) {
        (Side::Front, true) => write!(
            out,
            r#"<span class="cloze" data-cloze="{}" data-ordinal="{list}">[{}]</span>"#,
            htmlescape::encode_attribute(&content),
            cloze.hint.unwrap_or(NO_HINT),
        ),
        (Side::Back, true) => write!(out, r#"<span class="cloze" data-ordinal="{list}">{content}</span>"#),
        (_, false) => write!(
            out,
            r#"<span class="cloze-inactive" data-ordinal="{list}">{content}</span>"#
        ),
    };
}

/// C8: each MathJax match keeps its opener and closer, and loses the markup
/// of the run between them (`text/strip-html#H2`, entities kept).
fn mathjax(rendered: &str) -> String {
    MATHJAX
        .replace_all(rendered, |found: &Captures<'_>| {
            let whole = &found[0];
            let (opener, closer) = (&whole[..2], &whole[whole.len() - 2..]);
            format!("{opener}{}{closer}", html::remove_markup(&found[1]))
        })
        .into_owned()
}

/// C11: the class of an image-occlusion cloze's element.
fn image_occlusion_class(cloze: &Cloze<'_>, active: bool, side: Side) -> &'static str {
    if cloze.numbers.contains(&0) || (side == Side::Front && active) {
        "cloze"
    } else if !active {
        "cloze-inactive"
    } else {
        "cloze-highlight"
    }
}

/// C11: the hook for an image-occlusion cloze's element,
/// `<div class="…" data-ordinal="…" ` + the shape attributes + `></div>`.
/// The shape attributes are U64's (`render/image-occlusion`); until U64
/// fills this in, the element is the empty string.
fn image_occlusion_element(_class: &str, _number_list: &str, _shape: &str) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloze_of(text: &str) -> Cloze<'_> {
        match tokenizer::parse(text).into_iter().next() {
            Some(Item::Cloze(cloze)) => cloze,
            other => panic!("{text:?} starts with {other:?}"),
        }
    }

    #[test]
    fn image_occlusion_classes_follow_the_c11_table() {
        let one = cloze_of("{{c1::image-occlusion:rect}}");
        assert_eq!(one.image_occlusion_shape(), Some("rect"));
        assert_eq!(image_occlusion_class(&one, true, Side::Front), "cloze");
        assert_eq!(image_occlusion_class(&one, true, Side::Back), "cloze-highlight");
        assert_eq!(image_occlusion_class(&one, false, Side::Front), "cloze-inactive");
        assert_eq!(image_occlusion_class(&one, false, Side::Back), "cloze-inactive");
        let zero = cloze_of("{{c0,2::image-occlusion:rect}}");
        assert_eq!(image_occlusion_class(&zero, false, Side::Back), "cloze");
        assert_eq!(image_occlusion_class(&zero, true, Side::Back), "cloze");
    }

    #[test]
    fn only_a_leading_prefix_text_makes_an_image_occlusion_cloze() {
        assert_eq!(cloze_of("{{c1::image-occlusion:}}").image_occlusion_shape(), Some(""));
        assert_eq!(cloze_of("{{c1::Image-occlusion:x}}").image_occlusion_shape(), None);
        assert_eq!(
            cloze_of("{{c1::{{c2::y}}image-occlusion:x}}").image_occlusion_shape(),
            None
        );
        assert_eq!(cloze_of("{{c1:: image-occlusion:x}}").image_occlusion_shape(), None);
    }
}
