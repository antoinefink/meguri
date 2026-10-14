//! GUID text (`specs/text/guid.md` G1, G2). A new note's GUID is a random
//! u64 from `meguri_model::rng` written with [`encode`] (G3, R771); imported
//! and synced GUIDs are stored as they come.

/// G1: the 91 digits, value 0 first.
pub const DIGITS: &[u8; 91] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_`{|}~";

/// G2: `value` in base 91, most significant digit first, no padding; zero is
/// the empty string.
pub fn encode(value: u64) -> String {
    let mut reversed = Vec::with_capacity(10);
    let mut rest = value;
    while rest > 0 {
        reversed.push(DIGITS[(rest % 91) as usize]);
        rest /= 91;
    }
    reversed.iter().rev().map(|&digit| char::from(digit)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_match_the_spec_table() {
        for (value, text) in [
            (0, ""),
            (1, "b"),
            (25, "z"),
            (26, "A"),
            (51, "Z"),
            (52, "0"),
            (61, "9"),
            (62, "!"),
            (90, "~"),
            (91, "ba"),
            (92, "bb"),
            (8280, "~~"),
            (8281, "baa"),
            (427929800129788410, "~~~~~~~~~"),
            (427929800129788411, "baaaaaaaaa"),
            (4294967296, "!5St/"),
            (1700000000000, "c~MJPzD"),
            (9007199254740992, "b]BKf,uBG"),
            (9223372036854775808, "vYHz/gPN,i"),
            (12345678901234567890, "C<D~1=[N#b"),
            (18446744073709551615, "Rj&Z5m[>Zp"),
        ] {
            assert_eq!(encode(value), text, "{value}");
        }
    }

    #[test]
    fn the_alphabet_is_printable_ascii_without_four_characters() {
        let digits: Vec<char> = DIGITS.iter().map(|&b| b as char).collect();
        let expected: Vec<char> = (' '..='~').filter(|c| !matches!(c, ' ' | '"' | '\'' | '\\')).collect();
        let mut sorted = digits.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, expected);
        assert_eq!(&digits[..3], ['a', 'b', 'c']);
        assert_eq!(digits[62], '!');
    }
}
