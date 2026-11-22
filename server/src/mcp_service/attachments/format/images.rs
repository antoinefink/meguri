// SPDX-License-Identifier: MIT
use super::{RECORDS, le32};
fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes(b[..2].try_into().unwrap())
}
fn blocks(b: &[u8], at: &mut usize) -> bool {
    for _ in 0..RECORDS {
        let Some(&n) = b.get(*at) else {
            return false;
        };
        *at += 1;
        if n == 0 {
            return true;
        }
        *at += usize::from(n);
        if *at > b.len() {
            return false;
        }
    }
    false
}
pub(super) fn gif(b: &[u8]) -> bool {
    if b.len() < 14
        || !(b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a"))
        || le16(&b[6..]) == 0
        || le16(&b[8..]) == 0
    {
        return false;
    }
    let mut at = 13
        + if b[10] & 128 != 0 {
            3 * (2usize << (b[10] & 7))
        } else {
            0
        };
    let mut image = false;
    for _ in 0..RECORDS {
        match b.get(at) {
            Some(0x3b) => return image && at + 1 == b.len(),
            Some(0x21) => {
                if b.get(at + 1).is_none() {
                    return false;
                }
                at += 2;
                if !blocks(b, &mut at) {
                    return false;
                }
            }
            Some(0x2c) => {
                let Some(h) = b.get(at..at + 10) else {
                    return false;
                };
                if le16(&h[5..]) == 0 || le16(&h[7..]) == 0 {
                    return false;
                }
                at += 10
                    + if h[9] & 128 != 0 {
                        3 * (2usize << (h[9] & 7))
                    } else {
                        0
                    };
                if !b.get(at).is_some_and(|v| (2..=8).contains(v)) {
                    return false;
                }
                at += 1;
                let start = at;
                if !blocks(b, &mut at) || at == start + 1 {
                    return false;
                }
                image = true;
            }
            _ => return false,
        }
    }
    false
}
fn bitstream(kind: &[u8], p: &[u8]) -> bool {
    match kind {
        b"VP8 " => {
            p.len() > 10
                && p[0] & 1 == 0
                && p[3..6] == [0x9d, 0x01, 0x2a]
                && le16(&p[6..]) & 0x3fff != 0
                && le16(&p[8..]) & 0x3fff != 0
        }
        b"VP8L" => p.len() > 5 && p[0] == 0x2f && p[4] & 0xe0 == 0,
        _ => false,
    }
}
// RIFF sizes include padding. Walk animation subchunks as well as still images.
pub(super) fn webp(b: &[u8]) -> bool {
    if b.len() < 20
        || !b.starts_with(b"RIFF")
        || &b[8..12] != b"WEBP"
        || le32(&b[4..]) as usize != b.len() - 8
    {
        return false;
    }
    let mut at = 12;
    let mut image = false;
    for _ in 0..RECORDS {
        if at == b.len() {
            return image;
        }
        let Some(h) = b.get(at..at + 8) else {
            return false;
        };
        let n = le32(&h[4..]) as usize;
        let Some(end) = (at + 8).checked_add(n) else {
            return false;
        };
        let Some(p) = b.get(at + 8..end) else {
            return false;
        };
        match &h[..4] {
            b"VP8 " | b"VP8L" => {
                if !bitstream(&h[..4], p) {
                    return false;
                }
                image = true;
            }
            b"VP8X" => {
                if at != 12 || n != 10 || p[0] & 0xc1 != 0 || p[1..4] != [0, 0, 0] {
                    return false;
                }
            }
            b"ANIM" => {
                if n != 6 {
                    return false;
                }
            }
            b"ANMF" => {
                if n < 24 || p[15] & 0xfc != 0 {
                    return false;
                }
                let mut sub = 16;
                let mut frame = false;
                for _ in 0..RECORDS {
                    if sub == n {
                        break;
                    }
                    let Some(sh) = p.get(sub..sub + 8) else {
                        return false;
                    };
                    let size = le32(&sh[4..]) as usize;
                    let Some(data) = p.get(sub + 8..(sub + 8).saturating_add(size)) else {
                        return false;
                    };
                    if &sh[..4] != b"ALPH" {
                        if !bitstream(&sh[..4], data) {
                            return false;
                        }
                        frame = true;
                    }
                    sub += 8 + size + (size & 1);
                }
                if sub != n || !frame {
                    return false;
                }
                image = true;
            }
            b"ALPH" | b"ICCP" | b"EXIF" | b"XMP " => {}
            _ => return false,
        }
        at = end + (n & 1);
        if at > b.len() {
            return false;
        }
    }
    false
}
