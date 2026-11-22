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
fn le24(b: &[u8]) -> u32 {
    u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16)
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
    let mut canvas = None;
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
                canvas = Some((le24(&p[4..]) + 1, le24(&p[7..]) + 1));
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
                let Some((width, height)) = canvas else {
                    return false;
                };
                // Offsets are in pairs of pixels; dimensions store size minus one.
                // These sums of 24-bit fields fit in u32 even at their maxima.
                if le24(p) * 2 + le24(&p[6..]) + 1 > width
                    || le24(&p[3..]) * 2 + le24(&p[9..]) + 1 > height
                {
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
                    if matches!(&sh[..4], b"VP8 " | b"VP8L") {
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
            _ if canvas.is_some() => {}
            _ => return false,
        }
        at = end + (n & 1);
        if at > b.len() {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::webp;

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = kind.to_vec();
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() & 1 != 0 {
            out.push(0);
        }
        out
    }

    fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for chunk in chunks {
            body.extend_from_slice(chunk);
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    fn canvas(width: u32, height: u32, animated: bool) -> Vec<u8> {
        let mut data = vec![if animated { 2 } else { 0 }, 0, 0, 0];
        data.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        data.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        chunk(b"VP8X", &data)
    }

    fn lossless() -> Vec<u8> {
        chunk(b"VP8L", &[0x2f, 0, 0, 0, 0, 0x88, 0x88, 8])
    }

    // Raw 24-bit fields: x/2, y/2, width-1, height-1.
    fn animation(canvas_size: Option<(u32, u32)>, rect: [u32; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut data = Vec::new();
        for field in rect {
            data.extend_from_slice(&field.to_le_bytes()[..3]);
        }
        data.extend_from_slice(&[1, 0, 0, 0]); // Duration and frame flags.
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        let mut body = Vec::new();
        if let Some((width, height)) = canvas_size {
            body.push(canvas(width, height, true));
        }
        body.push(chunk(b"ANIM", &[0; 6]));
        body.push(chunk(b"ANMF", &data));
        riff(&body)
    }

    #[test]
    fn webp_frame_rectangles_decode_offsets_and_dimensions() {
        for rect in [[0, 0, 0, 0], [0, 0, 4, 3], [1, 1, 2, 1], [2, 0, 0, 3]] {
            assert!(webp(&animation(Some((5, 4)), rect, &[lossless()])), "{rect:?}");
        }
        for rect in [
            [0, 0, 5, 3], // Width exceeds canvas by one.
            [0, 0, 4, 4], // Height exceeds canvas by one.
            [1, 1, 3, 1], // Doubled x plus decoded width exceeds canvas.
            [1, 1, 2, 2], // Doubled y plus decoded height exceeds canvas.
            [3, 0, 0, 0], // Origin outside canvas, even for a one-pixel frame.
            [0, 2, 0, 0],
        ] {
            assert!(!webp(&animation(Some((5, 4)), rect, &[lossless()])), "{rect:?}");
        }
        assert!(webp(&animation(Some((1, 1)), [0; 4], &[lossless()])));
        assert!(!webp(&animation(None, [0; 4], &[lossless()])));
    }

    #[test]
    fn webp_frame_rectangles_use_all_24_bits_without_overflow() {
        let canvas = Some((0x1000000, 0x1000000));
        for rect in [[0, 0, 0xffffff, 0xffffff], [0x7fffff, 0x7fffff, 1, 1]] {
            assert!(webp(&animation(canvas, rect, &[lossless()])), "{rect:?}");
        }
        for rect in [
            [0x800000, 0, 0, 0],
            [0, 0x800000, 0, 0],
            [0xffffff, 0xffffff, 0xffffff, 0xffffff],
        ] {
            assert!(!webp(&animation(canvas, rect, &[lossless()])), "{rect:?}");
        }
    }

    #[test]
    fn webp_extended_stills_and_frames_accept_unknown_trailing_chunks() {
        let lossy = chunk(b"VP8 ", &[0, 0, 0, 0x9d, 1, 0x2a, 1, 0, 1, 0, 0]);
        for image in [lossless(), lossy] {
            for unknown in [chunk(b"UNKN", &[]), chunk(b"UNKN", b"odd"), chunk(b"UNKN", b"even")] {
                assert!(webp(&riff(&[
                    canvas(1, 1, false), image.clone(), unknown.clone(),
                ])));
                assert!(webp(&animation(
                    Some((1, 1)), [0; 4], &[image.clone(), unknown.clone()],
                )));
                assert!(webp(&animation(
                    Some((1, 1)), [0; 4], &[chunk(b"ALPH", &[0, 1]), unknown, image.clone()],
                )));
            }
        }
    }

    #[test]
    fn webp_unknown_chunks_do_not_replace_a_known_bitstream() {
        let unknown = chunk(b"UNKN", b"opaque");
        assert!(!webp(&riff(&[canvas(1, 1, false), unknown.clone()])));
        assert!(!webp(&animation(Some((1, 1)), [0; 4], &[unknown.clone()])));
        assert!(!webp(&animation(
            Some((1, 1)), [0; 4], &[chunk(b"ALPH", &[0]), unknown.clone()],
        )));
        for kind in [b"VP8 ", b"VP8L"] {
            let invalid = chunk(kind, &[0; 12]);
            assert!(!webp(&riff(&[
                canvas(1, 1, false), lossless(), invalid.clone(), unknown.clone(),
            ])));
            assert!(!webp(&animation(
                Some((1, 1)), [0; 4], &[lossless(), invalid, unknown.clone()],
            )));
        }
    }

    #[test]
    fn webp_unknown_chunks_still_require_complete_sizes_and_padding() {
        let mut missing_pad = chunk(b"UNKN", b"odd");
        missing_pad.pop();
        let mut oversized = chunk(b"UNKN", b"data");
        oversized[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        for malformed in [missing_pad, oversized, b"UNKN\0\0\0".to_vec()] {
            assert!(!webp(&riff(&[
                canvas(1, 1, false), lossless(), malformed.clone(),
            ])));
            assert!(!webp(&animation(
                Some((1, 1)), [0; 4], &[lossless(), malformed],
            )));
        }
        let mut bytes = riff(&[
            canvas(1, 1, false), lossless(), chunk(b"UNKN", b"odd"),
        ]);
        bytes[4] ^= 1;
        assert!(!webp(&bytes), "RIFF size must still match");
    }
}
