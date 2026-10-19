//! A6: normalization and token coordinates stay separate from display text.
use super::{Descriptor, Mode, unicode10_marks::is_mark};
use difflib::sequencematcher::SequenceMatcher;
use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

static BREAKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:\n|<br\s?/?>|</?div>)+").expect("A6 line breaks"));

pub fn expected_text(expected: &str) -> String {
    let silent = crate::av_tags::remove(expected);
    let lines = BREAKS.replace_all(&silent, " ");
    meguri_text::html::strip_markup(&lines).trim().to_owned()
}

pub fn compare(expected: &str, typed: &str, mode: Mode, ci: bool) -> String {
    let expected = expected_text(expected);
    if typed.is_empty() {
        return format!("<code id=typeans>{}</code>", htmlescape::encode_minimal(&expected));
    }
    let (t, x, entries, display): (Vec<char>, Vec<char>, Vec<String>, String) = match mode {
        Mode::Plain => {
            let x: Vec<char> = expected.nfc().collect();
            let display = x.iter().collect();
            (typed.nfc().collect(), x, Vec::new(), display)
        }
        Mode::Nc => {
            let t = typed.nfkd().filter(|c| !is_combining_mark(*c)).collect();
            let mut x = Vec::new();
            let mut entries: Vec<String> = Vec::new();
            for ch in expected.nfkd() {
                if is_combining_mark(ch) {
                    if let Some(entry) = entries.last_mut() {
                        entry.push(ch);
                    }
                } else {
                    x.push(ch);
                    entries.push(ch.to_string());
                }
            }
            (t, x, entries, expected)
        }
    };
    let fold = |chars: &[char]| -> Vec<char> {
        if ci {
            let mapper = icu_casemap::CaseMapper::new();
            chars.iter().map(|c| mapper.simple_fold(*c)).collect()
        } else {
            chars.to_vec()
        }
    };
    let (tf, xf) = (fold(&t), fold(&x));
    if tf == xf {
        return format!(
            "<code id=typeans><span class=typeGood>{}</span></code>",
            htmlescape::encode_minimal(&display)
        );
    }
    let mut top = String::new();
    let mut bottom = String::new();
    let mut matcher = SequenceMatcher::new(&tf, &xf);
    for op in matcher.get_opcodes() {
        let (i, end_i, j, end_j) = (op.first_start, op.first_end, op.second_start, op.second_end);
        match &op.tag[..] {
            "equal" => token(&mut top, "typeGood", &isolated(&t[i..end_i])),
            "delete" | "replace" => token(&mut top, "typeBad", &isolated(&t[i..end_i])),
            "insert" => token(&mut top, "typeMissed", &"-".repeat(end_j - j)),
            _ => unreachable!("difflib opcode"),
        }
        if op.tag != "delete" {
            let class = if op.tag == "equal" { "typeGood" } else { "typeMissed" };
            let text = if mode == Mode::Nc {
                entries[j..end_j].concat()
            } else {
                isolated(&x[j..end_j])
            };
            token(&mut bottom, class, &text);
        }
    }
    format!("<code id=typeans>{top}<br><span id=typearrow>&darr;</span><br>{bottom}</code>")
}
fn isolated(chars: &[char]) -> String {
    let mut out = String::new();
    if chars.first().is_some_and(|c| is_mark(*c)) {
        out.push('\u{a0}');
    }
    out.extend(chars);
    out
}
fn token(out: &mut String, class: &str, text: &str) {
    out.push_str(&format!(
        "<span class={class}>{}</span>",
        htmlescape::encode_minimal(text)
    ));
}

/// The reveal-time comparison C. The caller inserts this into each answer slot.
pub fn compare_typed_answer(descriptor: &Descriptor, typed: &str) -> String {
    compare(
        &descriptor.expected,
        typed,
        descriptor.mode,
        descriptor.case_insensitive,
    )
}
