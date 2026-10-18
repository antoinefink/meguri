use meguri_text::{media_name::safe_media_name, normalize::nfc};

pub fn case_key(name: &str) -> String {
    name.to_lowercase()
}
pub fn lowercase_candidate(name: &str) -> String {
    safe_media_name(&case_key(name))
}
pub fn normalized(name: &str) -> bool {
    !name.is_empty() && safe_media_name(name) == name
}
pub fn safe_component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}
pub fn ignored(name: &str) -> bool {
    let name = name.strip_prefix(':').unwrap_or(name);
    let chars: Vec<_> = name
        .chars()
        .map(|c| if c == 'ſ' { 's' } else { c.to_ascii_lowercase() })
        .collect();
    chars.len() == 9
        && ((chars[..6] == ['t', 'h', 'u', 'm', 'b', 's'] && chars[7..] == ['d', 'b'])
            || chars[1..] == ['d', 's', '_', 's', 't', 'o', 'r', 'e'])
}
pub fn disk_name(name: &str) -> Option<String> {
    let normalized = nfc(name);
    if cfg!(target_vendor = "apple") || normalized == name {
        Some(normalized.into_owned())
    } else {
        None
    }
}
fn cut(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
pub fn hash_name(name: &str, hash: &str) -> String {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let ext = cut(ext, 10);
    format!("{}-{hash}.{ext}", cut(stem, 77 - ext.len()))
}
