//! Pre-index ZIP bounds, with byte-exact names and no archive-derived paths.
//! Keep only needed central records. A read-only virtual suffix gives
//! zip its normal entry/CRC/deflate decoder without indexing unknown metadata.
use crate::{Result, cancelled, corrupt, reader::decode_error};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
};

const ENTRY_LIMIT: u64 = 400_000;
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn read_at(file: &mut File, offset: u64, b: &mut [u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(b).map_err(decode_error)
}

struct Directory {
    start: u64,
    end: u64,
    prefix: u64,
    count: u64,
}
fn directory(file: &mut File, check: &(dyn Fn() -> bool + Sync)) -> Result<Directory> {
    let length = file.metadata()?.len();
    let tail_len = length.min(65_535 + 22) as usize;
    let mut tail = vec![0; tail_len];
    let tail_start = length - tail_len as u64;
    read_at(file, tail_start, &mut tail)?;
    let e = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..].starts_with(b"PK\x05\x06") && i + 22 + usize::from(u16_at(&tail, i + 20)) == tail.len())
        .ok_or_else(|| corrupt("ZIP end record"))?;
    if u16_at(&tail, e + 4) != 0 || u16_at(&tail, e + 6) != 0 {
        return Err(corrupt("multi-disk ZIP"));
    }
    let classic_disk_count = u16_at(&tail, e + 8);
    let classic_count = u16_at(&tail, e + 10);
    let classic_size = u32_at(&tail, e + 12);
    let classic_offset = u32_at(&tail, e + 16);
    let mut count = u64::from(classic_count);
    let mut size = u64::from(classic_size);
    let mut offset = u64::from(classic_offset);
    let mut end = tail_start + e as u64;
    let mut locator = [0; 20];
    let has_locator = end >= 20 && {
        read_at(file, end - 20, &mut locator)?;
        locator.starts_with(b"PK\x06\x07")
    };
    let requires64 = classic_disk_count == u16::MAX
        || classic_count == u16::MAX
        || classic_size == u32::MAX
        || classic_offset == u32::MAX;
    let mut zip64_offset = None;
    // Inspect optional ZIP64 too: zip may prefer it over ordinary classic
    // fields. Never let those records bypass the allocation/count bound.
    if has_locator {
        if u32_at(&locator, 4) != 0 || u32_at(&locator, 16) != 1 {
            return Err(corrupt("multi-disk ZIP64"));
        }
        let locator_position = end - 20;
        let (record_position, record) = zip64_record(file, locator_position, check)?;
        let b = &record;
        if u32_at(b, 16) != 0 || u32_at(b, 20) != 0 || u64_at(b, 24) != u64_at(b, 32) {
            return Err(corrupt("multi-disk ZIP64"));
        }
        count = u64_at(b, 32);
        size = u64_at(b, 40);
        offset = u64_at(b, 48);
        if count > ENTRY_LIMIT {
            return Err(corrupt("ZIP member count exceeds bound"));
        }
        if (classic_count != u16::MAX && u64::from(classic_count) != count)
            || (classic_disk_count != u16::MAX && u64::from(classic_disk_count) != count)
            || (classic_size != u32::MAX && u64::from(classic_size) != size)
            || (classic_offset != u32::MAX && u64::from(classic_offset) != offset)
        {
            return Err(corrupt("ambiguous ZIP64/classic metadata"));
        }
        end = record_position;
        zip64_offset = Some(u64_at(&locator, 8));
    } else if requires64 {
        return Err(corrupt("missing ZIP64 locator"));
    } else if classic_disk_count != classic_count {
        return Err(corrupt("ZIP directory counts disagree"));
    }
    if count > ENTRY_LIMIT {
        return Err(corrupt("ZIP member count exceeds bound"));
    }
    let start = end.checked_sub(size).ok_or_else(|| corrupt("ZIP directory size"))?;
    let prefix = start
        .checked_sub(offset)
        .ok_or_else(|| corrupt("ZIP directory offset"))?;
    if zip64_offset.is_some_and(|o| o.checked_add(prefix) != Some(end)) {
        return Err(corrupt("ZIP64 locator offset"));
    }
    Ok(Directory {
        start,
        end,
        prefix,
        count,
    })
}

// ZIP64's extensible sector has a 64-bit size. Locate its fixed header with
// overlapping fixed buffers rather than allocating the extension, also when
// an executable prefix shifts the locator's relative offset.
fn zip64_record(file: &mut File, end: u64, check: &(dyn Fn() -> bool + Sync)) -> Result<(u64, [u8; 56])> {
    let mut scan_end = end;
    let mut bytes = vec![0; 64 * 1024 + 55];
    loop {
        cancelled(check)?;
        let start = scan_end.saturating_sub(64 * 1024);
        let length = (end.min(scan_end.saturating_add(55)) - start) as usize;
        read_at(file, start, &mut bytes[..length])?;
        for i in (0..length.saturating_sub(55)).rev() {
            if bytes[i..].starts_with(b"PK\x06\x06")
                && u64_at(&bytes, i + 4) >= 44
                && (start + i as u64)
                    .checked_add(12)
                    .and_then(|n| n.checked_add(u64_at(&bytes, i + 4)))
                    == Some(end)
            {
                let mut record = [0; 56];
                record.copy_from_slice(&bytes[i..i + 56]);
                return Ok((start + i as u64, record));
            }
        }
        if start == 0 {
            return Err(corrupt("ZIP64 end record"));
        }
        scan_end = start;
    }
}

pub(crate) struct BoundedZip {
    file: File,
    data_end: u64,
    suffix: Vec<u8>,
    position: u64,
}
pub(crate) struct NeededArchive {
    pub archive: zip::ZipArchive<BoundedZip>,
    pub meta: Option<usize>,
    pub collection: Option<usize>,
    pub media: Option<usize>,
    pub payloads: BTreeMap<u32, usize>,
}

pub(crate) fn open(mut file: File, check: &(dyn Fn() -> bool + Sync)) -> Result<NeededArchive> {
    cancelled(check)?;
    let dir = directory(&mut file, check)?;
    let mut position = dir.start;
    let mut actual = 0_u64;
    // Retain a digest and location per member; compare source names on a
    // digest collision so duplicate detection remains byte-exact.
    let mut names = BTreeMap::<[u8; 32], (u64, u16)>::new();
    let mut suffix = Vec::new();
    let (mut meta, mut collection, mut media) = (None, None, None);
    let mut payloads = BTreeMap::new();
    let mut needed = 0_u64;
    while position < dir.end {
        cancelled(check)?;
        actual += 1;
        if actual > ENTRY_LIMIT {
            return Err(corrupt("ZIP member count exceeds bound"));
        }
        if dir.end - position < 46 {
            return Err(corrupt("ZIP central record"));
        }
        let mut record = [0; 46];
        read_at(&mut file, position, &mut record)?;
        if !record.starts_with(b"PK\x01\x02") {
            return Err(corrupt("ZIP central signature"));
        }
        let name_len = u16_at(&record, 28);
        let extra_len = u16_at(&record, 30);
        let next = position
            .checked_add(46 + u64::from(name_len) + u64::from(extra_len) + u64::from(u16_at(&record, 32)))
            .filter(|n| *n <= dir.end)
            .ok_or_else(|| corrupt("ZIP central extent"))?;
        let mut name = vec![0; usize::from(name_len)];
        read_at(&mut file, position + 46, &mut name)?;
        let hash: [u8; 32] = Sha256::digest(&name).into();
        if let Some((prior, length)) = names.insert(hash, (position + 46, name_len)) {
            let mut previous = vec![0; usize::from(length)];
            read_at(&mut file, prior, &mut previous)?;
            return Err(corrupt(if previous == name {
                "duplicate ZIP name"
            } else {
                "ambiguous ZIP name fingerprint"
            }));
        }
        let numeric = !name.is_empty() && name.iter().all(u8::is_ascii_digit);
        if !numeric {
            let trimmed = name.strip_suffix(b"/").unwrap_or(&name);
            let trimmed = trimmed
                .strip_prefix(b"+")
                .or_else(|| trimmed.strip_prefix(b"-"))
                .unwrap_or(trimmed);
            if !trimmed.is_empty() && trimmed.iter().all(u8::is_ascii_digit) {
                return Err(corrupt("noncanonical numeric ZIP name"));
            }
        }
        let number = if numeric {
            let text = std::str::from_utf8(&name).map_err(|_| corrupt("numeric ZIP name"))?;
            let number: u32 = text.parse().map_err(|_| corrupt("numeric ZIP name bound"))?;
            if number.to_string() != text {
                return Err(corrupt("noncanonical numeric ZIP name"));
            }
            Some(number)
        } else {
            None
        };
        let mut payload_slot = None;
        let slot = match name.as_slice() {
            b"meta" => Some(&mut meta),
            b"collection.anki21b" => Some(&mut collection),
            b"media" => Some(&mut media),
            _ if numeric => Some(&mut payload_slot),
            _ => None,
        };
        if let Some(slot) = slot {
            // Required payloads are ordinary files; archive modes never grant
            // authority to create a link or directory in owned staging.
            let mode = u32_at(&record, 38) >> 16;
            if record[5] == 3 && !matches!(mode & 0o170000, 0 | 0o100000) {
                return Err(corrupt("nonregular needed ZIP entry"));
            }
            if u16_at(&record, 8) & 1 != 0 || !matches!(u16_at(&record, 10), 0 | 8) {
                return Err(corrupt("encrypted or unsupported needed ZIP entry"));
            }
            if !matches!(u16_at(&record, 34), 0 | 0xffff) {
                return Err(corrupt("multi-disk ZIP entry"));
            }
            let mut extra = vec![0; usize::from(extra_len)];
            read_at(&mut file, position + 46 + u64::from(name_len), &mut extra)?;
            let (compressed, decoded, local) = entry_sizes(&record, &extra)?;
            let local = local
                .checked_add(dir.prefix)
                .filter(|o| *o < dir.start)
                .ok_or_else(|| corrupt("ZIP local offset"))?;
            // The virtual suffix must never become entry data. Validate the
            // physical local extent before handing any decoder its reader.
            let mut header = [0; 30];
            read_at(&mut file, local, &mut header)?;
            if !header.starts_with(b"PK\x03\x04")
                || u16_at(&header, 6) & 1 != 0
                || u16_at(&header, 8) != u16_at(&record, 10)
                || u16_at(&header, 26) != name_len
            {
                return Err(corrupt("needed ZIP local header"));
            }
            let data = local
                .checked_add(30 + u64::from(name_len) + u64::from(u16_at(&header, 28)))
                .ok_or_else(|| corrupt("ZIP local extent"))?;
            if data.checked_add(compressed).is_none_or(|end| end > dir.start) {
                return Err(corrupt("ZIP data crosses central directory"));
            }
            let mut local_name = vec![0; usize::from(name_len)];
            read_at(&mut file, local + 30, &mut local_name)?;
            if local_name != name {
                return Err(corrupt("ZIP local/central name mismatch"));
            }
            *slot = Some(needed as usize);
            if let Some(number) = number {
                payloads.insert(number, needed as usize);
            }
            needed += 1;
            // Canonical bounded central metadata, with absolute local offset.
            // Preserve CRC, flags, method and decoded sizes for zip's checks.
            record[6..8].copy_from_slice(&45_u16.to_le_bytes());
            record[20..28].fill(0xff);
            record[30..32].copy_from_slice(&28_u16.to_le_bytes());
            record[32..34].fill(0);
            record[42..46].fill(0xff);
            record[34..36].fill(0);
            suffix.extend_from_slice(&record);
            suffix.extend_from_slice(&name);
            suffix.extend_from_slice(&[1, 0, 24, 0]);
            suffix.extend_from_slice(&decoded.to_le_bytes());
            suffix.extend_from_slice(&compressed.to_le_bytes());
            suffix.extend_from_slice(&local.to_le_bytes());
        }
        position = next;
    }
    if actual != dir.count {
        return Err(corrupt("ZIP actual/declaration count mismatch"));
    }
    drop(names);
    let central_size = suffix.len() as u64;
    let zip64_position = dir
        .start
        .checked_add(central_size)
        .ok_or_else(|| corrupt("ZIP extent overflow"))?;
    suffix.extend_from_slice(b"PK\x06\x06");
    suffix.extend_from_slice(&44_u64.to_le_bytes());
    suffix.extend_from_slice(&[45, 0, 45, 0]);
    suffix.extend_from_slice(&[0; 8]);
    suffix.extend_from_slice(&needed.to_le_bytes());
    suffix.extend_from_slice(&needed.to_le_bytes());
    suffix.extend_from_slice(&central_size.to_le_bytes());
    suffix.extend_from_slice(&dir.start.to_le_bytes());
    suffix.extend_from_slice(b"PK\x06\x07");
    suffix.extend_from_slice(&0_u32.to_le_bytes());
    suffix.extend_from_slice(&zip64_position.to_le_bytes());
    suffix.extend_from_slice(&1_u32.to_le_bytes());
    suffix.extend_from_slice(b"PK\x05\x06");
    suffix.extend_from_slice(&[0; 4]);
    suffix.extend_from_slice(&[0xff; 12]);
    suffix.extend_from_slice(&[0; 2]);
    let archive = zip::ZipArchive::new(BoundedZip {
        file,
        data_end: dir.start,
        suffix,
        position: 0,
    })?;
    if archive.len() != needed as usize {
        return Err(corrupt("filtered ZIP directory"));
    }
    Ok(NeededArchive {
        archive,
        meta,
        collection,
        media,
        payloads,
    })
}
fn entry_sizes(record: &[u8], mut extra: &[u8]) -> Result<(u64, u64, u64)> {
    let (c, d, o) = (u32_at(record, 20), u32_at(record, 24), u32_at(record, 42));
    let mut values = [u64::from(d), u64::from(c), u64::from(o)];
    let needed = [d == u32::MAX, c == u32::MAX, o == u32::MAX];
    let mut found = false;
    while !extra.is_empty() {
        if extra.len() < 4 {
            return Err(corrupt("ZIP extra header"));
        }
        let length = usize::from(u16_at(extra, 2));
        if length > extra.len() - 4 {
            return Err(corrupt("ZIP extra length"));
        }
        if u16_at(extra, 0) == 1 {
            if found {
                return Err(corrupt("duplicate ZIP64 extra"));
            }
            found = true;
            let mut payload = &extra[4..4 + length];
            for (value, required) in values.iter_mut().zip(needed) {
                if required {
                    if payload.len() < 8 {
                        return Err(corrupt("ZIP64 entry size"));
                    }
                    *value = u64_at(payload, 0);
                    payload = &payload[8..];
                }
            }
            if u16_at(record, 34) == u16::MAX && (payload.len() < 4 || u32_at(payload, 0) != 0) {
                return Err(corrupt("ZIP64 entry disk"));
            }
        }
        extra = &extra[4 + length..];
    }
    if (needed.into_iter().any(|v| v) || u16_at(record, 34) == u16::MAX) && !found {
        return Err(corrupt("missing ZIP64 extra"));
    }
    Ok((values[1], values[0], values[2]))
}
impl Read for BoundedZip {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = if self.position < self.data_end {
            self.file.seek(SeekFrom::Start(self.position))?;
            let length = (self.data_end - self.position).min(bytes.len() as u64) as usize;
            self.file.read(&mut bytes[..length])?
        } else {
            let offset = self.position - self.data_end;
            if offset >= self.suffix.len() as u64 {
                0
            } else {
                (&self.suffix[offset as usize..]).read(bytes)?
            }
        };
        self.position += n as u64;
        Ok(n)
    }
}
impl Seek for BoundedZip {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.data_end) + self.suffix.len() as i128 + i128::from(n),
        };
        self.position =
            u64::try_from(next).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "ZIP seek overflow"))?;
        Ok(self.position)
    }
}
