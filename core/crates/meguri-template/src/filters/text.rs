//! The `text` filter (`specs/render/filters.md` F3).

use meguri_text::html;

/// F3: markup removed, then entities decoded all or nothing
/// (`text/strip-html#H4`).
pub fn text(text: &str) -> String {
    html::strip_markup(text).into_owned()
}
