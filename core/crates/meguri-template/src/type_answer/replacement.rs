//! A4.5: parse once, expand captures independently for each marker.
use super::InvalidReplacement;

#[derive(Debug)]
enum Part {
    Text(String),
    Capture(usize),
}
#[derive(Debug)]
pub(super) struct Replacement(Vec<Part>);
impl Replacement {
    pub(super) fn parse(text: &str) -> Result<Self, InvalidReplacement> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                literal.push(ch);
                continue;
            }
            let next = chars.next().ok_or(InvalidReplacement)?;
            let control = match next {
                '\\' => Some('\\'),
                'a' => Some('\u{7}'),
                'b' => Some('\u{8}'),
                'f' => Some('\u{c}'),
                'n' => Some('\n'),
                'r' => Some('\r'),
                't' => Some('\t'),
                'v' => Some('\u{b}'),
                _ => None,
            };
            if let Some(c) = control {
                literal.push(c);
                continue;
            }
            let capture = if next == 'g' {
                if chars.next() != Some('<') {
                    return Err(InvalidReplacement);
                }
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('>') => break,
                        Some(c) => name.push(c),
                        None => return Err(InvalidReplacement),
                    }
                }
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(InvalidReplacement);
                }
                Some(name.parse::<usize>().map_err(|_| InvalidReplacement)?)
            } else if next.is_ascii_digit() {
                let mut digits = next.to_string();
                if next == '0' {
                    for _ in 0..2 {
                        if chars.peek().is_some_and(|c| matches!(*c, '0'..='7')) {
                            digits.push(chars.next().expect("peeked"));
                        } else {
                            break;
                        }
                    }
                    literal.push(char::from(
                        u8::from_str_radix(&digits, 8).map_err(|_| InvalidReplacement)?,
                    ));
                    continue;
                }
                if chars.peek().is_some_and(char::is_ascii_digit) {
                    digits.push(chars.next().expect("peeked"));
                }
                if digits.len() == 2
                    && digits.bytes().all(|b| matches!(b, b'0'..=b'7'))
                    && chars.peek().is_some_and(|c| matches!(*c, '0'..='7'))
                {
                    digits.push(chars.next().expect("peeked"));
                    literal.push(char::from(
                        u8::from_str_radix(&digits, 8).map_err(|_| InvalidReplacement)?,
                    ));
                    continue;
                }
                Some(digits.parse::<usize>().map_err(|_| InvalidReplacement)?)
            } else {
                if next.is_ascii_alphabetic() {
                    return Err(InvalidReplacement);
                }
                literal.push('\\');
                literal.push(next);
                None
            };
            if let Some(capture) = capture {
                if capture > 1 {
                    return Err(InvalidReplacement);
                }
                parts.push(Part::Text(std::mem::take(&mut literal)));
                parts.push(Part::Capture(capture));
            }
        }
        parts.push(Part::Text(literal));
        Ok(Self(parts))
    }
    pub(super) fn expand(&self, whole: &str, spec: &str, out: &mut String) {
        for part in &self.0 {
            match part {
                Part::Text(s) => out.push_str(s),
                Part::Capture(0) => out.push_str(whole),
                Part::Capture(_) => out.push_str(spec),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_grammar_has_typed_errors_and_never_reparses_capture_text() {
        for template in [
            "\\",
            r"\g",
            r"\g<",
            r"\g<2>",
            r"\g<name>",
            r"\8",
            r"\99",
            r"\777",
            r"\z",
        ] {
            assert!(Replacement::parse(template).is_err(), "{template:?}");
        }
        let template = Replacement::parse(r"\a\b\f\n\r\t\v\000\377\g<0>\1\&\é").unwrap();
        let mut out = String::new();
        template.expand("[[type:X\\q]]", r"X\q", &mut out);
        assert_eq!(out, "\u{7}\u{8}\u{c}\n\r\t\u{b}\0ÿ[[type:X\\q]]X\\q\\&\\é");
    }
}
