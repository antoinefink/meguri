// SPDX-License-Identifier: MIT
//! Bounded structural recognition; media samples are never decoded or rewritten.
//! Every walker is capped by both the 8 MiB caller limit and a record budget.
mod audio;
mod images;
mod isobmff;
mod pdf;
#[cfg(test)]
mod tests;
const RECORDS: usize = 65536;
fn be16(b: &[u8]) -> u16 { u16::from_be_bytes(b[..2].try_into().unwrap()) }
fn be32(b: &[u8]) -> u32 { u32::from_be_bytes(b[..4].try_into().unwrap()) }
fn le32(b: &[u8]) -> u32 { u32::from_le_bytes(b[..4].try_into().unwrap()) }
pub(super) fn detect(bytes: &[u8]) -> Option<&'static str> {
    if png(bytes) { Some("png") } else if jpeg(bytes) { Some("jpg") } else if mp3(bytes) { Some("mp3") } else if images::gif(bytes) { Some("gif") }
    else if images::webp(bytes) { Some("webp") }
    else if let Some(ext) = isobmff::detect(bytes) { Some(ext) }
    else if let Some(ext) = audio::detect(bytes) { Some(ext) }
    else if pdf::valid(bytes) { Some("pdf") }
    else { None }
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
        if at >= b.len() || b[at] != 0xff { return false; }
        let fill_start = at;
        while b.get(at) == Some(&0xff) {
            at += 1;
            if at - fill_start > 65536 { return false; }
        }
        let Some(&marker) = b.get(at) else { return false; }; at += 1;
        if matches!(marker,0x00|0xd8|0xd9|0xd0..=0xd7) { return false; }
        let Some(size) = b.get(at..at+2) else { return false; };
        let length = u16::from_be_bytes(size.try_into().unwrap()) as usize;
        if length < 2 || at + length > b.len() { return false; }
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
        let Some(rest) = b.get(offset..) else { return false; }; b = rest;
    }
    let Some(first) = mp3_frame(b) else { return false; };
    mp3_frame(&b[first..]).is_some()
}
#[cfg(test)]
mod legacy_tests {
    use super::*;
    #[test]
    fn signatures_require_headers_and_complete_frames() {
        for b in [b"".as_slice(), b"<svg>generated</svg>", b"\x89PNG\r\n\x1a\n", b"ID3", &[0xff,0xd8,0xff,0xd9]] { assert_eq!(detect(b),None); }
        let mut audio = vec![0;834]; audio[..4].copy_from_slice(&[0xff,0xfb,0x90,0]); audio[417..421].copy_from_slice(&[0xff,0xfb,0x90,0]);
        assert_eq!(detect(&audio),Some("mp3"));
        assert_eq!(detect(&audio[..833]),None);
    }
    #[test]
    fn jpeg_accepts_large_app_segments_and_bounds_each_fill_run() {
        let mut image = vec![0xff,0xd8];
        for marker in [0xe0,0xe1] {
            image.extend_from_slice(&[0xff,marker]);
            image.extend_from_slice(&40002u16.to_be_bytes());
            image.resize(image.len() + 40000,0);
        }
        let frame_start = image.len();
        assert!(frame_start > 65536);
        image.extend_from_slice(&[0xff,0xc0,0,11,8,0,1,0,1,1,1,0x11,0]);
        image.extend_from_slice(&[0xff,0xda,0,8,1,1,0,0,63,0,1,0xff,0xd9]);
        assert_eq!(detect(&image),Some("jpg"));

        // The existing marker prefix is the last byte of this 64 KiB run.
        image.splice(frame_start..frame_start,vec![0xff;65535]);
        assert_eq!(detect(&image),Some("jpg"));
        image.insert(frame_start,0xff);
        assert_eq!(detect(&image),None);
    }
    #[test]
    fn jpeg_rejects_segment_ending_past_file_bounds() {
        let image = [0xff,0xd8,0xff,0xe0,0xff,0xff,0xff,0xd9];
        assert_eq!(detect(&image),None);
    }
    #[test]
    fn mp3_accepts_large_id3_metadata_and_requires_complete_input() {
        let metadata_len = 70 * 1024;
        let mut audio = b"ID3\x03\0\0".to_vec();
        for shift in [21,14,7,0] {
            audio.push(((metadata_len >> shift) & 0x7f) as u8);
        }
        audio.resize(10 + metadata_len,0);
        assert!(metadata_len > 65536);
        for _ in 0..2 {
            audio.extend_from_slice(&[0xff,0xfb,0x90,0]);
            audio.resize(audio.len() + 413,0);
        }
        assert_eq!(detect(&audio),Some("mp3"));
        assert_eq!(detect(&audio[..audio.len()-1]),None);
        assert_eq!(detect(&audio[..10+metadata_len-1]),None);
    }
}
