//! A11.3: the `decks.kind` blob with a normal deck's description and a
//! filtered deck's searches rewritten. Only those string values and the
//! length prefixes that enclose them change (each a minimal varint); every
//! other byte, unknown fields included, stays in place.

/// One field of a message as it lies in the bytes.
struct Field {
    number: u64,
    wire: u8,
    /// The field's first byte (its tag).
    start: usize,
    /// After the tag.
    tag_end: usize,
    /// A length-delimited field's payload.
    payload: (usize, usize),
    end: usize,
}

fn varint(bytes: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for i in 0..10 {
        let byte = *bytes.get(at + i)?;
        value |= u64::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, at + i + 1));
        }
    }
    None
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// The value of a field after its tag; returns its end and payload.
fn skip_value(bytes: &[u8], at: usize, number: u64, wire: u8) -> Option<(usize, (usize, usize))> {
    match wire {
        0 => varint(bytes, at).map(|(_, next)| (next, (at, next))),
        1 => (at + 8 <= bytes.len()).then_some((at + 8, (at, at + 8))),
        2 => {
            let (len, payload) = varint(bytes, at)?;
            let end = payload.checked_add(usize::try_from(len).ok()?)?;
            (end <= bytes.len()).then_some((end, (payload, end)))
        }
        3 => {
            // A group: fields up to its end group.
            let mut q = at;
            loop {
                let (tag, next) = varint(bytes, q)?;
                let (inner_number, inner_wire) = (tag >> 3, (tag & 7) as u8);
                if inner_wire == 4 {
                    return (inner_number == number).then_some((next, (at, q)));
                }
                q = skip_value(bytes, next, inner_number, inner_wire)?.0;
            }
        }
        5 => (at + 4 <= bytes.len()).then_some((at + 4, (at, at + 4))),
        _ => None,
    }
}

fn fields(bytes: &[u8]) -> Option<Vec<Field>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let (tag, tag_end) = varint(bytes, at)?;
        let (number, wire) = (tag >> 3, (tag & 7) as u8);
        if number == 0 {
            return None;
        }
        let (end, payload) = skip_value(bytes, tag_end, number, wire)?;
        out.push(Field {
            number,
            wire,
            start: at,
            tag_end,
            payload,
            end,
        });
        at = end;
    }
    Some(out)
}

/// What a rewrite makes of a payload: `None` when it does not decode,
/// `Some(None)` when it is unchanged, `Some(Some(bytes))` otherwise.
type Rewritten = Option<Option<Vec<u8>>>;

/// Re-encodes `bytes` with each length-delimited field `number` replaced by
/// `rewrite` of its payload.
fn rewrite_fields(bytes: &[u8], number: u64, rewrite: &mut dyn FnMut(&[u8]) -> Rewritten) -> Rewritten {
    let mut out = Vec::with_capacity(bytes.len());
    let mut changed = false;
    for field in fields(bytes)? {
        if field.number == number
            && field.wire == 2
            && let Some(payload) = rewrite(&bytes[field.payload.0..field.payload.1])?
        {
            out.extend_from_slice(&bytes[field.start..field.tag_end]);
            put_varint(&mut out, payload.len() as u64);
            out.extend_from_slice(&payload);
            changed = true;
            continue;
        }
        out.extend_from_slice(&bytes[field.start..field.end]);
    }
    Some(changed.then_some(out))
}

/// A string field's payload through `f`.
fn as_string(f: &mut dyn FnMut(&str) -> String, bytes: &[u8]) -> Rewritten {
    Some(Some(f(&String::from_utf8_lossy(bytes)).into_bytes()))
}

/// A11.3, a normal deck: the blob with field 1 `normal`'s field 4
/// `description` through `description`; `None` when it has none or does
/// not decode.
pub fn rewrite_description(blob: &[u8], description: &mut dyn FnMut(&str) -> String) -> Option<Vec<u8>> {
    rewrite_fields(blob, 1, &mut |normal| {
        rewrite_fields(normal, 4, &mut |text| as_string(description, text))
    })?
}

/// A11.3, a filtered deck: the blob with field 2 `filtered`'s field 2
/// `search_terms`' field 1 `search` through `search`, in blob order; `None`
/// when it has none or does not decode.
pub fn rewrite_searches(blob: &[u8], search: &mut dyn FnMut(&str) -> String) -> Option<Vec<u8>> {
    rewrite_fields(blob, 2, &mut |filtered| {
        rewrite_fields(filtered, 2, &mut |term| {
            rewrite_fields(term, 1, &mut |text| as_string(search, text))
        })
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_strings_and_their_lengths_change() {
        // normal { config_id 1, description "Hund", unknown group 11 {} }
        let blob = [0x0A, 0x0A, 0x08, 0x01, 0x22, 0x04, b'H', b'u', b'n', b'd', 0x5B, 0x5C];
        let out = rewrite_description(&blob, &mut |d| format!("{d}{d}")).unwrap();
        assert!(rewrite_searches(&blob, &mut |s| s.to_owned()).is_none());
        assert_eq!(
            out,
            [
                0x0A, 0x0E, 0x08, 0x01, 0x22, 0x08, b'H', b'u', b'n', b'd', b'H', b'u', b'n', b'd', 0x5B, 0x5C
            ]
        );
        // filtered { reschedule, term { search "a", limit 5 }, term { search "bc" } }
        let blob = [
            0x12, 0x0F, 0x08, 0x01, 0x12, 0x05, 0x0A, 0x01, b'a', 0x10, 0x05, 0x12, 0x04, 0x0A, 0x02, b'b', b'c',
        ];
        let blob = &blob[..];
        let out = rewrite_searches(blob, &mut |s| s.to_uppercase()).unwrap();
        assert!(rewrite_description(blob, &mut |d| d.to_owned()).is_none());
        assert_eq!(
            out,
            [
                0x12, 0x0F, 0x08, 0x01, 0x12, 0x05, 0x0A, 0x01, b'A', 0x10, 0x05, 0x12, 0x04, 0x0A, 0x02, b'B', b'C'
            ]
        );
        // No description: unchanged.
        assert!(rewrite_description(&[0x0A, 0x02, 0x08, 0x01], &mut |d| d.to_owned()).is_none());
        // Undecodable.
        assert!(rewrite_description(&[0x0A, 0x05, 0x08], &mut |d| d.to_owned()).is_none());
    }
}
