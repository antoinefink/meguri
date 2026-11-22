// SPDX-License-Identifier: MIT
//! A bounded envelope/token check, not a PDF renderer or a sanitizer.
//! Compressed object streams are inspected with a shared expansion budget.
use super::RECORDS;
use std::collections::{BTreeMap, BTreeSet};
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
#[derive(Clone, Copy)]
enum Length {
    Direct(usize),
    Indirect { object: usize, generation: usize },
}
fn stream_length(b: &[u8], at: &mut usize, budget: &mut usize) -> Option<Length> {
    let value = unsigned(token(b, at, budget)?)?;
    let mut next = *at;
    let following = token(b, &mut next, budget)?;
    if !delimiter(following[0]) {
        let generation = unsigned(following)?;
        if value == 0 || generation > 65535 || token(b, &mut next, budget)? != b"R" {
            return None;
        }
        *at = next;
        Some(Length::Indirect { object: value, generation })
    } else {
        Some(Length::Direct(value))
    }
}
// A stream is skipped only by its declared length, never by searching its data.
fn stream_boundary(b: &[u8], start: usize, length: usize) -> Option<(usize, usize)> {
    let data_end = start.checked_add(length)?;
    b.get(start..data_end)?;
    let mut end = data_end;
    if b.get(end) == Some(&13) { end += 1; }
    if b.get(end) == Some(&10) { end += 1; }
    let after = end.checked_add(9)?;
    if b.get(end..after)? != b"endstream" || !b.get(after).is_some_and(|c| delimiter(*c)) {
        return None;
    }
    Some((data_end, after))
}
fn decode_stream(b: &[u8], flate: bool, expanded: &mut usize) -> Option<Vec<u8>> {
    let mut decoded = Vec::new();
    if flate {
        flate2::read::ZlibDecoder::new(b)
            .take((*expanded + 1) as u64)
            .read_to_end(&mut decoded).ok()?;
    } else {
        if b.len() > *expanded { return None; }
        decoded.extend_from_slice(b);
    }
    *expanded = expanded.checked_sub(decoded.len())?;
    Some(decoded)
}
// Skip a metadata value structurally, so names inside strings or nested values
// cannot impersonate xref dictionary keys. All lookups share the walker budget.
fn value<'a>(b: &'a [u8], at: &mut usize, budget: &mut usize, depth: usize) -> Option<&'a [u8]> {
    if depth > 64 { return None; }
    let first = token(b, at, budget)?;
    let start = *at - first.len();
    match first {
        b"(" => {
            let mut nesting = 1;
            while nesting > 0 {
                match *b.get(*at)? {
                    b'\\' => { *at += 1; b.get(*at)?; }
                    b'(' => { nesting += 1; if nesting > 64 { return None; } }
                    b')' => nesting -= 1,
                    _ => {}
                }
                *at += 1;
            }
        }
        b"[" | b"<" => {
            let dictionary = first == b"<" && b.get(*at) == Some(&b'<');
            if dictionary { *at += 1; }
            if first == b"<" && !dictionary {
                while *b.get(*at)? != b'>' {
                    if !space(b[*at]) && hex(b[*at]).is_none() { return None; }
                    *at += 1;
                }
                *at += 1;
            } else {
                loop {
                    let mut next = *at;
                    let t = token(b, &mut next, budget)?;
                    if (!dictionary && t == b"]") || (dictionary && t == b">") {
                        if dictionary && token(b, &mut next, budget)? != b">" { return None; }
                        *at = next;
                        break;
                    }
                    value(b, at, budget, depth + 1)?;
                }
            }
        }
        _ if unsigned(first).is_some() => {
            let mut next = *at;
            if token(b, &mut next, budget).and_then(unsigned).is_some()
                && token(b, &mut next, budget)? == b"R" {
                *at = next;
            }
        }
        _ => {}
    }
    Some(&b[start..*at])
}
fn metadata<'a>(b: &'a [u8], at: &mut usize, budget: &mut usize) -> Option<BTreeMap<Vec<u8>, &'a [u8]>> {
    if token(b, at, budget)? != b"<" || token(b, at, budget)? != b"<" { return None; }
    let mut fields = BTreeMap::new();
    loop {
        let key = token(b, at, budget)?;
        if key == b">" {
            return (token(b, at, budget)? == b">").then_some(fields);
        }
        let key = name(key.strip_prefix(b"/")?)?;
        if fields.insert(key, value(b, at, budget, 0)?).is_some() { return None; }
    }
}
fn numbers(b: &[u8], budget: &mut usize) -> Option<Vec<usize>> {
    let mut at = 0;
    if token(b, &mut at, budget)? != b"[" { return None; }
    let mut values = Vec::new();
    loop {
        let t = token(b, &mut at, budget)?;
        if t == b"]" { return Some(values); }
        values.push(unsigned(t)?);
    }
}
// Xref streams commonly use PNG row predictors. Reconstruct rows in place so
// the shared expansion budget covers the complete decoded allocation.
fn xref_predictor(mut bytes: Vec<u8>, params: &[u8], row: usize, budget: &mut usize) -> Option<Vec<u8>> {
    let fields = metadata(params, &mut 0, budget)?;
    let number = |key: &[u8], default| -> Option<usize> {
        fields.get(key).map_or(Some(default), |v| unsigned(v))
    };
    let predictor = number(b"Predictor", 1)?;
    if predictor == 1 { return Some(bytes); }
    if !(10..=15).contains(&predictor) || number(b"Colors", 1)? != 1
        || number(b"BitsPerComponent", 8)? != 8 || number(b"Columns", 1)? != row {
        return None;
    }
    let stride = row.checked_add(1)?;
    if bytes.len() % stride != 0 { return None; }
    let mut out = 0;
    for source in (0..bytes.len()).step_by(stride) {
        *budget = budget.checked_sub(1)?;
        let filter = bytes[source];
        for column in 0..row {
            let left = if column > 0 { bytes[out - 1] } else { 0 };
            let up = if out >= row { bytes[out - row] } else { 0 };
            let upper_left = if out >= row && column > 0 { bytes[out - row - 1] } else { 0 };
            let add = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => {
                    let p = i32::from(left) + i32::from(up) - i32::from(upper_left);
                    let distances = [(p - i32::from(left)).abs(), (p - i32::from(up)).abs(), (p - i32::from(upper_left)).abs()];
                    if distances[0] <= distances[1] && distances[0] <= distances[2] { left }
                    else if distances[1] <= distances[2] { up } else { upper_left }
                }
                _ => return None,
            };
            bytes[out] = bytes[source + 1 + column].wrapping_add(add);
            out += 1;
        }
    }
    bytes.truncate(out);
    Some(bytes)
}
#[derive(Clone, Copy)]
enum Entry {
    Free,
    Plain { generation: usize, offset: usize },
    Compressed { stream: usize, index: usize },
}
struct Revision {
    xref: usize,
    entries: BTreeMap<usize, Entry>,
}
struct Lookup<'a> {
    file: &'a [u8],
    xref: usize,
    revisions: Vec<Revision>,
    scalars: BTreeMap<(usize, usize, usize), usize>,
    resolving: BTreeSet<(usize, usize, usize)>,
    scalar_offsets: BTreeSet<usize>,
    streams: BTreeMap<usize, usize>,
    inspected: BTreeSet<usize>,
}
impl Lookup<'_> {
    fn section(&self, offset: usize, budget: &mut usize, expanded: &mut usize)
        -> Option<(BTreeMap<usize, Entry>, Option<usize>, Option<usize>)> {
        let mut offsets = BTreeMap::new();
        *budget = budget.checked_sub(1)?;
        let b = self.file.get(offset..)?;
        let mut at = 0;
        let first = token(b, &mut at, budget)?;
        let fields;
        if first == b"xref" {
            loop {
                let t = token(b, &mut at, budget)?;
                if t == b"trailer" { break; }
                let start = unsigned(t)?;
                let count = unsigned(token(b, &mut at, budget)?)?;
                start.checked_add(count)?;
                for object in start..start + count {
                    let position = unsigned(token(b, &mut at, budget)?)?;
                    let generation = unsigned(token(b, &mut at, budget)?)?;
                    let entry = match token(b, &mut at, budget)? {
                        b"n" if generation <= 65535 => Entry::Plain { generation, offset: position },
                        b"f" => Entry::Free,
                        _ => return None,
                    };
                    offsets.entry(object).or_insert(entry);
                }
            }
            fields = metadata(b, &mut at, budget)?;
        } else {
            unsigned(first)?;
            unsigned(token(b, &mut at, budget)?)?;
            if token(b, &mut at, budget)? != b"obj" { return None; }
            fields = metadata(b, &mut at, budget)?;
            if name(fields.get(b"Type".as_slice())?.strip_prefix(b"/")?)? != b"XRef" {
                return None;
            }
            let length = unsigned(fields.get(b"Length".as_slice())?)?;
            if token(b, &mut at, budget)? != b"stream" { return None; }
            match b.get(at) {
                Some(13) => { at += 1; if b.get(at) == Some(&10) { at += 1; } }
                Some(10) => at += 1,
                _ => return None,
            }
            let (end, _) = stream_boundary(b, at, length)?;
            let flate = if let Some(filter) = fields.get(b"Filter".as_slice()) {
                let mut pos = 0;
                let first = token(filter, &mut pos, budget)?;
                let array = first == b"[";
                let filter_name = if array { token(filter, &mut pos, budget)? } else { first };
                if name(filter_name.strip_prefix(b"/")?)? != b"FlateDecode"
                    || (array && token(filter, &mut pos, budget)? != b"]") {
                    return None;
                }
                true
            } else { false };
            let mut decoded = decode_stream(&b[at..end], flate, expanded)?;
            let widths = numbers(fields.get(b"W".as_slice())?, budget)?;
            if widths.len() != 3 || widths.iter().any(|w| *w > std::mem::size_of::<usize>()) { return None; }
            let row: usize = widths.iter().sum();
            if row == 0 { return None; }
            if let Some(params) = fields.get(b"DecodeParms".as_slice()) {
                if *params != b"null" {
                    let mut pos = 0;
                    let params = if token(params, &mut pos, budget)? == b"[" {
                        let item = value(params, &mut pos, budget, 0)?;
                        if token(params, &mut pos, budget)? != b"]" { return None; }
                        item
                    } else { *params };
                    if params != b"null" {
                        decoded = xref_predictor(decoded, params, row, budget)?;
                    }
                }
            }
            let size = unsigned(fields.get(b"Size".as_slice())?)?;
            let index = match fields.get(b"Index".as_slice()) {
                Some(index) => numbers(index, budget)?,
                None => vec![0, size],
            };
            if index.len() % 2 != 0 { return None; }
            let mut cursor = 0usize;
            for pair in index.chunks_exact(2) {
                let stop = pair[0].checked_add(pair[1])?;
                if stop > size { return None; }
                for object in pair[0]..stop {
                    *budget = budget.checked_sub(1)?;
                    let bytes = decoded.get(cursor..cursor.checked_add(row)?)?;
                    cursor += row;
                    let mut parts = [0usize; 3];
                    let mut column = 0;
                    for (part, width) in parts.iter_mut().zip(&widths) {
                        for byte in &bytes[column..column + width] {
                            *part = part.checked_mul(256)?.checked_add(usize::from(*byte))?;
                        }
                        column += width;
                    }
                    let kind = if widths[0] == 0 { 1 } else { parts[0] };
                    let entry = match kind {
                        1 if parts[2] <= 65535 => Entry::Plain { generation: parts[2], offset: parts[1] },
                        0 => Entry::Free,
                        2 if parts[1] > 0 => Entry::Compressed { stream: parts[1], index: parts[2] },
                        _ => return None,
                    };
                    offsets.entry(object).or_insert(entry);
                }
            }
            if cursor != decoded.len() { return None; }
        }
        let link = |key: &[u8]| -> Option<Option<usize>> {
            match fields.get(key) {
                Some(v) => Some(Some(unsigned(v)?)),
                None => Some(None),
            }
        };
        Some((offsets, link(b"Prev")?, link(b"XRefStm")?))
    }
    fn load(&mut self, budget: &mut usize, expanded: &mut usize) -> Option<()> {
        let mut sections = BTreeMap::new();
        let mut hybrids = BTreeSet::new();
        let mut pending = vec![self.xref];
        while let Some(offset) = pending.pop() {
            *budget = budget.checked_sub(1)?;
            if sections.contains_key(&offset) { continue; }
            let section = self.section(offset, budget, expanded)?;
            if let Some(prev) = section.1 { pending.push(prev); }
            if let Some(hybrid) = section.2 {
                hybrids.insert(hybrid);
                pending.push(hybrid);
            }
            sections.insert(offset, section);
        }
        // Both links are followed: a hybrid's Prev can point to the previous
        // supplement while the table's Prev points to the previous table.
        for (&xref, (entries, _, hybrid)) in sections.iter().rev() {
            if hybrids.contains(&xref) && xref != self.xref { continue; }
            *budget = budget.checked_sub(entries.len())?;
            let mut entries = entries.clone();
            let mut next = *hybrid;
            let mut visited = BTreeSet::from([xref]);
            while let Some(offset) = next {
                if !visited.insert(offset) { return None; }
                let (extra, _, hybrid) = sections.get(&offset)?;
                *budget = budget.checked_sub(extra.len())?;
                // A hybrid stream supplies the compressed entries represented
                // as free placeholders in its companion table.
                entries.extend(extra.iter().map(|(&id, &entry)| (id, entry)));
                next = *hybrid;
            }
            self.revisions.push(Revision { xref, entries });
        }
        (!self.revisions.is_empty()).then_some(())
    }
    fn entry(&self, revision: usize, object: usize) -> Option<Entry> {
        self.revisions.get(revision..)?.iter().find_map(|r| r.entries.get(&object).copied())
    }
    fn resolve(&mut self, length: Length, position: usize, budget: &mut usize, expanded: &mut usize) -> Option<usize> {
        if let Length::Direct(n) = length { return Some(n); }
        if self.revisions.is_empty() { self.load(budget, expanded)?; }
        // The structural walk also visits superseded objects. Their Length
        // references belong to the revision enclosing their physical position.
        let revision = self.revisions.iter().rposition(|r| r.xref >= position).unwrap_or(0);
        self.scalar(length, revision, budget, expanded)
    }
    fn scalar(&mut self, length: Length, revision: usize, budget: &mut usize, expanded: &mut usize) -> Option<usize> {
        let (object, generation) = match length {
            Length::Direct(n) => return Some(n),
            Length::Indirect { object, generation } => (object, generation),
        };
        let key = (revision, object, generation);
        if let Some(n) = self.scalars.get(&key) { return Some(*n); }
        *budget = budget.checked_sub(1)?;
        if self.resolving.len() >= 64 || !self.resolving.insert(key) { return None; }
        let n = match self.entry(revision, object)? {
            Entry::Free => return None,
            Entry::Plain { generation: actual, offset } if actual == generation => {
                self.indexed(offset)?;
                let b = self.file.get(offset..)?;
                let mut at = 0;
                if unsigned(token(b, &mut at, budget)?)? != object
                    || unsigned(token(b, &mut at, budget)?)? != generation
                    || token(b, &mut at, budget)? != b"obj" { return None; }
                let n = unsigned(token(b, &mut at, budget)?)?;
                if token(b, &mut at, budget)? != b"endobj" { return None; }
                *budget = budget.checked_sub(at)?;
                n
            }
            Entry::Compressed { stream, index } if generation == 0 => {
                self.compressed_scalar(revision, object, stream, index, budget, expanded)?
            }
            _ => return None,
        };
        self.resolving.remove(&key);
        self.scalars.insert(key, n);
        Some(n)
    }
    fn indexed(&mut self, offset: usize) -> Option<()> {
        if self.streams.range(..=offset).next_back().is_some_and(|(_, end)| offset < *end) {
            return None;
        }
        self.scalar_offsets.insert(offset);
        Some(())
    }
    fn compressed_scalar(&mut self, revision: usize, object: usize, stream: usize, index: usize,
        budget: &mut usize, expanded: &mut usize) -> Option<usize> {
        let Entry::Plain { generation, offset } = self.entry(revision, stream)? else { return None; };
        self.indexed(offset)?;
        let b = self.file.get(offset..)?;
        let mut at = 0;
        if unsigned(token(b, &mut at, budget)?)? != stream
            || unsigned(token(b, &mut at, budget)?)? != generation
            || token(b, &mut at, budget)? != b"obj" { return None; }
        let dictionary = at;
        let fields = metadata(b, &mut at, budget)?;
        tokens(&b[dictionary..at], &mut Seen::default(), budget, expanded, true, self)?;
        if name(fields.get(b"Type".as_slice())?.strip_prefix(b"/")?)? != b"ObjStm" { return None; }
        let count = unsigned(fields.get(b"N".as_slice())?)?;
        let first = unsigned(fields.get(b"First".as_slice())?)?;
        if count == 0 || count > *budget || index >= count { return None; }
        let mut length_at = 0;
        let declared = fields.get(b"Length".as_slice())?;
        let length = match unsigned(declared) {
            Some(n) => Length::Direct(n),
            None => stream_length(declared, &mut length_at, budget)?,
        };
        let length = self.scalar(length, revision, budget, expanded)?;
        if token(b, &mut at, budget)? != b"stream" { return None; }
        match b.get(at) {
            Some(13) => { at += 1; if b.get(at) == Some(&10) { at += 1; } }
            Some(10) => at += 1,
            _ => return None,
        }
        let start = at;
        *budget = budget.checked_sub(start)?;
        let (end, after) = stream_boundary(b, start, length)?;
        at = after;
        if token(b, &mut at, budget)? != b"endobj" { return None; }
        let absolute = offset.checked_add(start)?;
        let absolute_after = offset.checked_add(after)?;
        if self.scalar_offsets.range(absolute..absolute_after).next().is_some() { return None; }
        self.streams.insert(absolute, absolute_after);
        let flate = match fields.get(b"Filter".as_slice()) {
            Some(filter) => {
                let mut pos = 0;
                let t = token(filter, &mut pos, budget)?;
                let array = t == b"[";
                let t = if array { token(filter, &mut pos, budget)? } else { t };
                if name(t.strip_prefix(b"/")?)? != b"FlateDecode"
                    || (array && token(filter, &mut pos, budget)? != b"]") { return None; }
                true
            }
            None => false,
        };
        let decoded = decode_stream(&b[start..end], flate, expanded)?;
        tokens(&decoded, &mut Seen::default(), budget, expanded, true, self)?;
        self.inspected.insert(absolute);
        let header = decoded.get(..first)?;
        let mut cursor = 0;
        let mut ids = BTreeSet::new();
        let mut objects = Vec::new();
        for _ in 0..count {
            let id = unsigned(token(header, &mut cursor, budget)?)?;
            let relative = unsigned(token(header, &mut cursor, budget)?)?;
            let position = first.checked_add(relative)?;
            if id == 0 || !ids.insert(id) || position >= decoded.len()
                || objects.last().is_some_and(|&(_, previous)| position <= previous) { return None; }
            objects.push((id, position));
        }
        if token(header, &mut cursor, budget).is_some() || cursor != header.len() { return None; }
        let gap = &decoded[first..objects[0].1];
        let mut cursor = 0;
        if token(gap, &mut cursor, budget).is_some() || cursor != gap.len() { return None; }
        for (i, &(_, start)) in objects.iter().enumerate() {
            let end = objects.get(i + 1).map_or(decoded.len(), |&(_, position)| position);
            let body = &decoded[start..end];
            let mut cursor = 0;
            value(body, &mut cursor, budget, 0)?;
            if token(body, &mut cursor, budget).is_some() || cursor != body.len() { return None; }
        }
        let (id, start) = objects[index];
        if id != object { return None; }
        let end = objects.get(index + 1).map_or(decoded.len(), |&(_, position)| position);
        let scalar = &decoded[start..end];
        let mut cursor = 0;
        let n = unsigned(token(scalar, &mut cursor, budget)?)?;
        if token(scalar, &mut cursor, budget).is_some() || cursor != scalar.len() { return None; }
        Some(n)
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
    lookup: &mut Lookup<'_>,
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
                    length = Some(stream_length(b, &mut at, budget)?);
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
                        let n = lookup.resolve(length?, at, budget, expanded)?;
                        let (end, after) = stream_boundary(b, at, n)?;
                        // An xref entry cannot turn bytes within a stream into
                        // a scalar object, including forward-referenced objects.
                        if lookup.scalar_offsets.range(at..after).next().is_some() { return None; }
                        lookup.streams.insert(at, after);
                        if dictionary.iter().any(|v| v == b"ObjStm") && !lookup.inspected.contains(&at) {
                            let flate = dictionary.iter().any(|v| v == b"FlateDecode");
                            if !flate && dictionary.iter().any(|v| v == b"Filter") { return None; }
                            let decoded = decode_stream(&b[at..end], flate, expanded)?;
                            tokens(&decoded, seen, budget, expanded, true, lookup)?;
                        }
                        at = after;
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
            let end = xref[..xref.len().min(65536)].windows(6).position(|v| v == b"stream")?;
            if end > 65536 || !xref[..end].windows(5).any(|v| v == b"/XRef") {
                return None;
            }
        }
        let mut seen = Seen::default();
        let mut budget = RECORDS;
        let mut expanded = 16 * 1024 * 1024;
        let mut lookup = Lookup {
            file: b, xref: offset, revisions: Vec::new(),
            scalars: BTreeMap::new(), resolving: BTreeSet::new(),
            scalar_offsets: BTreeSet::new(), streams: BTreeMap::new(), inspected: BTreeSet::new(),
        };
        tokens(b, &mut seen, &mut budget, &mut expanded, false, &mut lookup)?;
        (seen.catalog && seen.pages && seen.objects > 0 && seen.objects == seen.ends).then_some(())
    }
    check(b).is_some()
}
