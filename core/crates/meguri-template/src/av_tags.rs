//! Whole-side audio extraction (render/av-tags A1–A11). HTML context does
//! not change the grammar; directives consume their opaque content at once.
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvSide {
    Question,
    Answer,
}
impl AvSide {
    pub fn letter(self) -> char {
        match self {
            Self::Question => 'q',
            Self::Answer => 'a',
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AvTag {
    Sound { name: String },
    Tts { language: String, content: String },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtractedSide {
    pub text: String,
    pub av: Vec<AvTag>,
}

#[derive(Clone, Copy)]
struct Name {
    end: usize,
    hash: u64,
}
#[derive(Clone, Copy)]
struct Closing {
    start: usize,
    name_start: usize,
    end: usize,
}
struct Index {
    names: HashMap<usize, Name>,
    closes: HashMap<(usize, u64), Vec<Closing>>,
    brackets: Vec<usize>,
    quotes: Vec<usize>,
}
fn white(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}
impl Index {
    fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut index = Self {
            names: HashMap::new(),
            closes: HashMap::new(),
            brackets: Vec::new(),
            quotes: Vec::new(),
        };
        let (mut end, mut hash, mut power) = (bytes.len(), 0u64, 1u64);
        // Index a name at each prefix without rescanning overlapping names.
        // Fingerprints only select candidates: full byte equality below is
        // authoritative, including when two distinct names have one hash.
        for i in (0..bytes.len()).rev() {
            if white(bytes[i]) || bytes[i] == b']' {
                end = i;
                hash = 0;
                power = 1;
            } else {
                hash = (u64::from(bytes[i]) + 1).wrapping_mul(power).wrapping_add(hash);
                power = power.wrapping_mul(257);
            }
            if bytes[i] == b']' {
                index.brackets.push(i);
            }
            if bytes[i] == b'"' {
                index.quotes.push(i);
            }
            if end == i {
                continue;
            }
            if i >= 6 && &bytes[i - 6..i] == b"[anki:" {
                index.names.insert(i, Name { end, hash });
            }
            if i >= 7 && &bytes[i - 7..i] == b"[/anki:" && bytes.get(end) == Some(&b']') {
                index.closes.entry((end - i, hash)).or_default().push(Closing {
                    start: i - 7,
                    name_start: i,
                    end,
                });
            }
        }
        index.brackets.reverse();
        index.quotes.reverse();
        for closes in index.closes.values_mut() {
            closes.reverse();
        }
        index
    }
    fn next(items: &[usize], at: usize) -> Option<usize> {
        items.get(items.partition_point(|p| *p < at)).copied()
    }
}
struct Directive<'a> {
    name: &'a str,
    options: Vec<(&'a str, &'a str)>,
    content: &'a str,
    end: usize,
}
fn directive<'a>(text: &'a str, start: usize, index: &Index) -> Option<Directive<'a>> {
    let name_start = start + 6;
    let name_info = index.names.get(&name_start)?;
    let name = &text[name_start..name_info.end];
    let candidates = index.closes.get(&(name.len(), name_info.hash))?;
    if candidates.last().is_none_or(|close| close.start < name_info.end) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut at = name_info.end;
    let mut options = Vec::new();
    let mut positions = HashMap::new();
    loop {
        while bytes.get(at).copied().is_some_and(white) {
            at += 1;
        }
        if bytes.get(at) == Some(&b']') {
            at += 1;
            break;
        }
        let key_start = at;
        while bytes.get(at).is_some_and(|b| !white(*b) && *b != b']' && *b != b'=') {
            at += 1;
        }
        if at == key_start || bytes.get(at) != Some(&b'=') {
            return None;
        }
        let key = &text[key_start..at];
        at += 1;
        let quoted_end = if bytes.get(at) == Some(&b'"') {
            Index::next(&index.quotes, at + 1)
        } else {
            None
        };
        let value = if let Some(close) = quoted_end {
            let value = &text[at + 1..close];
            at = close + 1;
            value
        } else {
            let value_start = at;
            while bytes.get(at).is_some_and(|b| !white(*b) && *b != b']' && *b != b'"') {
                at += 1;
            }
            &text[value_start..at]
        };
        if let Some(position) = positions.get(key).copied() {
            options[position] = (key, value);
        } else {
            positions.insert(key, options.len());
            options.push((key, value));
        }
    }
    let first = candidates.partition_point(|close| close.start < at);
    let close = candidates[first..]
        .iter()
        .find(|close| &text[close.name_start..close.end] == name)?;
    Some(Directive {
        name,
        options,
        content: &text[at..close.start],
        end: close.end + 1,
    })
}
fn normal(out: &mut String, tag: &Directive<'_>) {
    out.push_str("[anki:");
    out.push_str(tag.name);
    for (key, value) in &tag.options {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        let quote = value.bytes().any(|b| white(b) || b == b']');
        if quote {
            out.push('"');
        }
        out.push_str(value);
        if quote {
            out.push('"');
        }
    }
    out.push(']');
    out.push_str(tag.content);
    out.push_str("[/anki:");
    out.push_str(tag.name);
    out.push(']');
}
fn marker(out: &mut ExtractedSide, side: AvSide, tag: AvTag) {
    out.text
        .push_str(&format!("[anki:play:{}:{}]", side.letter(), out.av.len()));
    out.av.push(tag);
}

pub fn extract(text: &str, side: AvSide) -> ExtractedSide {
    let index = Index::new(text);
    let mut out = ExtractedSide {
        text: String::with_capacity(text.len()),
        av: Vec::new(),
    };
    let mut at = 0;
    while let Some(relative) = text[at..].find('[') {
        let start = at + relative;
        out.text.push_str(&text[at..start]);
        if text[start..].starts_with("[sound:") {
            let name_start = start + 7;
            if let Some(end) = Index::next(&index.brackets, name_start)
                && end > name_start
            {
                marker(
                    &mut out,
                    side,
                    AvTag::Sound {
                        name: meguri_text::entities::decode_text(&text[name_start..end]).into_owned(),
                    },
                );
                at = end + 1;
                continue;
            }
        }
        if text[start..].starts_with("[anki:")
            && let Some(tag) = directive(text, start, &index)
        {
            if tag.name == "tts" {
                let language = tag
                    .options
                    .iter()
                    .find(|(key, _)| *key == "lang")
                    .map_or("", |(_, value)| *value);
                if language.is_empty() {
                    out.text.push_str("[tts: lang not set]");
                } else {
                    marker(
                        &mut out,
                        side,
                        AvTag::Tts {
                            language: language.into(),
                            content: tag.content.into(),
                        },
                    );
                }
            } else {
                normal(&mut out.text, &tag);
            }
            at = tag.end;
            continue;
        }
        out.text.push('[');
        at = start + 1;
    }
    out.text.push_str(&text[at..]);
    out
}
