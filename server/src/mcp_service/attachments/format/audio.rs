// SPDX-License-Identifier: MIT
use super::{RECORDS, be16, be32, le32};
pub(super) fn detect(b: &[u8]) -> Option<&'static str> {
    if wav(b) {
        Some("wav")
    } else if flac(b) {
        Some("flac")
    } else if adts(b) {
        Some("aac")
    } else if iff(b, false) {
        Some("aiff")
    } else if iff(b, true) {
        Some("aifc")
    } else if caf(b) {
        Some("caf")
    } else if ogg(b) {
        Some("ogg")
    } else {
        None
    }
}
fn wav(b: &[u8]) -> bool {
    if b.len() < 12
        || &b[..4] != b"RIFF"
        || &b[8..12] != b"WAVE"
        || le32(&b[4..]) as usize != b.len() - 8
    {
        return false;
    }
    let mut at = 12;
    let mut format = false;
    let mut data = false;
    for _ in 0..RECORDS {
        if at == b.len() {
            return format && data;
        }
        let Some(h) = b.get(at..at + 8) else {
            return false;
        };
        let n = le32(&h[4..]) as usize;
        let Some(p) = b.get(at + 8..(at + 8).saturating_add(n)) else {
            return false;
        };
        match &h[..4] {
            b"fmt " => {
                if format
                    || n < 16
                    || p[2..4] == [0, 0]
                    || le32(&p[4..]) == 0
                    || p[12..14] == [0, 0]
                {
                    return false;
                }
                let code = u16::from_le_bytes(p[..2].try_into().unwrap());
                if !matches!(code, 1 | 2 | 3 | 6 | 7 | 17 | 0xfffe) || (code == 0xfffe && n < 40) {
                    return false;
                }
                if n > 16
                    && (n < 18
                        || usize::from(u16::from_le_bytes(p[16..18].try_into().unwrap())) > n - 18)
                {
                    return false;
                }
                format = true;
            }
            b"data" => {
                if n == 0 {
                    return false;
                }
                data = true;
            }
            _ => {}
        }
        at += 8 + n + (n & 1);
    }
    false
}
fn flac(b: &[u8]) -> bool {
    if !b.starts_with(b"fLaC") {
        return false;
    }
    let mut at = 4;
    for index in 0..RECORDS {
        let Some(h) = b.get(at..at + 4) else {
            return false;
        };
        let n = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
        let Some(p) = b.get(at + 4..at + 4 + n) else {
            return false;
        };
        let kind = h[0] & 0x7f;
        if kind == 127 || (index == 0 && (kind != 0 || n != 34)) || (index > 0 && kind == 0) {
            return false;
        }
        if index == 0 && (be16(p) < 16 || be16(&p[2..]) < be16(p) || (be32(&p[10..]) >> 12) == 0) {
            return false;
        }
        at += 4 + n;
        if h[0] & 0x80 != 0 {
            let Some(frame) = b.get(at..) else {
                return false;
            };
            let mut crc = 0u16;
            for &byte in frame {
                crc ^= u16::from(byte) << 8;
                for _ in 0..8 {
                    crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x8005 } else { 0 };
                }
            }
            return crc == 0
                && frame.len() >= 8
                && frame[0] == 0xff
                && frame[1] & 0xfe == 0xf8
                && frame[2] >> 4 != 0
                && frame[2] & 15 != 15
                && frame[3] & 1 == 0;
        }
    }
    false
}
fn adts(b: &[u8]) -> bool {
    let mut at = 0;
    let mut frames = 0;
    for _ in 0..RECORDS {
        if at == b.len() {
            return frames > 0;
        }
        let Some(h) = b.get(at..at + 7) else {
            return false;
        };
        if h[0] != 0xff || h[1] & 0xf6 != 0xf0 || h[2] & 0x3c >= 0x34 {
            return false;
        }
        let size =
            (usize::from(h[3] & 3) << 11) | (usize::from(h[4]) << 3) | usize::from(h[5] >> 5);
        let header = if h[1] & 1 != 0 { 7 } else { 9 };
        if size <= header || size > b.len() - at {
            return false;
        }
        at += size;
        frames += 1;
    }
    false
}
fn iff(b: &[u8], compressed: bool) -> bool {
    if b.len() < 12
        || &b[..4] != b"FORM"
        || &b[8..12] != if compressed { b"AIFC" } else { b"AIFF" }
        || be32(&b[4..]) as usize != b.len() - 8
    {
        return false;
    }
    let mut at = 12;
    let mut comm = false;
    let mut sound = false;
    for _ in 0..RECORDS {
        if at == b.len() {
            return comm && sound;
        }
        let Some(h) = b.get(at..at + 8) else {
            return false;
        };
        let n = be32(&h[4..]) as usize;
        let Some(p) = b.get(at + 8..(at + 8).saturating_add(n)) else {
            return false;
        };
        match &h[..4] {
            b"COMM" => {
                if comm
                    || n < if compressed { 23 } else { 18 }
                    || be16(p) == 0
                    || be32(&p[2..]) == 0
                    || be16(&p[6..]) == 0
                    || be16(&p[8..]) & 0x7fff == 0
                {
                    return false;
                }
                if compressed && 23 + usize::from(p[22]) > n {
                    return false;
                }
                comm = true;
            }
            b"SSND" => {
                if n <= 8 || be32(p) as usize >= n - 8 {
                    return false;
                }
                sound = true;
            }
            _ => {}
        }
        at += 8 + n + (n & 1);
    }
    false
}
fn caf(b: &[u8]) -> bool {
    if b.len() < 8 || &b[..4] != b"caff" || b[4..8] != [0, 1, 0, 0] {
        return false;
    }
    let mut at = 8;
    let mut desc = false;
    let mut data = false;
    for _ in 0..RECORDS {
        if at == b.len() {
            return desc && data;
        }
        let Some(h) = b.get(at..at + 12) else {
            return false;
        };
        let size = i64::from_be_bytes(h[4..12].try_into().unwrap());
        let n = if size == -1 && &h[..4] == b"data" {
            b.len() - at - 12
        } else {
            let Ok(n) = usize::try_from(size) else {
                return false;
            };
            n
        };
        let Some(p) = b.get(at + 12..(at + 12).saturating_add(n)) else {
            return false;
        };
        match &h[..4] {
            b"desc" => {
                if desc || at != 8 || n != 32 {
                    return false;
                }
                let rate = f64::from_be_bytes(p[..8].try_into().unwrap());
                if !rate.is_finite() || rate <= 0.0 || be32(&p[24..]) == 0 {
                    return false;
                }
                desc = true;
            }
            b"data" => {
                if n <= 4 {
                    return false;
                }
                data = true;
            }
            _ => {}
        }
        at += 12 + n;
    }
    false
}
// Ogg packet lacing can span pages. CRC and sequence checks cover every page;
// only codec identification packets are buffered, with a small fixed ceiling.
fn ogg(b: &[u8]) -> bool {
    let mut at = 0;
    let mut serial = None;
    // At most one new serial per page, bounded by the file's RECORDS budget.
    let mut seen_serials = std::collections::HashSet::new();
    let mut sequence = 0u32;
    let mut packet = Vec::new();
    let mut identified = false;
    let mut continued = false;
    let mut ended = false;
    let mut packets = 0;
    let mut is_opus = false;
    let mut packet_size = 0usize;
    for _ in 0..RECORDS {
        if ended {
            if !identified || continued || packets < if is_opus { 3 } else { 4 } {
                return false;
            }
            if at == b.len() {
                return true;
            }
            // Chained audio streams have fresh BOS, serial, codec and sequence.
            serial = None;
            sequence = 0;
            packet.clear();
            identified = false;
            packets = 0;
        }
        if at == b.len() {
            return false;
        }
        let Some(h) = b.get(at..at + 27) else {
            return false;
        };
        if &h[..4] != b"OggS" || h[4] != 0 || h[5] & !7 != 0 {
            return false;
        }
        let id = le32(&h[14..]);
        if let Some(prior) = serial {
            if prior != id || h[5] & 2 != 0 {
                return false;
            }
        } else {
            if h[5] & 2 == 0 || h[5] & 1 != 0 || !seen_serials.insert(id) {
                return false;
            }
            serial = Some(id);
        }
        if le32(&h[18..]) != sequence || (h[5] & 1 != 0) != continued {
            return false;
        }
        sequence = sequence.wrapping_add(1);
        let n = usize::from(h[26]);
        let Some(laces) = b.get(at + 27..at + 27 + n) else {
            return false;
        };
        let size: usize = laces.iter().map(|v| usize::from(*v)).sum();
        let end = at + 27 + n + size;
        let Some(page) = b.get(at..end) else {
            return false;
        };
        let mut crc = 0u32;
        for (i, byte) in page.iter().enumerate() {
            crc ^= u32::from(if (22..26).contains(&i) { 0 } else { *byte }) << 24;
            for _ in 0..8 {
                crc = (crc << 1)
                    ^ if crc & 0x8000_0000 != 0 {
                        0x04c1_1db7
                    } else {
                        0
                    };
            }
        }
        if crc != le32(&h[22..]) {
            return false;
        }
        let mut body = at + 27 + n;
        for &lace in laces {
            let size = usize::from(lace);
            packet_size += size;
            if !identified && packet_size > 512 {
                return false;
            }
            let keep = size.min(512 - packet.len());
            packet.extend_from_slice(&b[body..body + keep]);
            body += size;
            continued = lace == 255;
            if !continued && !identified {
                let opus = packet.len() >= 19
                    && packet.starts_with(b"OpusHead")
                    && packet[8] <= 15
                    && packet[9] > 0
                    && (packet[18] == 0 && packet[9] <= 2
                        || packet[18] != 0 && packet.len() >= 21 + usize::from(packet[9]));
                let vorbis = packet.len() == 30
                    && packet.starts_with(b"\x01vorbis")
                    && le32(&packet[7..]) == 0
                    && packet[11] > 0
                    && le32(&packet[12..]) > 0
                    && packet[29] & 1 == 1
                    && (6..=13).contains(&(packet[28] & 15))
                    && packet[28] >> 4 >= packet[28] & 15
                    && packet[28] >> 4 <= 13;
                if !opus && !vorbis {
                    return false;
                }
                identified = true;
                is_opus = opus;
            }
            if !continued {
                if packets == 1
                    && (packet_size < 16
                        || !packet.starts_with(if is_opus { b"OpusTags" } else { b"\x03vorbis" }))
                {
                    return false;
                }
                if !is_opus && packets == 2 && !packet.starts_with(b"\x05vorbis") {
                    return false;
                }
                if packet_size == 0 {
                    return false;
                }
                packets += 1;
                packet.clear();
                packet_size = 0;
            }
        }
        ended = h[5] & 4 != 0;
        at = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ogg;
    use crate::mcp_service::attachment_formats as fixtures;

    #[test]
    fn ogg_chains_require_serials_unique_across_the_whole_file() {
        for codecs in [[true, false, true], [false, true, false]] {
            let mut distinct = Vec::new();
            for (index, opus) in codecs.into_iter().enumerate() {
                distinct.extend(fixtures::ogg_with_serial(opus, index as u32));
            }
            assert!(ogg(&distinct), "distinct serials, including zero");
            assert!(ogg(&distinct), "serial tracking is local to each file");

            let mut duplicate = fixtures::ogg_with_serial(codecs[0], 17);
            duplicate.extend(fixtures::ogg_with_serial(codecs[1], 17));
            assert!(!ogg(&duplicate), "adjacent streams reuse a serial");

            let mut duplicate = fixtures::ogg_with_serial(codecs[0], 17);
            duplicate.extend(fixtures::ogg_with_serial(codecs[1], 18));
            duplicate.extend(fixtures::ogg_with_serial(codecs[2], 17));
            assert!(!ogg(&duplicate), "a nonadjacent stream reuses a serial");
        }
    }
}
