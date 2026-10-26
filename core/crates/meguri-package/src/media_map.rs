//! Bounded protobuf media inventory. Unknown-only messages are empty maps.
use crate::{Result, cancelled, corrupt};

pub(crate) struct MediaRecord<'a> {
    pub name: &'a str,
    pub length: u32,
    pub sha1: &'a [u8],
}
fn entry(mut bytes: &[u8]) -> Result<MediaRecord<'_>> {
    let mut name = "";
    let mut sha1: &[u8] = &[];
    let mut length = 0;
    while !bytes.is_empty() {
        let (field, wire) = key(&mut bytes)?;
        match field {
            1 => {
                require(wire, 2)?;
                name = std::str::from_utf8(payload(&mut bytes)?).map_err(|_| corrupt("media UTF-8"))?;
            }
            2 => {
                require(wire, 0)?;
                length = u32::try_from(varint(&mut bytes)?).map_err(|_| corrupt("media uint32 length"))?;
            }
            255 => {
                require(wire, 0)?;
                varint(&mut bytes)?;
            }
            3 => {
                require(wire, 2)?;
                sha1 = payload(&mut bytes)?;
            }
            _ => skip(&mut bytes, field, wire, 1)?,
        }
    }
    if sha1.len() != 20 {
        return Err(corrupt("media SHA1 length"));
    }
    // A valid media name is <=120 bytes. Refuse before the transform allocates.
    if name.is_empty() || name.len() > 120 || meguri_text::media_name::safe_media_name(name) != name {
        return Err(corrupt("unsafe media name"));
    }
    Ok(MediaRecord { name, length, sha1 })
}
fn require(wire: u8, expected: u8) -> Result<()> {
    if wire == expected {
        Ok(())
    } else {
        Err(corrupt("media known field wire type"))
    }
}
fn key(bytes: &mut &[u8]) -> Result<(u32, u8)> {
    let value = varint(bytes)?;
    if value > u64::from(u32::MAX) || value >> 3 == 0 {
        return Err(corrupt("media field number"));
    }
    let wire = (value & 7) as u8;
    if wire > 5 {
        return Err(corrupt("media wire type"));
    }
    Ok(((value >> 3) as u32, wire))
}
fn skip(bytes: &mut &[u8], field: u32, wire: u8, depth: usize) -> Result<()> {
    if depth >= 100 {
        return Err(corrupt("media protobuf nesting"));
    }
    match wire {
        0 => {
            varint(bytes)?;
        }
        1 => {
            take(bytes, 8)?;
        }
        2 => {
            payload(bytes)?;
        }
        3 => {
            // Group ends are not fields for the depth limit. Keys inside a
            // group are unknown, including numbers declared by its parent.
            loop {
                let (nested, wire) = key(bytes)?;
                if wire == 4 {
                    if nested == field {
                        break;
                    }
                    return Err(corrupt("media mismatched group"));
                }
                skip(bytes, nested, wire, depth + 1)?;
            }
        }
        5 => {
            take(bytes, 4)?;
        }
        _ => return Err(corrupt("media unexpected group end")),
    }
    Ok(())
}
fn take<'a>(bytes: &mut &'a [u8], n: u64) -> Result<&'a [u8]> {
    if n > bytes.len() as u64 {
        return Err(corrupt("media truncated payload"));
    }
    let (head, rest) = bytes.split_at(n as usize);
    *bytes = rest;
    Ok(head)
}
fn payload<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8]> {
    let n = varint(bytes)?;
    take(bytes, n)
}
fn varint(bytes: &mut &[u8]) -> Result<u64> {
    let mut value = 0;
    for index in 0..10 {
        let b = take(bytes, 1)?[0];
        if index == 9 && b > 1 {
            return Err(corrupt("media varint overflow"));
        }
        value |= u64::from(b & 0x7f) << (7 * index);
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(corrupt("media varint overflow"))
}

/// Borrowed records keep verification from duplicating the bounded decoded map.
pub(crate) fn records(
    mut bytes: &[u8],
    check: &(dyn Fn() -> bool + Sync),
    mut visit: impl FnMut(MediaRecord<'_>) -> Result<()>,
) -> Result<()> {
    while !bytes.is_empty() {
        cancelled(check)?;
        let (field, wire) = key(&mut bytes)?;
        if field == 1 {
            require(wire, 2)?;
            visit(entry(payload(&mut bytes)?)?)?;
        } else {
            skip(&mut bytes, field, wire, 0)?;
        }
    }
    Ok(())
}
fn varint_length(mut value: u64) -> u64 {
    let mut n = 1;
    while value >= 128 {
        n += 1;
        value >>= 7;
    }
    n
}
pub(crate) fn encoded_record_length(name: &str, length: u32) -> u64 {
    let payload = 1 + varint_length(name.len() as u64) + name.len() as u64 + 1 + varint_length(u64::from(length)) + 22;
    1 + varint_length(payload) + payload
}
fn write_varint(output: &mut impl std::io::Write, mut value: u64) -> std::io::Result<()> {
    let mut bytes = [0; 10];
    let mut n = 0;
    while value >= 128 {
        bytes[n] = value as u8 | 0x80;
        value >>= 7;
        n += 1;
    }
    bytes[n] = value as u8;
    output.write_all(&bytes[..=n])
}
pub(crate) fn write_record(output: &mut impl std::io::Write, name: &str, length: u32, sha1: &[u8; 20]) -> Result<()> {
    let payload = 1 + varint_length(name.len() as u64) + name.len() as u64 + 1 + varint_length(u64::from(length)) + 22;
    output.write_all(&[10])?;
    write_varint(output, payload)?;
    output.write_all(&[10])?;
    write_varint(output, name.len() as u64)?;
    output.write_all(name.as_bytes())?;
    output.write_all(&[16])?;
    write_varint(output, u64::from(length))?;
    output.write_all(&[26, 20])?;
    output.write_all(sha1)?;
    Ok(())
}
