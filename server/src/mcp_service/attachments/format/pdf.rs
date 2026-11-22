// SPDX-License-Identifier: MIT
//! A bounded envelope/token check, not a PDF renderer or a sanitizer.
//! Compressed object streams are inspected with a shared expansion budget.
use super::RECORDS;
use std::io::Read;
fn space(c: u8) -> bool {
    matches!(c, 0 | 9 | 10 | 12 | 13 | 32)
}
fn delimiter(c: u8) -> bool {
    space(c) || b"()<>[]{}/%".contains(&c)
}
fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
fn trim(mut b: &[u8]) -> &[u8] {
    while b.last().is_some_and(|v| space(*v)) {
        b = &b[..b.len() - 1];
    }
    b
}
fn name(b: &[u8]) -> Option<Vec<u8>> {
    let mut decoded = Vec::new();
    let mut at = 0;
    while at < b.len() {
        if decoded.len() >= 128 {
            return None;
        }
        if b[at] == b'#' {
            decoded.push((hex(*b.get(at + 1)?)? << 4) | hex(*b.get(at + 2)?)?);
            at += 3;
        } else {
            decoded.push(b[at]);
            at += 1;
        }
    }
    Some(decoded)
}
// Look ahead only through PDF tokens, sharing the walker's record budget.
fn token<'a>(b: &'a [u8], at: &mut usize, budget: &mut usize) -> Option<&'a [u8]> {
    loop {
        *budget = budget.checked_sub(1)?;
        match *b.get(*at)? {
            c if space(c) => {
                while b.get(*at).is_some_and(|c| space(*c)) {
                    *at += 1;
                }
            }
            b'%' => {
                while *at < b.len() && !matches!(b[*at], 10 | 13) {
                    *at += 1;
                }
            }
            _ => break,
        }
    }
    let start = *at;
    *at += 1;
    if b[start] == b'/' || !delimiter(b[start]) {
        while *at < b.len() && !delimiter(b[*at]) {
            *at += 1;
        }
    }
    Some(&b[start..*at])
}
fn unsigned(b: &[u8]) -> Option<usize> {
    let b = b.strip_prefix(b"+").unwrap_or(b);
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    b.iter().try_fold(0usize, |n, c| {
        n.checked_mul(10)?.checked_add(usize::from(c - b'0'))
    })
}
fn stream_length(b: &[u8], at: &mut usize, budget: &mut usize) -> Option<Option<usize>> {
    let value = unsigned(token(b, at, budget)?)?;
    let mut next = *at;
    let following = token(b, &mut next, budget)?;
    if !delimiter(following[0]) {
        // An indirect Length uses the bounded terminator fallback. Require
        // complete integer and R tokens, including when R touches >> or %.
        if value == 0 || unsigned(following)? > 65535 || token(b, &mut next, budget)? != b"R" {
            return None;
        }
        *at = next;
        Some(None)
    } else {
        Some(Some(value))
    }
}
fn passive_destination(b: &[u8], at: &mut usize, budget: &mut usize) -> Option<()> {
    if token(b, at, budget)? != b"["
        || unsigned(token(b, at, budget)?)? == 0
        || unsigned(token(b, at, budget)?)? > 65535
        || token(b, at, budget)? != b"R"
    {
        return None;
    }
    let mode = name(token(b, at, budget)?.strip_prefix(b"/")?)?;
    let (coordinates, nullable) = match mode.as_slice() {
        b"Fit" | b"FitB" => (0, false),
        b"XYZ" => (3, true),
        b"FitH" | b"FitV" | b"FitBH" | b"FitBV" => (1, true),
        b"FitR" => (4, false),
        _ => return None,
    };
    for _ in 0..coordinates {
        let value = token(b, at, budget)?;
        if nullable && value == b"null" {
            continue;
        }
        let number = value
            .strip_prefix(b"+")
            .or_else(|| value.strip_prefix(b"-"))
            .unwrap_or(value);
        if !number.iter().any(u8::is_ascii_digit)
            || number.iter().filter(|c| **c == b'.').count() > 1
            || !number.iter().all(|c| c.is_ascii_digit() || *c == b'.')
        {
            return None;
        }
    }
    (token(b, at, budget)? == b"]").then_some(())
}
#[derive(Default)]
struct Seen {
    catalog: bool,
    pages: bool,
    objects: usize,
    ends: usize,
}
fn tokens(
    b: &[u8],
    seen: &mut Seen,
    budget: &mut usize,
    expanded: &mut usize,
    nested: bool,
) -> Option<()> {
    let mut at = 0;
    let mut dictionary = Vec::<Vec<u8>>::new();
    let mut length = None;
    while at < b.len() {
        *budget = budget.checked_sub(1)?;
        match b[at] {
            c if space(c) => at += 1,
            b'%' => {
                while at < b.len() && !matches!(b[at], 10 | 13) {
                    at += 1;
                }
            }
            b'(' => {
                let mut depth = 1;
                at += 1;
                while at < b.len() && depth > 0 {
                    match b[at] {
                        b'\\' => {
                            at += 1;
                        }
                        b'(' => {
                            depth += 1;
                            if depth > 64 {
                                return None;
                            }
                        }
                        b')' => depth -= 1,
                        _ => {}
                    }
                    at += 1;
                }
                if depth != 0 {
                    return None;
                }
            }
            b'<' if b.get(at + 1) == Some(&b'<') => at += 2,
            b'>' if b.get(at + 1) == Some(&b'>') => at += 2,
            b'<' => {
                at += 1;
                while at < b.len() && b[at] != b'>' {
                    if !space(b[at]) && hex(b[at]).is_none() {
                        return None;
                    }
                    at += 1;
                }
                if at == b.len() {
                    return None;
                }
                at += 1;
            }
            b'/' => {
                at += 1;
                let start = at;
                while at < b.len() && !delimiter(b[at]) {
                    at += 1;
                }
                let name = name(&b[start..at])?;
                if matches!(
                    name.as_slice(),
                    b"JavaScript"
                        | b"JS"
                        | b"Launch"
                        | b"AA"
                        | b"RichMedia"
                        | b"EmbeddedFile"
                        | b"GoToR"
                        | b"SubmitForm"
                        | b"ImportData"
                        | b"Encrypt"
                        | b"XFA"
                ) {
                    return None;
                }
                seen.catalog |= name == b"Catalog";
                seen.pages |= name == b"Pages";
                if name == b"OpenAction" {
                    passive_destination(b, &mut at, budget)?;
                }
                if name == b"Length" {
                    length = stream_length(b, &mut at, budget)?;
                }
                dictionary.push(name);
                if dictionary.len() > 4096 {
                    return None;
                }
            }
            c if delimiter(c) => at += 1,
            _ => {
                let start = at;
                while at < b.len() && !delimiter(b[at]) {
                    at += 1;
                }
                match &b[start..at] {
                    b"obj" => {
                        seen.objects += 1;
                        dictionary.clear();
                        length = None;
                    }
                    b"endobj" => {
                        seen.ends += 1;
                        dictionary.clear();
                        length = None;
                    }
                    b"stream" => {
                        if nested {
                            return None;
                        }
                        match b.get(at) {
                            Some(13) => {
                                at += 1;
                                if b.get(at) == Some(&10) {
                                    at += 1;
                                }
                            }
                            Some(10) => at += 1,
                            _ => return None,
                        }
                        let end = if let Some(n) = length {
                            let mut end = at.checked_add(n)?;
                            b.get(at..end)?;
                            if b.get(end) == Some(&13) {
                                end += 1;
                            }
                            if b.get(end) == Some(&10) {
                                end += 1;
                            }
                            if b.get(end..end + 9)? != b"endstream" {
                                return None;
                            }
                            end
                        } else {
                            b[at..].windows(9).position(|v| v == b"endstream")? + at
                        };
                        if dictionary.iter().any(|v| v == b"ObjStm") {
                            let mut decoded = Vec::new();
                            if dictionary.iter().any(|v| v == b"FlateDecode") {
                                let decoder = flate2::read::ZlibDecoder::new(&b[at..end]);
                                decoder
                                    .take((*expanded + 1) as u64)
                                    .read_to_end(&mut decoded)
                                    .ok()?;
                            } else if dictionary.iter().any(|v| v == b"Filter") {
                                return None;
                            } else {
                                decoded.extend_from_slice(&b[at..end]);
                            }
                            *expanded = expanded.checked_sub(decoded.len())?;
                            tokens(&decoded, seen, budget, expanded, true)?;
                        }
                        at = end + 9;
                        dictionary.clear();
                    }
                    _ => {}
                }
            }
        }
    }
    Some(())
}
pub(super) fn valid(b: &[u8]) -> bool {
    fn check(b: &[u8]) -> Option<()> {
        if b.len() < 40
            || !b.starts_with(b"%PDF-")
            || !matches!(
                b.get(5..8)?,
                b"1.0" | b"1.1" | b"1.2" | b"1.3" | b"1.4" | b"1.5" | b"1.6" | b"1.7" | b"2.0"
            )
        {
            return None;
        }
        let tail = trim(b);
        if !tail.ends_with(b"%%EOF") {
            return None;
        }
        let before = trim(&tail[..tail.len() - 5]);
        let start = before.windows(9).rposition(|v| v == b"startxref")?;
        let offset = std::str::from_utf8(before.get(start + 9..)?)
            .ok()?
            .trim()
            .parse::<usize>()
            .ok()?;
        let xref = b.get(offset..)?;
        if !xref.starts_with(b"xref") {
            // Xref streams start at an indirect object; require /Type /XRef
            // within its bounded dictionary, before its binary stream.
            let end = xref.windows(6).position(|v| v == b"stream")?;
            if end > 65536 || !xref[..end].windows(5).any(|v| v == b"/XRef") {
                return None;
            }
        }
        let mut seen = Seen::default();
        let mut budget = RECORDS;
        let mut expanded = 16 * 1024 * 1024;
        tokens(b, &mut seen, &mut budget, &mut expanded, false)?;
        (seen.catalog && seen.pages && seen.objects > 0 && seen.objects == seen.ends).then_some(())
    }
    check(b).is_some()
}
