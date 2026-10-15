//! Media names and files (`specs/fixtures/anonymizer.md` A9.1, A10): the
//! name map, the [`Names`] the walker resolves references with, and the
//! placeholders written into the clone's media folder.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use meguri_text::{entities, fold, media_refs, normalize};

use super::keys::{MEDIA_DOMAIN, Salt};
use super::text::{Names, Reference};
use super::{AnonymizeError, MediaCounts};

/// A9.1: characters a well-formed name never holds (`text/media-filenames`
/// F1 step 2's list, control characters included).
fn is_forbidden(c: char) -> bool {
    matches!(
        c,
        '[' | ']' | '<' | '>' | ':' | '"' | '/' | '?' | '*' | '^' | '\\' | '|'
    ) || normalize::is_ascii_control(c)
}

/// A9.1: whether a decoded name is a well-formed local name.
pub fn is_well_formed(decoded: &str) -> bool {
    !decoded.is_empty() && !media_refs::is_remote(decoded) && !decoded.chars().any(is_forbidden)
}

/// A9.1: the key of a name: the fold of its NFC form.
pub fn key_of(name: &str) -> String {
    fold::folded(&normalize::nfc(name))
}

/// A9.1: the extension of a key: the text after its last `.` when it is 1 to
/// 10 ASCII letters or digits, in lower case.
pub fn extension(key: &str) -> Option<String> {
    let (_, ext) = key.rsplit_once('.')?;
    ((1..=10).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| ext.to_ascii_lowercase())
}

/// The extensions of image-occlusion images (A10.2).
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "ico", "avif", "svg"];

/// One regular file of the source media folder.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// The name as the folder holds it.
    pub name: String,
    pub clone: String,
    pub size: u64,
}

/// The name map (A9.1) and what the walker resolves through it.
#[derive(Debug)]
pub struct NameMap<'s> {
    salt: &'s Salt,
    /// The source folder's files by key.
    pub files: HashMap<String, SourceFile>,
    /// Clone names given to keys (not `_` names): key by clone name, to find
    /// hash collisions.
    hashed: HashMap<String, String>,
    /// Every name the map has seen (files and well-formed references), for
    /// V4's tokens.
    pub domain: BTreeSet<String>,
    /// Keys referenced since the last [`NameMap::take_referenced`].
    referenced: Vec<String>,
    pub counts: MediaCounts,
    /// The first error met while resolving (A18 `hash_collision`).
    pub error: Option<AnonymizeError>,
}

impl<'s> NameMap<'s> {
    /// Reads the source media folder's names (A9.1): every regular file
    /// directly inside it; other entries are skipped and counted.
    pub fn new(salt: &'s Salt, media_dir: &Path) -> Result<NameMap<'s>, AnonymizeError> {
        let mut map = NameMap {
            salt,
            files: HashMap::new(),
            hashed: HashMap::new(),
            domain: BTreeSet::new(),
            referenced: Vec::new(),
            counts: MediaCounts::default(),
            error: None,
        };
        let entries = match fs::read_dir(media_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(map),
            Err(e) => return Err(AnonymizeError::io("reading the media folder", &e)),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| AnonymizeError::io("reading the media folder", &e))?;
            let kind = entry
                .file_type()
                .map_err(|e| AnonymizeError::io("reading the media folder", &e))?;
            match entry.file_name().into_string() {
                Ok(name) if kind.is_file() => {
                    let size = entry
                        .metadata()
                        .map_err(|e| AnonymizeError::io("reading the media folder", &e))?
                        .len();
                    names.push((name, size));
                }
                _ => map.counts.skipped_entries += 1,
            }
        }
        // Sorted, so a collision is found the same way on every run.
        names.sort();
        for (name, size) in names {
            let key = key_of(&name);
            if map.files.contains_key(&key) {
                return Err(AnonymizeError::MediaNameCollision);
            }
            let clone = if name.starts_with('_') {
                name.clone()
            } else {
                map.hashed_name(&key)?
            };
            map.domain.insert(name.clone());
            map.files.insert(key, SourceFile { name, clone, size });
        }
        map.counts.files = map.files.len() as u64;
        Ok(map)
    }

    /// A9.1: `H(media, key)` and the key's extension, checked against the
    /// clone names already given.
    fn hashed_name(&mut self, key: &str) -> Result<String, AnonymizeError> {
        let mut clone = self.salt.hash(MEDIA_DOMAIN, key);
        if let Some(ext) = extension(key) {
            clone.push('.');
            clone.push_str(&ext);
        }
        match self.hashed.get(&clone) {
            Some(other) if other != key => Err(AnonymizeError::HashCollision),
            Some(_) => Ok(clone),
            None => {
                self.hashed.insert(clone.clone(), key.to_owned());
                Ok(clone)
            }
        }
    }

    /// The clone name of a key, without recording it (V1).
    pub fn clone_name(&self, key: &str) -> Option<String> {
        if let Some(file) = self.files.get(key) {
            return Some(file.clone.clone());
        }
        let mut clone = self.salt.hash(MEDIA_DOMAIN, key);
        if let Some(ext) = extension(key) {
            clone.push('.');
            clone.push_str(&ext);
        }
        Some(clone)
    }

    /// The clone name of a key that is no file of the folder.
    fn clone_of(&mut self, key: &str) -> Option<String> {
        if let Some(file) = self.files.get(key) {
            return Some(file.clone.clone());
        }
        match self.hashed_name(key) {
            Ok(clone) => Some(clone),
            Err(e) => {
                self.error.get_or_insert(e);
                None
            }
        }
    }

    /// The keys referenced since the last call.
    pub fn take_referenced(&mut self) -> Vec<String> {
        std::mem::take(&mut self.referenced)
    }

    /// The clone names of every source file, by clone name.
    pub fn clone_index(&self) -> HashMap<String, String> {
        self.files
            .values()
            .map(|file| (key_of(&file.clone), file.clone.clone()))
            .collect()
    }
}

impl Names for NameMap<'_> {
    fn reference(&mut self, raw: &str) -> Reference {
        if media_refs::is_remote(raw) {
            self.counts.refs_remote += 1;
            return Reference::Remote;
        }
        let decoded = entities::decode_text(raw);
        if !is_well_formed(&decoded) {
            self.counts.refs_malformed += 1;
            return Reference::Malformed;
        }
        let key = key_of(&decoded);
        self.counts.refs += 1;
        if !self.files.contains_key(&key) {
            self.counts.refs_missing += 1;
        }
        self.domain.insert(decoded.clone().into_owned());
        self.referenced.push(key.clone());
        if decoded.starts_with('_') {
            // Maps to itself, raw form for raw form.
            return Reference::Local(raw.to_owned());
        }
        match self.clone_of(&key) {
            Some(clone) => Reference::Local(clone),
            None => Reference::Local(raw.to_owned()),
        }
    }

    fn file(&mut self, candidate: &str, decode: bool, need_extension: bool) -> Option<String> {
        if need_extension && !candidate.contains('.') {
            return None;
        }
        let decoded = if decode {
            entities::decode_text(candidate)
        } else {
            candidate.into()
        };
        let key = key_of(&decoded);
        if need_extension && extension(&key).is_none() {
            return None;
        }
        let clone = self.files.get(&key)?.clone.clone();
        if need_extension {
            self.counts.bare_names += 1;
        }
        self.referenced.push(key);
        Some(clone)
    }
}

/// V1's names for the clone field: located names stay as written, and a
/// file is a file of the clone's media folder.
pub struct CloneNames<'a> {
    /// Key of each clone file name → that name.
    pub by_key: &'a HashMap<String, String>,
}

impl Names for CloneNames<'_> {
    fn reference(&mut self, raw: &str) -> Reference {
        if media_refs::is_remote(raw) {
            return Reference::Remote;
        }
        if is_well_formed(&entities::decode_text(raw)) {
            Reference::Local(raw.to_owned())
        } else {
            Reference::Malformed
        }
    }

    fn file(&mut self, candidate: &str, decode: bool, need_extension: bool) -> Option<String> {
        if need_extension && !candidate.contains('.') {
            return None;
        }
        let decoded = if decode {
            entities::decode_text(candidate)
        } else {
            candidate.into()
        };
        let key = key_of(&decoded);
        if need_extension && extension(&key).is_none() {
            return None;
        }
        self.by_key.get(&key).cloned()
    }
}

/// Whether a key names an image an image-occlusion note may show (A10.2).
pub fn is_image_key(key: &str) -> bool {
    extension(key).is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.as_str()))
}

// ---------------------------------------------------------------- A10.2

/// The pixel size of an image, read from its first bytes (A10.2); `None`
/// when the format is not one of the table's or a dimension is 0.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |i: usize| -> Option<u32> { Some(u32::from(u16::from_be_bytes(bytes.get(i..i + 2)?.try_into().ok()?))) };
    let le16 = |i: usize| -> Option<u32> { Some(u32::from(u16::from_le_bytes(bytes.get(i..i + 2)?.try_into().ok()?))) };
    let le24 = |i: usize| -> Option<u32> {
        let b = bytes.get(i..i + 3)?;
        Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
    };
    let size = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
        (w, h)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        (le16(6)?, le16(8)?)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut at = 2;
        loop {
            if *bytes.get(at)? != 0xFF {
                return None;
            }
            let marker = *bytes.get(at + 1)?;
            let length = be16(at + 2)? as usize;
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                // Length, one precision byte, then height and width.
                break (be16(at + 7)?, be16(at + 5)?);
            }
            at += 2 + length;
        }
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let chunk = bytes.get(12..16)?;
        let data = 20;
        match chunk {
            b"VP8X" => (le24(data + 4)? + 1, le24(data + 7)? + 1),
            b"VP8L" => {
                if *bytes.get(data)? != 0x2F {
                    return None;
                }
                let bits = u32::from_le_bytes(bytes.get(data + 1..data + 5)?.try_into().ok()?);
                ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
            }
            b"VP8 " => {
                if bytes.get(data + 3..data + 6)? != [0x9D, 0x01, 0x2A] {
                    return None;
                }
                (le16(data + 6)? & 0x3FFF, le16(data + 8)? & 0x3FFF)
            }
            _ => return None,
        }
    } else {
        return None;
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

// ---------------------------------------------------------------- A10.3

/// CRC-32 (ISO 3309, as PNG chunks use it).
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for part in parts {
        for &byte in *part {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
    }
    !crc
}

/// Bits filled from each byte's least significant bit (RFC 1951 §3.1.1).
struct Bits {
    out: Vec<u8>,
    used: u32,
}

impl Bits {
    fn push_bit(&mut self, bit: u32) {
        if self.used == 0 {
            self.out.push(0);
        }
        if bit != 0 {
            *self.out.last_mut().expect("a byte is open") |= 1 << self.used;
        }
        self.used = (self.used + 1) % 8;
    }

    /// A Huffman code: most significant bit first.
    fn code(&mut self, value: u32, len: u32) {
        for i in (0..len).rev() {
            self.push_bit((value >> i) & 1);
        }
    }

    /// A header field or extra bits: least significant bit first.
    fn field(&mut self, value: u32, len: u32) {
        for i in 0..len {
            self.push_bit((value >> i) & 1);
        }
    }
}

/// A fixed-Huffman match length (RFC 1951 §3.2.5, §3.2.6): code, code
/// length, extra bits, extra bit count.
fn length_code(length: u32) -> (u32, u32, u32, u32) {
    if length == 258 {
        return (0b1100_0101, 8, 0, 0);
    }
    const BASES: [(u32, u32); 28] = [
        (3, 0),
        (4, 0),
        (5, 0),
        (6, 0),
        (7, 0),
        (8, 0),
        (9, 0),
        (10, 0),
        (11, 1),
        (13, 1),
        (15, 1),
        (17, 1),
        (19, 2),
        (23, 2),
        (27, 2),
        (31, 2),
        (35, 3),
        (43, 3),
        (51, 3),
        (59, 3),
        (67, 4),
        (83, 4),
        (99, 4),
        (115, 4),
        (131, 5),
        (163, 5),
        (195, 5),
        (227, 5),
    ];
    let index = BASES
        .iter()
        .rposition(|&(base, _)| base <= length)
        .expect("lengths start at 3");
    let (base, extra) = BASES[index];
    let symbol = 257 + index as u32;
    let (code, len) = if symbol <= 279 {
        (symbol - 256, 7)
    } else {
        (0b1100_0000 + (symbol - 280), 8)
    };
    (code, len, length - base, extra)
}

/// A10.3: the black PNG of `width` × `height` (bit depth 1, gray), its
/// image data one fixed-Huffman deflate block of zeros.
pub fn black_png(width: u32, height: u32) -> Vec<u8> {
    let row = 1 + u64::from(width).div_ceil(8);
    let raw_len = u64::from(height) * row;
    let mut bits = Bits {
        out: Vec::new(),
        used: 0,
    };
    bits.field(1, 1); // BFINAL
    bits.field(1, 2); // BTYPE 01, fixed codes
    const LITERAL_ZERO: (u32, u32) = (0b0011_0000, 8);
    bits.code(LITERAL_ZERO.0, LITERAL_ZERO.1);
    let mut remaining = raw_len - 1;
    while remaining >= 258 {
        bits.code(0b1100_0101, 8);
        bits.code(0, 5); // distance 1
        remaining -= 258;
    }
    if remaining >= 3 {
        let (code, len, extra, extra_len) = length_code(remaining as u32);
        bits.code(code, len);
        bits.field(extra, extra_len);
        bits.code(0, 5);
    } else {
        for _ in 0..remaining {
            bits.code(LITERAL_ZERO.0, LITERAL_ZERO.1);
        }
    }
    bits.code(0, 7); // end of block
    let adler = ((raw_len % 65521) as u32) << 16 | 1;
    let mut idat = vec![0x78, 0x01];
    idat.extend_from_slice(&bits.out);
    idat.extend_from_slice(&adler.to_be_bytes());

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[1, 0, 0, 0, 0]);

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [
        (b"IHDR", ihdr.as_slice()),
        (b"IDAT", idat.as_slice()),
        (b"IEND", &[][..]),
    ] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        png.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
    }
    png
}

// ---------------------------------------------------------------- A10.4

/// A10.4: the SVG placeholder for a source SVG's bytes.
pub fn svg_placeholder(source: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(source);
    let attrs = first_svg_attributes(&text);
    let mut out = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    let mut any = false;
    for name in ["width", "height", "viewBox"] {
        if let Some(value) = attrs.get(&name.to_ascii_lowercase())
            && value.bytes().all(|b| b.is_ascii_digit() || b" .,+-eE%px".contains(&b))
        {
            out.push_str(&format!(" {name}=\"{value}\""));
            any = true;
        }
    }
    if !any {
        out.push_str(" width=\"1\" height=\"1\"");
    }
    out.push_str("/>");
    out.into_bytes()
}

/// The attributes of the first `<svg` start tag (A6.1's tokenizer), by
/// lower-case name.
fn first_svg_attributes(text: &str) -> BTreeMap<String, String> {
    struct Collect {
        attrs: Option<BTreeMap<String, String>>,
    }
    let mut collect = Collect { attrs: None };
    super::text::start_tags(text, &mut |name, attrs| {
        if collect.attrs.is_none() && name.eq_ignore_ascii_case("svg") {
            collect.attrs = Some(
                attrs
                    .iter()
                    .rev()
                    .map(|(n, v)| (n.to_ascii_lowercase(), v.to_string()))
                    .collect(),
            );
            return false;
        }
        true
    });
    collect.attrs.unwrap_or_default()
}

// ---------------------------------------------------------------- A10.1

/// What a clone media file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placeholder {
    /// A `_` file: its bytes.
    Copy,
    /// An image-occlusion PNG of this size (`None`: size unknown, 1 × 1).
    Png(Option<(u32, u32)>),
    /// An image-occlusion SVG.
    Svg,
    /// Random bytes from the stream `media:<clone name>`.
    Random,
}

/// One written clone media file.
#[derive(Clone, Debug)]
pub struct Written {
    pub clone: String,
    pub size: u64,
    pub placeholder: Placeholder,
    /// The placeholder is longer than the source file.
    pub larger: bool,
}

const CHUNK: usize = 1 << 20;

/// A10.1: writes every source file into `out_media` under its clone name.
pub fn write_files(
    map: &NameMap<'_>,
    salt: &Salt,
    source_media: &Path,
    out_media: &Path,
    io_keys: &BTreeSet<String>,
) -> Result<Vec<Written>, AnonymizeError> {
    let io_err = |e: io::Error| AnonymizeError::io("writing the clone's media", &e);
    fs::create_dir_all(out_media).map_err(io_err)?;
    let mut files: Vec<(&String, &SourceFile)> = map.files.iter().collect();
    files.sort_by(|a, b| a.1.clone.cmp(&b.1.clone));
    let mut written = Vec::with_capacity(files.len());
    for (key, file) in files {
        let from = source_media.join(&file.name);
        let to = out_media.join(&file.clone);
        let read_err = |e: io::Error| AnonymizeError::io("reading a source media file", &e);
        let (placeholder, bytes_written) = if file.name.starts_with('_') {
            (Placeholder::Copy, fs::copy(&from, &to).map_err(read_err)?)
        } else if io_keys.contains(key) {
            let source = fs::read(&from).map_err(read_err)?;
            let (placeholder, mut body) = if extension(key).as_deref() == Some("svg") {
                (Placeholder::Svg, svg_placeholder(&source))
            } else {
                let size = image_size(&source);
                let (w, h) = size.unwrap_or((1, 1));
                (Placeholder::Png(size), black_png(w, h))
            };
            if body.len() < source.len() {
                let pad = if placeholder == Placeholder::Svg { b' ' } else { 0 };
                body.resize(source.len(), pad);
            }
            fs::write(&to, &body).map_err(io_err)?;
            (placeholder, body.len() as u64)
        } else {
            let mut stream = salt.stream(&format!("media:{}", file.clone));
            let mut input = fs::File::open(&from).map_err(read_err)?;
            let mut output = io::BufWriter::new(fs::File::create(&to).map_err(io_err)?);
            let mut buf = vec![0u8; CHUNK];
            let mut total = 0u64;
            loop {
                // As many random bytes as the source holds: read to count.
                let n = read_full(&mut input, &mut buf).map_err(read_err)?;
                if n == 0 {
                    break;
                }
                stream.fill(&mut buf[..n]);
                output.write_all(&buf[..n]).map_err(io_err)?;
                total += n as u64;
            }
            output.flush().map_err(io_err)?;
            (Placeholder::Random, total)
        };
        written.push(Written {
            clone: file.clone.clone(),
            size: bytes_written,
            larger: bytes_written > file.size,
            placeholder,
        });
    }
    Ok(written)
}

/// Reads until `buf` is full or the input ends.
fn read_full(input: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match input.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha(bytes: &[u8]) -> String {
        super::super::keys::hex(&Sha256::digest(bytes))
    }

    #[test]
    fn png_placeholders_match_the_spec_vectors() {
        for (w, h, len, digest) in [
            (
                1,
                1,
                67,
                "836c5e8c94b74be78456122528bb2a44b4cf61b3922f211e2bae5bf327f95f09",
            ),
            (
                3,
                2,
                67,
                "d3bfdfaae80e2d8dfd859fb9f4d382a6368fde337ddea9dd7d79a6e32a143176",
            ),
            (
                100,
                100,
                76,
                "44e90f2c74614e596431e6781d61332c5753102570825f8649066c6b196e314f",
            ),
            (
                2000,
                1500,
                2439,
                "35cee25202bbfc7757a71169d17778f7abd76d81da56afb050a7c1bbb2c7d9bc",
            ),
            (
                4000,
                3000,
                9534,
                "10cd98720f649dd5d1897222b639f89f1100a77b95618fbf925903777b79d179",
            ),
        ] {
            let png = black_png(w, h);
            assert_eq!(png.len(), len, "{w}x{h}");
            assert_eq!(sha(&png), digest, "{w}x{h}");
        }
    }

    #[test]
    fn sizes_are_read_from_each_format() {
        assert_eq!(image_size(&black_png(3, 2)), Some((3, 2)));
        let gif = b"GIF89a\x05\x00\x07\x00rest";
        assert_eq!(image_size(gif), Some((5, 7)));
        // JPEG: SOI, an APP0 segment of length 4, then SOF0 (length 11,
        // precision 8, height 2, width 3).
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0xAA, 0xBB, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x02, 0x00, 0x03, 0x01,
        ];
        assert_eq!(image_size(&jpeg), Some((3, 2)));
        // A DHT (C4) before the SOF is skipped.
        let jpeg_dht = [
            0xFF, 0xD8, 0xFF, 0xC4, 0x00, 0x02, 0xFF, 0xC2, 0x00, 0x0B, 0x08, 0x00, 0x09, 0x00, 0x04,
        ];
        assert_eq!(image_size(&jpeg_dht), Some((4, 9)));
        let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        vp8x.extend_from_slice(&[0, 0, 0, 0, 99, 0, 0, 49, 0, 0]);
        assert_eq!(image_size(&vp8x), Some((100, 50)));
        let mut vp8l = b"RIFF\0\0\0\0WEBPVP8L\x05\0\0\0\x2f".to_vec();
        let bits: u32 = 9 | (19 << 14);
        vp8l.extend_from_slice(&bits.to_le_bytes());
        assert_eq!(image_size(&vp8l), Some((10, 20)));
        let mut vp8 = b"RIFF\0\0\0\0WEBPVP8 \x0a\0\0\0".to_vec();
        vp8.extend_from_slice(&[0, 0, 0, 0x9D, 0x01, 0x2A, 30, 0xC0, 40, 0]);
        assert_eq!(image_size(&vp8), Some((30, 40)));
        assert_eq!(image_size(b"not an image"), None);
        assert_eq!(image_size(b"GIF89a\x00\x00\x07\x00"), None);
    }

    #[test]
    fn svg_placeholders_keep_numeric_sizes() {
        let source = br#"<?xml version="1.0"?><!-- x --><svg xmlns="http://www.w3.org/2000/svg" width="300px" height="200" viewBox="0 0 300 200"><text>Leber</text></svg>"#;
        assert_eq!(
            svg_placeholder(source),
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="300px" height="200" viewBox="0 0 300 200"/>"#.to_vec()
        );
        assert_eq!(
            svg_placeholder(br#"<svg width="calc(1)">"#),
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#.to_vec()
        );
    }

    #[test]
    fn extensions_are_short_ascii_suffixes() {
        assert_eq!(extension("notes.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension("readme"), None);
        assert_eq!(extension("a.verylongextension"), None);
        assert_eq!(extension("x.png").as_deref(), Some("png"));
        assert_eq!(extension("x."), None);
    }
}
