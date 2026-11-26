//! Candidate names from package/apkg-export A9. Existing field media/HTML
//! parsers own entity decoding and HTML removal.
use meguri_model::notetype::Notetype;
use regex::Regex;
use sha1::{Digest, Sha1};
use std::{collections::BTreeSet, sync::LazyLock};

static LATEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)\[latex\](.*?)\[/latex\]|\[\$\](.*?)\[/\$\]|\[\$\$\](.*?)\[/\$\$\]").expect("A9.3")
});
static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br>|<br/>|<div>").expect("A9.3 line breaks"));
static CSS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)@import\s+"(_[^"]*.css)"|'(_[^']*.css)'|url\(\s*"(_[^"]+)"|'(_[^']+)'|(_.+?)\s*\)"#)
        .expect("A9.4")
});
static TEMPLATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\[sound:(_[^\]]+)\]|"(_[^"]+)"|'(_[^']+)'|\b(?:src|data)=(_[^> \t\n\x0c\r]+)"#).expect("A9.5")
});

fn add(out: &mut BTreeSet<String>, name: &str) {
    let name = meguri_text::normalize::nfc(name).into_owned();
    if !name.is_empty() && name != "." && name != ".." && !name.contains('/') {
        out.insert(name);
    }
}
pub(crate) fn fields(out: &mut BTreeSet<String>, field: &str, svg: bool) {
    for name in meguri_text::media_refs::references(field) {
        add(out, &name);
    }
    for captures in LATEX.captures_iter(field) {
        let text = if let Some(body) = captures.get(1) {
            body.as_str().to_owned()
        } else if let Some(body) = captures.get(2) {
            format!("${}$", body.as_str())
        } else {
            format!(
                "\\begin{{displaymath}}{}\\end{{displaymath}}",
                captures.get(3).expect("third form").as_str()
            )
        };
        let lines = LINE.replace_all(&text, "\n");
        let plain = meguri_text::html::strip_markup(&lines);
        add(
            out,
            &format!(
                "latex-{:x}.{}",
                Sha1::digest(plain.as_bytes()),
                if svg { "svg" } else { "png" }
            ),
        );
    }
}
pub(crate) fn notetype(out: &mut BTreeSet<String>, nt: &Notetype) {
    let mut scan = |pattern: &Regex, text: &str| {
        for captures in pattern.captures_iter(text) {
            if let Some(name) = captures.iter().skip(1).flatten().next() {
                add(out, name.as_str());
            }
        }
    };
    scan(&CSS, &nt.config.css);
    for template in &nt.templates {
        scan(&TEMPLATE, &template.config.q_format);
        scan(&TEMPLATE, &template.config.a_format);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latex_hashes_and_decoded_references() {
        let mut out = BTreeSet::new();
        fields(
            &mut out,
            r#"[latex]a<br/>b[/latex]<img src="x&amp;y.png">[sound:é.mp3]"#,
            false,
        );
        assert!(out.contains("latex-fcd127ffa1016069006ad91f3f361248f9bdf272.png"));
        assert!(out.contains("x&y.png"));
        assert!(out.contains("é.mp3"));
    }
    #[test]
    fn static_patterns_keep_spec_matching_rules() {
        let names = |re: &Regex, s: &str| {
            re.captures_iter(s)
                .map(|c| c.iter().skip(1).flatten().next().unwrap().as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&CSS, r#"@IMPORT "_a.CSS"; url( _f.woff ) url('_g.ttf')"#),
            ["_a.CSS", "_f.woff", "_g.ttf"]
        );
        assert_eq!(
            names(&TEMPLATE, r#"<img src=_x.png alt=a class="_hint">[sound:_t.mp3]"#),
            ["_x.png", "_hint", "_t.mp3"]
        );
    }
}
