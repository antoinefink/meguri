// SPDX-License-Identifier: MIT
//! Bounded signature/header recognition, never image/audio decoding.
pub(super) fn detect(bytes: &[u8]) -> Option<&'static str> {
    if png(bytes) { Some("png") } else if jpeg(bytes) { Some("jpg") } else if mp3(bytes) { Some("mp3") } else { None }
}
fn png(b: &[u8]) -> bool {
    if b.len() < 57 || !b.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR") || !b.ends_with(b"\0\0\0\0IEND\xaeB`\x82") { return false; }
    let width = u32::from_be_bytes(b[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(b[20..24].try_into().unwrap());
    let depth = b[24];
    width > 0 && height > 0 && width <= 1_000_000 && height <= 1_000_000
        && match b[25] { 0 => matches!(depth,1|2|4|8|16), 2|4|6 => matches!(depth,8|16), 3 => matches!(depth,1|2|4|8), _ => false }
        && b[26] == 0 && b[27] == 0 && b[28] <= 1
}
fn jpeg(b: &[u8]) -> bool {
    if !b.starts_with(&[0xff,0xd8]) || !b.ends_with(&[0xff,0xd9]) { return false; }
    let mut at = 2; let mut frame = false;
    for _ in 0..256 {
        if at >= b.len().min(65536) || b[at] != 0xff { return false; }
        while b.get(at) == Some(&0xff) { at += 1; if at >= 65536 { return false; } }
        let Some(&marker) = b.get(at) else { return false; }; at += 1;
        if matches!(marker,0x00|0xd8|0xd9|0xd0..=0xd7) { return false; }
        let Some(size) = b.get(at..at+2) else { return false; };
        let length = u16::from_be_bytes(size.try_into().unwrap()) as usize;
        if length < 2 || at + length > b.len().min(65536) { return false; }
        if marker == 0xda { return frame && length >= 6 && at + length + 2 < b.len(); }
        if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
            if length < 8 || b[at+3..at+5] == [0,0] || b[at+5..at+7] == [0,0] { return false; }
            frame = true;
        }
        at += length;
    }
    false
}
fn mp3_frame(b: &[u8]) -> Option<usize> {
    let h = b.get(..4)?;
    if h[0] != 0xff || h[1] & 0xe0 != 0xe0 || h[1] & 6 != 2 || h[1] & 0x18 == 8 || h[3] & 3 == 2 { return None; }
    let version = (h[1] >> 3) & 3;
    let bitrate = (h[2] >> 4) as usize;
    let rate = ((h[2] >> 2) & 3) as usize;
    if bitrate == 0 || bitrate == 15 || rate == 3 { return None; }
    let rates = [44100,48000,32000];
    let rate = rates[rate] / match version { 3 => 1, 2 => 2, 0 => 4, _ => return None };
    let kbps = if version == 3 { [0,32,40,48,56,64,80,96,112,128,160,192,224,256,320][bitrate] }
        else { [0,8,16,24,32,40,48,56,64,80,96,112,128,144,160][bitrate] };
    let length = (if version == 3 {144} else {72}) * kbps * 1000 / rate + usize::from((h[2] >> 1) & 1);
    (length >= 4 && length <= b.len()).then_some(length)
}
fn mp3(mut b: &[u8]) -> bool {
    if b.starts_with(b"ID3") {
        let Some(h) = b.get(..10) else { return false; };
        if !(2..=4).contains(&h[3]) || h[4] == 0xff || h[6..10].iter().any(|v|v & 0x80 != 0) { return false; }
        let size = h[6..10].iter().fold(0usize,|n,v|(n << 7) | usize::from(*v));
        let offset = 10 + size + if h[3] == 4 && h[5] & 0x10 != 0 {10} else {0};
        if offset > 65536 { return false; }
        let Some(rest) = b.get(offset..) else { return false; }; b = rest;
    }
    let Some(first) = mp3_frame(b) else { return false; };
    mp3_frame(&b[first..]).is_some()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signatures_require_headers_and_complete_frames() {
        for b in [b"".as_slice(), b"<svg>generated</svg>", b"\x89PNG\r\n\x1a\n", b"ID3", &[0xff,0xd8,0xff,0xd9]] { assert_eq!(detect(b),None); }
        let mut audio = vec![0;834]; audio[..4].copy_from_slice(&[0xff,0xfb,0x90,0]); audio[417..421].copy_from_slice(&[0xff,0xfb,0x90,0]);
        assert_eq!(detect(&audio),Some("mp3"));
        assert_eq!(detect(&audio[..833]),None);
    }
}
