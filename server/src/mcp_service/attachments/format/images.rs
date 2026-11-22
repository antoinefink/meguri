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
fn bitstream(kind: &[u8], p: &[u8]) -> Option<(u32, u32)> {
    match kind {
        b"VP8 "
            if p.len() > 10
                && p[0] & 1 == 0
                && p[3..6] == [0x9d, 0x01, 0x2a]
                && le16(&p[6..]) & 0x3fff != 0
                && le16(&p[8..]) & 0x3fff != 0 =>
        {
            Some((
                u32::from(le16(&p[6..]) & 0x3fff),
                u32::from(le16(&p[8..]) & 0x3fff),
            ))
        }
        b"VP8L" if p.len() > 5 && p[0] == 0x2f && p[4] & 0xe0 == 0 => {
            let dimensions = le32(&p[1..]);
            Some(((dimensions & 0x3fff) + 1, ((dimensions >> 14) & 0x3fff) + 1))
        }
        _ => None,
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
        if n & 1 != 0 && b.get(end) != Some(&0) {
            return false;
        }
        match &h[..4] {
            b"VP8 " | b"VP8L" => {
                let Some(dimensions) = bitstream(&h[..4], p) else {
                    return false;
                };
                if image || canvas.is_some_and(|size| size != dimensions) {
                    return false;
                }
                image = true;
            }
            b"VP8X" => {
                if at != 12 || n != 10 || p[0] & 0xc1 != 0 || p[1..4] != [0, 0, 0] {
                    return false;
                }
                let dimensions = (le24(&p[4..]) + 1, le24(&p[7..]) + 1);
                if dimensions.0.checked_mul(dimensions.1).is_none() {
                    return false;
                }
                canvas = Some(dimensions);
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
                let frame_size = (le24(&p[6..]) + 1, le24(&p[9..]) + 1);
                if le24(p) * 2 + frame_size.0 > width
                    || le24(&p[3..]) * 2 + frame_size.1 > height
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
                    let Some(end) = (sub + 8).checked_add(size) else {
                        return false;
                    };
                    let Some(data) = p.get(sub + 8..end) else {
                        return false;
                    };
                    if size & 1 != 0 && p.get(end) != Some(&0) {
                        return false;
                    }
                    if matches!(&sh[..4], b"VP8 " | b"VP8L") {
                        if frame || bitstream(&sh[..4], data) != Some(frame_size) {
                            return false;
                        }
                        frame = true;
                    }
                    sub = end + (size & 1);
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
        lossless_size(1, 1)
    }

    fn lossless_size(width: u32, height: u32) -> Vec<u8> {
        let mut data = vec![0x2f];
        data.extend_from_slice(&((width - 1) | ((height - 1) << 14)).to_le_bytes());
        data.extend_from_slice(&[0x88, 0x88, 8]);
        chunk(b"VP8L", &data)
    }

    fn lossy(width: u16, height: u16) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 0x9d, 1, 0x2a];
        data.extend_from_slice(&width.to_le_bytes());
        data.extend_from_slice(&height.to_le_bytes());
        data.push(0);
        chunk(b"VP8 ", &data)
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
            for image in [
                lossless_size(rect[2] + 1, rect[3] + 1),
                lossy((rect[2] + 1) as u16, (rect[3] + 1) as u16),
            ] {
                assert!(webp(&animation(Some((5, 4)), rect, &[image])), "{rect:?}");
            }
        }
        for rect in [
            [0, 0, 5, 3], // Width exceeds canvas by one.
            [0, 0, 4, 4], // Height exceeds canvas by one.
            [1, 1, 3, 1], // Doubled x plus decoded width exceeds canvas.
            [1, 1, 2, 2], // Doubled y plus decoded height exceeds canvas.
            [3, 0, 0, 0], // Origin outside canvas, even for a one-pixel frame.
            [0, 2, 0, 0],
        ] {
            let image = lossless_size(rect[2] + 1, rect[3] + 1);
            assert!(!webp(&animation(Some((5, 4)), rect, &[image])), "{rect:?}");
        }
        assert!(webp(&animation(Some((1, 1)), [0; 4], &[lossless()])));
        assert!(!webp(&animation(None, [0; 4], &[lossless()])));
    }

    #[test]
    fn webp_frame_rectangles_use_all_24_bits_without_overflow() {
        for (canvas, edge) in [
            ((0x1000000, 1), [0x7fffff, 0, 1, 0]),
            ((1, 0x1000000), [0, 0x7fffff, 0, 1]),
        ] {
            let image = lossless_size(edge[2] + 1, edge[3] + 1);
            assert!(webp(&animation(Some(canvas), edge, &[image])));
            for rect in [
                [0x800000, 0, 0, 0],
                [0, 0x800000, 0, 0],
                [0xffffff, 0xffffff, 0xffffff, 0xffffff],
                [0, 0, 0xffffff, 0xffffff],
            ] {
                assert!(!webp(&animation(Some(canvas), rect, &[lossless()])), "{rect:?}");
            }
        }
    }

    #[test]
    fn webp_canvas_area_must_fit_u32() {
        for size in [(65536, 65536), (0x1000000, 256), (0x1000000, 0x1000000)] {
            assert!(!webp(&animation(Some(size), [0; 4], &[lossless()])));
            assert!(!webp(&riff(&[canvas(size.0, size.1, false), lossless()])));
        }
        assert!(webp(&animation(Some((65535, 65537)), [0; 4], &[lossless()])));
    }

    #[test]
    fn webp_bitstream_dimensions_must_match_frame_or_still_canvas() {
        for image in [lossless_size(3, 2), lossy(3, 2)] {
            assert!(webp(&riff(&[image.clone()])));
            assert!(webp(&riff(&[canvas(3, 2, false), image.clone()])));
            assert!(webp(&animation(Some((5, 4)), [1, 1, 2, 1], &[image.clone()])));
            for (width, height) in [(2, 2), (3, 1), (4, 2), (3, 3)] {
                assert!(!webp(&riff(&[canvas(width, height, false), image.clone()])));
                assert!(!webp(&animation(
                    Some((5, 4)), [0, 0, width - 1, height - 1], &[image.clone()],
                )));
            }
        }
        for image in [lossless_size(16384, 16384), lossy(16383, 16383)] {
            assert!(webp(&riff(&[image])));
        }
    }

    #[test]
    fn webp_rejects_duplicate_image_chunks() {
        for first in [lossless(), lossy(1, 1)] {
            for second in [lossless(), lossy(1, 1)] {
                let chunks = [first.clone(), chunk(b"UNKN", b"odd"), second];
                assert!(!webp(&riff(&chunks)));
                let mut extended = vec![canvas(1, 1, false)];
                extended.extend_from_slice(&chunks);
                assert!(!webp(&riff(&extended)));
                assert!(!webp(&animation(Some((1, 1)), [0; 4], &chunks)));
            }
        }
    }

    #[test]
    fn webp_extended_stills_and_frames_accept_unknown_trailing_chunks() {
        for image in [lossless(), lossy(1, 1)] {
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
    fn webp_odd_chunks_require_zero_padding() {
        for pad in [0, 1, 0xff] {
            let mut unknown = chunk(b"UNKN", b"odd");
            *unknown.last_mut().unwrap() = pad;
            assert_eq!(
                webp(&riff(&[canvas(1, 1, false), lossless(), unknown.clone()])),
                pad == 0,
            );
            assert_eq!(
                webp(&animation(Some((1, 1)), [0; 4], &[lossless(), unknown.clone()])),
                pad == 0,
            );
            assert_eq!(
                webp(&animation(Some((1, 1)), [0; 4], &[unknown, lossless()])),
                pad == 0,
            );

            let mut image = lossy(1, 1);
            *image.last_mut().unwrap() = pad;
            assert_eq!(webp(&riff(&[image.clone()])), pad == 0);
            assert_eq!(
                webp(&animation(Some((1, 1)), [0; 4], &[image])),
                pad == 0,
            );
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
