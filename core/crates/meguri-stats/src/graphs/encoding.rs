//! The record encoding (`stats/graphs-common` G6, decided 2026-10-27): what
//! U103's bridge carries, and what the 256 KB record limit counts.
//!
//! - Integers are zigzag LEB128 varints: 7 bits per byte, a value from −64
//!   to 63 in one byte, one of magnitude below 8,192 in at most two, below
//!   2²⁷ in at most four.
//! - A sparse map, whatever its key, is written in ascending key order as
//!   runs of consecutive keys: its number of runs, then per run its start
//!   key (the first run's as is, each later one as its distance from the
//!   previous run's last key), its length, then one value array per member
//!   with exactly that many items. A key with no data starts a new run.
//! - Every other list is its length followed by its items.
//! - A text is its UTF-8 byte length as an unsigned LEB128 varint, then the
//!   bytes; a member with no text is the length 0.
//!
//! G6 leaves floats open: a float is written as its IEEE-754 bits, little
//! endian, 4 bytes for binary32 and 8 for binary64 (a reading of this
//! unit; the bridge decodes the same way).

/// Writes one record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Encoder {
    bytes: Vec<u8>,
}

/// One member of a sparse map's runs: integers, or texts.
#[derive(Clone, Copy, Debug)]
pub enum Member<'a> {
    Integers(&'a [i64]),
    Texts(&'a [&'a str]),
}

impl Encoder {
    pub fn new() -> Encoder {
        Encoder::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// An unsigned LEB128 varint.
    pub fn unsigned(&mut self, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                self.bytes.push(byte);
                return;
            }
            self.bytes.push(byte | 0x80);
        }
    }

    /// A zigzag LEB128 varint.
    pub fn integer(&mut self, value: i64) {
        self.unsigned(((value << 1) ^ (value >> 63)) as u64);
    }

    pub fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    pub fn f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    pub fn text(&mut self, text: &str) {
        self.unsigned(text.len() as u64);
        self.bytes.extend_from_slice(text.as_bytes());
    }

    /// A list: its length, then each item written by `item`.
    pub fn list<T>(&mut self, items: &[T], mut item: impl FnMut(&mut Encoder, &T)) {
        self.integer(items.len() as i64);
        for value in items {
            item(self, value);
        }
    }

    /// A sparse map: `keys` ascending and distinct, `members` one value per
    /// key each, in the same order.
    pub fn sparse_map(&mut self, keys: &[i64], members: &[Member<'_>]) {
        debug_assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
        let mut runs: Vec<std::ops::Range<usize>> = Vec::new();
        for (index, key) in keys.iter().enumerate() {
            match runs.last_mut() {
                Some(run) if keys[run.end - 1].checked_add(1) == Some(*key) => run.end = index + 1,
                _ => runs.push(index..index + 1),
            }
        }
        self.integer(runs.len() as i64);
        let mut previous: Option<i64> = None;
        for run in runs {
            let start = keys[run.start];
            self.integer(match previous {
                None => start,
                Some(last) => start.wrapping_sub(last),
            });
            self.integer(run.len() as i64);
            for member in members {
                match member {
                    Member::Integers(values) => values[run.clone()].iter().for_each(|v| self.integer(*v)),
                    Member::Texts(texts) => texts[run.clone()].iter().for_each(|t| self.text(t)),
                }
            }
            previous = Some(keys[run.end - 1]);
        }
    }
}

/// Why bytes do not decode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub &'static str);

/// Reads what an [`Encoder`] wrote (the bridge's side; tests).
#[derive(Clone, Debug)]
pub struct Decoder<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// A decoded sparse map: keys ascending, one value list per member.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecodedMap {
    pub keys: Vec<i64>,
    pub integers: Vec<Vec<i64>>,
    pub texts: Vec<Vec<String>>,
}

/// What each member of a map to decode holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Integers,
    Texts,
}

impl<'a> Decoder<'a> {
    pub fn new(bytes: &'a [u8]) -> Decoder<'a> {
        Decoder { bytes, at: 0 }
    }

    pub fn is_done(&self) -> bool {
        self.at == self.bytes.len()
    }

    pub fn unsigned(&mut self) -> Result<u64, DecodeError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.bytes.get(self.at).ok_or(DecodeError("truncated"))?;
            self.at += 1;
            // The tenth byte holds the 64th bit only.
            if shift == 63 && byte & 0x7f > 1 {
                return Err(DecodeError("a varint over 64 bits"));
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError("a varint over 64 bits"))
    }

    pub fn integer(&mut self) -> Result<i64, DecodeError> {
        let value = self.unsigned()?;
        Ok(((value >> 1) as i64) ^ -((value & 1) as i64))
    }

    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let slice = self.bytes.get(self.at..self.at + N).ok_or(DecodeError("truncated"))?;
        self.at += N;
        Ok(slice.try_into().expect("N bytes"))
    }

    pub fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_bits(u32::from_le_bytes(self.fixed::<4>()?)))
    }

    pub fn f64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_bits(u64::from_le_bytes(self.fixed::<8>()?)))
    }

    pub fn text(&mut self) -> Result<String, DecodeError> {
        let len = usize::try_from(self.unsigned()?).map_err(|_| DecodeError("a text length"))?;
        let end = self.at.checked_add(len).ok_or(DecodeError("a text length"))?;
        let bytes = self.bytes.get(self.at..end).ok_or(DecodeError("truncated"))?;
        self.at = end;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError("a text that is not UTF-8"))
    }

    fn count(&mut self) -> Result<usize, DecodeError> {
        usize::try_from(self.integer()?).map_err(|_| DecodeError("a negative count"))
    }

    pub fn list<T>(&mut self, mut item: impl FnMut(&mut Decoder<'a>) -> Result<T, DecodeError>) -> Result<Vec<T>, DecodeError> {
        let count = self.count()?;
        (0..count).map(|_| item(self)).collect()
    }

    pub fn sparse_map(&mut self, members: &[MemberKind]) -> Result<DecodedMap, DecodeError> {
        let mut map = DecodedMap {
            integers: vec![Vec::new(); members.iter().filter(|m| **m == MemberKind::Integers).count()],
            texts: vec![Vec::new(); members.iter().filter(|m| **m == MemberKind::Texts).count()],
            ..DecodedMap::default()
        };
        let runs = self.count()?;
        let mut previous: Option<i64> = None;
        for _ in 0..runs {
            let coded = self.integer()?;
            let start = match previous {
                None => coded,
                Some(last) => last.wrapping_add(coded),
            };
            let len = self.count()?;
            if len == 0 {
                return Err(DecodeError("an empty run"));
            }
            for offset in 0..len {
                map.keys.push(start.wrapping_add(offset as i64));
            }
            let (mut integers, mut texts) = (0, 0);
            for member in members {
                match member {
                    MemberKind::Integers => {
                        for _ in 0..len {
                            let value = self.integer()?;
                            map.integers[integers].push(value);
                        }
                        integers += 1;
                    }
                    MemberKind::Texts => {
                        for _ in 0..len {
                            let value = self.text()?;
                            map.texts[texts].push(value);
                        }
                        texts += 1;
                    }
                }
            }
            previous = map.keys.last().copied();
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes_of(value: i64) -> usize {
        let mut encoder = Encoder::new();
        encoder.integer(value);
        encoder.len()
    }

    #[test]
    fn a_varint_beyond_64_bits_or_a_huge_text_does_not_decode() {
        let mut over = vec![0x80u8; 9];
        over.push(0x02);
        assert!(Decoder::new(&over).unsigned().is_err());
        let mut max = vec![0xffu8; 9];
        max.push(0x01);
        assert_eq!(Decoder::new(&max).unsigned().unwrap(), u64::MAX);
        let mut huge = Encoder::new();
        huge.unsigned(u64::MAX);
        assert!(Decoder::new(&huge.into_bytes()).text().is_err());
    }

    #[test]
    fn g6_varint_sizes() {
        assert_eq!(bytes_of(-64), 1);
        assert_eq!(bytes_of(63), 1);
        assert_eq!(bytes_of(64), 2);
        assert_eq!(bytes_of(8_191), 2);
        assert_eq!(bytes_of(-8_192), 2);
        assert_eq!(bytes_of(8_192), 3);
        assert_eq!(bytes_of((1 << 27) - 1), 4);
        assert_eq!(bytes_of(i64::MAX), 10);
        assert_eq!(bytes_of(i64::MIN), 10);
    }

    #[test]
    fn g6_integers_round_trip() {
        let values = [0, 1, -1, 63, -64, 64, 8_191, -8_192, 1 << 40, i64::MAX, i64::MIN];
        let mut encoder = Encoder::new();
        for value in values {
            encoder.integer(value);
        }
        encoder.f32(-0.5);
        encoder.f64(f64::MIN_POSITIVE);
        encoder.text("prop:rated=-7304");
        encoder.text("");
        let bytes = encoder.into_bytes();
        let mut decoder = Decoder::new(&bytes);
        for value in values {
            assert_eq!(decoder.integer().unwrap(), value);
        }
        assert_eq!(decoder.f32().unwrap(), -0.5);
        assert_eq!(decoder.f64().unwrap(), f64::MIN_POSITIVE);
        assert_eq!(decoder.text().unwrap(), "prop:rated=-7304");
        assert_eq!(decoder.text().unwrap(), "");
        assert!(decoder.is_done());
    }

    #[test]
    fn g6_a_map_is_runs_of_consecutive_keys() {
        let keys = [-366, -365, -364, -10, 0, 1, 5];
        let counts = [1, 2, 3, 4, 5, 6, 7];
        let times = [10, 20, 30, 40, 50, 60, 70];
        let mut encoder = Encoder::new();
        encoder.sparse_map(&keys, &[Member::Integers(&counts), Member::Integers(&times)]);
        let bytes = encoder.into_bytes();
        // Four runs: (-366, 3), (-10: distance 354, 1), (0: distance 10, 2),
        // (5: distance 4, 1).
        let mut decoder = Decoder::new(&bytes);
        assert_eq!(decoder.integer().unwrap(), 4);
        assert_eq!(decoder.integer().unwrap(), -366);
        assert_eq!(decoder.integer().unwrap(), 3);
        let mut decoder = Decoder::new(&bytes);
        let map = decoder
            .sparse_map(&[MemberKind::Integers, MemberKind::Integers])
            .unwrap();
        assert!(decoder.is_done());
        assert_eq!(map.keys, keys);
        assert_eq!(map.integers, vec![counts.to_vec(), times.to_vec()]);
    }

    #[test]
    fn g6_a_map_with_a_text_member_and_an_empty_map() {
        let mut encoder = Encoder::new();
        encoder.sparse_map(&[], &[Member::Integers(&[])]);
        encoder.sparse_map(&[7, 8], &[Member::Integers(&[3, 0]), Member::Texts(&["prop:rated=-1", ""])]);
        encoder.list(&[1u8, 2, 3], |e, v| e.integer(i64::from(*v)));
        let bytes = encoder.into_bytes();
        let mut decoder = Decoder::new(&bytes);
        assert_eq!(decoder.sparse_map(&[MemberKind::Integers]).unwrap(), DecodedMap {
            integers: vec![Vec::new()],
            ..DecodedMap::default()
        });
        let map = decoder.sparse_map(&[MemberKind::Integers, MemberKind::Texts]).unwrap();
        assert_eq!(map.keys, [7, 8]);
        assert_eq!(map.texts, vec![vec!["prop:rated=-1".to_owned(), String::new()]]);
        assert_eq!(decoder.list(|d| d.integer()).unwrap(), [1, 2, 3]);
        assert!(decoder.is_done());
    }

    #[test]
    fn g6_twenty_years_of_daily_counts_fit_the_limit() {
        // G6's size argument: 7,305 consecutive days of five kinds, counts of
        // at most 2 bytes and times of at most 4 bytes.
        let keys: Vec<i64> = (-7_304..=0).collect();
        let counts: Vec<i64> = keys.iter().map(|_| 150).collect();
        let times: Vec<i64> = keys.iter().map(|_| 9_000_000).collect();
        let members: Vec<Member<'_>> = (0..5)
            .map(|_| Member::Integers(&counts))
            .chain((0..5).map(|_| Member::Integers(&times)))
            .collect();
        let mut encoder = Encoder::new();
        encoder.sparse_map(&keys, &members);
        assert!(encoder.len() <= 262_144, "{}", encoder.len());
    }
}
