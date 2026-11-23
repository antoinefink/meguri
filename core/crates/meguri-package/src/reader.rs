//! Bounded read-back verification for P11/P15 publication. No entry name becomes
//! a filesystem path. The collection is hashed in fixed-size decoded chunks.
use crate::{Error, Result, cancelled, corrupt};
use sha1::Digest as Sha1Digest;
use sha2::{Digest as Sha256Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[cfg(test)]
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
/// Require the exact P11/P15 writer members and order. General inspection
/// separately permits other names under P8's 400,000-record bound.
fn bounded_directory(file: &mut File, media_count: usize, check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
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
    if count == 0xffff
        || size == 0xffff_ffff
        || u32_at(&tail, end + 16) == 0xffff_ffff
        || (end >= 20 && &tail[end - 20..end - 16] == b"PK\x06\x07")
    {
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
        let zip64_count = u64_at(&tail, record + 32);
        let zip64_size = u64_at(&tail, record + 40);
        let zip64_offset = u64_at(&tail, record + 48);
        if zip64_count > 400_000 {
            return Err(corrupt("central directory entry limit"));
        }
        if (count != 0xffff && count != zip64_count)
            || (u16_at(&tail, end + 8) != 0xffff && u64::from(u16_at(&tail, end + 8)) != zip64_count)
            || (size != 0xffff_ffff && size != zip64_size)
            || (central_offset != 0xffff_ffff && central_offset != zip64_offset)
        {
            return Err(corrupt("ambiguous ZIP64/classic metadata"));
        }
        count = zip64_count;
        size = zip64_size;
        central_end = tail_start + record as u64;
        central_offset = zip64_offset;
        zip64_record_offset = Some(u64_at(&tail, end - 12));
    } else if u64::from(u16_at(&tail, end + 8)) != count {
        return Err(corrupt("ZIP directory counts disagree"));
    }
    if count > 400_000 {
        return Err(corrupt("central directory entry limit"));
    }
    if count != media_count as u64 + 4 {
        return Err(corrupt("writer entry count"));
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
    for index in 0..media_count + 4 {
        let expected = writer_name(index, media_count);
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
pub(crate) fn bounded_read(
    input: impl Read,
    limit: u64,
    total: &mut u64,
    check: &(dyn Fn() -> bool + Sync),
    mut consume: impl FnMut(&[u8]),
) -> Result<u64> {
    bounded_read_fallible(input, limit, total, check, |bytes| {
        consume(bytes);
        Ok(())
    })
}
pub(crate) fn bounded_read_fallible(
    mut input: impl Read,
    limit: u64,
    total: &mut u64,
    check: &(dyn Fn() -> bool + Sync),
    mut consume: impl FnMut(&[u8]) -> Result<()>,
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
        consume(&buffer[..n])?;
    }
}
pub(crate) fn decode_error(error: std::io::Error) -> Error {
    if error.raw_os_error().is_some() {
        error.into()
    } else {
        corrupt(format!("entry decode/CRC: {error}"))
    }
}
pub(crate) fn decoded_limit(compressed: u64, absolute: u64) -> u64 {
    absolute.min(compressed.saturating_mul(1000).max(RATIO_FLOOR))
}
pub(crate) fn zstd_read(
    input: impl Read,
    limit: u64,
    total: &mut u64,
    check: &(dyn Fn() -> bool + Sync),
    mut consume: impl FnMut(&[u8]),
) -> Result<u64> {
    zstd_read_fallible(input, limit, total, check, |bytes| {
        consume(bytes);
        Ok(())
    })
}
pub(crate) fn zstd_read_fallible(
    input: impl Read,
    limit: u64,
    total: &mut u64,
    check: &(dyn Fn() -> bool + Sync),
    consume: impl FnMut(&[u8]) -> Result<()>,
) -> Result<u64> {
    // Even an arbitrarily long run of skippable frames must observe cancel
    // while the decoder is looking for its next output byte.
    struct CheckedInput<'a, R> {
        input: R,
        check: &'a (dyn Fn() -> bool + Sync),
    }
    impl<R: Read> Read for CheckedInput<'_, R> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if (self.check)() {
                return Err(std::io::Error::other("package read cancelled"));
            }
            self.input.read(bytes)
        }
    }
    let mut input = CheckedInput { input, check };
    let mut magic = [0; 4];
    input
        .read_exact(&mut magic)
        .map_err(|error| if check() { Error::Cancelled } else { decode_error(error) })?;
    if magic != [0x28, 0xb5, 0x2f, 0xfd] && !(magic[0] & 0xf0 == 0x50 && magic[1..] == [0x2a, 0x4d, 0x18]) {
        return Err(corrupt("zstd magic"));
    }
    let mut decoder = zstd::stream::read::Decoder::new(std::io::Cursor::new(magic).chain(input)).map_err(|error| {
        if check() {
            Error::Cancelled
        } else {
            corrupt(format!("zstd header: {error}"))
        }
    })?;
    decoder
        .window_log_max(27)
        .map_err(|error| corrupt(format!("zstd window: {error}")))?;
    // Decoder's default reads concatenated/skippable frames through EOF,
    // verifying checksums and rejecting trailing non-frame bytes.
    let result = bounded_read_fallible(&mut decoder, limit, total, check, consume);
    if result.is_err() {
        cancelled(check)?;
    }
    result
}

pub(crate) fn verify(path: &Path, expected: &SnapshotDigest, check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
    verify_export(path, expected, &[], check)
}
fn writer_name(index: usize, media_count: usize) -> std::borrow::Cow<'static, str> {
    match index {
        0 => "meta".into(),
        1 => "collection.anki21b".into(),
        2 => "collection.anki2".into(),
        n if n == media_count + 3 => "media".into(),
        n => (n - 3).to_string().into(),
    }
}
/// Validate the P15 writer output before publication; collection intake uses its own policy.
pub(crate) fn verify_export(
    path: &Path,
    expected: &SnapshotDigest,
    media: &[crate::export_capture::MediaFile],
    check: &(dyn Fn() -> bool + Sync),
) -> Result<()> {
    let mut input = File::open(path)?;
    bounded_directory(&mut input, media.len(), check)?;
    let mut zip = zip::ZipArchive::new(input).map_err(|failure| corrupt(format!("ZIP: {failure}")))?;
    let member_count = media.len() + 4;
    if zip.len() != member_count {
        return Err(corrupt("writer entry count"));
    }
    for position in 0..member_count {
        let member = zip
            .by_index(position)
            .map_err(|failure| corrupt(format!("ZIP entry: {failure}")))?;
        let actual = member
            .name()
            .map_err(|failure| corrupt(format!("ZIP entry name: {failure}")))?;
        if actual.as_ref() != writer_name(position, media.len()).as_ref() {
            return Err(corrupt("ZIP entry names disagree with bounded directory"));
        }
    }

    let mut decoded_bytes = 0;
    let mut metadata = Vec::new();
    let member = zip.by_index(0).map_err(|failure| corrupt(format!("meta: {failure}")))?;
    bounded_read(member, META_LIMIT, &mut decoded_bytes, check, |part| {
        metadata.extend_from_slice(part)
    })?;
    match meta_version(&metadata)? {
        3 => (),
        1 | 2 => return Err(Error::UnsupportedFormat),
        _ => return Err(Error::TooNew),
    }

    let member = zip
        .by_index(1)
        .map_err(|failure| corrupt(format!("collection: {failure}")))?;
    let maximum = decoded_limit(member.compressed_size(), COLLECTION_LIMIT);
    let mut collection_hash = Sha256::new();
    let collection_length = zstd_read(member, maximum, &mut decoded_bytes, check, |part| {
        collection_hash.update(part)
    })?;
    let collection_sha256: [u8; 32] = collection_hash.finalize().into();
    if (collection_length, collection_sha256) != (expected.length, expected.sha256) {
        return Err(corrupt("snapshot length or SHA-256"));
    }

    let member = zip
        .by_index(2)
        .map_err(|failure| corrupt(format!("placeholder: {failure}")))?;
    let mut sqlite_header = [0_u8; 100];
    let mut header_used = 0;
    // Read through EOF for CRC validation, retaining only the fixed SQLite header.
    let placeholder_length = bounded_read(member, TOTAL_LIMIT, &mut decoded_bytes, check, |part| {
        let copied = part.len().min(sqlite_header.len() - header_used);
        sqlite_header[header_used..header_used + copied].copy_from_slice(&part[..copied]);
        header_used += copied;
    })?;
    if placeholder_length < 100 || &sqlite_header[..16] != b"SQLite format 3\0" || sqlite_header[18..20] != [1, 1] {
        return Err(corrupt("placeholder SQLite header"));
    }

    for (number, source) in media.iter().enumerate() {
        let mut content_hash = sha1::Sha1::new();
        let required_length = u64::from(source.length());
        // Media uses its declared uint32 length and the aggregate budget, without a ratio limit.
        let length = zstd_read_fallible(
            zip.by_index(number + 3)?,
            required_length,
            &mut decoded_bytes,
            check,
            |part| {
                content_hash.update(part);
                meguri_model::faultpoint!("package.export.verify_chunk")?;
                Ok(())
            },
        )?;
        let content_sha1: [u8; 20] = content_hash.finalize().into();
        if (length, content_sha1) != (required_length, source.sha1) {
            return Err(corrupt("media length or SHA-1"));
        }
    }

    let member = zip.by_index(media.len() + 3)?;
    if media.is_empty() {
        zstd_read(member, 0, &mut decoded_bytes, check, |_| {})?;
    } else {
        let maximum = decoded_limit(member.compressed_size(), crate::export_capture::MAP_LIMIT);
        let mut next_source = media.iter();
        let mut source = next_source.next();
        let mut pending = Vec::with_capacity(160);
        zstd_read_fallible(member, maximum, &mut decoded_bytes, check, |part| {
            let mut consumed = 0;
            while consumed < part.len() {
                let required = source.ok_or_else(|| corrupt("extra media map record"))?;
                let size = crate::media_map::encoded_record_length(required.name(), required.length()) as usize;
                let copied = (size - pending.len()).min(part.len() - consumed);
                pending.extend_from_slice(&part[consumed..consumed + copied]);
                consumed += copied;
                if pending.len() < size {
                    continue;
                }
                let mut records_seen = 0;
                crate::media_map::records(&pending, check, |record| {
                    records_seen += 1;
                    let decoded_fields = (record.name, record.length, record.sha1);
                    if decoded_fields != (required.name(), required.length(), required.sha1.as_slice()) {
                        return Err(corrupt("media map record differs from captured inventory"));
                    }
                    Ok(())
                })?;
                if records_seen != 1 {
                    return Err(corrupt("media map record framing"));
                }
                pending.clear();
                source = next_source.next();
            }
            Ok(())
        })?;
        if source.is_some() || !pending.is_empty() {
            return Err(corrupt("missing media map record"));
        }
    }
    drop(zip);
    cancelled(check)
}

// P3's protobuf message, including bounded unknown groups (model P2).
pub(crate) fn meta_version(mut bytes: &[u8]) -> Result<i32> {
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
