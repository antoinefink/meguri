//! Bounded pre-index ZIP validation, following the package reader's on-disk
//! directory pattern. Only a request-sized canonical directory reaches zip's
//! IndexMap. Raw member duplicates are checked before indexing; _meta's JSON
//! key semantics are independent. Entry data still uses normal ZipFile Read.
use super::{JobError, Method, check_cancel, decode_io_error, download_error};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn read_at<R: Read + Seek>(file: &mut R, offset: u64, bytes: &mut [u8]) -> Result<(), JobError> {
    file.seek(SeekFrom::Start(offset)).map_err(decode_io_error)?;
    file.read_exact(bytes).map_err(decode_io_error)
}
fn check(cancelled: &dyn Fn() -> bool) -> Result<(), JobError> {
    check_cancel(cancelled, Method::DownloadFiles)
}

struct Directory {
    start: u64,
    end: u64,
    prefix: u64,
    count: u64,
}
fn directory<R: Read + Seek>(
    file: &mut R,
    length: u64,
    max: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Directory, JobError> {
    check(cancelled)?;
    let tail_len = length.min(65_535 + 22) as usize;
    let mut tail = vec![0; tail_len];
    let tail_start = length - tail_len as u64;
    read_at(file, tail_start, &mut tail)?;
    let e = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..].starts_with(b"PK\x05\x06") && i + 22 + usize::from(u16_at(&tail, i + 20)) == tail.len())
        .ok_or_else(|| download_error("ZIP end record is invalid"))?;
    if u16_at(&tail, e + 4) != 0 || u16_at(&tail, e + 6) != 0 {
        return Err(download_error("ZIP spans multiple disks"));
    }
    let disk_count = u16_at(&tail, e + 8);
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
    let needs64 =
        disk_count == u16::MAX || classic_count == u16::MAX || classic_size == u32::MAX || classic_offset == u32::MAX;
    let mut zip64_offset = None;
    if has_locator {
        if u32_at(&locator, 4) != 0 || u32_at(&locator, 16) != 1 {
            return Err(download_error("ZIP64 spans multiple disks"));
        }
        let (position, record) = zip64_record(file, end - 20, cancelled)?;
        if u32_at(&record, 16) != 0 || u32_at(&record, 20) != 0 || u64_at(&record, 24) != u64_at(&record, 32) {
            return Err(download_error("ZIP64 directory disk fields disagree"));
        }
        count = u64_at(&record, 32);
        size = u64_at(&record, 40);
        offset = u64_at(&record, 48);
        if (classic_count != u16::MAX && u64::from(classic_count) != count)
            || (disk_count != u16::MAX && u64::from(disk_count) != count)
            || (classic_size != u32::MAX && u64::from(classic_size) != size)
            || (classic_offset != u32::MAX && u64::from(classic_offset) != offset)
        {
            return Err(download_error("ZIP64 and classic directory fields disagree"));
        }
        end = position;
        zip64_offset = Some(u64_at(&locator, 8));
    } else if needs64 || disk_count != classic_count {
        return Err(download_error("ZIP directory counts or ZIP64 locator are invalid"));
    }
    // Every central record has only three 16-bit variable-length fields.
    // Bound both declarations before any IndexMap or record inventory exists.
    if count > max as u64 || size > max as u64 * (46 + 3 * 65_535) {
        return Err(download_error("ZIP directory exceeds requested inventory bound"));
    }
    let start = end
        .checked_sub(size)
        .ok_or_else(|| download_error("ZIP directory size is invalid"))?;
    let prefix = start
        .checked_sub(offset)
        .ok_or_else(|| download_error("ZIP directory offset is invalid"))?;
    if zip64_offset.is_some_and(|o| o.checked_add(prefix) != Some(end)) {
        return Err(download_error("ZIP64 locator offset is invalid"));
    }
    Ok(Directory {
        start,
        end,
        prefix,
        count,
    })
}

fn zip64_record<R: Read + Seek>(
    file: &mut R,
    end: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<(u64, [u8; 56]), JobError> {
    let mut scan_end = end;
    let mut bytes = vec![0; 64 * 1024 + 55];
    loop {
        check(cancelled)?;
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
            return Err(download_error("ZIP64 end record is invalid"));
        }
        scan_end = start;
    }
}

fn entry_sizes(record: &[u8], mut extra: &[u8]) -> Result<(u64, u64, u64), JobError> {
    let mut values = [
        u64::from(u32_at(record, 24)),
        u64::from(u32_at(record, 20)),
        u64::from(u32_at(record, 42)),
    ];
    let needed = values.map(|v| v == u64::from(u32::MAX));
    let mut found = false;
    while !extra.is_empty() {
        if extra.len() < 4 {
            return Err(download_error("ZIP extra header is truncated"));
        }
        let length = usize::from(u16_at(extra, 2));
        if length > extra.len() - 4 {
            return Err(download_error("ZIP extra length is invalid"));
        }
        if u16_at(extra, 0) == 1 {
            if found {
                return Err(download_error("ZIP64 extra is repeated"));
            }
            found = true;
            let mut payload = &extra[4..4 + length];
            for (value, required) in values.iter_mut().zip(needed) {
                if required {
                    if payload.len() < 8 {
                        return Err(download_error("ZIP64 size is truncated"));
                    }
                    *value = u64_at(payload, 0);
                    payload = &payload[8..];
                }
            }
            if u16_at(record, 34) == u16::MAX && (payload.len() < 4 || u32_at(payload, 0) != 0) {
                return Err(download_error("ZIP64 entry disk is invalid"));
            }
        }
        extra = &extra[4 + length..];
    }
    if (needed.into_iter().any(|v| v) || u16_at(record, 34) == u16::MAX) && !found {
        return Err(download_error("ZIP64 extra is missing"));
    }
    Ok((values[1], values[0], values[2]))
}

pub(super) struct BoundedZip<R> {
    file: R,
    data_end: u64,
    suffix: Vec<u8>,
    position: u64,
}
pub(super) fn open<R: Read + Seek>(
    mut file: R,
    length: u64,
    max: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<BoundedZip<R>, JobError> {
    let dir = directory(&mut file, length, max, cancelled)?;
    let mut position = dir.start;
    let mut actual = 0u64;
    let mut names: Vec<([u8; 32], u64, u16)> = Vec::with_capacity(max);
    let mut suffix = Vec::new();
    while position < dir.end {
        check(cancelled)?;
        actual += 1;
        if actual > max as u64 || dir.end - position < 46 {
            return Err(download_error(
                "ZIP actual inventory exceeds bound or has a truncated record",
            ));
        }
        let mut record = [0; 46];
        read_at(&mut file, position, &mut record)?;
        if !record.starts_with(b"PK\x01\x02") {
            return Err(download_error("ZIP central signature is invalid"));
        }
        let name_len = u16_at(&record, 28);
        let extra_len = u16_at(&record, 30);
        let next = position
            .checked_add(46 + u64::from(name_len) + u64::from(extra_len) + u64::from(u16_at(&record, 32)))
            .filter(|n| *n <= dir.end)
            .ok_or_else(|| download_error("ZIP central extent is invalid"))?;
        let mut name = vec![0; usize::from(name_len)];
        read_at(&mut file, position + 46, &mut name)?;
        let hash: [u8; 32] = Sha256::digest(&name).into();
        for &(prior_hash, prior, prior_len) in &names {
            if prior_hash == hash {
                let mut previous = vec![0; usize::from(prior_len)];
                read_at(&mut file, prior, &mut previous)?;
                if previous == name {
                    return Err(download_error("ZIP contains a repeated raw member name"));
                }
            }
        }
        names.push((hash, position + 46, name_len));
        if u16_at(&record, 8) & 1 != 0 || !matches!(u16_at(&record, 10), 0 | 8) {
            return Err(download_error("ZIP entry is encrypted or uses unsupported compression"));
        }
        if !matches!(u16_at(&record, 34), 0 | 0xffff) {
            return Err(download_error("ZIP entry spans multiple disks"));
        }
        let mut extra = vec![0; usize::from(extra_len)];
        read_at(&mut file, position + 46 + u64::from(name_len), &mut extra)?;
        let (compressed, decoded, local) = entry_sizes(&record, &extra)?;
        let local = local
            .checked_add(dir.prefix)
            .filter(|o| *o < dir.start)
            .ok_or_else(|| download_error("ZIP local offset is invalid"))?;
        let mut header = [0; 30];
        read_at(&mut file, local, &mut header)?;
        if !header.starts_with(b"PK\x03\x04")
            || u16_at(&header, 6) & 1 != 0
            || u16_at(&header, 8) != u16_at(&record, 10)
            || u16_at(&header, 26) != name_len
        {
            return Err(download_error("ZIP local header disagrees with central record"));
        }
        let data = local
            .checked_add(30 + u64::from(name_len) + u64::from(u16_at(&header, 28)))
            .ok_or_else(|| download_error("ZIP local extent overflow"))?;
        if data.checked_add(compressed).is_none_or(|end| end > dir.start) {
            return Err(download_error("ZIP entry crosses central directory"));
        }
        let mut local_name = vec![0; usize::from(name_len)];
        read_at(&mut file, local + 30, &mut local_name)?;
        if local_name != name {
            return Err(download_error("ZIP local and central names disagree"));
        }
        // Keep names/CRC/method/decoded sizes, discard comments and unrelated
        // extra metadata. zip cannot follow an alternate unbounded directory.
        record[6..8].copy_from_slice(&45_u16.to_le_bytes());
        record[20..28].fill(0xff);
        record[30..32].copy_from_slice(&28_u16.to_le_bytes());
        record[32..36].fill(0);
        record[42..46].fill(0xff);
        suffix.extend_from_slice(&record);
        suffix.extend_from_slice(&name);
        suffix.extend_from_slice(&[1, 0, 24, 0]);
        suffix.extend_from_slice(&decoded.to_le_bytes());
        suffix.extend_from_slice(&compressed.to_le_bytes());
        suffix.extend_from_slice(&local.to_le_bytes());
        position = next;
    }
    if actual != dir.count {
        return Err(download_error("ZIP declared and actual directory counts disagree"));
    }
    let size = suffix.len() as u64;
    // Canonical ZIP64 footer supports large physical offsets without trusting
    // an unbounded original ZIP64 extensible sector.
    let zip64_position = dir
        .start
        .checked_add(size)
        .ok_or_else(|| download_error("ZIP directory overflow"))?;
    suffix.extend_from_slice(b"PK\x06\x06");
    suffix.extend_from_slice(&44_u64.to_le_bytes());
    suffix.extend_from_slice(&45_u16.to_le_bytes());
    suffix.extend_from_slice(&45_u16.to_le_bytes());
    suffix.extend_from_slice(&[0; 8]);
    suffix.extend_from_slice(&actual.to_le_bytes());
    suffix.extend_from_slice(&actual.to_le_bytes());
    suffix.extend_from_slice(&size.to_le_bytes());
    suffix.extend_from_slice(&dir.start.to_le_bytes());
    suffix.extend_from_slice(b"PK\x06\x07");
    suffix.extend_from_slice(&0_u32.to_le_bytes());
    suffix.extend_from_slice(&zip64_position.to_le_bytes());
    suffix.extend_from_slice(&1_u32.to_le_bytes());
    suffix.extend_from_slice(b"PK\x05\x06");
    suffix.extend_from_slice(&[0; 4]);
    suffix.extend_from_slice(&(actual as u16).to_le_bytes());
    suffix.extend_from_slice(&(actual as u16).to_le_bytes());
    suffix.extend_from_slice(&(size as u32).to_le_bytes());
    suffix.extend_from_slice(&(dir.start.min(u64::from(u32::MAX)) as u32).to_le_bytes());
    suffix.extend_from_slice(&0_u16.to_le_bytes());
    check(cancelled)?;
    Ok(BoundedZip {
        file,
        data_end: dir.start,
        suffix,
        position: 0,
    })
}
impl<R: Read + Seek> Read for BoundedZip<R> {
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
impl<R> Seek for BoundedZip<R> {
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
