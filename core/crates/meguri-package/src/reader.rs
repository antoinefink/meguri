//! Bounded read-back verification for P11 publication. No entry name becomes
//! a filesystem path. The collection is hashed in fixed-size decoded chunks.
use crate::{Error, Result, cancelled, corrupt};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const NAMES: [&str; 4] = ["meta", "collection.anki21b", "collection.anki2", "media"];
const META_LIMIT: u64 = 64 * 1024;
const COLLECTION_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 16 * 1024 * 1024 * 1024;
const RATIO_FLOOR: u64 = 16 * 1024 * 1024;

pub(crate) struct SnapshotDigest {
    pub length: u64,
    pub sha256: [u8; 32],
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed ZIP field"))
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("fixed ZIP64 field"))
}
fn read_at(file: &mut File, offset: u64, bytes: &mut [u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(bytes).map_err(decode_error)
}

/// Inspect the on-disk directory before ZipArchive can allocate its index.
/// Also count actual records: trusting the EOCD count alone is insufficient.
/// Require the four archive members emitted by the writer; a general
/// inspect reader will permit other names under P8's 400,000-record bound.
fn bounded_directory(file: &mut File, check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
    cancelled(check)?;
    let length = file.metadata()?.len();
    let tail_length = length.min(65_535 + 22 + 20 + 56) as usize;
    let tail_start = length - tail_length as u64;
    let mut tail = vec![0; tail_length];
    read_at(file, tail_start, &mut tail)?;
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..].starts_with(b"PK\x05\x06") && i + 22 + usize::from(u16_at(&tail, i + 20)) == tail.len())
        .ok_or_else(|| corrupt("ZIP end record"))?;
    if u16_at(&tail, end + 4) != 0 || u16_at(&tail, end + 6) != 0 {
        return Err(corrupt("multi-disk ZIP"));
    }
    let mut count = u64::from(u16_at(&tail, end + 10));
    let mut size = u64::from(u32_at(&tail, end + 12));
    let mut central_end = tail_start + end as u64;
    let mut central_offset = u64::from(u32_at(&tail, end + 16));
    let mut zip64_record_offset = None;
    if count == 0xffff || size == 0xffff_ffff || u32_at(&tail, end + 16) == 0xffff_ffff {
        if end < 20
            || &tail[end - 20..end - 16] != b"PK\x06\x07"
            || u32_at(&tail, end - 16) != 0
            || u32_at(&tail, end - 4) != 1
        {
            return Err(corrupt("ZIP64 locator"));
        }
        // Locate the physical record, allowing a prefix before the ZIP data.
        let record = (0..end.saturating_sub(20))
            .rev()
            .find(|&i| {
                i + 56 <= end - 20
                    && tail[i..].starts_with(b"PK\x06\x06")
                    && u64_at(&tail, i + 4).checked_add(i as u64 + 12) == Some((end - 20) as u64)
            })
            .ok_or_else(|| corrupt("ZIP64 end record"))?;
        if u32_at(&tail, record + 16) != 0
            || u32_at(&tail, record + 20) != 0
            || u64_at(&tail, record + 24) != u64_at(&tail, record + 32)
        {
            return Err(corrupt("multi-disk ZIP64"));
        }
        count = u64_at(&tail, record + 32);
        size = u64_at(&tail, record + 40);
        central_end = tail_start + record as u64;
        central_offset = u64_at(&tail, record + 48);
        zip64_record_offset = Some(u64_at(&tail, end - 12));
    } else if u64::from(u16_at(&tail, end + 8)) != count {
        return Err(corrupt("ZIP directory counts disagree"));
    }
    if count > 400_000 {
        return Err(corrupt("central directory entry limit"));
    }
    if count != 4 {
        return Err(corrupt("writer requires four entries"));
    }
    let mut position = central_end
        .checked_sub(size)
        .ok_or_else(|| corrupt("ZIP directory size"))?;
    let prefix = position
        .checked_sub(central_offset)
        .ok_or_else(|| corrupt("ZIP directory offset"))?;
    if zip64_record_offset.is_some_and(|offset| offset.checked_add(prefix) != Some(central_end)) {
        return Err(corrupt("ZIP64 locator offset"));
    }
    for expected in NAMES {
        cancelled(check)?;
        if central_end.saturating_sub(position) < 46 {
            return Err(corrupt("ZIP directory record"));
        }
        let mut record = [0; 46];
        read_at(file, position, &mut record)?;
        if &record[..4] != b"PK\x01\x02" {
            return Err(corrupt("ZIP directory signature"));
        }
        if u16_at(&record, 8) & 1 != 0 || !matches!(u16_at(&record, 10), 0 | 8) {
            return Err(corrupt("encrypted or unsupported ZIP entry"));
        }
        if usize::from(u16_at(&record, 28)) != expected.len() {
            return Err(corrupt("writer entry name"));
        }
        let next = position
            .checked_add(
                46 + u64::from(u16_at(&record, 28)) + u64::from(u16_at(&record, 30)) + u64::from(u16_at(&record, 32)),
            )
            .filter(|n| *n <= central_end)
            .ok_or_else(|| corrupt("ZIP directory extent"))?;
        let mut name = [0; 32];
        read_at(file, position + 46, &mut name[..expected.len()])?;
        if &name[..expected.len()] != expected.as_bytes() {
            return Err(corrupt("writer entry order or duplicate name"));
        }
        position = next;
    }
    if position != central_end {
        return Err(corrupt("unaccounted central directory records"));
    }
    Ok(())
}

/// Check the limit before passing any excess bytes to a consumer. Read only
/// up to the first byte past the bound, including when the source lies about
/// its declared uncompressed size.
fn bounded_read(
    mut input: impl Read,
    limit: u64,
    total: &mut u64,
    check: &(dyn Fn() -> bool + Sync),
    mut consume: impl FnMut(&[u8]),
) -> Result<u64> {
    let mut buffer = [0; 64 * 1024];
    let mut count = 0;
    loop {
        cancelled(check)?;
        let remaining_capacity = (limit - count).min(TOTAL_LIMIT - *total);
        let wanted = (remaining_capacity + 1).min(buffer.len() as u64) as usize;
        let n = input.read(&mut buffer[..wanted]).map_err(decode_error)?;
        if n == 0 {
            return Ok(count);
        }
        if n as u64 > remaining_capacity {
            return Err(corrupt("decoded byte limit"));
        }
        count += n as u64;
        *total += n as u64;
        consume(&buffer[..n]);
    }
}
fn decode_error(error: std::io::Error) -> Error {
    if error.raw_os_error().is_some() {
        error.into()
    } else {
        corrupt(format!("entry decode/CRC: {error}"))
    }
}
fn decoded_limit(compressed: u64, absolute: u64) -> u64 {
    absolute.min(compressed.saturating_mul(1000).max(RATIO_FLOOR))
}
fn zstd_read(
    input: impl Read,
    limit: u64,
    total: &mut u64,
    check: &(dyn Fn() -> bool + Sync),
    consume: impl FnMut(&[u8]),
) -> Result<u64> {
    let mut input = input;
    let mut magic = [0; 4];
    input.read_exact(&mut magic).map_err(decode_error)?;
    if magic != [0x28, 0xb5, 0x2f, 0xfd] && !(magic[0] & 0xf0 == 0x50 && magic[1..] == [0x2a, 0x4d, 0x18]) {
        return Err(corrupt("zstd magic"));
    }
    let mut decoder = zstd::stream::read::Decoder::new(std::io::Cursor::new(magic).chain(input))
        .map_err(|error| corrupt(format!("zstd header: {error}")))?;
    decoder
        .window_log_max(27)
        .map_err(|error| corrupt(format!("zstd window: {error}")))?;
    // Decoder's default reads concatenated/skippable frames through EOF,
    // verifying checksums and rejecting trailing non-frame bytes.
    bounded_read(&mut decoder, limit, total, check, consume)
}

pub(crate) fn verify(path: &Path, expected: &SnapshotDigest, check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
    let mut file = File::open(path)?;
    bounded_directory(&mut file, check)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| corrupt(format!("ZIP: {error}")))?;
    if archive.len() != 4 {
        return Err(corrupt("writer entry count"));
    }
    for (index, expected_name) in NAMES.into_iter().enumerate() {
        let entry = archive
            .by_index(index)
            .map_err(|error| corrupt(format!("ZIP entry: {error}")))?;
        if entry
            .name()
            .map_err(|error| corrupt(format!("ZIP entry name: {error}")))?
            .as_ref()
            != expected_name
        {
            return Err(corrupt("ZIP entry names disagree with bounded directory"));
        }
    }
    let mut total = 0;
    let mut meta = Vec::new();
    {
        let entry = archive.by_index(0).map_err(|error| corrupt(format!("meta: {error}")))?;
        bounded_read(entry, META_LIMIT, &mut total, check, |bytes| {
            meta.extend_from_slice(bytes)
        })?;
    }
    match meta_version(&meta)? {
        3 => {}
        1 | 2 => return Err(Error::UnsupportedFormat),
        _ => return Err(Error::TooNew),
    }
    let mut digest = Sha256::new();
    {
        let entry = archive
            .by_index(1)
            .map_err(|error| corrupt(format!("collection: {error}")))?;
        let limit = decoded_limit(entry.compressed_size(), COLLECTION_LIMIT);
        let count = zstd_read(entry, limit, &mut total, check, |bytes| digest.update(bytes))?;
        let hash: [u8; 32] = digest.finalize().into();
        if count != expected.length || hash != expected.sha256 {
            return Err(corrupt("snapshot length or SHA-256"));
        }
    }
    {
        let entry = archive
            .by_index(2)
            .map_err(|error| corrupt(format!("placeholder: {error}")))?;
        let mut header = Vec::with_capacity(100);
        // Placeholder is also streamed in full to check its CRC. It shares
        // the aggregate P8 budget and cannot allocate from a ZIP size field.
        let count = bounded_read(entry, TOTAL_LIMIT, &mut total, check, |bytes| {
            let take = bytes.len().min(100 - header.len());
            header.extend_from_slice(&bytes[..take]);
        })?;
        if count < 100 || &header[..16] != b"SQLite format 3\0" || header[18..20] != [1, 1] {
            return Err(corrupt("placeholder SQLite header"));
        }
    }
    {
        let entry = archive
            .by_index(3)
            .map_err(|error| corrupt(format!("media: {error}")))?;
        // Empty protobuf map is the writer contract. Reject at the first
        // decoded byte rather than allocating a media map we cannot accept.
        zstd_read(entry, 0, &mut total, check, |_| {})?;
    }
    drop(archive); // every ZIP/decoder handle closes before AtomicFile renames
    cancelled(check)
}

// P3's protobuf message, including bounded unknown groups (model P2).
fn meta_version(mut bytes: &[u8]) -> Result<i32> {
    let mut version = 0;
    let mut groups = Vec::new();
    while !bytes.is_empty() {
        let key = varint(&mut bytes)?;
        if key > u64::from(u32::MAX) || key >> 3 == 0 {
            return Err(corrupt("meta field number"));
        }
        let field = key >> 3;
        let wire = key & 7;
        if wire != 4 && groups.len() >= 100 {
            return Err(corrupt("meta nesting"));
        }
        if field == 1 && groups.is_empty() {
            if wire != 0 {
                return Err(corrupt("meta version wire type"));
            }
            version = varint(&mut bytes)? as i32;
            continue;
        }
        match wire {
            0 => {
                varint(&mut bytes)?;
            }
            1 => take(&mut bytes, 8)?,
            2 => {
                let length = varint(&mut bytes)?;
                take(&mut bytes, length)?;
            }
            3 => groups.push(field),
            4 if groups.pop() == Some(field) => {}
            5 => take(&mut bytes, 4)?,
            _ => return Err(corrupt("meta wire type or group")),
        }
    }
    if !groups.is_empty() {
        return Err(corrupt("meta truncated group"));
    }
    Ok(version)
}
fn take(bytes: &mut &[u8], length: u64) -> Result<()> {
    if length > bytes.len() as u64 {
        return Err(corrupt("meta truncated payload"));
    }
    *bytes = &bytes[length as usize..];
    Ok(())
}
fn varint(bytes: &mut &[u8]) -> Result<u64> {
    let mut value = 0;
    for index in 0..10 {
        let byte = *bytes.first().ok_or_else(|| corrupt("meta truncated varint"))?;
        *bytes = &bytes[1..];
        if index == 9 && byte > 1 {
            return Err(corrupt("meta varint overflow"));
        }
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(corrupt("meta varint overflow"))
}

#[cfg(test)]
mod tests;
