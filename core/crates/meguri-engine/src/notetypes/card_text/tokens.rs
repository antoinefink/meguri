//! A CSS Syntax Level 3 tokenizer over the stored styling, keeping each
//! token's byte range in the source so an edit can replace exactly the
//! bytes of one value (`notetypes/workspace` W10). It never fails and never
//! rewrites anything; it records whether the text ends inside a comment, a
//! string or a `url(`, which W10.1 counts as not well formed.
//!
//! The source is read as written: CR, CR LF and form feed count as newlines
//! where the tokenizer needs one, but no byte is changed.

use std::ops::Range;

/// One token's kind. Values are as CSS reads them (escapes decoded).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Whitespace,
    Comment,
    Ident(String),
    Function(String),
    AtKeyword(String),
    Hash(String),
    String(String),
    BadString,
    Url,
    BadUrl,
    Delim(char),
    Number(f64),
    Percentage(f64),
    Dimension(f64, String),
    Cdo,
    Cdc,
    Colon,
    Semicolon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    OpenCurly,
    CloseCurly,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
}

impl Token {
    /// Whitespace and comments: what a value's characters never begin or
    /// end with.
    pub fn is_trivia(&self) -> bool {
        matches!(self.kind, Kind::Whitespace | Kind::Comment)
    }
}

/// The tokens of a whole text, and whether it ended inside a comment, a
/// string or a `url(`.
pub(crate) struct Tokens {
    pub tokens: Vec<Token>,
    pub unclosed: bool,
}

pub(crate) fn tokenize(source: &str) -> Tokens {
    let mut lexer = Lexer {
        chars: source.char_indices().collect(),
        at: 0,
        len: source.len(),
        unclosed: false,
    };
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token() {
        tokens.push(token);
    }
    Tokens {
        tokens,
        unclosed: lexer.unclosed,
    }
}

struct Lexer {
    chars: Vec<(usize, char)>,
    /// Index into `chars`.
    at: usize,
    len: usize,
    unclosed: bool,
}

fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{c}')
}

fn is_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t') || is_newline(c)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_ident_char(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '-'
}

fn is_valid_escape(first: Option<char>, second: Option<char>) -> bool {
    first == Some('\\') && second.is_some_and(|c| !is_newline(c))
}

fn starts_ident(first: Option<char>, second: Option<char>, third: Option<char>) -> bool {
    match first {
        Some('-') => {
            second.is_some_and(|c| is_ident_start(c) || c == '-') || is_valid_escape(second, third)
        }
        Some(c) if is_ident_start(c) => true,
        Some('\\') => is_valid_escape(first, second),
        _ => false,
    }
}

fn starts_number(first: Option<char>, second: Option<char>, third: Option<char>) -> bool {
    match first {
        Some('+' | '-') => {
            second.is_some_and(|c| c.is_ascii_digit())
                || (second == Some('.') && third.is_some_and(|c| c.is_ascii_digit()))
        }
        Some('.') => second.is_some_and(|c| c.is_ascii_digit()),
        Some(c) => c.is_ascii_digit(),
        None => false,
    }
}

impl Lexer {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).map(|(_, c)| *c)
    }

    fn offset(&self) -> usize {
        self.chars.get(self.at).map_or(self.len, |(at, _)| *at)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.at += 1;
        Some(c)
    }

    /// A newline counts once, CR LF included.
    fn bump_newline(&mut self) {
        if self.bump() == Some('\r') && self.peek(0) == Some('\n') {
            self.at += 1;
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        let start = self.offset();
        let c = self.peek(0)?;
        let kind = if c == '/' && self.peek(1) == Some('*') {
            self.at += 2;
            loop {
                match self.peek(0) {
                    None => {
                        self.unclosed = true;
                        break;
                    }
                    Some('*') if self.peek(1) == Some('/') => {
                        self.at += 2;
                        break;
                    }
                    Some(_) => self.at += 1,
                }
            }
            Kind::Comment
        } else if is_whitespace(c) {
            while self.peek(0).is_some_and(is_whitespace) {
                self.at += 1;
            }
            Kind::Whitespace
        } else {
            self.at += 1;
            match c {
                '"' | '\'' => self.string(c),
                '#' => {
                    if self.peek(0).is_some_and(is_ident_char) || is_valid_escape(self.peek(0), self.peek(1)) {
                        Kind::Hash(self.ident_sequence())
                    } else {
                        Kind::Delim('#')
                    }
                }
                '(' => Kind::OpenParen,
                ')' => Kind::CloseParen,
                '[' => Kind::OpenSquare,
                ']' => Kind::CloseSquare,
                '{' => Kind::OpenCurly,
                '}' => Kind::CloseCurly,
                ',' => Kind::Comma,
                ':' => Kind::Colon,
                ';' => Kind::Semicolon,
                '+' | '.' if starts_number(Some(c), self.peek(0), self.peek(1)) => {
                    self.at -= 1;
                    self.numeric()
                }
                '-' => {
                    if starts_number(Some(c), self.peek(0), self.peek(1)) {
                        self.at -= 1;
                        self.numeric()
                    } else if self.peek(0) == Some('-') && self.peek(1) == Some('>') {
                        self.at += 2;
                        Kind::Cdc
                    } else if starts_ident(Some(c), self.peek(0), self.peek(1)) {
                        self.at -= 1;
                        self.ident_like()
                    } else {
                        Kind::Delim('-')
                    }
                }
                '<' if self.peek(0) == Some('!') && self.peek(1) == Some('-') && self.peek(2) == Some('-') => {
                    self.at += 3;
                    Kind::Cdo
                }
                '@' => {
                    if starts_ident(self.peek(0), self.peek(1), self.peek(2)) {
                        Kind::AtKeyword(self.ident_sequence())
                    } else {
                        Kind::Delim('@')
                    }
                }
                '\\' => {
                    if is_valid_escape(Some('\\'), self.peek(0)) {
                        self.at -= 1;
                        self.ident_like()
                    } else {
                        Kind::Delim('\\')
                    }
                }
                c if c.is_ascii_digit() => {
                    self.at -= 1;
                    self.numeric()
                }
                c if is_ident_start(c) => {
                    self.at -= 1;
                    self.ident_like()
                }
                other => Kind::Delim(other),
            }
        };
        Some(Token {
            kind,
            span: start..self.offset(),
        })
    }

    /// After the opening quote.
    fn string(&mut self, quote: char) -> Kind {
        let mut value = String::new();
        loop {
            match self.peek(0) {
                None => {
                    self.unclosed = true;
                    return Kind::String(value);
                }
                Some(c) if c == quote => {
                    self.at += 1;
                    return Kind::String(value);
                }
                // The newline is not consumed: it is the next token's.
                Some(c) if is_newline(c) => return Kind::BadString,
                Some('\\') => match self.peek(1) {
                    None => self.at += 1,
                    Some(c) if is_newline(c) => {
                        self.at += 1;
                        self.bump_newline();
                    }
                    Some(_) => {
                        self.at += 1;
                        value.push(self.escape());
                    }
                },
                Some(c) => {
                    self.at += 1;
                    value.push(c);
                }
            }
        }
    }

    /// After the backslash of a valid escape.
    fn escape(&mut self) -> char {
        let Some(first) = self.bump() else {
            return '\u{fffd}';
        };
        if !first.is_ascii_hexdigit() {
            return first;
        }
        let mut digits = String::from(first);
        while digits.len() < 6 && self.peek(0).is_some_and(|c| c.is_ascii_hexdigit()) {
            digits.push(self.bump().unwrap_or('0'));
        }
        if self.peek(0).is_some_and(is_whitespace) {
            self.bump_newline();
        }
        let value = u32::from_str_radix(&digits, 16).unwrap_or(0);
        match char::from_u32(value) {
            Some(c) if value != 0 => c,
            _ => '\u{fffd}',
        }
    }

    fn ident_sequence(&mut self) -> String {
        let mut value = String::new();
        loop {
            match self.peek(0) {
                Some(c) if is_ident_char(c) => {
                    self.at += 1;
                    value.push(c);
                }
                Some('\\') if is_valid_escape(Some('\\'), self.peek(1)) => {
                    self.at += 1;
                    value.push(self.escape());
                }
                _ => return value,
            }
        }
    }

    fn ident_like(&mut self) -> Kind {
        let name = self.ident_sequence();
        if name.eq_ignore_ascii_case("url") && self.peek(0) == Some('(') {
            self.at += 1;
            while self.peek(0).is_some_and(is_whitespace) && self.peek(1).is_some_and(is_whitespace) {
                self.at += 1;
            }
            let quoted = |c: Option<char>| matches!(c, Some('"' | '\''));
            if quoted(self.peek(0)) || (self.peek(0).is_some_and(is_whitespace) && quoted(self.peek(1))) {
                return Kind::Function(name);
            }
            return self.url();
        }
        if self.peek(0) == Some('(') {
            self.at += 1;
            return Kind::Function(name);
        }
        Kind::Ident(name)
    }

    /// After `url(` and its white space: an unquoted URL.
    fn url(&mut self) -> Kind {
        while self.peek(0).is_some_and(is_whitespace) {
            self.at += 1;
        }
        loop {
            match self.peek(0) {
                None => {
                    self.unclosed = true;
                    return Kind::Url;
                }
                Some(')') => {
                    self.at += 1;
                    return Kind::Url;
                }
                Some(c) if is_whitespace(c) => {
                    while self.peek(0).is_some_and(is_whitespace) {
                        self.at += 1;
                    }
                    match self.peek(0) {
                        None => {
                            self.unclosed = true;
                            return Kind::Url;
                        }
                        Some(')') => {
                            self.at += 1;
                            return Kind::Url;
                        }
                        Some(_) => return self.bad_url(),
                    }
                }
                Some('"' | '\'' | '(') => return self.bad_url(),
                Some(c) if ((c as u32) < 0x20 && !is_whitespace(c)) || c == '\u{7f}' => return self.bad_url(),
                Some('\\') => {
                    if is_valid_escape(Some('\\'), self.peek(1)) {
                        self.at += 1;
                        self.escape();
                    } else {
                        return self.bad_url();
                    }
                }
                Some(_) => self.at += 1,
            }
        }
    }

    fn bad_url(&mut self) -> Kind {
        loop {
            match self.peek(0) {
                None => {
                    self.unclosed = true;
                    return Kind::BadUrl;
                }
                Some(')') => {
                    self.at += 1;
                    return Kind::BadUrl;
                }
                Some('\\') if is_valid_escape(Some('\\'), self.peek(1)) => {
                    self.at += 1;
                    self.escape();
                }
                Some(_) => self.at += 1,
            }
        }
    }

    fn numeric(&mut self) -> Kind {
        let mut text = String::new();
        if let Some(sign @ ('+' | '-')) = self.peek(0) {
            self.at += 1;
            text.push(sign);
        }
        let digits = |lexer: &mut Lexer, text: &mut String| {
            while let Some(c) = lexer.peek(0).filter(char::is_ascii_digit) {
                lexer.at += 1;
                text.push(c);
            }
        };
        digits(self, &mut text);
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
            text.push('.');
            digits(self, &mut text);
        }
        if matches!(self.peek(0), Some('e' | 'E'))
            && (self.peek(1).is_some_and(|c| c.is_ascii_digit())
                || (matches!(self.peek(1), Some('+' | '-')) && self.peek(2).is_some_and(|c| c.is_ascii_digit())))
        {
            self.at += 1;
            text.push('e');
            if let Some(sign @ ('+' | '-')) = self.peek(0) {
                self.at += 1;
                text.push(sign);
            }
            digits(self, &mut text);
        }
        let value: f64 = text.parse().unwrap_or(0.0);
        if starts_ident(self.peek(0), self.peek(1), self.peek(2)) {
            return Kind::Dimension(value, self.ident_sequence());
        }
        if self.peek(0) == Some('%') {
            self.at += 1;
            return Kind::Percentage(value);
        }
        Kind::Number(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, tokenize};

    fn kinds(source: &str) -> Vec<Kind> {
        tokenize(source).tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn spans_cover_the_source_byte_for_byte() {
        let source = ".card { font: 12pt/1.5 \"Hira\\\"gino\" ; color: #0aF } /* é */ @media (x) { a {} }\r\n";
        let tokens = tokenize(source).tokens;
        let joined: String = tokens.iter().map(|t| &source[t.span.clone()]).collect();
        assert_eq!(joined, source);
        assert!(!tokenize(source).unclosed);
    }

    #[test]
    fn tokens_read_as_css_reads_them() {
        assert_eq!(
            kinds("a-b -x --y 1.5e2px 50% -.5 +3"),
            [
                Kind::Ident("a-b".into()),
                Kind::Whitespace,
                Kind::Ident("-x".into()),
                Kind::Whitespace,
                Kind::Ident("--y".into()),
                Kind::Whitespace,
                Kind::Dimension(150.0, "px".into()),
                Kind::Whitespace,
                Kind::Percentage(50.0),
                Kind::Whitespace,
                Kind::Number(-0.5),
                Kind::Whitespace,
                Kind::Number(3.0),
            ]
        );
        assert_eq!(kinds("\\78 yz"), [Kind::Ident("xyz".into())]);
        assert_eq!(kinds("url( a.png )"), [Kind::Url]);
        assert_eq!(kinds("url(\"a\")"), [Kind::Function("url".into()), Kind::String("a".into()), Kind::CloseParen]);
        assert_eq!(kinds("'a\nb'")[0], Kind::BadString);
        assert_eq!(kinds("<!-- -->"), [Kind::Cdo, Kind::Whitespace, Kind::Cdc]);
    }

    #[test]
    fn an_end_inside_a_comment_string_or_url_is_unclosed() {
        for source in ["a /* b", "a 'b", "a \"b\\", "url(a", "url(a b", "url(a\"b"] {
            assert!(tokenize(source).unclosed, "{source:?}");
        }
        for source in ["a /* b */", "'b'", "url(a)", "'a\n x"] {
            assert!(!tokenize(source).unclosed, "{source:?}");
        }
    }
}
