//! The `hint` filter (`specs/render/filters.md` F5, R965).

/// F5: a link labelled with the key that reveals the text, or the text
/// unchanged when it is only Unicode white space. The id is the first eight
/// bytes of the BLAKE3 hash of the text then the key, in hexadecimal.
pub fn hint(text: &str, key: &str) -> String {
    if text.trim().is_empty() {
        return text.to_owned();
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(text.as_bytes());
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let id: String = digest.as_bytes()[..8].iter().map(|b| format!("{b:02x}")).collect();
    // One line break, then six lines that each end with one.
    [
        "",
        r##"<a class=hint href="#""##,
        r#"onclick="this.style.display='none';"#,
        &format!("document.getElementById('hint{id}').style.display='block';"),
        r#"return false;" draggable=false>"#,
        &format!("{key}</a>"),
        &format!(r#"<div id="hint{id}" class=hint style="display: none">{text}</div>"#),
    ]
    .iter()
    .fold(String::new(), |mut out, line| {
        out.push_str(line);
        out.push('\n');
        out
    })
}
