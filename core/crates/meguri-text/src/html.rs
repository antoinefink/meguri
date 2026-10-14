//! Field HTML to plain text (`specs/text/strip-html.md`). Pattern-based on
//! purpose (H1): an HTML parser repairs malformed markup its own way, and the
//! stored sort field, checksum and duplicate checks would drift with it.

use std::borrow::Cow;
use std::sync::{LazyLock, OnceLock};

use regex::{Captures, Regex};

use crate::entities;
use crate::media_refs;

/// H2's block forms in the order they are tried (comment, style block,
/// script block), each with the closing delimiter it ends with.
const BLOCK_FORMS: [(&str, &str); 3] = [
    ("<!--.*?-->", "-->"),
    ("<style.*?>.*?</style>", "</style>"),
    ("<script.*?>.*?</script>", "</script>"),
];

/// H2's last form: any tag.
const TAG_FORM: &str = "<.*?>";

/// How many subsets of [`BLOCK_FORMS`] there are; bit `b` of a subset stands
/// for block form `b`.
const BLOCK_SETS: usize = 1 << BLOCK_FORMS.len();

/// Each block form's closing delimiter, case-insensitive like the form.
static CLOSERS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    BLOCK_FORMS.map(|(_, closer)| Regex::new(&format!("(?i){}", regex::escape(closer))).expect("H2 closer pattern"))
});

/// `MARKUP[set]`: the block forms in `set` and the tag form, in H2's order,
/// as one leftmost-first alternation. `(?s)` lets every run cross line
/// breaks; `(?i)` matters only for the block names. Compiled on first use.
static MARKUP: [OnceLock<Regex>; BLOCK_SETS] = [const { OnceLock::new() }; BLOCK_SETS];

fn markup(set: usize) -> &'static Regex {
    MARKUP[set].get_or_init(|| {
        let forms: Vec<&str> = BLOCK_FORMS
            .iter()
            .enumerate()
            .filter(|&(b, _)| set & (1 << b) != 0)
            .map(|(_, &(form, _))| form)
            .chain([TAG_FORM])
            .collect();
        Regex::new(&format!("(?si){}", forms.join("|"))).expect("H2 pattern")
    })
}

/// H6 step 1: `<br` + whitespace + optional `/` + `>`, a bare `<div>`, and LF.
static LINE_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i:<br\s*/?>|<div>)|\n").expect("H6 line break pattern"));

/// H6 step 2: `[[type:` + one or more non-`]` + `]]`.
static TYPE_ANSWER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[type:[^\]]+\]\]").expect("H6 type-answer pattern"));

/// H6 step 3: `[sound:` + one or more non-`]` + `]`, narrower than M3.
static LINE_SOUND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[sound:([^\]]+)\]").expect("H6 sound pattern"));

/// H2: removes comments, style and script blocks and tags, scanning left to
/// right. Entities stay. Cloze processing reuses this step (U61).
pub fn remove_markup(text: &str) -> Cow<'_, str> {
    // A block form that fails at a `<` has searched the rest of the text for
    // its closer, so a run of unclosed blocks took quadratic time. A block
    // form matches nowhere past the start of its last closer, so it leaves
    // the alternation there; no match changes. Closers never overlap, so
    // `find_iter` sees the last one.
    let last_closers: [Option<usize>; 3] =
        std::array::from_fn(|b| CLOSERS[b].find_iter(text).last().map(|closer| closer.start()));
    let mut kept = String::new();
    let mut at = 0;
    loop {
        let set = (0..BLOCK_FORMS.len())
            .filter(|&b| last_closers[b].is_some_and(|start| start >= at))
            .fold(0, |set, b| set | (1 << b));
        // Every form ends with `>`, so a match is never empty.
        let Some(found) = markup(set).find_at(text, at) else {
            break;
        };
        kept.push_str(&text[at..found.start()]);
        at = found.end();
    }
    if at == 0 {
        return Cow::Borrowed(text);
    }
    kept.push_str(&text[at..]);
    Cow::Owned(kept)
}

/// H4: [`remove_markup`], then entity decoding (H3). No trimming.
pub fn strip_markup(text: &str) -> Cow<'_, str> {
    match remove_markup(text) {
        Cow::Borrowed(rest) => entities::decode_text(rest),
        Cow::Owned(rest) => Cow::Owned(entities::decode_text(&rest).into_owned()),
    }
}

/// H5: like [`strip_markup`], but each media tag first becomes its raw file
/// name between two spaces, so the name survives the stripping.
pub fn strip_markup_keep_media(text: &str) -> Cow<'_, str> {
    let mut tags = media_refs::media_tags(text).peekable();
    if tags.peek().is_none() {
        return strip_markup(text);
    }
    let mut named = String::with_capacity(text.len());
    let mut copied = 0;
    for tag in tags {
        named.push_str(&text[copied..tag.start]);
        named.push(' ');
        named.push_str(tag.raw_name);
        named.push(' ');
        copied = tag.start + tag.matched.len();
    }
    named.push_str(&text[copied..]);
    Cow::Owned(strip_markup(&named).into_owned())
}

/// H6, media names dropped: the one-line text of browser cells, Card Info
/// and CSV export.
pub fn text_line(text: &str) -> String {
    let joined = LINE_BREAK.replace_all(text, " ");
    let unmarked = TYPE_ANSWER.replace_all(&joined, "");
    let silent = LINE_SOUND.replace_all(&unmarked, "");
    strip_markup(&silent).trim().to_owned()
}

/// H6, media names kept: sound tags become their names and media tags their
/// file names.
pub fn text_line_keep_media(text: &str) -> String {
    let joined = LINE_BREAK.replace_all(text, " ");
    let unmarked = TYPE_ANSWER.replace_all(&joined, "");
    let voiced = LINE_SOUND.replace_all(&unmarked, |sound: &Captures<'_>| sound[1].to_owned());
    strip_markup_keep_media(&voiced).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn markup_removal_follows_the_forms_in_order() {
        let r = |t: &str| remove_markup(t).into_owned();
        assert_eq!(r("ka<b>ta</b>na"), "katana");
        assert_eq!(r("keep<!-- hidden > still hidden -->this"), "keepthis");
        assert_eq!(r("<!---->a"), "a");
        assert_eq!(r("<!-->b-->c"), "c");
        assert_eq!(r("<!-- open > tail"), " tail");
        assert_eq!(r("x<style>p { color: red }</style>y"), "xy");
        assert_eq!(r("x<STYLE type=\"text/css\">p{}</StYlE>y"), "xy");
        assert_eq!(r("x<styles>q</style>y"), "xy");
        assert_eq!(r("x<style>no end"), "xno end");
        assert_eq!(r("x<style a>b</style >c</style>d"), "xd");
        assert_eq!(r("x<script>if (a > b) {}</script>y"), "xy");
        assert_eq!(r("x<script>a</style>b</script>c"), "xc");
        assert_eq!(r("<SCRIPT\nsrc=x>y</script>z"), "z");
        // U+017F LONG S matches `s` case-insensitively (H1).
        assert_eq!(r("<\u{17f}tyle>gone</style>shown"), "shown");
        assert_eq!(r("<\u{17f}cript>gone</script>shown"), "shown");
        assert_eq!(r("a < b > c"), "a  c");
        assert_eq!(r("a < b"), "a < b");
        assert_eq!(r("a<b<c>d"), "ad");
        assert_eq!(r("<<em>>"), ">");
        assert_eq!(r("x<\n>y"), "xy");
        assert_eq!(r("<img alt=\"q>r\" src=\"s.png\">"), "r\" src=\"s.png\">");
        assert_eq!(r("<"), "<");
        assert_eq!(r("&amp;"), "&amp;");
    }

    /// A block form that fails scans to the end of the text. Repeated
    /// unclosed blocks must not make that happen at every `<`: the time
    /// would grow with the square of the length.
    #[test]
    #[allow(clippy::disallowed_methods)] // times the stripping, never fed to it
    fn unclosed_blocks_take_linear_time() {
        const COPIES: usize = 2_000;
        let unclosed = format!("<style>a</style>{}", "<style>x<script>y<!-- >z".repeat(COPIES));
        let plain = format!("<style>a</style>{}", "<b>xxxxxx<i>yyyyyy<u>zzz".repeat(COPIES));
        assert_eq!(unclosed.len(), plain.len());
        let fastest = |text: &str, expected: &str| {
            (0..5)
                .map(|_| {
                    let start = Instant::now();
                    assert_eq!(remove_markup(text), expected);
                    start.elapsed()
                })
                .min()
                .expect("five runs")
        };
        let unclosed_time = fastest(&unclosed, &"xyz".repeat(COPIES));
        let plain_time = fastest(&plain, &"xxxxxxyyyyyyzzz".repeat(COPIES));
        assert!(
            unclosed_time < plain_time * 10,
            "{} bytes: {unclosed_time:?} unclosed, {plain_time:?} plain",
            unclosed.len()
        );
    }

    #[test]
    fn plain_stripping_decodes_after_removing_markup() {
        let s = |t: &str| strip_markup(t).into_owned();
        assert_eq!(s("<b>Tom</b> &amp; Jerry"), "Tom & Jerry");
        assert_eq!(s("&lt;b&gt;not a tag&lt;/b&gt;"), "<b>not a tag</b>");
        assert_eq!(s("<i>&lt;</i>tag<i>&gt;</i>"), "<tag>");
        assert_eq!(s("<b>&</b>"), "&");
        assert_eq!(s("ok &amp; <b>then</b> &unknown;"), "ok &amp; then &unknown;");
        assert_eq!(s(" <br> spaced\t"), "  spaced\t");
        assert_eq!(s("<img src=\"pic one.png\">caption"), "caption");
    }

    #[test]
    fn keeping_media_puts_raw_names_between_spaces() {
        let k = |t: &str| strip_markup_keep_media(t).into_owned();
        assert_eq!(k("<img src=\"a b.jpg\">text"), " a b.jpg text");
        assert_eq!(k("<img src=\"a<b>.jpg\">"), " a.jpg ");
        assert_eq!(k("<img src=\"a&amp;b.png\"> &bogus;"), " a&amp;b.png  &bogus;");
        assert_eq!(k("<img src=\"a&amp;b.png\">"), " a&b.png ");
        assert_eq!(k("[sound:x.mp3]"), "[sound:x.mp3]");
        assert_eq!(k("<b>私</b>"), "私");
        assert_eq!(k("<img src=\"https://e.com/a.png\">"), " https://e.com/a.png ");
    }

    #[test]
    fn text_lines_follow_the_steps() {
        assert_eq!(text_line("[sound:a b.mp3]c"), "c");
        assert_eq!(text_line_keep_media("[sound:a b.mp3]c"), "a b.mp3c");
        assert_eq!(text_line("<img src=\"pic.png\">[sound:say.mp3]"), "");
        assert_eq!(
            text_line_keep_media("<img src=\"pic.png\">[sound:say.mp3]"),
            "pic.png say.mp3"
        );
        assert_eq!(text_line("<div>block</div><div>block</div>"), "block block");
        assert_eq!(text_line("first\n\nsecond"), "first  second");
        assert_eq!(text_line("&nbsp;"), "");
        assert_eq!(text_line("a<br>b<BR/>c<br />d<br  >e<br\t/>f"), "a b c d e f");
        // Only the bare <div> becomes a space.
        assert_eq!(text_line("a<div class=x>b"), "ab");
        assert_eq!(text_line("a<DIV>b"), "a b");
        // CR is not a line break here.
        assert_eq!(text_line("a\r\nb"), "a\r b");
        assert_eq!(text_line("x[[type:Back]]y"), "xy");
        assert_eq!(text_line("x[[Type:Back]]y"), "x[[Type:Back]]y");
        assert_eq!(text_line("x[[type:]]y"), "x[[type:]]y");
        // The narrower sound form: no `]` inside the name.
        assert_eq!(text_line("[sound:]x]"), "[sound:]x]");
        assert_eq!(text_line_keep_media("[sound:a]b]"), "ab]");
        assert_eq!(text_line(" \u{3000}<b>x</b>\u{2028} "), "x");
        assert_eq!(text_line("<b>a</b>  <i>b</i>"), "a  b");
    }
}
