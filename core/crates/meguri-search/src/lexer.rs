//! Reading search text into characters and term forms (`specs/search/
//! grammar.md` G1, G3). Positions are indexes into the normalized,
//! trimmed characters; [`Lexer::input_range`] turns them into offsets of the
//! caller's text (`errors.md` E5).

use std::ops::Range;

use crate::error::{ErrorKind, SearchError};

/// G1: the 25 code points with the Unicode White_Space property.
pub(crate) fn is_white_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{D}'
            | ' '
            | '\u{85}'
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// The text after G1: every White_Space character a space, then trimmed.
pub(crate) struct Lexer {
    chars: Vec<char>,
    /// Characters trimmed from the start: G1 maps one character to one, so
    /// this is the only shift between positions here and in the input.
    lead: usize,
}

/// Where a term's text lies (G3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// `"…"`: the inside, still escaped.
    Quoted { inside: Range<usize> },
    /// `key:"…"`: the key and the inside of the quotes, still escaped.
    PartlyQuoted { key: Range<usize>, inside: Range<usize> },
    /// A run of characters: the whole span.
    Unquoted,
}

/// One term as written: its form and every character it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawTerm {
    pub form: Form,
    pub span: Range<usize>,
}

impl RawTerm {
    /// The characters G6 reads: the inside of the quotes, or the whole run.
    pub fn text(&self) -> Range<usize> {
        match &self.form {
            Form::Quoted { inside } => inside.clone(),
            Form::PartlyQuoted { .. } | Form::Unquoted => self.span.clone(),
        }
    }
}

impl Lexer {
    pub fn new(input: &str) -> Lexer {
        let normalized: Vec<char> = input.chars().map(|c| if is_white_space(c) { ' ' } else { c }).collect();
        let lead = normalized.iter().take_while(|c| **c == ' ').count();
        let trail = normalized[lead..].iter().rev().take_while(|c| **c == ' ').count();
        Lexer {
            chars: normalized[lead..normalized.len() - trail].to_vec(),
            lead,
        }
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied()
    }

    pub fn chars(&self, range: Range<usize>) -> &[char] {
        &self.chars[range]
    }

    pub fn string(&self, range: Range<usize>) -> String {
        self.chars[range].iter().collect()
    }

    /// Offsets of the caller's text for a range of positions here.
    pub fn input_range(&self, range: Range<usize>) -> Range<usize> {
        range.start + self.lead..range.end + self.lead
    }

    pub fn error(&self, kind: ErrorKind, range: Range<usize>) -> SearchError {
        SearchError::new(kind, self.input_range(range))
    }

    pub fn skip_spaces(&self, mut i: usize) -> usize {
        while self.at(i) == Some(' ') {
            i += 1;
        }
        i
    }

    /// G3: the term starting at `i`, which holds neither a space, `(` nor
    /// `)`. The forms are tried in order: quoted, partly quoted, unquoted.
    pub fn term(&self, i: usize) -> Result<RawTerm, SearchError> {
        if self.at(i) == Some('"') {
            let inside = self.quoted(i)?;
            return Ok(RawTerm {
                span: i..inside.end + 1,
                form: Form::Quoted { inside },
            });
        }
        if let Some(key_end) = self.partly_quoted_key(i) {
            let inside = self.quoted(key_end + 1)?;
            return Ok(RawTerm {
                span: i..inside.end + 1,
                form: Form::PartlyQuoted {
                    key: i..key_end,
                    inside,
                },
            });
        }
        let end = self.unquoted_end(i)?;
        Ok(RawTerm {
            form: Form::Unquoted,
            span: i..end,
        })
    }

    /// G3 form 1 at the `"` at `open`: the inside's range.
    fn quoted(&self, open: usize) -> Result<Range<usize>, SearchError> {
        let mut j = open + 1;
        loop {
            match self.at(j) {
                // An escape takes the next character, whatever it is; a
                // text ending in a pending escape is unclosed too.
                Some('\\') if j + 1 < self.len() => j += 2,
                Some('"') if j == open + 1 => return Err(self.error(ErrorKind::EmptyQuote, open..j + 1)),
                Some('"') => return Ok(open + 1..j),
                Some(_) => j += 1,
                None => return Err(self.error(ErrorKind::UnclosedQuote, open..self.len())),
            }
        }
    }

    /// G3 form 2: the end of the key when the term at `i` is a key, `:` and
    /// a `"` with nothing between them.
    fn partly_quoted_key(&self, i: usize) -> Option<usize> {
        let mut j = i;
        while let Some(c) = self.at(j) {
            match c {
                '"' | '(' | ')' | ':' | ' ' => break,
                '\\' => match self.at(j + 1) {
                    Some(' ') | None => return None,
                    Some(_) => j += 2,
                },
                _ => j += 1,
            }
        }
        (j > i && self.at(j) == Some(':') && self.at(j + 1) == Some('"')).then_some(j)
    }

    /// G3 form 3: the end of the run starting at `i`.
    fn unquoted_end(&self, i: usize) -> Result<usize, SearchError> {
        let mut j = i;
        while let Some(c) = self.at(j) {
            match c {
                ' ' | '"' | '(' | ')' => break,
                '\\' => match self.at(j + 1) {
                    None => {
                        let provided = "\\".to_owned();
                        return Err(self.error(ErrorKind::UnknownEscape { provided }, j..j + 1));
                    }
                    // The offending text runs to the end of the text.
                    Some(' ') => {
                        let provided = self.string(j..self.len());
                        return Err(self.error(ErrorKind::UnknownEscape { provided }, j..self.len()));
                    }
                    Some(_) => j += 2,
                },
                _ => j += 1,
            }
        }
        Ok(j)
    }
}
