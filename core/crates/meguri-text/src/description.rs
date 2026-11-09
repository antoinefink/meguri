//! Deck descriptions as safe HTML (`specs/text/description-html.md`): Meguri's
//! own design, with no oracle parity. Unlike field stripping, this parses
//! with a real HTML5 parser: the output is only displayed.

mod fragment;

use pulldown_cmark::{Options, Parser, html};

use fragment::{Fragment, HTML_NAMESPACE, NodeData, NodeId};

/// IDs in a display fragment, decoded by the HTML5 parser without rewriting
/// its source. Replay allocation (render/display-transform D4) reserves these
/// before emitting any controls; authored lookalikes never become ownership.
pub fn element_ids(html: &str) -> std::collections::BTreeSet<String> {
    Fragment::parse(html)
        .nodes
        .into_iter()
        .filter_map(|node| match node.data {
            NodeData::Element { attrs, .. } => attrs
                .into_iter()
                .find(|attr| attr.name.local.as_ref() == "id")
                .map(|attr| attr.value.to_string()),
            _ => None,
        })
        .collect()
}

/// D3.1: elements dropped together with everything inside them.
const REMOVED_WITH_CONTENT: [&str; 37] = [
    "script", "style", "template", "iframe", "frame", "frameset", "object", "embed", "applet", "noscript", "noembed",
    "noframes", "title", "head", "link", "meta", "base", "svg", "math", "audio", "video", "source", "track", "canvas",
    "map", "area", "input", "button", "select", "option", "optgroup", "datalist", "textarea", "output", "progress",
    "meter", "dialog",
];

/// D3.2: elements written out; any other element is unwrapped.
const ALLOWED: [&str; 68] = [
    "a",
    "abbr",
    "b",
    "bdi",
    "bdo",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "pre",
    "q",
    "rb",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "time",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
];

/// Allowed elements that have no content and no end tag.
const VOID: [&str; 5] = ["br", "col", "hr", "img", "wbr"];

/// D3.5: properties a `style` declaration may set.
const STYLE_PROPERTIES: [&str; 9] = [
    "color",
    "background-color",
    "font-weight",
    "font-style",
    "font-size",
    "font-family",
    "text-decoration",
    "text-align",
    "vertical-align",
];

/// D3.5: a declaration whose value contains one of these is dropped.
const STYLE_FORBIDDEN: [&str; 9] = ["url(", "expression(", "\\", "<", ">", "@", "/*", "{", "}"];

/// D3.4: `data:` image types an `img` may show.
const DATA_IMAGE_TYPES: [&str; 5] = ["image/png", "image/jpeg", "image/gif", "image/webp", "image/avif"];

/// D1, D2: the HTML a deck page shows for a description: CommonMark when the
/// deck's `md` flag is set, then the sanitizer (D3) in both cases.
pub fn description_html(desc: &str, markdown: bool) -> String {
    if desc.is_empty() {
        return String::new();
    }
    if !markdown {
        return sanitize_html(desc);
    }
    // No extension is enabled: no tables, strikethrough, task lists,
    // footnotes or bare-URL links. Raw HTML passes through to the sanitizer,
    // and a soft break is written as a line break character.
    let mut rendered = String::with_capacity(desc.len() * 2);
    html::push_html(&mut rendered, Parser::new_ext(desc, Options::empty()));
    sanitize_html(&rendered)
}

/// D3: `html` parsed as an HTML5 fragment and written back with only the
/// allowed elements, attributes, URLs and style declarations. Also for other
/// short outside HTML Meguri displays (sync server messages, U161).
pub fn sanitize_html(html: &str) -> String {
    let fragment = Fragment::parse(html);
    let mut out = String::with_capacity(html.len());
    // An explicit stack instead of recursion: nesting depth is the input's.
    let mut stack = vec![Visit::Enter(0)];
    while let Some(visit) = stack.pop() {
        match visit {
            Visit::Close(name) => {
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
            Visit::Enter(id) => enter(&fragment, id, &mut out, &mut stack),
        }
    }
    out
}

enum Visit<'a> {
    Enter(NodeId),
    Close(&'a str),
}

/// Writes what node `id` contributes before its children and schedules its
/// children and end tag.
fn enter<'a>(fragment: &'a Fragment, id: NodeId, out: &mut String, stack: &mut Vec<Visit<'a>>) {
    let node = &fragment.nodes[id];
    let push_children = |stack: &mut Vec<Visit<'a>>| stack.extend(node.children.iter().rev().map(|&c| Visit::Enter(c)));
    match &node.data {
        NodeData::Document => push_children(stack),
        NodeData::Ignored => {}
        NodeData::Text(text) => escape_into(out, text, false),
        NodeData::Element { name, attrs } => {
            // SVG and MathML content only occurs inside svg and math, which
            // go with their content.
            if name.ns.as_ref() != HTML_NAMESPACE {
                return;
            }
            let tag: &str = name.local.as_ref();
            if REMOVED_WITH_CONTENT.contains(&tag) {
                return;
            }
            let Some(&tag) = ALLOWED.iter().find(|&&allowed| allowed == tag) else {
                push_children(stack);
                return;
            };
            if tag == "img"
                && !attrs
                    .iter()
                    .any(|a| plain_name(a) == Some("src") && image_source_allowed(&a.value))
            {
                return;
            }
            out.push('<');
            out.push_str(tag);
            for attr in attrs {
                let Some(attr_name) = plain_name(attr) else { continue };
                if let Some(value) = kept_value(tag, attr_name, &attr.value) {
                    out.push(' ');
                    out.push_str(attr_name);
                    out.push_str("=\"");
                    escape_into(out, &value, true);
                    out.push('"');
                }
            }
            if tag == "a" {
                out.push_str(" rel=\"noopener noreferrer\"");
            }
            out.push('>');
            if !VOID.contains(&tag) {
                stack.push(Visit::Close(tag));
                push_children(stack);
            }
        }
    }
}

/// The local name of an attribute in no namespace (every HTML attribute).
fn plain_name(attr: &html5ever::Attribute) -> Option<&str> {
    (attr.name.ns.is_empty() && attr.name.prefix.is_none()).then(|| attr.name.local.as_ref())
}

/// D3.3–D3.5: the value an attribute keeps on an allowed element, or `None`
/// when the attribute goes.
fn kept_value(tag: &str, attr: &str, value: &str) -> Option<String> {
    let allowed = match attr {
        "title" | "lang" | "dir" => true,
        "style" => return filtered_style(value),
        "href" => tag == "a" && link_allowed(value),
        "src" => tag == "img",
        "alt" | "width" | "height" => tag == "img",
        "color" | "size" | "face" => tag == "font",
        "start" | "reversed" | "type" => tag == "ol",
        "value" => tag == "li",
        "colspan" | "rowspan" | "valign" => matches!(tag, "td" | "th"),
        "align" => matches!(tag, "td" | "th" | "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"),
        "scope" => tag == "th",
        "span" => matches!(tag, "col" | "colgroup"),
        "datetime" => tag == "time",
        "open" => tag == "details",
        _ => false,
    };
    allowed.then(|| value.to_owned())
}

/// The URL as a browser parses it: C0 controls and spaces trimmed at both
/// ends, tabs and line breaks removed (the URL standard's preprocessing).
/// Every URL decision is made on this form, so whitespace cannot hide a
/// scheme.
fn as_browser_reads(url: &str) -> String {
    url.trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// The URL's scheme, lower-cased, or `None` for a relative URL.
fn scheme(url: &str) -> Option<String> {
    let (head, _) = url.split_once(':')?;
    let mut chars = head.chars();
    let starts_alpha = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (starts_alpha && rest_ok).then(|| head.to_ascii_lowercase())
}

/// D3.4: `a href` keeps absolute `http`, `https` and `mailto` URLs.
fn link_allowed(href: &str) -> bool {
    scheme(&as_browser_reads(href)).is_some_and(|s| matches!(s.as_str(), "http" | "https" | "mailto"))
}

/// D3.4: `img src` keeps `https` URLs, `data:` URLs of the listed image
/// types and bare media file names.
fn image_source_allowed(src: &str) -> bool {
    let url = as_browser_reads(src);
    match scheme(&url).as_deref() {
        Some("https") => true,
        Some("data") => {
            let body = &url["data:".len()..];
            body.split_once(',').is_some_and(|(meta, _)| {
                let media_type = meta.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
                DATA_IMAGE_TYPES.contains(&media_type.as_str())
            })
        }
        Some(_) => false,
        None => is_media_file_name(&url),
    }
}

/// A name the host resolves inside the media folder: not empty, not rooted
/// (`/`, or `\`, which browsers read as `/`), and without a `..` segment in
/// any spelling the URL standard treats as one.
fn is_media_file_name(url: &str) -> bool {
    if url.is_empty() || url.starts_with(['/', '\\']) {
        return false;
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    !path
        .split(['/', '\\'])
        .any(|segment| matches!(segment.to_ascii_lowercase().as_str(), ".." | ".%2e" | "%2e." | "%2e%2e"))
}

/// D3.5: the declarations of a `style` value that may stay, joined with
/// `; `, or `None` when none may.
fn filtered_style(style: &str) -> Option<String> {
    let kept: Vec<&str> = style
        .split(';')
        .map(str::trim)
        .filter(|declaration| {
            let Some((property, value)) = declaration.split_once(':') else {
                return false;
            };
            let property = property.trim().to_ascii_lowercase();
            let value = value.to_ascii_lowercase();
            STYLE_PROPERTIES.contains(&property.as_str()) && !STYLE_FORBIDDEN.iter().any(|bad| value.contains(bad))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

/// Escapes `text` for a text node, or for a double-quoted attribute value.
fn escape_into(out: &mut String, text: &str, in_attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' if !in_attribute => out.push_str("&lt;"),
            '>' if !in_attribute => out.push_str("&gt;"),
            '"' if in_attribute => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contract ignores a final line break (D4).
    fn render(desc: &str, markdown: bool) -> String {
        let out = description_html(desc, markdown);
        out.strip_suffix('\n').unwrap_or(&out).to_owned()
    }

    #[test]
    fn the_spec_examples() {
        for (md, input, output) in [
            (false, "<script>alert(1)</script><b>x</b>", "<b>x</b>"),
            (
                false,
                "<a href=\"https://example.org\" onclick=\"steal()\" target=\"_blank\">l</a>",
                "<a href=\"https://example.org\" rel=\"noopener noreferrer\">l</a>",
            ),
            (
                false,
                "<form action=\"https://example.org\"><input name=\"q\"><b>t</b></form>",
                "<b>t</b>",
            ),
            (
                false,
                "<link rel=\"stylesheet\" href=\"https://example.org/x.css\"><p>p</p>",
                "<p>p</p>",
            ),
            (false, "<b>bold</b>", "<b>bold</b>"),
            (false, "**bold**", "**bold**"),
            (true, "**bold**", "<p><strong>bold</strong></p>"),
            (
                true,
                "[site](javascript:alert(1))",
                "<p><a rel=\"noopener noreferrer\">site</a></p>",
            ),
            (
                false,
                "<img src=\"https://example.org/a.png\" onerror=\"x()\">",
                "<img src=\"https://example.org/a.png\">",
            ),
            (false, "<img src=\"http://example.org/a.png\">", ""),
            (false, "<img src=\"pic one.jpg\">", "<img src=\"pic one.jpg\">"),
            (
                false,
                "<span style=\"color: red; position: fixed\">r</span>",
                "<span style=\"color: red\">r</span>",
            ),
            (false, "<div class=\"x\" id=\"y\">d</div>", "<div>d</div>"),
            (false, "<ruby>漢<rt>かん</rt></ruby>", "<ruby>漢<rt>かん</rt></ruby>"),
            (false, "", ""),
            (true, "", ""),
        ] {
            assert_eq!(render(input, md), output, "md={md} {input:?}");
        }
    }

    #[test]
    fn removed_elements_take_their_content_along() {
        for tag in [
            "style", "template", "iframe", "noscript", "title", "textarea", "button", "select", "svg", "math",
        ] {
            let html = format!("a<{tag}>hidden</{tag}>b");
            assert_eq!(sanitize_html(&html), "ab", "{tag}");
        }
        for (input, output) in [
            ("a<audio src=x.mp3>fallback</audio>b", "ab"),
            ("a<object data=x>in</object>b<embed src=y>c", "abc"),
            ("<svg><a href=\"https://e.org\">x</a></svg>y", "y"),
            // HTML at an integration point stays inside the svg or math
            // element, so it goes with it.
            (
                "a<math><annotation-xml encoding=\"text/html\"><b>hidden</b></annotation-xml></math>z",
                "az",
            ),
            (
                "a<math><annotation-xml encoding=\"application/xhtml+xml\"><b>hidden</b></annotation-xml></math>z",
                "az",
            ),
            (
                "a<math><annotation-xml encoding=\"Text/HTML\"><p>hidden</p></annotation-xml></math>z",
                "az",
            ),
            ("a<math><mi><b>hidden</b></mi></math>z", "az"),
            ("a<svg><foreignObject><b>hidden</b></foreignObject></svg>z", "az"),
            // Without an HTML encoding, `b` leaves the math element, as it
            // does in a browser.
            (
                "a<math><annotation-xml><b>shown</b></annotation-xml></math>z",
                "a<b>shown</b>z",
            ),
        ] {
            assert_eq!(sanitize_html(input), output, "{input:?}");
        }
    }

    #[test]
    fn other_elements_are_unwrapped() {
        for (input, output) in [
            ("<fieldset><legend>L</legend><label>x</label></fieldset>", "Lx"),
            ("<my-widget>c</my-widget>", "c"),
            ("<html><body><b>x</b></body></html>", "<b>x</b>"),
            ("a<!-- note -->b", "ab"),
            ("<form><form><i>n</i></form></form>", "<i>n</i>"),
        ] {
            assert_eq!(sanitize_html(input), output, "{input:?}");
        }
    }

    #[test]
    fn attributes_are_filtered_per_element() {
        for (input, output) in [
            (
                "<p align=center title=t class=c onmouseover=x() data-a=1>p</p>",
                "<p align=\"center\" title=\"t\">p</p>",
            ),
            (
                "<font color=red size=3 face=serif style=\"color: blue\">f</font>",
                "<font color=\"red\" size=\"3\" face=\"serif\" style=\"color: blue\">f</font>",
            ),
            ("<span align=left>s</span>", "<span>s</span>"),
            (
                "<table><tr><td colspan=2 width=9>c</td></tr></table>",
                "<table><tbody><tr><td colspan=\"2\">c</td></tr></tbody></table>",
            ),
            (
                "<ol start=3 reversed><li value=5>i</li></ol>",
                "<ol start=\"3\" reversed=\"\"><li value=\"5\">i</li></ol>",
            ),
            (
                "<a href=\"https://e.org\" rel=\"opener\" ping=\"https://t\">x</a>",
                "<a href=\"https://e.org\" rel=\"noopener noreferrer\">x</a>",
            ),
            (
                "<img src=a.png srcset=\"b.png 2x\" alt=A width=1 height=2>",
                "<img src=\"a.png\" alt=\"A\" width=\"1\" height=\"2\">",
            ),
        ] {
            assert_eq!(sanitize_html(input), output, "{input:?}");
        }
    }

    #[test]
    fn link_urls_keep_only_web_and_mail_schemes() {
        for (href, kept) in [
            ("https://e.org/x", true),
            ("HTTP://e.org", true),
            ("mailto:a@e.org", true),
            ("javascript:alert(1)", false),
            ("JavaScript:alert(1)", false),
            ("java\tscript:alert(1)", false),
            (" javascript:alert(1)", false),
            ("data:text/html,x", false),
            ("file:///etc/passwd", false),
            ("/relative", false),
            ("page.html", false),
            ("#top", false),
            ("", false),
        ] {
            let html = format!("<a href=\"{href}\">x</a>");
            let out = sanitize_html(&html);
            assert_eq!(out.contains("href="), kept, "{href:?}: {out}");
            assert!(out.contains("rel=\"noopener noreferrer\""), "{out}");
        }
    }

    #[test]
    fn image_sources_keep_https_images_and_media_names() {
        for (src, kept) in [
            ("https://e.org/a.png", true),
            ("data:image/png;base64,iVBORw0K", true),
            ("DATA:IMAGE/JPEG;base64,/9j/", true),
            ("data:image/gif,x", true),
            ("data:image/webp;base64,x", true),
            ("data:image/avif;base64,x", true),
            ("pic.png", true),
            ("sub/pic.png", true),
            ("日本 語.png", true),
            ("http://e.org/a.png", false),
            ("//e.org/a.png", false),
            (" //e.org/a.png", false),
            ("\\\\e.org\\a.png", false),
            ("/abs.png", false),
            ("../up.png", false),
            ("a/../b.png", false),
            ("data:image/svg+xml,<svg/>", false),
            ("data:text/html,x", false),
            ("javascript:x", false),
            ("file:///x.png", false),
            ("", false),
        ] {
            let html = format!("<img src=\"{src}\">");
            let out = sanitize_html(&html);
            assert_eq!(!out.is_empty(), kept, "{src:?}: {out}");
        }
        assert_eq!(sanitize_html("a<img alt=x>b"), "ab");
    }

    #[test]
    fn inline_styles_keep_safe_declarations() {
        for (style, kept) in [
            ("color: red", Some("color: red")),
            ("COLOR: Red; Font-Weight: bold", Some("COLOR: Red; Font-Weight: bold")),
            (
                " color : red ;; text-align:center ",
                Some("color : red; text-align:center"),
            ),
            ("position: fixed", None),
            ("background-color: url(x.png)", None),
            ("background-color: URL(x.png)", None),
            ("color: expression(alert(1))", None),
            ("color: red /* x */", None),
            ("color: \\72 ed", None),
            ("color: red; font-family: <b>", Some("color: red")),
            ("font-size: 12px; behavior: url(x)", Some("font-size: 12px")),
            ("color", None),
            ("@import x; color: red", Some("color: red")),
            ("color: {red}", None),
        ] {
            let html = format!("<span style=\"{style}\">s</span>");
            let expected = match kept {
                Some(value) => format!("<span style=\"{value}\">s</span>"),
                None => "<span>s</span>".to_owned(),
            };
            assert_eq!(sanitize_html(&html), expected, "{style:?}");
        }
    }

    #[test]
    fn text_is_escaped_on_output() {
        assert_eq!(sanitize_html("a &lt;script&gt; &amp; b"), "a &lt;script&gt; &amp; b");
        assert_eq!(
            sanitize_html("<b title='\"x\" & y'>t</b>"),
            "<b title=\"&quot;x&quot; &amp; y\">t</b>"
        );
        assert_eq!(sanitize_html("<plaintext><b>x"), "&lt;b&gt;x");
    }

    #[test]
    fn markdown_is_commonmark_without_extensions() {
        assert_eq!(render("a\nb", true), "<p>a\nb</p>");
        assert_eq!(render("| a |\n|---|\n| b |", true), "<p>| a |\n|---|\n| b |</p>");
        assert_eq!(render("~~s~~", true), "<p>~~s~~</p>");
        assert_eq!(render("<b>raw</b> *x*", true), "<p><b>raw</b> <em>x</em></p>");
        assert_eq!(render("https://e.org", true), "<p>https://e.org</p>");
        // The raw HTML block's line break stays as text.
        assert_eq!(render("<script>x</script>\n\n# T", true), "\n<h1>T</h1>");
        assert_eq!(
            render("[l](https://e.org)", true),
            "<p><a href=\"https://e.org\" rel=\"noopener noreferrer\">l</a></p>"
        );
    }
}
