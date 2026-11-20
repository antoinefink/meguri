//! Classifying the text at the cursor (`specs/search/editing-services.md`
//! D5): whether live search can run the text now, or should keep its last
//! results while the user finishes a term. Like the other editing services
//! it reads no collection, config or clock (D1). Offsets are characters of
//! the text as the caller gave it; the FFI converts UTF-16 (D1).

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::ast::{CardState, Comparison, NodeKind, Property, Query, Term};
use crate::builder::search_field_text;
use crate::error::{ErrorKind, SearchError};
use crate::lexer::{Form, Lexer, RawTerm, is_white_space};
use crate::parser::{operator_word, parse};

/// D5's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tail {
    /// The text parses and no incomplete form holds: run it.
    Valid,
    /// A form of D5.1 holds: keep the last results, show no error.
    Incomplete { form: Incomplete, range: Range<usize> },
    /// The parse error, as canonicalization gives it.
    Error(SearchError),
}

/// D5.1's forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Incomplete {
    EmptyValue,
    DanglingMinus,
    UnclosedQuote,
    EmptyQuote,
    UnclosedGroup,
    OperatorWord,
    TrailingEscape,
    PartialValue,
}

impl Incomplete {
    pub fn name(self) -> &'static str {
        match self {
            Incomplete::EmptyValue => "empty_value",
            Incomplete::DanglingMinus => "dangling_minus",
            Incomplete::UnclosedQuote => "unclosed_quote",
            Incomplete::EmptyQuote => "empty_quote",
            Incomplete::UnclosedGroup => "unclosed_group",
            Incomplete::OperatorWord => "operator_word",
            Incomplete::TrailingEscape => "trailing_escape",
            Incomplete::PartialValue => "partial_value",
        }
    }
}

/// D1: a cursor past the end of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidOffset;

/// D5: `text` with the cursor at character `cursor`.
pub fn classify_tail(text: &str, cursor: usize) -> Result<Tail, InvalidOffset> {
    if cursor > text.chars().count() {
        return Err(InvalidOffset);
    }
    let text = search_field_text(text);
    let view = View::new(&text);
    Ok(match parse(&text) {
        Ok(query) => view.parsed(&query, cursor),
        Err(error) => view.failed(error, cursor),
    })
}

/// The text as the caller's characters and as the lexer reads it.
struct View {
    chars: Vec<char>,
    lexer: Lexer,
    /// Characters G1 trims from the start: caller offset minus lexer position.
    lead: usize,
}

impl View {
    fn new(text: &str) -> View {
        let lexer = Lexer::new(text);
        View {
            chars: text.chars().collect(),
            lead: lexer.input_range(0..0).start,
            lexer,
        }
    }

    /// The term starting at caller offset `at`.
    fn raw_term(&self, at: usize) -> Option<RawTerm> {
        self.lexer.term(at.checked_sub(self.lead)?).ok()
    }

    /// The term's key, ASCII letters lowered, and its value as written (D3.2
    /// `value_range`): after the first unescaped colon, inside the quotes of
    /// a quoted or partly quoted term. `None` for plain text.
    fn key_and_value(&self, raw: &RawTerm) -> Option<(String, String)> {
        if let Form::PartlyQuoted { key, inside } = &raw.form {
            return Some((
                self.lexer.string(key.clone()).to_ascii_lowercase(),
                self.lexer.string(inside.clone()),
            ));
        }
        let text = raw.text();
        let chars = self.lexer.chars(text.clone());
        let mut k = 0;
        while k < chars.len() {
            match chars[k] {
                '\\' => k += 2,
                ':' => {
                    let key: String = chars[..k].iter().collect();
                    let value: String = chars[k + 1..].iter().collect();
                    return Some((key.to_ascii_lowercase(), value));
                }
                _ => k += 1,
            }
        }
        None
    }

    /// D3.2's range of a term starting at `span.start`: a negating `-`
    /// directly before an unquoted term belongs to it (G4: the term would
    /// otherwise have taken the `-` in).
    fn with_negation(&self, span: Range<usize>) -> Range<usize> {
        match span.start.checked_sub(1) {
            Some(minus) if self.chars.get(minus) == Some(&'-') => minus..span.end,
            _ => span,
        }
    }

    fn parsed(&self, query: &Query, cursor: usize) -> Tail {
        // A negated term's range is its negation's (D3.2).
        let mut negations = vec![None; query.nodes().len()];
        for node in query.nodes() {
            if let NodeKind::Not(item) = node.kind {
                negations[item.index()] = Some(node.span.clone());
            }
        }
        for (index, node) in query.nodes().iter().enumerate() {
            let NodeKind::Term(term) = &node.kind else { continue };
            let span = node.span.clone();
            let range = negations[index].clone().unwrap_or_else(|| span.clone());
            // A bare `-` that is no negation's item (`--` negates a `-`).
            if matches!(term, Term::Text(text) if text == "-")
                && negations[index].is_none()
                && span.len() == 1
                && self.chars.get(span.start) == Some(&'-')
                && span.end == cursor
            {
                return incomplete(Incomplete::DanglingMinus, span);
            }
            if matches!(term, Term::Text(_) | Term::WholeCollection) {
                continue;
            }
            let Some(raw) = self.raw_term(span.start) else { continue };
            let closing_quote = matches!(raw.form, Form::Quoted { .. } | Form::PartlyQuoted { .. });
            let at_end = span.end == cursor || (closing_quote && span.end == cursor + 1);
            if !at_end {
                continue;
            }
            if let Some((_, value)) = self.key_and_value(&raw)
                && (value.is_empty() || ends_in_empty_part(term, &value))
            {
                return incomplete(Incomplete::EmptyValue, range);
            }
        }
        Tail::Valid
    }

    fn failed(&self, error: SearchError, cursor: usize) -> Tail {
        let range = error.range();
        let form = match error.kind() {
            ErrorKind::UnclosedQuote if cursor > range.start => Some((Incomplete::UnclosedQuote, range)),
            ErrorKind::EmptyQuote if cursor == range.start + 1 => Some((Incomplete::EmptyQuote, range)),
            ErrorKind::UnclosedGroup if cursor > range.start => {
                Some((Incomplete::UnclosedGroup, range.start..self.chars.len()))
            }
            ErrorKind::MisplacedAnd | ErrorKind::MisplacedOr if self.operator_word_pending(&range, cursor) => {
                Some((Incomplete::OperatorWord, range))
            }
            ErrorKind::UnknownEscape { provided } if provided == "\\" && range.end == cursor => {
                Some((Incomplete::TrailingEscape, range))
            }
            kind if is_value_error(kind) && range.end == cursor && self.completable(&range) => {
                Some((Incomplete::PartialValue, self.with_negation(range)))
            }
            _ => None,
        };
        match form {
            Some((form, range)) => incomplete(form, range),
            None => Tail::Error(error),
        }
    }

    /// D5.1 `operator_word`: the word ends at the cursor, or it follows an
    /// item of its sequence that is no operator word, is the sequence's last
    /// item, and only spaces stand between it and the cursor.
    fn operator_word_pending(&self, word: &Range<usize>, cursor: usize) -> bool {
        if word.end == cursor {
            return true;
        }
        let spaces_only = cursor >= word.end && self.chars[word.end..cursor].iter().all(|c| is_white_space(*c));
        spaces_only && self.last_of_its_sequence(word) && self.follows_an_item(word.start)
    }

    fn last_of_its_sequence(&self, word: &Range<usize>) -> bool {
        let Some(end) = word.end.checked_sub(self.lead) else {
            return false;
        };
        matches!(self.lexer.at(self.lexer.skip_spaces(end)), None | Some(')'))
    }

    /// Whether the innermost sequence open at caller offset `at` ends, so
    /// far, in an item that is not an operator word. The text before `at`
    /// read without error, so its terms lex.
    fn follows_an_item(&self, at: usize) -> bool {
        #[derive(Clone, Copy, PartialEq)]
        enum Last {
            Nothing,
            Item,
            Operator,
        }
        let Some(end) = at.checked_sub(self.lead) else {
            return false;
        };
        let mut open = vec![Last::Nothing];
        let mut i = 0;
        loop {
            i = self.lexer.skip_spaces(i);
            if i >= end {
                break;
            }
            let last = match self.lexer.at(i) {
                Some('(') => {
                    open.push(Last::Nothing);
                    i += 1;
                    continue;
                }
                Some('-') if self.lexer.at(i + 1) == Some('(') => {
                    open.push(Last::Nothing);
                    i += 2;
                    continue;
                }
                Some(')') => {
                    open.pop();
                    i += 1;
                    Last::Item
                }
                Some('-') if self.lexer.at(i + 1).is_some_and(|c| c != ' ' && c != ')') => {
                    // A negated term or a negated operator word: an item.
                    let Ok(raw) = self.lexer.term(i + 1) else { return false };
                    i = raw.span.end;
                    Last::Item
                }
                _ => {
                    let Ok(raw) = self.lexer.term(i) else { return false };
                    i = raw.span.end;
                    let word = matches!(raw.form, Form::Unquoted)
                        && operator_word(self.lexer.chars(raw.span.clone())).is_some();
                    if word { Last::Operator } else { Last::Item }
                }
            };
            match open.last_mut() {
                Some(top) => *top = last,
                None => return false,
            }
        }
        open.last() == Some(&Last::Item)
    }

    /// D5.1 `partial_value`: the unquoted term in `range` would parse with
    /// some characters other than space, `"`, `(` and `)` added at its end.
    fn completable(&self, range: &Range<usize>) -> bool {
        let Some(raw) = self.raw_term(range.start) else {
            return false;
        };
        if raw.form != Form::Unquoted {
            return false;
        }
        match self.key_and_value(&raw) {
            Some((key, value)) => value_completable(&key, &value),
            None => false,
        }
    }
}

fn incomplete(form: Incomplete, range: Range<usize>) -> Tail {
    Tail::Incomplete { form, range }
}

/// D5.1's empty parts: nothing after a lower-case mode prefix of a tag or
/// field term, after the comma of `dupe:`, or after the operator of
/// `prop:cds:<key>`.
fn ends_in_empty_part(term: &Term, value: &str) -> bool {
    match term {
        Term::Tag { .. } | Term::Field { .. } => value == "re:" || value == "nc:",
        Term::Duplicates { text, .. } => text.is_empty(),
        Term::Property {
            property: Property::CustomString { value, .. },
            ..
        } => value.is_empty(),
        _ => false,
    }
}

/// D5.1's value errors of a term with a key.
fn is_value_error(kind: &ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::InvalidState { .. }
            | ErrorKind::InvalidFlag
            | ErrorKind::InvalidProperty { .. }
            | ErrorKind::InvalidOperator { .. }
            | ErrorKind::InvalidNumber { .. }
            | ErrorKind::InvalidWholeNumber { .. }
            | ErrorKind::InvalidPositiveWholeNumber { .. }
            | ErrorKind::InvalidNegativeWholeNumber { .. }
            | ErrorKind::InvalidAnswerButton { .. }
            | ErrorKind::InvalidIdList { .. }
            | ErrorKind::InvalidDupe
    )
}

/// Whether the value of a reserved key's term, as written, becomes one
/// `grammar.md` G8 reads once characters are added after it. Every
/// character a completion here needs (digits, letters, `,`, `:`, `.`,
/// comparison signs) may be added.
fn value_completable(key: &str, value: &str) -> bool {
    const U32: (i128, i128) = (0, u32::MAX as i128);
    match key {
        "is" => CardState::ALL
            .iter()
            .any(|state| state.word().len() > value.len() && state.word().starts_with(value)),
        "flag" => integer(value, false, (0, 7)),
        "added" | "edited" | "introduced" | "resched" => integer(value, false, U32),
        "rated" => match value.split_once(':') {
            None => integer(value, false, U32),
            Some((days, button)) => days.parse::<u32>().is_ok() && integer(button, false, (1, 4)),
        },
        "did" | "nid" | "cid" => id_list_prefix(value),
        "mid" => integer(value, true, (i64::MIN as i128, i64::MAX as i128)),
        "dupe" => !value.contains(',') && integer_or_whole(value, (i64::MIN as i128, i64::MAX as i128)),
        "prop" => property_completable(value),
        _ => false,
    }
}

/// A signed value that already parses (a comma can follow), or one that
/// digits make parse.
fn integer_or_whole(value: &str, range: (i128, i128)) -> bool {
    value.parse::<i64>().is_ok() || integer(value, true, range)
}

/// Whether one or more characters added after `text` make it an integer in
/// `range` by `grammar.md` G9's forms: an optional `+` (or `-` when
/// `signed`), then ASCII digits.
fn integer(text: &str, signed: bool, (low, high): (i128, i128)) -> bool {
    if low > high {
        return false;
    }
    if text.is_empty() {
        return signed || high >= 0;
    }
    let (negative, digits) = match text.as_bytes()[0] {
        b'+' => (false, &text[1..]),
        b'-' if signed => (true, &text[1..]),
        _ => (false, text),
    };
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // The magnitudes the sign allows.
    let (least, most) = if negative {
        if low > 0 {
            return false;
        }
        ((-high).max(0), -low)
    } else {
        if high < 0 {
            return false;
        }
        (low.max(0), high)
    };
    let significant = digits.trim_start_matches('0');
    if significant.len() > 20 {
        return false;
    }
    let magnitude: i128 = if significant.is_empty() {
        0
    } else {
        match significant.parse() {
            Ok(value) => value,
            Err(_) => return false,
        }
    };
    // k more digits reach magnitude * 10^k to magnitude * 10^k + 10^k - 1.
    let mut scale: i128 = 1;
    for _ in 0..20 {
        scale *= 10;
        let first = magnitude * scale;
        if first > most {
            return false;
        }
        if first + scale > least {
            return true;
        }
    }
    false
}

/// An id list that digits after it complete: empty, or whole ids each
/// followed by a comma (G8's digits are Unicode `Nd`).
fn id_list_prefix(value: &str) -> bool {
    static PREFIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:\d+(?:,\d+)*,)?$").expect("the id-list prefix pattern compiles"));
    PREFIX.is_match(value)
}

/// G8.1's properties, in the order the parser tries them.
const PROPERTIES: [&str; 11] = [
    "ivl", "due", "reps", "lapses", "ease", "pos", "rated", "resched", "s", "d", "r",
];

/// Whether characters added after a `prop:` value make it parse (G8.1).
fn property_completable(value: &str) -> bool {
    // A property not yet whole: the rest of its name, an operator and an
    // operand complete it, and no earlier property can take the text.
    if PROPERTIES
        .iter()
        .any(|name| name.len() > value.len() && name.starts_with(value))
        || ["cdn:", "cds:"].iter().any(|prefix| prefix.starts_with(value))
    {
        return true;
    }
    // The property the parser takes: what follows it must become an
    // operator and an operand.
    let custom = |prefix: &str| {
        let rest = value.strip_prefix(prefix)?;
        let key = rest.chars().take_while(char::is_ascii_alphanumeric).count();
        (key > 0).then(|| &value[..prefix.len() + key])
    };
    let Some(property) = PROPERTIES
        .into_iter()
        .find(|name| value.starts_with(name))
        .or_else(|| custom("cdn:"))
        .or_else(|| custom("cds:"))
    else {
        return false;
    };
    let rest = &value[property.len()..];
    if rest.is_empty() || rest == "!" {
        return true;
    }
    let Some(comparison) = Comparison::BY_LENGTH
        .into_iter()
        .find(|c| rest.starts_with(c.symbol()))
    else {
        return false;
    };
    operand_completable(property, &rest[comparison.symbol().len()..])
}

fn operand_completable(property: &str, operand: &str) -> bool {
    const U32: (i128, i128) = (0, u32::MAX as i128);
    const NOT_POSITIVE: (i128, i128) = (i32::MIN as i128, 0);
    match property {
        "ease" | "s" | "d" | "r" => float_prefix(operand),
        custom if custom.starts_with("cdn:") => float_prefix(operand),
        custom if custom.starts_with("cds:") => true,
        "due" => integer(operand, true, (i32::MIN as i128, i32::MAX as i128)),
        "rated" => match operand.split_once(':') {
            None => integer(operand, true, NOT_POSITIVE),
            Some((days, button)) => {
                days.parse::<i32>().is_ok_and(|d| d <= 0) && integer(button, false, (1, 4))
            }
        },
        "resched" => !operand.contains(':') && integer(operand, true, NOT_POSITIVE),
        _ => integer(operand, false, U32),
    }
}

/// Whether `text` begins some text Rust's `f32` parsing accepts (G9): a
/// sign, then `inf`, `infinity` or `nan` in any letter case, or digits with
/// an optional fraction and an exponent after at least one digit.
fn float_prefix(text: &str) -> bool {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let lower = body.to_ascii_lowercase();
    if "infinity".starts_with(&lower) || "nan".starts_with(&lower) {
        return true;
    }
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    let mut digits = digit_run(&chars, &mut i);
    if chars.get(i) == Some(&'.') {
        i += 1;
        digits += digit_run(&chars, &mut i);
    }
    match chars.get(i) {
        None => true,
        Some('e' | 'E') if digits > 0 => {
            i += 1;
            if matches!(chars.get(i), Some('+' | '-')) {
                i += 1;
            }
            digit_run(&chars, &mut i);
            i == chars.len()
        }
        Some(_) => false,
    }
}

/// Skips the ASCII digits at `i`, returning how many there were.
fn digit_run(chars: &[char], i: &mut usize) -> usize {
    let start = *i;
    while chars.get(*i).is_some_and(char::is_ascii_digit) {
        *i += 1;
    }
    *i - start
}
