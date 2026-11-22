// SPDX-License-Identifier: MIT
//! ISO box framing, image item metadata and audio-only track admission.
use super::{RECORDS, be16, be32};
use std::collections::{BTreeMap, BTreeSet};
struct BoxRef<'a> {
    kind: &'a [u8],
    data: &'a [u8],
    payload_start: usize,
}
fn boxes<'a>(mut b: &'a [u8], budget: &mut usize) -> Option<Vec<BoxRef<'a>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while !b.is_empty() {
        *budget = budget.checked_sub(1)?;
        let h = b.get(..8)?;
        let size = be32(h);
        let (n, header) = match size {
            0 => (b.len(), 8),
            1 => (
                usize::try_from(u64::from_be_bytes(b.get(8..16)?.try_into().ok()?)).ok()?,
                16,
            ),
            _ => (size as usize, 8),
        };
        if n < header {
            return None;
        }
        out.push(BoxRef {
            kind: &h[4..8],
            data: b.get(header..n)?,
            payload_start: at + header,
        });
        b = b.get(n..)?;
        at += n;
    }
    Some(out)
}
fn child<'a>(items: &'a [BoxRef<'a>], kind: &[u8]) -> Option<&'a [u8]> {
    let mut found = items.iter().filter(|v| v.kind == kind);
    let data = found.next()?.data;
    found.next().is_none().then_some(data)
}
fn image(meta: &[u8], file_len: usize, budget: &mut usize, avif: bool) -> Option<()> {
    if meta.get(..4)? != [0, 0, 0, 0] {
        return None;
    }
    let items = boxes(meta.get(4..)?, budget)?;
    let pitm = child(&items, b"pitm")?;
    let primary = match *pitm.first()? {
        0 if pitm.len() == 6 => u32::from(be16(&pitm[4..])),
        1 if pitm.len() == 8 => be32(&pitm[4..]),
        _ => return None,
    };
    let iloc = child(&items, b"iloc")?;
    let located = locations(
        iloc,
        primary,
        file_len,
        child(&items, b"idat").map(|v| v.len()),
        budget,
    )?;
    let iinf = child(&items, b"iinf")?;
    let header = if *iinf.first()? == 0 { 6 } else { 8 };
    let count = if header == 6 {
        usize::from(be16(iinf.get(4..6)?))
    } else {
        be32(iinf.get(4..8)?) as usize
    };
    let entries = boxes(iinf.get(header..)?, budget)?;
    if count == 0 || entries.len() != count {
        return None;
    }
    let mut coded = false;
    let mut kinds = BTreeMap::new();
    for entry in entries {
        if entry.kind != b"infe" {
            return None;
        }
        let version = *entry.data.first()?;
        let offset = match version {
            2 => 8,
            3 => 10,
            _ => return None,
        };
        let kind = entry.data.get(offset..offset + 4)?;
        let id = if version == 2 {
            u32::from(be16(entry.data.get(4..6)?))
        } else {
            be32(entry.data.get(4..8)?)
        };
        if kinds.insert(id, kind).is_some() && avif {
            return None;
        }
        if kind == if avif { b"av01" } else { b"hvc1" } {
            coded = true;
        }
        if !entry.data.get(offset + 4..)?.contains(&0) {
            return None;
        }
    }
    let properties = boxes(child(&items, b"iprp")?, budget)?;
    let configs = boxes(child(&properties, b"ipco")?, budget)?;
    let mut spatial = false;
    let mut codec = false;
    for property in &configs {
        if property.kind == b"ispe" {
            if property.data.len() != 12
                || be32(&property.data[4..]) == 0
                || be32(&property.data[8..]) == 0
            {
                return None;
            }
            spatial = true;
        }
        if property.kind == if avif { b"av1C" } else { b"hvcC" } {
            if property.data.len() < if avif { 4 } else { 23 } {
                return None;
            }
            if property.data[0] != if avif { 0x81 } else { 1 } {
                return None;
            }
            codec = true;
        }
    }
    if avif {
        let coded_items = match *kinds.get(&primary)? {
            b"av01" => vec![primary],
            b"grid" => grid_inputs(child(&items, b"iref")?, primary, budget)?,
            _ => return None,
        };
        if coded_items.iter().any(|id| {
            kinds.get(id).copied() != Some(b"av01".as_slice()) || !located.contains(id)
        }) {
            return None;
        }
        avif_associations(&properties, &configs, &kinds, &coded_items, budget)?;
    }
    (coded && spatial && codec).then_some(())
}

// Support a coded primary or a grid whose direct inputs are coded images.
// Other derived graphs are deliberately unsupported; no recursive graph walk.
fn grid_inputs(b: &[u8], primary: u32, budget: &mut usize) -> Option<Vec<u32>> {
    let h = b.get(..4)?;
    if h[0] > 1 || h[1..] != [0, 0, 0] {
        return None;
    }
    let width = if h[0] == 0 { 2 } else { 4 };
    let references = boxes(b.get(4..)?, budget)?;
    let mut inputs = BTreeMap::new();
    for reference in references.iter().filter(|v| v.kind == b"dimg") {
        let mut at = 0;
        let from = item_id(reference.data, &mut at, width)?;
        let count = usize::from(be16(reference.data.get(at..at + 2)?));
        at += 2;
        let mut to = Vec::new();
        for _ in 0..count {
            *budget = budget.checked_sub(1)?;
            to.push(item_id(reference.data, &mut at, width)?);
        }
        if count == 0 || at != reference.data.len() || inputs.insert(from, to).is_some() {
            return None;
        }
    }
    inputs.remove(&primary)
}

fn item_id(b: &[u8], at: &mut usize, width: usize) -> Option<u32> {
    let bytes = b.get(*at..*at + width)?;
    *at += width;
    Some(if width == 2 {
        u32::from(be16(bytes))
    } else {
        be32(bytes)
    })
}

fn avif_associations(
    properties: &[BoxRef<'_>],
    configs: &[BoxRef<'_>],
    kinds: &BTreeMap<u32, &[u8]>,
    coded: &[u32],
    budget: &mut usize,
) -> Option<()> {
    let mut configured = BTreeSet::new();
    for ipma in properties.iter().filter(|v| v.kind == b"ipma") {
        let b = ipma.data;
        let h = b.get(..8)?;
        if h[0] > 1 || h[1..3] != [0, 0] || h[3] & !1 != 0 {
            return None;
        }
        let id_width = if h[0] == 0 { 2 } else { 4 };
        let wide = h[3] & 1 != 0;
        let mut at = 8;
        for _ in 0..be32(&h[4..]) {
            *budget = budget.checked_sub(1)?;
            let id = item_id(b, &mut at, id_width)?;
            kinds.get(&id)?;
            let count = usize::from(*b.get(at)?);
            at += 1;
            for _ in 0..count {
                *budget = budget.checked_sub(1)?;
                let raw = if wide {
                    let value = be16(b.get(at..at + 2)?);
                    at += 2;
                    value
                } else {
                    let value = u16::from(*b.get(at)?);
                    at += 1;
                    value
                };
                let mask = if wide { 0x7fff } else { 0x7f };
                let index = usize::from(raw & mask);
                // Index zero denotes no association, never a codec config.
                if index == 0 {
                    if raw != 0 {
                        return None;
                    }
                    continue;
                }
                if configs.get(index - 1)?.kind == b"av1C" {
                    configured.insert(id);
                }
            }
        }
        if at != b.len() {
            return None;
        }
    }
    coded.iter().all(|id| configured.contains(id)).then_some(())
}
// Item locations may use file offsets or the meta box's idat payload. Unknown
// external references and constructions cannot be verified without outbound I/O.
fn locations(
    b: &[u8],
    primary: u32,
    file_len: usize,
    idat_len: Option<usize>,
    budget: &mut usize,
) -> Option<BTreeSet<u32>> {
    fn integer(b: &[u8], at: &mut usize, n: usize) -> Option<usize> {
        if !matches!(n, 0 | 4 | 8) {
            return None;
        }
        let mut value = 0usize;
        for &byte in b.get(*at..*at + n)? {
            value = value.checked_mul(256)?.checked_add(usize::from(byte))?;
        }
        *at += n;
        Some(value)
    }
    let h = b.get(..8)?;
    let version = h[0];
    if version > 2 {
        return None;
    }
    let offset_size = usize::from(h[4] >> 4);
    let length_size = usize::from(h[4] & 15);
    let base_size = usize::from(h[5] >> 4);
    let index_size = if version == 0 {
        0
    } else {
        usize::from(h[5] & 15)
    };
    let mut at = if version == 2 { 10 } else { 8 };
    let count = if version == 2 {
        be32(b.get(6..10)?) as usize
    } else {
        usize::from(be16(&h[6..]))
    };
    let mut found = BTreeSet::new();
    for _ in 0..count {
        *budget = budget.checked_sub(1)?;
        let id = if version == 2 {
            let v = be32(b.get(at..at + 4)?);
            at += 4;
            v
        } else {
            let v = u32::from(be16(b.get(at..at + 2)?));
            at += 2;
            v
        };
        let construction = if version > 0 {
            let v = be16(b.get(at..at + 2)?);
            at += 2;
            v
        } else {
            0
        };
        if be16(b.get(at..at + 2)?) != 0 || construction > 1 {
            return None;
        }
        at += 2;
        let base = integer(b, &mut at, base_size)?;
        let extents = usize::from(be16(b.get(at..at + 2)?));
        at += 2;
        if extents == 0 {
            return None;
        }
        for _ in 0..extents {
            *budget = budget.checked_sub(1)?;
            integer(b, &mut at, index_size)?;
            let offset = integer(b, &mut at, offset_size)?;
            let length = integer(b, &mut at, length_size)?;
            let limit = if construction == 1 {
                idat_len?
            } else {
                file_len
            };
            if length == 0 || base.checked_add(offset)?.checked_add(length)? > limit {
                return None;
            }
        }
        found.insert(id);
    }
    (found.contains(&primary) && at == b.len()).then_some(found)
}
fn audio(moov: &[u8], media: &[(u64, u64)], budget: &mut usize) -> Option<()> {
    let items = boxes(moov, budget)?;
    if items.iter().any(|v| v.kind == b"mvex") {
        return None;
    }
    let mut tracks = 0;
    let mut sample = false;
    for track in items.iter().filter(|v| v.kind == b"trak") {
        let track = boxes(track.data, budget)?;
        let mdia = boxes(child(&track, b"mdia")?, budget)?;
        let handler = child(&mdia, b"hdlr")?;
        // Reject video, hint, subtitle and unknown tracks; every track is audio.
        if handler.get(8..12)? != b"soun" {
            return None;
        }
        let minf = boxes(child(&mdia, b"minf")?, budget)?;
        let stbl = boxes(child(&minf, b"stbl")?, budget)?;
        let stsd = child(&stbl, b"stsd")?;
        if stsd.get(..4)? != [0, 0, 0, 0] {
            return None;
        }
        let entries = boxes(stsd.get(8..)?, budget)?;
        if entries.is_empty() || be32(stsd.get(4..8)?) as usize != entries.len() {
            return None;
        }
        let dinf = boxes(child(&minf, b"dinf")?, budget)?;
        let dref = child(&dinf, b"dref")?;
        if dref.get(..4)? != [0, 0, 0, 0] {
            return None;
        }
        let references = boxes(dref.get(8..)?, budget)?;
        if be32(dref.get(4..8)?) as usize != references.len() {
            return None;
        }
        let mut local = Vec::with_capacity(entries.len());
        for entry in &entries {
            if !matches!(
                entry.kind,
                b"mp4a"
                    | b"alac"
                    | b"ac-3"
                    | b"ec-3"
                    | b"Opus"
                    | b"fLaC"
                    | b"lpcm"
                    | b"sowt"
                    | b"twos"
            ) || entry.data.len() < 28
            {
                return None;
            }
            // AudioSampleEntry versions 0, 1 and 2 have different fixed headers.
            let header = match be16(&entry.data[8..]) {
                0 => 28,
                1 => 44,
                2 => 64,
                _ => return None,
            };
            boxes(entry.data.get(header..)?, budget)?;
            let index = usize::from(be16(&entry.data[6..8]));
            local.push(index.checked_sub(1).and_then(|i| references.get(i))
                .is_some_and(|v| v.kind == b"url " && v.data == [0, 0, 0, 1]));
        }
        sample |= audio_samples(&stbl, &local, media)?;
        tracks += 1;
    }
    (tracks > 0 && sample).then_some(())
}

// A size table can describe millions of fixed-size PCM samples compactly.
// Keep that representation and sum runs without allocating per sample.
struct SampleSizes<'a> {
    count: usize,
    fixed: u32,
    width: u8,
    data: &'a [u8],
    consumed: usize,
}
impl<'a> SampleSizes<'a> {
    fn parse(stbl: &[BoxRef<'a>]) -> Option<Self> {
        let table = one_of(stbl, b"stsz", b"stz2")?;
        let h = table.data.get(..12)?;
        if h[..4] != [0, 0, 0, 0] {
            return None;
        }
        let count = be32(&h[8..]) as usize;
        let (fixed, width) = if table.kind == b"stsz" {
            (be32(&h[4..]), 32)
        } else {
            if h[4..7] != [0, 0, 0] || !matches!(h[7], 4 | 8 | 16) {
                return None;
            }
            (0, h[7])
        };
        let data = table.data.get(12..)?;
        let bytes = if fixed > 0 {
            0
        } else {
            count.checked_mul(usize::from(width))?.checked_add(7)? / 8
        };
        (data.len() == bytes).then_some(Self { count, fixed, width, data, consumed: 0 })
    }

    // Consume samples once, in table order. Variable sizes are bounded by the
    // validated byte extent; fixed-size PCM runs need no per-sample loop.
    fn take_sum(&mut self, count: usize) -> Option<u64> {
        let start = self.consumed;
        let end = start.checked_add(count)?;
        if end > self.count {
            return None;
        }
        self.consumed = end;
        if self.fixed > 0 {
            return (count as u64).checked_mul(u64::from(self.fixed));
        }
        let mut sum = 0u64;
        for i in start..end {
            let size = match self.width {
                4 => u32::from((self.data[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 15),
                8 => u32::from(self.data[i]),
                16 => u32::from(be16(&self.data[i * 2..])),
                32 => be32(&self.data[i * 4..]),
                _ => return None,
            };
            sum = sum.checked_add(u64::from(size))?;
        }
        Some(sum)
    }
}

fn one_of<'a, 'b>(items: &'b [BoxRef<'a>], a: &[u8], b: &[u8]) -> Option<&'b BoxRef<'a>> {
    let mut found = items.iter().filter(|v| v.kind == a || v.kind == b);
    let table = found.next()?;
    found.next().is_none().then_some(table)
}

fn table_rows(b: &[u8], width: usize) -> Option<&[u8]> {
    let h = b.get(..8)?;
    if h[..4] != [0, 0, 0, 0] {
        return None;
    }
    let count = be32(&h[4..]) as usize;
    let rows = b.get(8..)?;
    (count.checked_mul(width)? == rows.len()).then_some(rows)
}

fn audio_samples(
    stbl: &[BoxRef<'_>],
    local: &[bool],
    media: &[(u64, u64)],
) -> Option<bool> {
    let mut sizes = SampleSizes::parse(stbl)?;
    let table = one_of(stbl, b"stco", b"co64")?;
    let width = if table.kind == b"stco" { 4 } else { 8 };
    let offsets = table_rows(table.data, width)?;
    let chunks = offsets.len() / width;
    let rows = table_rows(child(stbl, b"stsc")?, 12)?;
    let mut runs = Vec::new();
    let mut previous = 0;
    // Table rows and chunks are bounded by their exact on-disk byte extents,
    // independently of the box budget. A small AAC file can exceed RECORDS.
    for row in rows.chunks_exact(12) {
        let first = be32(row) as usize;
        let samples = be32(&row[4..]) as usize;
        let description = be32(&row[8..]) as usize;
        if first <= previous
            || first > chunks
            || samples == 0
            || description == 0
            || !local.get(description - 1).copied().unwrap_or(false)
        {
            return None;
        }
        runs.push((first, samples));
        previous = first;
    }
    if chunks == 0 {
        return (sizes.count == 0 && runs.is_empty()).then_some(false);
    }
    if runs.first()?.0 != 1 {
        return None;
    }
    let mut run = 0;
    let mut found = false;
    for (i, offset) in offsets.chunks_exact(width).enumerate() {
        if runs.get(run + 1).is_some_and(|v| v.0 == i + 1) { run += 1; }
        let count = runs[run].1;
        let length = sizes.take_sum(count)?;
        let start = if width == 4 {
            u64::from(be32(offset))
        } else {
            u64::from_be_bytes(offset.try_into().ok()?)
        };
        let end = start.checked_add(length)?;
        // Samples in a chunk are contiguous. Checking the whole chunk proves
        // every nonempty sample is inside one top-level mdat payload.
        // Top-level mdat payloads are disjoint and sorted by file offset.
        // Chunk offsets need not be sorted: binary search avoids chunks * mdats.
        let range = media.partition_point(|&(low, _)| low <= start).checked_sub(1)?;
        if end > media[range].1 {
            return None;
        }
        found |= length > 0;
    }
    (sizes.consumed == sizes.count).then_some(found)
}
pub(super) fn detect(b: &[u8]) -> Option<&'static str> {
    let mut budget = RECORDS;
    let top = boxes(b, &mut budget)?;
    let ftyp = child(&top, b"ftyp")?;
    if ftyp.len() < 8 || ftyp.len() % 4 != 0 {
        return None;
    }
    let has = |brand: &[u8]| &ftyp[..4] == brand || ftyp[8..].chunks_exact(4).any(|v| v == brand);
    let avif = has(b"avif") || has(b"avis");
    let heic = [b"heic", b"heix", b"hevc", b"hevx"].iter().any(|v| has(*v));
    let heif = has(b"mif1") || has(b"msf1");
    if avif || heic || heif {
        let meta = child(&top, b"meta")?;
        image(meta, b.len(), &mut budget, avif)?;
        if !top.iter().any(|v| v.kind == b"mdat" && !v.data.is_empty()) {
            let items = boxes(meta.get(4..)?, &mut budget)?;
            if child(&items, b"idat")?.is_empty() {
                return None;
            }
        }
        return Some(if avif {
            "avif"
        } else if heic {
            "heic"
        } else {
            "heif"
        });
    }
    if !has(b"M4A ") && !has(b"isom") && !has(b"mp42") && !has(b"mp41") && !has(b"qt  ") {
        return None;
    }
    if !top.iter().any(|v| v.kind == b"mdat" && !v.data.is_empty()) {
        return None;
    }
    if top.iter().any(|v| v.kind == b"moof") {
        return None;
    }
    let media: Vec<_> = top.iter().filter(|v| v.kind == b"mdat")
        .map(|v| (v.payload_start as u64, (v.payload_start + v.data.len()) as u64))
        .collect();
    audio(child(&top, b"moov")?, &media, &mut budget)?;
    Some("m4a")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_service::attachment_formats::{iso_box, m4a};

    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    // All test boxes have ordinary headers. Audio data precedes moov and the
    // AVIF builder uses idat, so edits never invalidate unrelated file offsets.
    fn replace(b: &[u8], path: &[&[u8]], payload: Option<&[u8]>) -> Vec<u8> {
        let mut budget = RECORDS;
        let items = boxes(b, &mut budget).unwrap();
        let mut out = Vec::new();
        let mut found = false;
        for item in items {
            let kind: &[u8; 4] = item.kind.try_into().unwrap();
            if item.kind != path[0] {
                out.extend(iso_box(kind, item.data));
                continue;
            }
            found = true;
            if path.len() == 1 {
                if let Some(data) = payload { out.extend(iso_box(kind, data)); }
            } else {
                let prefix = if item.kind == b"meta" { 4 } else { 0 };
                let mut data = item.data[..prefix].to_vec();
                data.extend(replace(&item.data[prefix..], &path[1..], payload));
                out.extend(iso_box(kind, &data));
            }
        }
        assert!(found);
        out
    }

    fn associations(version: u8, wide: bool, entries: &[(u32, &[u16])]) -> Vec<u8> {
        let mut out = vec![version, 0, 0, u8::from(wide)];
        out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for &(id, indices) in entries {
            if version == 0 { out.extend_from_slice(&(id as u16).to_be_bytes()); }
            else { out.extend_from_slice(&id.to_be_bytes()); }
            out.push(indices.len() as u8);
            for &index in indices {
                if wide { out.extend_from_slice(&index.to_be_bytes()); }
                else { out.push(index as u8); }
            }
        }
        out
    }

    fn avif(
        primary: u32,
        kinds: &[(u32, &[u8; 4])],
        ipma: Option<&[u8]>,
        dimg: Option<(u8, u32, &[u32])>,
    ) -> Vec<u8> {
        let mut meta = vec![0; 4];
        let mut pitm = vec![1, 0, 0, 0];
        pitm.extend_from_slice(&primary.to_be_bytes());
        meta.extend(iso_box(b"pitm", &pitm));
        let mut iloc = vec![2, 0, 0, 0, 0x44, 0];
        iloc.extend_from_slice(&(kinds.len() as u32).to_be_bytes());
        let mut iinf = words(&[0x01000000, kinds.len() as u32]);
        for &(id, kind) in kinds {
            iloc.extend_from_slice(&id.to_be_bytes());
            iloc.extend_from_slice(&[0, 1, 0, 0, 0, 1]); // idat, data ref 0, one extent
            iloc.extend(words(&[0, 16]));
            let mut infe = vec![3, 0, 0, 0];
            infe.extend_from_slice(&id.to_be_bytes());
            infe.extend_from_slice(&[0, 0]);
            infe.extend_from_slice(kind);
            infe.push(0);
            iinf.extend(iso_box(b"infe", &infe));
        }
        meta.extend(iso_box(b"iloc", &iloc));
        meta.extend(iso_box(b"iinf", &iinf));
        let mut configs = iso_box(b"ispe", &words(&[0, 1, 1]));
        configs.extend(iso_box(b"av1C", &[0x81, 0, 0, 0]));
        let mut iprp = iso_box(b"ipco", &configs);
        if let Some(ipma) = ipma { iprp.extend(iso_box(b"ipma", ipma)); }
        meta.extend(iso_box(b"iprp", &iprp));
        if let Some((version, from, to)) = dimg {
            let mut reference = Vec::new();
            let width = if version == 0 { 2 } else { 4 };
            reference.extend_from_slice(&from.to_be_bytes()[4 - width..]);
            reference.extend_from_slice(&(to.len() as u16).to_be_bytes());
            for id in to { reference.extend_from_slice(&id.to_be_bytes()[4 - width..]); }
            let mut iref = vec![version, 0, 0, 0];
            iref.extend(iso_box(b"dimg", &reference));
            meta.extend(iso_box(b"iref", &iref));
        }
        meta.extend(iso_box(b"idat", &[0; 16]));
        let mut out = iso_box(b"ftyp", b"avif\0\0\0\0mif1");
        out.extend(iso_box(b"meta", &meta));
        out
    }

    #[test]
    fn avif_requires_the_primary_items_codec_association() {
        for version in [0, 1] {
            let id = if version == 0 { 1 } else { 70000 };
            for wide in [false, true] {
                for essential in [false, true] {
                    let codec = 2 | if essential { if wide { 0x8000 } else { 0x80 } } else { 0 };
                    let ipma = associations(version, wide, &[(id, &[1, codec])]);
                    let bytes = avif(id, &[(id, b"av01")], Some(&ipma), None);
                    assert_eq!(detect(&bytes), Some("avif"));
                    for n in 0..ipma.len() {
                        assert_eq!(detect(&replace(&bytes, &[b"meta", b"iprp", b"ipma"], Some(&ipma[..n]))), None);
                    }
                }
            }
        }
        let kinds = [(1, b"av01"), (2, b"av01")];
        for ipma in [
            None,
            Some(associations(0, false, &[])),
            Some(associations(0, false, &[(2, &[2])])), // other image
            Some(associations(0, false, &[(9, &[2])])), // absent item
            Some(associations(0, false, &[(1, &[1])])), // ispe is not av1C
            Some(associations(0, false, &[(1, &[0])])),
            Some(associations(0, false, &[(1, &[0x80])])),
            Some(associations(0, false, &[(1, &[2, 3])])), // out of range
            Some(associations(0, true, &[(1, &[0x8002, 0xffff])])),
        ] {
            assert_eq!(detect(&avif(1, &kinds, ipma.as_deref(), None)), None);
        }
        for brand in [b"heic", b"mif1"] {
            assert!(detect(&crate::mcp_service::attachment_formats::heif(brand)).is_some(), "HEIC keeps its existing admission");
        }
    }

    #[test]
    fn avif_wide_property_indices_and_grid_inputs_are_resolved() {
        let ipma = associations(1, true, &[(70000, &[0x8082])]);
        let bytes = avif(70000, &[(70000, b"av01")], Some(&ipma), None);
        let mut configs = iso_box(b"ispe", &words(&[0, 1, 1]));
        for _ in 0..128 { configs.extend(iso_box(b"free", &[])); }
        configs.extend(iso_box(b"av1C", &[0x81, 0, 0, 0])); // index 130
        assert_eq!(detect(&replace(&bytes, &[b"meta", b"iprp", b"ipco"], Some(&configs))), Some("avif"));
        for version in [0, 1] {
            let base = if version == 0 { 1 } else { 70000 };
            let kinds = [(base, b"grid"), (base + 1, b"av01"), (base + 2, b"av01")];
            let to = [base + 1, base + 2];
            let ipma = associations(version, true, &[(base, &[1]), (base + 1, &[0x8002]), (base + 2, &[0x8002])]);
            let bytes = avif(base, &kinds, Some(&ipma), Some((version, base, &to)));
            assert_eq!(detect(&bytes), Some("avif"));
            let mut budget = RECORDS;
            let top = boxes(&bytes, &mut budget).unwrap();
            let meta = boxes(&child(&top, b"meta").unwrap()[4..], &mut budget).unwrap();
            let reference = child(&meta, b"iref").unwrap();
            for n in 0..reference.len() {
                assert_eq!(detect(&replace(&bytes, &[b"meta", b"iref"], Some(&reference[..n]))), None);
            }
            let mut iloc = child(&meta, b"iloc").unwrap().to_vec();
            // Locations are 18 bytes per item; hide the second coded tile.
            iloc[46..50].copy_from_slice(&(base + 3).to_be_bytes());
            assert_eq!(detect(&replace(&bytes, &[b"meta", b"iloc"], Some(&iloc))), None);
            for targets in [&[][..], &[base][..], &[base + 3][..]] {
                assert_eq!(detect(&avif(base, &kinds, Some(&ipma), Some((version, base, targets)))), None);
            }
            assert_eq!(detect(&avif(base, &kinds, Some(&ipma), None)), None);
            assert_eq!(detect(&avif(base, &kinds, Some(&ipma), Some((version, base + 1, &to)))), None);
            let wrong = associations(version, true, &[(base, &[0x8002]), (base + 1, &[0x8002])]);
            assert_eq!(detect(&avif(base, &kinds, Some(&wrong), Some((version, base, &to)))), None, "every coded tile needs its own association");
            assert_eq!(detect(&avif(base, &[(base, b"grid"), (base + 1, b"grid"), (base + 2, b"av01")], Some(&ipma), Some((version, base, &to)))), None, "nested derived inputs are unsupported");
        }
    }

    const STBL: &[&[u8]] = &[b"moov", b"trak", b"mdia", b"minf", b"stbl"];
    fn audio_table(bytes: &[u8], kind: &[u8], data: Option<&[u8]>) -> Vec<u8> {
        let mut path = STBL.to_vec();
        path.push(kind);
        replace(bytes, &path, data)
    }
    fn audio_offset(bytes: &[u8]) -> u32 {
        let mut budget = RECORDS;
        boxes(bytes, &mut budget).unwrap().iter()
            .find(|v| v.kind == b"mdat").unwrap().payload_start as u32
    }

    // Structural AAC-like packets, deliberately opaque rather than decodable.
    // Exercise variable sizes independently of the playable PCM fixture.
    fn aac_like(count: usize, per_chunk: usize, separate_runs: bool, separate_mdats: bool) -> Vec<u8> {
        assert_eq!(count % per_chunk, 0);
        let base = m4a(false);
        let mut budget = RECORDS;
        let top = boxes(&base, &mut budget).unwrap();
        let sizes: Vec<u32> = (0..count).map(|i| 32 + (i % 5) as u32).collect();
        let chunks = count / per_chunk;
        let mut bytes = iso_box(b"ftyp", b"M4A \0\0\0\0isom");
        let mut offsets = words(&[0, chunks as u32]);
        let mut length = 0;
        for group in sizes.chunks_exact(per_chunk) {
            let group_len = group.iter().map(|v| *v as usize).sum::<usize>();
            let start = bytes.len() + 8 + if separate_mdats { 0 } else { length };
            offsets.extend_from_slice(&(start as u32).to_be_bytes());
            if separate_mdats { bytes.extend(iso_box(b"mdat", &vec![0; group_len])); }
            length += group_len;
        }
        if !separate_mdats { bytes.extend(iso_box(b"mdat", &vec![0; length])); }
        bytes.extend(iso_box(b"moov", child(&top, b"moov").unwrap()));
        let mut stsz = words(&[0, 0, count as u32]);
        stsz.extend(words(&sizes));
        let mut stsc = words(&[0, if separate_runs { chunks as u32 } else { 1 }]);
        for i in 0..if separate_runs { chunks } else { 1 } {
            stsc.extend(words(&[(i + 1) as u32, per_chunk as u32, 1]));
        }
        bytes = audio_table(&bytes, b"stsz", Some(&stsz));
        bytes = audio_table(&bytes, b"stco", Some(&offsets));
        bytes = audio_table(&bytes, b"stsc", Some(&stsc));
        bytes = audio_table(&bytes, b"stts", Some(&words(&[0, 1, count as u32, 1024])));
        let at = bytes.windows(4).position(|v| v == b"sowt").unwrap();
        bytes[at..at + 4].copy_from_slice(b"mp4a");
        bytes
    }

    #[test]
    fn long_variable_audio_tables_are_bounded_by_bytes() {
        let count = 70000; // More than the generic box budget, still under 8 MiB.
        assert!(count > RECORDS);
        for (per_chunk, separate_runs) in [(count, false), (1, false), (1, true)] {
            let bytes = aac_like(count, per_chunk, separate_runs, false);
            assert!(bytes.len() < crate::mcp_service::attachments::FILE_BYTES);
            assert_eq!(detect(&bytes), Some("m4a"), "{per_chunk}, {separate_runs}");
            let mut overrun = bytes.clone();
            let last = overrun.windows(4).position(|v| v == b"stsz").unwrap() + 16 + (count - 1) * 4;
            let size = be32(&overrun[last..]);
            overrun[last..last + 4].copy_from_slice(&(size + 1).to_be_bytes());
            assert_eq!(detect(&overrun), None, "the last packet is checked too");
        }
    }

    #[test]
    fn audio_many_media_ranges_and_unordered_chunk_offsets() {
        let count = 1024;
        let bytes = aac_like(count, 1, true, true);
        assert_eq!(detect(&bytes), Some("m4a"));
        let mut budget = RECORDS;
        let media = boxes(&bytes, &mut budget).unwrap();
        // Reverse the file order of the chunks. Reverse sizes too, so every
        // chunk still exactly fills its own mdat. No monotone-offset assumption.
        let mut offsets = words(&[0, count as u32]);
        for item in media.iter().filter(|v| v.kind == b"mdat").rev() {
            offsets.extend_from_slice(&(item.payload_start as u32).to_be_bytes());
        }
        let mut sizes = words(&[0, 0, count as u32]);
        for i in (0..count).rev() { sizes.extend(words(&[32 + (i % 5) as u32])); }
        let reversed = audio_table(&bytes, b"stco", Some(&offsets));
        let reversed = audio_table(&reversed, b"stsz", Some(&sizes));
        assert_eq!(detect(&reversed), Some("m4a"));
        // A chunk cannot cross an mdat boundary, even into another mdat.
        sizes[12..16].copy_from_slice(&100u32.to_be_bytes());
        assert_eq!(detect(&audio_table(&reversed, b"stsz", Some(&sizes))), None);
    }

    #[test]
    fn audio_table_extents_and_sample_walk_limits() {
        let bytes = m4a(false);
        // Huge claimed tables cannot cause a walk without their row bytes.
        for (kind, table) in [
            (b"stsz", words(&[0, 0, u32::MAX])),
            (b"stco", words(&[0, u32::MAX, audio_offset(&bytes)])),
            (b"stsc", words(&[0, u32::MAX, 1, 800, 1])),
        ] {
            assert_eq!(detect(&audio_table(&bytes, kind, Some(&table))), None);
        }
        // Compact size tables have the same byte-extent requirement.
        for width in [4, 8, 16] {
            let table = iso_box(b"stz2", &words(&[0, width, u32::MAX]));
            let removed = audio_table(&bytes, b"stsz", None);
            let mut budget = RECORDS;
            let mut stbl = &removed[..];
            for kind in STBL {
                let items = boxes(stbl, &mut budget).unwrap();
                stbl = items.into_iter().find(|v| v.kind == *kind).unwrap().data;
            }
            assert_eq!(detect(&replace(&removed, STBL, Some(&[stbl, &table].concat()))), None);
        }
        // Fixed-size PCM may have a huge count, but is checked in one run.
        let huge = audio_table(&bytes, b"stsz", Some(&words(&[0, u32::MAX, u32::MAX])));
        let huge = audio_table(&huge, b"stsc", Some(&words(&[0, 1, 1, u32::MAX, 1])));
        assert_eq!(detect(&huge), None);
        let mut sizes = SampleSizes { count: 3, fixed: 0, width: 8, data: &[2, 3, 4], consumed: 0 };
        assert_eq!(sizes.take_sum(2), Some(5));
        assert_eq!(sizes.take_sum(2), None); // Cannot consume past the validated extent.
        assert_eq!(sizes.take_sum(1), Some(4));
        assert_eq!(sizes.take_sum(1), None);
        // Many actual boxes still obey the generic structural budget.
        let mut excess = bytes;
        for _ in 0..RECORDS { excess.extend(iso_box(b"free", &[])); }
        assert_eq!(detect(&excess), None);
    }

    #[test]
    fn audio_referenced_descriptions_require_local_data_references() {
        let bytes = m4a(false);
        const DREF: &[&[u8]] = &[b"moov", b"trak", b"mdia", b"minf", b"dinf", b"dref"];
        let mut external = words(&[0, 1]);
        external.extend(iso_box(b"url ", b"\0\0\0\0https://example.invalid/audio\0"));
        assert_eq!(detect(&replace(&bytes, DREF, Some(&external))), None);
        assert_eq!(detect(&replace(&bytes, DREF, None)), None);
        for index in [0u16, 2] {
            let mut invalid = bytes.clone();
            let at = invalid.windows(4).position(|v| v == b"sowt").unwrap() + 4;
            invalid[at + 6..at + 8].copy_from_slice(&index.to_be_bytes());
            assert_eq!(detect(&invalid), None);
        }
        let mut budget = RECORDS;
        let mut stbl = &bytes[..];
        for kind in STBL {
            let items = boxes(stbl, &mut budget).unwrap();
            stbl = items.into_iter().find(|v| v.kind == *kind).unwrap().data;
        }
        let items = boxes(stbl, &mut budget).unwrap();
        let stsd = child(&items, b"stsd").unwrap();
        let mut descriptions = words(&[0, 2]);
        descriptions.extend_from_slice(&stsd[8..]);
        let mut unused = stsd[8..].to_vec();
        unused[14..16].copy_from_slice(&2u16.to_be_bytes());
        descriptions.extend(unused);
        let bytes = audio_table(&bytes, b"stsd", Some(&descriptions));
        assert_eq!(detect(&bytes), Some("m4a"), "unused description need not be local");
        let mut references = words(&[0, 2]);
        references.extend(iso_box(b"url ", &[0, 0, 0, 1]));
        references.extend(iso_box(b"url ", b"\0\0\0\0https://example.invalid/audio\0"));
        let bytes = replace(&bytes, DREF, Some(&references));
        assert_eq!(detect(&bytes), Some("m4a"));
        let external = audio_table(&bytes, b"stsc", Some(&words(&[0, 1, 1, 800, 2])));
        assert_eq!(detect(&external), None, "referenced external description is rejected");
        let local_second = words(&[0, 2]);
        let local_second = [local_second, iso_box(b"url ", &[0, 0, 0, 1]), iso_box(b"url ", &[0, 0, 0, 1])].concat();
        assert_eq!(detect(&replace(&external, DREF, Some(&local_second))), Some("m4a"));
    }

    #[test]
    fn pcm_audio_and_supported_size_and_offset_tables() {
        let bytes = m4a(false);
        assert_eq!(detect(&bytes), Some("m4a"));
        let offset = audio_offset(&bytes);
        let mut variable = words(&[0, 0, 800]);
        variable.extend(words(&[2; 800]));
        assert_eq!(detect(&audio_table(&bytes, b"stsz", Some(&variable))), Some("m4a"));
        for width in [4u8, 8, 16] {
            let mut compact = vec![0, 0, 0, 0, 0, 0, 0, width];
            compact.extend_from_slice(&800u32.to_be_bytes());
            compact.extend(match width { 4 => vec![0x22; 400], 8 => vec![2; 800], _ => [0, 2].repeat(800) });
            let compact = iso_box(b"stz2", &compact);
            let sz = audio_table(&bytes, b"stsz", None);
            // Add stz2 alongside the other existing tables.
            let at = sz.windows(4).position(|v| v == b"stbl").unwrap() + 4;
            let len = be32(&sz[at - 8..]) as usize - 8;
            let stbl = [sz[at..at + len].to_vec(), compact].concat();
            assert_eq!(detect(&replace(&sz, STBL, Some(&stbl))), Some("m4a"));
            let mut truncated = stbl.clone();
            truncated.pop();
            assert_eq!(detect(&replace(&sz, STBL, Some(&truncated))), None);
            // Both size-table variants together are ambiguous.
            assert_eq!(detect(&replace(&bytes, STBL, Some(&[
                stbl, iso_box(b"stsz", &words(&[0, 2, 800]))
            ].concat()))), None);
        }
        let mut co64 = words(&[0, 1]);
        co64.extend_from_slice(&u64::from(offset).to_be_bytes());
        let mut path = STBL.to_vec(); path.push(b"stco");
        let removed = replace(&bytes, &path, None);
        let at = removed.windows(4).position(|v| v == b"stbl").unwrap() + 4;
        let len = be32(&removed[at - 8..]) as usize - 8;
        let stbl = [removed[at..at + len].to_vec(), iso_box(b"co64", &co64)].concat();
        assert_eq!(detect(&replace(&removed, STBL, Some(&stbl))), Some("m4a"));
        let mut invalid = replace(&removed, STBL, Some(&stbl));
        let at = invalid.windows(4).position(|v| v == b"co64").unwrap() + 12;
        invalid[at..at + 8].copy_from_slice(&u64::MAX.to_be_bytes());
        assert_eq!(detect(&invalid), None, "64-bit sample end must not overflow");
        let both = [stbl, iso_box(b"stco", &words(&[0, 1, offset]))].concat();
        assert_eq!(detect(&replace(&removed, STBL, Some(&both))), None);
        let split = audio_table(&bytes, b"stco", Some(&words(&[0, 2, offset, offset + 400])));
        let split = audio_table(&split, b"stsc", Some(&words(&[0, 2, 1, 200, 1, 2, 600, 1])));
        assert_eq!(detect(&split), Some("m4a"));
        // Structural admission still permits ordinary AAC/ALAC sample entries.
        // These mutations test recognition only; the PCM fixture is playable.
        for codec in [b"mp4a", b"alac"] {
            let mut bytes = bytes.clone();
            let at = bytes.windows(4).position(|v| v == b"sowt").unwrap();
            bytes[at..at + 4].copy_from_slice(codec);
            assert_eq!(detect(&bytes), Some("m4a"));
        }
    }

    #[test]
    fn audio_rejects_missing_invalid_or_unmapped_samples() {
        let bytes = m4a(false);
        let offset = audio_offset(&bytes);
        for kind in [b"stsz", b"stsc", b"stco"] {
            assert_eq!(detect(&audio_table(&bytes, kind, None)), None);
        }
        for (kind, data) in [
            (b"stsz", words(&[0, 2, 0])),
            (b"stsz", words(&[0, 0, 800])), // missing sizes
            (b"stsz", words(&[0, 3, 800])), // sample end past mdat
            (b"stsz", words(&[0, 2, 799])), // chunk/sample count mismatch
            (b"stsc", words(&[0, 0])),
            (b"stsc", words(&[0, 1, 0, 800, 1])),
            (b"stsc", words(&[0, 1, 2, 800, 1])),
            (b"stsc", words(&[0, 1, 1, 0, 1])),
            (b"stsc", words(&[0, 1, 1, 800, 0])),
            (b"stsc", words(&[0, 1, 1, 800, 2])),
            (b"stsc", words(&[0, 2, 1, 800, 1, 1, 800, 1])),
            (b"stco", words(&[0, 0])),
            (b"stco", words(&[0, 2, offset])),
            (b"stco", words(&[0, 1, 0])),
            (b"stco", words(&[0, 1, offset - 8])), // mdat header
            (b"stco", words(&[0, 1, offset + 1])),
            (b"stco", words(&[0, 1, u32::MAX])),
        ] {
            assert_eq!(detect(&audio_table(&bytes, kind, Some(&data))), None, "{kind:?}: {data:?}");
        }
        let mut headers_only = bytes.clone();
        for kind in [b"stsz", b"stsc", b"stco"] { headers_only = audio_table(&headers_only, kind, None); }
        assert_eq!(detect(&headers_only), None, "brand, audio header and arbitrary mdat are insufficient");
        let mut fragmented = bytes.clone();
        fragmented.extend(iso_box(b"moof", &[]));
        assert_eq!(detect(&fragmented), None);
        let at = bytes.windows(4).position(|v| v == b"moov").unwrap() + 4;
        let moov = [bytes[at..].to_vec(), iso_box(b"mvex", &[])].concat();
        assert_eq!(detect(&replace(&bytes, &[b"moov"], Some(&moov))), None);
        assert_eq!(detect(&m4a(true)), None, "all tracks must be audio");
        let mut budget = 0;
        assert!(boxes(&bytes, &mut budget).is_none());
    }
}
