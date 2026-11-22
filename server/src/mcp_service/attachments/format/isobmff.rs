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
                if configs.get(index - 1)?.kind == b"av1C" && !configured.insert(id) {
                    // Count associations per item, including repeats of the
                    // same property and associations in separate ipma boxes.
                    return None;
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
            let extensions = boxes(entry.data.get(header..)?, budget)?;
            let configured = entry.kind != b"mp4a" || mp4a_config(extensions, budget).is_some();
            let index = usize::from(be16(&entry.data[6..8]));
            local.push(configured && index.checked_sub(1).and_then(|i| references.get(i))
                .is_some_and(|v| v.kind == b"url " && v.data == [0, 0, 0, 1]));
        }
        sample |= audio_samples(&stbl, &local, media)?;
        tracks += 1;
    }
    (tracks > 0 && sample).then_some(())
}

// QuickTime can put esds inside wave alongside frma, mp4a and a terminator.
// Only wave is a container here; the shared box budget bounds nested layouts.
fn mp4a_config(mut extensions: Vec<BoxRef<'_>>, budget: &mut usize) -> Option<()> {
    let mut configured = false;
    while let Some(extension) = extensions.pop() {
        match extension.kind {
            b"wave" => extensions.extend(boxes(extension.data, budget)?),
            b"esds" => {
                if configured { return None; }
                esds(extension.data, budget)?;
                configured = true;
            }
            _ => {}
        }
    }
    configured.then_some(())
}

// MPEG-4 descriptor lengths use at most four base-128 bytes. Padded lengths
// are ordinary, but neither a fifth byte nor a payload past its parent is.
fn descriptor<'a>(bytes: &mut &'a [u8], budget: &mut usize) -> Option<(u8, &'a [u8])> {
    *budget = budget.checked_sub(1)?;
    let tag = *bytes.first()?;
    let mut length = 0usize;
    for at in 1..=4 {
        let byte = *bytes.get(at)?;
        length = length.checked_mul(128)?.checked_add(usize::from(byte & 0x7f))?;
        if byte & 0x80 == 0 {
            let start = at + 1;
            let end = start.checked_add(length)?;
            let data = bytes.get(start..end)?;
            *bytes = bytes.get(end..)?;
            return Some((tag, data));
        }
    }
    None
}

fn esds(b: &[u8], budget: &mut usize) -> Option<()> {
    if b.get(..4)? != [0, 0, 0, 0] { return None; }
    let mut root = b.get(4..)?;
    let (tag, es) = descriptor(&mut root, budget)?;
    if tag != 3 || !root.is_empty() { return None; }
    let flags = *es.get(2)?;
    // This description is used for self-contained samples, never a URL ES.
    if flags & 0x40 != 0 { return None; }
    let mut at = 3;
    if flags & 0x80 != 0 { at += 2; } // dependsOn_ES_ID
    if flags & 0x20 != 0 { at += 2; } // OCR_ES_Id
    let mut children = es.get(at..)?;
    let mut configured = false;
    while !children.is_empty() {
        let (tag, config) = descriptor(&mut children, budget)?;
        if tag != 4 { continue; }
        if configured { return None; }
        let h = config.get(..13)?;
        // Registered audio object type indications, and audio streamType (5)
        // with its reserved bit set. Never admit a video decoder as mp4a.
        if !matches!(h[0], 0x40 | 0x66..=0x69 | 0x6b | 0xa5 | 0xa6 | 0xa9..=0xae | 0xc1 | 0xdd)
            || h[1] >> 2 != 5 || h[1] & 1 == 0
        { return None; }
        let mut specific = None;
        let mut nested = &config[13..];
        while !nested.is_empty() {
            let (tag, data) = descriptor(&mut nested, budget)?;
            if tag == 5 && specific.replace(data).is_some() { return None; }
        }
        // MPEG-4 audio and MPEG-2 AAC need decoder-specific configuration.
        // MPEG-1/2 audio and other codecs need no AAC AudioSpecificConfig.
        if matches!(h[0], 0x40 | 0x66..=0x68) {
            aac_config(specific?)?;
        }
        configured = true;
    }
    configured.then_some(())
}

// Read only elementary configuration headers, not compressed AAC packets.
// All reads are bounded by the descriptor; PCE loops have fixed-width counts.
struct AudioBits<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl AudioBits<'_> {
    fn read(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | u32::from((self.bytes.get(self.at / 8)? >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Some(value)
    }
    fn remaining(&self) -> usize { self.bytes.len() * 8 - self.at }
    fn object_type(&mut self) -> Option<u32> {
        let aot = self.read(5)?;
        let aot = if aot == 31 { 32 + self.read(6)? } else { aot };
        (aot != 0).then_some(aot)
    }
    fn frequency(&mut self) -> Option<()> {
        match self.read(4)? {
            0..=12 => Some(()),
            15 if self.read(24)? != 0 => Some(()),
            _ => None,
        }
    }
    fn channels(&mut self) -> Option<u32> {
        let channels = self.read(4)?;
        matches!(channels, 0..=7 | 11..=14).then_some(channels)
    }
}

fn aac_config(bytes: &[u8]) -> Option<()> {
    let mut bits = AudioBits { bytes, at: 0 };
    let mut aot = bits.object_type()?;
    bits.frequency()?;
    let channels = bits.channels()?;
    let explicit_extension = matches!(aot, 5 | 29);
    if explicit_extension {
        // SBR and PS signal the core frequency first, then the output frequency
        // and the underlying audio object type (usually AAC-LC).
        bits.frequency()?;
        aot = bits.object_type()?;
        if aot == 22 { bits.channels()?; }
    }
    if !matches!(aot, 1..=4 | 6 | 7 | 17 | 19..=23) { return None; }
    let short_frame = bits.read(1)?;
    if aot == 3 && short_frame != 0 { return None; } // SSR has no 960-sample frame.
    if bits.read(1)? != 0 { bits.read(14)?; } // coreCoderDelay
    let extension = bits.read(1)?;
    if matches!(aot, 17 | 19..=23) && extension == 0 { return None; }
    if channels == 0 { aac_program_config(&mut bits)?; }
    if matches!(aot, 6 | 20) { bits.read(3)?; } // layerNr
    if extension != 0 {
        if aot == 22 { bits.read(5)?; bits.read(11)?; }
        if matches!(aot, 17 | 19 | 20 | 23) { bits.read(3)?; }
        // extensionFlag3 reserves a future header syntax.
        if bits.read(1)? != 0 { return None; }
    }
    if matches!(aot, 17 | 19..=23) && bits.read(2)? >= 2 {
        // These values require ErrorProtectionSpecificConfig, beyond the
        // elementary header supported here.
        return None;
    }
    if !explicit_extension && bits.remaining() >= 11 && bits.read(11)? == 0x2b7 {
        match bits.object_type()? {
            5 => {
                let sbr = bits.read(1)? != 0;
                if sbr { bits.frequency()?; }
                if bits.remaining() >= 11 && bits.read(11)? == 0x548 {
                    let ps = bits.read(1)? != 0;
                    if ps && !sbr { return None; }
                }
            }
            22 => {
                if bits.read(1)? != 0 { bits.frequency()?; }
                bits.channels()?;
            }
            _ => return None,
        }
    }
    Some(())
}

fn aac_program_config(bits: &mut AudioBits<'_>) -> Option<()> {
    bits.read(4)?; // element_instance_tag
    if bits.read(2)? == 3 { return None; } // reserved PCE object_type
    if bits.read(4)? > 12 { return None; }
    let front = bits.read(4)?;
    let side = bits.read(4)?;
    let back = bits.read(4)?;
    let lfe = bits.read(2)?;
    let associated = bits.read(3)?;
    let cc = bits.read(4)?;
    if front + side + back + lfe == 0 { return None; }
    if bits.read(1)? != 0 { bits.read(4)?; } // mono mixdown
    if bits.read(1)? != 0 { bits.read(4)?; } // stereo mixdown
    if bits.read(1)? != 0 { bits.read(3)?; } // matrix mixdown
    for _ in 0..front + side + back { bits.read(5)?; }
    for _ in 0..lfe + associated { bits.read(4)?; }
    for _ in 0..cc { bits.read(5)?; }
    let padding = (8 - bits.at % 8) % 8;
    bits.read(padding)?;
    let comments = bits.read(8)?;
    for _ in 0..comments { bits.read(8)?; }
    Some(())
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
    let timing = table_rows(child(stbl, b"stts")?, 8)?;
    let mut timed = 0usize;
    for row in timing.chunks_exact(8) {
        let count = be32(row) as usize;
        if count == 0 { return None; }
        timed = timed.checked_add(count)?;
    }
    if timed != sizes.count { return None; }
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

    #[test]
    fn avif_rejects_repeated_and_conflicting_codec_associations() {
        for version in [0, 1] {
            for wide in [false, true] {
                let essential = if wide { 0x8000 } else { 0x80 };
                let ipma = associations(version, wide, &[(1, &[2])]);
                let bytes = avif(1, &[(1, b"av01")], Some(&ipma), None);
                let configs = [
                    iso_box(b"ispe", &words(&[0, 1, 1])),
                    iso_box(b"av1C", &[0x81, 0, 0, 0]),
                    iso_box(b"av1C", &[0x81, 0x20, 0, 0]),
                ].concat();
                let bytes = replace(&bytes, &[b"meta", b"iprp", b"ipco"], Some(&configs));
                for entries in [
                    vec![(1, vec![2, 2])],
                    vec![(1, vec![2, essential | 2])],
                    vec![(1, vec![2, 3])],
                    vec![(1, vec![2]), (1, vec![3])],
                ] {
                    let entries: Vec<_> = entries.iter().map(|(id, v)| (*id, v.as_slice())).collect();
                    let repeated = associations(version, wide, &entries);
                    assert_eq!(detect(&replace(&bytes, &[b"meta", b"iprp", b"ipma"], Some(&repeated))), None);
                }
                let iprp = [iso_box(b"ipco", &configs), iso_box(b"ipma", &ipma), iso_box(b"ipma", &ipma)].concat();
                assert_eq!(detect(&replace(&bytes, &[b"meta", b"iprp"], Some(&iprp))), None);
                // Two coded grid inputs can share exactly the same property.
                let shared = associations(version, wide, &[(2, &[2]), (3, &[2])]);
                assert_eq!(detect(&avif(1, &[(1, b"grid"), (2, b"av01"), (3, b"av01")], Some(&shared), Some((version, 1, &[2, 3])))), Some("avif"));
            }
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

    fn audio_tables(bytes: &[u8]) -> &[u8] {
        let mut budget = RECORDS;
        let mut data = bytes;
        for kind in STBL {
            data = boxes(data, &mut budget).unwrap().into_iter()
                .find(|v| v.kind == *kind).unwrap().data;
        }
        data
    }

    fn mp4a_entry(bytes: &[u8], version: u16, extensions: &[u8]) -> Vec<u8> {
        let mut budget = RECORDS;
        let tables = boxes(audio_tables(bytes), &mut budget).unwrap();
        let entries = boxes(&child(&tables, b"stsd").unwrap()[8..], &mut budget).unwrap();
        let mut entry = entries[0].data[..28].to_vec();
        entry[8..10].copy_from_slice(&version.to_be_bytes());
        entry.resize(match version { 0 => 28, 1 => 44, 2 => 64, _ => unreachable!() }, 0);
        entry.extend_from_slice(extensions);
        let stsd = [words(&[0, 1]), iso_box(b"mp4a", &entry)].concat();
        audio_table(bytes, b"stsd", Some(&stsd))
    }

    fn aac_esds() -> Vec<u8> {
        // ES_ID 1, audio DecoderConfigDescriptor, 128 kbit/s, AAC-LC at
        // 8 kHz mono (AudioSpecificConfig 0x1588), SL predefined MP4 (2).
        vec![
            0, 0, 0, 0, 0x03, 0x19, 0, 1, 0, 0x04, 0x11, 0x40, 0x15,
            0, 0, 0, 0, 1, 0xf4, 0, 0, 1, 0xf4, 0,
            0x05, 0x02, 0x15, 0x88, 0x06, 0x01, 0x02,
        ]
    }

    fn test_descriptor(tag: u8, data: &[u8]) -> Vec<u8> {
        let mut length = vec![(data.len() & 127) as u8];
        let mut rest = data.len() >> 7;
        while rest > 0 {
            length.insert(0, 0x80 | (rest & 127) as u8);
            rest >>= 7;
        }
        [vec![tag], length, data.to_vec()].concat()
    }

    #[test]
    fn mp4a_requires_configured_audio_directly_or_inside_quicktime_wave() {
        let bytes = m4a(false);
        let esds = iso_box(b"esds", &aac_esds());
        let wave = iso_box(b"wave", &[
            iso_box(b"frma", b"mp4a"), iso_box(b"mp4a", &[0; 4]),
            esds.clone(), iso_box(&[0; 4], &[]),
        ].concat());
        for version in [0, 1, 2] {
            for extension in [&esds, &wave, &iso_box(b"wave", &wave)] {
                assert_eq!(detect(&mp4a_entry(&bytes, version, extension)), Some("m4a"));
            }
            for extension in [vec![], iso_box(b"esds", &[0; 4]), iso_box(b"wave", &[]),
                [esds.clone(), wave.clone()].concat(), [esds.clone(), esds.clone()].concat()]
            {
                assert_eq!(detect(&mp4a_entry(&bytes, version, &extension)), None);
            }
        }
        // An unused description need not have decoder configuration. Once a
        // chunk references it, it must satisfy the same admission as the first.
        let unconfigured = mp4a_entry(&bytes, 0, &[]);
        let configured = mp4a_entry(&bytes, 0, &esds);
        let mut budget = RECORDS;
        let good = boxes(audio_tables(&configured), &mut budget).unwrap();
        let bad = boxes(audio_tables(&unconfigured), &mut budget).unwrap();
        let descriptions = [words(&[0, 2]), child(&good, b"stsd").unwrap()[8..].to_vec(),
            child(&bad, b"stsd").unwrap()[8..].to_vec()].concat();
        let both = audio_table(&bytes, b"stsd", Some(&descriptions));
        assert_eq!(detect(&both), Some("m4a"));
        assert_eq!(detect(&audio_table(&both, b"stsc", Some(&words(&[0, 1, 1, 800, 2])))), None);
    }

    #[test]
    fn esds_descriptor_framing_and_audio_object_types() {
        let bytes = m4a(false);
        let valid = aac_esds();
        let admit = |data: &[u8]| detect(&mp4a_entry(&bytes, 0, &iso_box(b"esds", data)));
        assert_eq!(admit(&valid), Some("m4a"));
        for n in 0..valid.len() { assert_eq!(admit(&valid[..n]), None, "prefix {n}"); }
        for (at, value) in [(0, 1), (4, 2), (9, 5), (11, 0), (11, 0x20), (12, 0x11), (12, 0x14)] {
            let mut bad = valid.clone();
            bad[at] = value;
            assert_eq!(admit(&bad), None, "field {at}: {value}");
        }
        let wrap_es = |children: &[u8]| [vec![0; 4], test_descriptor(3, &[vec![0, 1, 0], children.to_vec()].concat())].concat();
        assert_eq!(admit(&wrap_es(&[6, 1, 2])), None, "missing decoder config");
        let config = &valid[11..24];
        for specific in [vec![], vec![5, 0], vec![5, 1, 0], vec![5, 2, 0x15],
            vec![5, 0x80], vec![5, 0x80, 0x80, 0x80, 0x80, 0],
            vec![5, 0xff, 0xff, 0xff, 0x7f], vec![5, 2, 0x15, 0x88, 5, 2, 0x15, 0x88]]
        {
            let decoder = test_descriptor(4, &[config.to_vec(), specific].concat());
            assert_eq!(admit(&wrap_es(&decoder)), None);
        }
        assert_eq!(admit(&wrap_es(&test_descriptor(4, &config[..12]))), None);
        assert_eq!(admit(&wrap_es(&[valid[9..28].to_vec(), valid[9..28].to_vec()].concat())), None);
        // These audio object indications carry no AAC AudioSpecificConfig.
        for object in [0x69, 0x6b, 0xa5, 0xa6, 0xa9, 0xad, 0xc1] {
            let mut config = config.to_vec();
            config[0] = object;
            assert_eq!(admit(&wrap_es(&test_descriptor(4, &config))), Some("m4a"));
        }
        for object in [0x66, 0x67, 0x68] {
            let mut configured = valid.clone();
            configured[11] = object;
            assert_eq!(admit(&configured), Some("m4a"));
        }
        // Four-byte padded lengths at every level are common encoder output.
        let specific = vec![5, 0x80, 0x80, 0x80, 2, 0x15, 0x88];
        let decoder = [vec![4, 0x80, 0x80, 0x80, 20], config.to_vec(), specific].concat();
        let padded = [vec![0, 0, 0, 0, 3, 0x80, 0x80, 0x80, 31, 0, 1, 0], decoder, vec![6, 1, 2]].concat();
        assert_eq!(admit(&padded), Some("m4a"));
        // Dependency and OCR IDs are independent of the forbidden URL flag.
        for flags in [0, 0x80, 0x20, 0xa0] {
            let mut es = vec![0, 1, flags];
            if flags & 0x80 != 0 { es.extend([0, 2]); }
            if flags & 0x20 != 0 { es.extend([0, 3]); }
            let header = es.clone();
            es.extend_from_slice(&valid[9..]);
            assert_eq!(admit(&[vec![0; 4], test_descriptor(3, &es)].concat()), Some("m4a"));
            for n in 3..header.len() {
                assert_eq!(admit(&[vec![0; 4], test_descriptor(3, &header[..n])].concat()), None);
            }
            for length in [0, 3, 130] {
                let mut es = vec![0, 1, flags | 0x40];
                if flags & 0x80 != 0 { es.extend([0, 2]); }
                es.push(length);
                es.extend(vec![b'x'; usize::from(length)]);
                if flags & 0x20 != 0 { es.extend([0, 3]); }
                es.extend_from_slice(&valid[9..]);
                assert_eq!(admit(&[vec![0; 4], test_descriptor(3, &es)].concat()), None);
            }
        }
        let mut trailing = valid.clone(); trailing.push(0);
        assert_eq!(admit(&trailing), None);
        let mut budget = 0;
        assert!(esds(&valid, &mut budget).is_none());
    }

    fn audio_bits(fields: &[(u32, usize)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut at = 0;
        for &(value, width) in fields {
            for shift in (0..width).rev() {
                if at % 8 == 0 { bytes.push(0); }
                bytes[at / 8] |= (((value >> shift) & 1) as u8) << (7 - at % 8);
                at += 1;
            }
        }
        bytes
    }

    #[test]
    fn aac_elementary_headers_and_sbr_ps_extensions() {
        let bytes = m4a(false);
        let valid = aac_esds();
        let admit = |object: u8, asc: &[u8]| {
            let mut config = valid[11..24].to_vec();
            config[0] = object;
            config.extend(test_descriptor(5, asc));
            let es = [vec![0, 1, 0], test_descriptor(4, &config)].concat();
            let esds = [vec![0; 4], test_descriptor(3, &es)].concat();
            detect(&mp4a_entry(&bytes, 0, &iso_box(b"esds", &esds)))
        };
        let lc = [(2, 5), (4, 4), (2, 4), (0, 3)];
        let sync = [lc.as_slice(), &[(0x2b7, 11), (5, 5), (1, 1), (3, 4)]].concat();
        let ps = [sync.as_slice(), &[(0x548, 11), (1, 1)]].concat();
        // AAC-LC stereo 44.1 kHz, mono 8 kHz, explicit HE-AAC and HE-AAC v2.
        let mut good = vec![vec![0x12, 0x10], vec![0x15, 0x88],
            vec![0x2b, 0x92, 0x08, 0], vec![0xeb, 0x92, 0x08, 0],
            audio_bits(&sync), audio_bits(&ps),
            audio_bits(&[(2, 5), (15, 4), (44100, 24), (2, 4), (0, 3)]),
            audio_bits(&[(5, 5), (7, 4), (2, 4), (15, 4), (44100, 24), (2, 5), (0, 3)]),
            audio_bits(&[(2, 5), (4, 4), (2, 4), (0, 3), (0x2b7, 11), (5, 5),
                (1, 1), (15, 4), (48000, 24)]),
            audio_bits(&[(2, 5), (4, 4), (2, 4), (0, 1), (1, 1), (7, 14), (0, 1)]),
            audio_bits(&[(2, 5), (4, 4), (2, 4), (1, 3), (0, 1)]),
            audio_bits(&[(6, 5), (4, 4), (2, 4), (0, 3), (0, 3)]),
            // ER AAC-LC: extensionFlag, resilience flags, extensionFlag3, epConfig.
            audio_bits(&[(17, 5), (4, 4), (2, 4), (1, 3), (0, 3), (0, 1), (0, 2)]),
            audio_bits(&[(17, 5), (4, 4), (2, 4), (1, 3), (7, 3), (0, 1), (1, 2)]),
        ];
        for channels in [1, 3, 4, 5, 6, 7, 11, 12, 13, 14] {
            good.push(audio_bits(&[(2, 5), (4, 4), (channels, 4), (0, 3)]));
        }
        // Channel configuration zero supplies a complete stereo PCE, including
        // byte alignment and the comment length. No packet decoding is needed.
        let pce = audio_bits(&[(2, 5), (4, 4), (0, 4), (0, 3), (0, 4), (1, 2), (4, 4),
            (1, 4), (0, 4), (0, 4), (0, 2), (0, 3), (0, 4), (0, 3), (1, 1), (0, 4),
            (0, 1), (0, 8)]);
        good.push(pce.clone());
        let mut bad = vec![vec![], vec![0x12], vec![0, 0], vec![0xff, 0xff], vec![0xf8],
            vec![0x17, 0x80], vec![0x12, 0x11], // truncated explicit rate / extensionFlag3
            audio_bits(&[(0, 5), (4, 4), (2, 4), (0, 3)]),
            audio_bits(&[(10, 5), (4, 4), (2, 4), (0, 3)]), // reserved AOT
            audio_bits(&[(2, 5), (15, 4), (0, 24), (2, 4), (0, 3)]),
            audio_bits(&[(2, 5), (4, 4), (2, 4), (0, 1), (1, 1)]), // core delay absent
            audio_bits(&[(3, 5), (4, 4), (2, 4), (4, 3)]), // SSR short frame
            audio_bits(&[(6, 5), (4, 4), (2, 4), (0, 3)]), // scalable layer absent
            audio_bits(&[(17, 5), (4, 4), (2, 4), (0, 3), (0, 2)]), // ER extensionFlag clear
            audio_bits(&[(17, 5), (4, 4), (2, 4), (1, 3)]), // resilience flags absent
            audio_bits(&[(17, 5), (4, 4), (2, 4), (1, 3), (0, 3), (1, 1), (0, 2)]), // extensionFlag3 set
            // ER scalable leaves only one epConfig bit after layer/resilience/extensionFlag3.
            audio_bits(&[(20, 5), (4, 4), (2, 4), (1, 3), (0, 3), (0, 3), (0, 1)]),
            audio_bits(&[(17, 5), (4, 4), (2, 4), (1, 3), (0, 3), (0, 1), (2, 2)]),
            audio_bits(&[(17, 5), (4, 4), (2, 4), (1, 3), (0, 3), (0, 1), (3, 2)]),
            audio_bits(&[(2, 5), (4, 4), (2, 4), (0, 3), (0x2b7, 11), (31, 5)]),
            audio_bits(&[(2, 5), (4, 4), (2, 4), (0, 3), (0x2b7, 11), (5, 5)]),
        ];
        for frequency in [13, 14] {
            bad.push(audio_bits(&[(2, 5), (frequency, 4), (2, 4), (0, 3)]));
            bad.push(audio_bits(&[(5, 5), (7, 4), (2, 4), (frequency, 4), (2, 5), (0, 3)]));
            bad.push(audio_bits(&[lc.as_slice(), &[(0x2b7, 11), (5, 5), (1, 1), (frequency, 4)]].concat()));
        }
        for aot in [19, 20, 21, 22, 23] {
            bad.push(audio_bits(&[(aot, 5), (4, 4), (2, 4), (0, 3), (0, 24)]));
        }
        for configuration in [&good[2], &good[7], &pce] {
            for n in 0..configuration.len() { bad.push(configuration[..n].to_vec()); }
        }
        // Before a complete sync marker, an LC header is valid on its own.
        for n in 4..good[8].len() { bad.push(good[8][..n].to_vec()); }
        for object in [0x40, 0x66, 0x67, 0x68] {
            for asc in &good { assert_eq!(admit(object, asc), Some("m4a"), "{object:x}: {asc:x?}"); }
            for asc in &bad { assert_eq!(admit(object, asc), None, "{object:x}: {asc:x?}"); }
        }
    }

    #[test]
    fn audio_requires_one_exact_timing_table_with_matching_sample_count() {
        let bytes = m4a(false);
        let valid = words(&[0, 2, 300, 1, 500, 2]);
        assert_eq!(detect(&audio_table(&bytes, b"stts", Some(&valid))), Some("m4a"));
        assert_eq!(detect(&audio_table(&bytes, b"stts", None)), None);
        for n in 0..valid.len() {
            assert_eq!(detect(&audio_table(&bytes, b"stts", Some(&valid[..n]))), None);
        }
        for invalid in [
            words(&[0, 0]), words(&[0, 1, 799, 1]), words(&[0, 1, 801, 1]),
            words(&[0, 2, 300, 1, 499, 1]), words(&[0, 2, 300, 1, 501, 1]),
            words(&[0, 1, 800, 1, 0]), words(&[0, 2, 800, 1]),
            words(&[1, 1, 800, 1]), words(&[0, u32::MAX, 800, 1]),
            words(&[0, 2, 0, 1, 800, 1]), words(&[0, 2, u32::MAX, 1, 801, 1]),
        ] {
            assert_eq!(detect(&audio_table(&bytes, b"stts", Some(&invalid))), None, "{invalid:?}");
        }
        let duplicate = [audio_tables(&bytes), &iso_box(b"stts", &words(&[0, 1, 800, 1]))].concat();
        assert_eq!(detect(&replace(&bytes, STBL, Some(&duplicate))), None);
        // A compact timing run supports long PCM without a per-sample budget.
        let count = 1_000_000;
        let long = replace(&bytes, &[b"mdat"], Some(&vec![0; count * 2]));
        let long = audio_table(&long, b"stsz", Some(&words(&[0, 2, count as u32])));
        let long = audio_table(&long, b"stsc", Some(&words(&[0, 1, 1, count as u32, 1])));
        let long = audio_table(&long, b"stts", Some(&words(&[0, 1, count as u32, 1])));
        assert!(long.len() < crate::mcp_service::attachments::FILE_BYTES);
        assert_eq!(detect(&long), Some("m4a"));
        // Many on-disk timing rows, like sizes and chunks, are bounded by bytes.
        let count = 70000;
        let variable = aac_like(count, count, false, false);
        let timing = [words(&[0, count as u32]), words(&[1, 1024]).repeat(count)].concat();
        assert_eq!(detect(&audio_table(&variable, b"stts", Some(&timing))), Some("m4a"));
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
        mp4a_entry(&bytes, 0, &iso_box(b"esds", &aac_esds()))
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
        let huge = audio_table(&huge, b"stts", Some(&words(&[0, 1, u32::MAX, 1])));
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
        // Other sample entries need no AAC-specific configuration.
        for codec in [b"alac", b"ac-3", b"Opus"] {
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
        for kind in [b"stsz", b"stsc", b"stco", b"stts"] {
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
            let mut invalid = audio_table(&bytes, kind, Some(&data));
            if kind == b"stsz" {
                invalid = audio_table(&invalid, b"stts", Some(&words(&[0, 1, be32(&data[8..]), 1])));
            }
            assert_eq!(detect(&invalid), None, "{kind:?}: {data:?}");
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
