// SPDX-License-Identifier: MIT
//! ISO box framing, image item metadata and audio-only track admission.
use super::{RECORDS, be16, be32};
struct BoxRef<'a> {
    kind: &'a [u8],
    data: &'a [u8],
}
fn boxes<'a>(mut b: &'a [u8], budget: &mut usize) -> Option<Vec<BoxRef<'a>>> {
    let mut out = Vec::new();
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
        });
        b = b.get(n..)?;
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
    locations(
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
    for property in configs {
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
    (coded && spatial && codec).then_some(())
}
// Item locations may use file offsets or the meta box's idat payload. Unknown
// external references and constructions cannot be verified without outbound I/O.
fn locations(
    b: &[u8],
    primary: u32,
    file_len: usize,
    idat_len: Option<usize>,
    budget: &mut usize,
) -> Option<()> {
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
    let mut found = false;
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
        found |= id == primary;
    }
    (found && at == b.len()).then_some(())
}
fn audio(moov: &[u8], budget: &mut usize) -> Option<()> {
    let items = boxes(moov, budget)?;
    let mut tracks = 0;
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
        for entry in entries {
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
        }
        tracks += 1;
    }
    (tracks > 0).then_some(())
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
    audio(child(&top, b"moov")?, &mut budget)?;
    Some("m4a")
}
