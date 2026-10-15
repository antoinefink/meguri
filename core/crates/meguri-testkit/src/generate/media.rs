//! Media files: names the notes reference, deterministic bytes of sampled
//! sizes. Images are valid PNGs (RGB, 8 bits, stored deflate blocks) padded
//! to their size with a private ancillary chunk; other files are bytes from
//! the file's own random stream.

use std::io::Write;
use std::path::Path;

use super::GenError;
use super::rng::{Rng, Stream};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFile {
    pub name: String,
    pub bytes: u64,
    pub kind: MediaKind,
    /// The run seed and the file's stream index: its bytes.
    pub seed: u64,
    pub index: u64,
}

impl MediaFile {
    /// The reference a field holds.
    pub fn reference(&self) -> String {
        match self.kind {
            MediaKind::Image => format!("<img src=\"{}\">", self.name),
            MediaKind::Audio => format!("[sound:{}]", self.name),
        }
    }

    pub fn contents(&self) -> Vec<u8> {
        let mut rng = Rng::stream(self.seed, Stream::Media, self.index);
        match self.kind {
            MediaKind::Image => png_of_size(self.bytes as usize, &mut rng),
            MediaKind::Audio => {
                let mut bytes = vec![0; self.bytes as usize];
                rng.fill(&mut bytes);
                bytes
            }
        }
    }
}

pub fn write_file(dir: &Path, file: &MediaFile) -> Result<(), GenError> {
    let path = dir.join(&file.name);
    let mut out = std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e| GenError::io(&path, e))?);
    out.write_all(&file.contents()).map_err(|e| GenError::io(&path, e))?;
    out.flush().map_err(|e| GenError::io(&path, e))
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
/// Private, ancillary, safe-to-copy chunk type that pads a PNG to its size.
const PAD_CHUNK: &[u8; 4] = b"mgPd";
const WIDTH: usize = 32;

/// CRC-32 (ISO 3309, as PNG uses it).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A zlib stream of stored (uncompressed) deflate blocks.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if raw.is_empty() {
        vec![&[][..]]
    } else {
        raw.chunks(65_535).collect()
    };
    let last = blocks.len() - 1;
    for (i, block) in blocks.iter().enumerate() {
        out.push(u8::from(i == last));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// A valid PNG of `width` × `height` RGB pixels.
pub fn png(width: usize, height: usize, rng: &mut Rng) -> Vec<u8> {
    let tint = rng.u64().to_le_bytes();
    let mut raw = Vec::with_capacity(height * (1 + 3 * width));
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            raw.push((x * 8) as u8 ^ tint[0]);
            raw.push((y * 8) as u8 ^ tint[1]);
            raw.push(((x + y) * 4) as u8 ^ tint[2]);
        }
    }
    let mut out = PNG_SIGNATURE.to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A valid PNG of exactly `size` bytes when `size` is at least the
/// smallest one (a 1 × 1 image); else that smallest PNG.
pub fn png_of_size(size: usize, rng: &mut Rng) -> Vec<u8> {
    let row = 1 + 3 * WIDTH;
    let smallest = png(1, 1, rng).len();
    // One spare byte per row covers the 5-byte header of each stored block.
    let height = (size.saturating_sub(smallest + 12 + 64) / (row + 1)).clamp(1, 1 << 15);
    let mut image = if size < smallest + row {
        png(1, 1, rng)
    } else {
        png(WIDTH, height, rng)
    };
    if image.len() + 12 <= size {
        // Insert the padding chunk before IEND (its last 12 bytes).
        let iend = image.split_off(image.len() - 12);
        let mut pad = vec![0u8; size - image.len() - 12 - 12];
        rng.fill(&mut pad);
        chunk(&mut image, PAD_CHUNK, &pad);
        image.extend_from_slice(&iend);
    }
    image
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let n = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= group.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
