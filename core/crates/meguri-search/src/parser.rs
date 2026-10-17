//! Search text to [`Query`] (`specs/search/grammar.md` G2-G9), reporting the
//! first error met from left to right (`errors.md` E4). Sequences and groups
//! are read with an explicit stack: no depth of nesting exhausts the call
//! stack (E6).

use std::ops::Range;
use std::sync::LazyLock;

use meguri_model::ids::NotetypeId;
use regex::Regex;

use crate::ast::{
    BoolOperator, CardState, Comparison, Float, IdKind, NodeId, NodeKind, Property, Query, QueryBuilder, TemplateRef,
    Term, TextMode,
};
use crate::error::{ErrorKind, SearchError};
use crate::lexer::{Form, Lexer, RawTerm};

/// Reads a search. The empty search (only white space) is the whole
/// collection (Q2).
pub fn parse(input: &str) -> Result<Query, SearchError> {
    let lexer = Lexer::new(input);
    let mut builder = QueryBuilder::default();
    let root = if lexer.is_empty() {
        let at = lexer.input_range(0..0);
        builder.push(NodeKind::Term(Term::WholeCollection), at)
    } else {
        Parser {
            lexer: &lexer,
            builder: &mut builder,
        }
        .search()?
    };
    Ok(builder.finish(root, Some(input.to_owned())))
}

struct Parser<'a> {
    lexer: &'a Lexer,
    builder: &'a mut QueryBuilder,
}

/// A sequence being read (G2): the OR of its runs of AND-joined items.
struct Sequence {
    runs: Vec<Vec<NodeId>>,
    /// An operator word read since the last item, with its position.
    pending: Option<(BoolOperator, Range<usize>)>,
    /// `None` for the whole search.
    group: Option<OpenGroup>,
}

struct OpenGroup {
    /// The position of `(`.
    open: usize,
    /// The position of a `-` directly before it.
    negated_at: Option<usize>,
}

impl Sequence {
    fn new(group: Option<OpenGroup>) -> Sequence {
        Sequence {
            runs: Vec::new(),
            pending: None,
            group,
        }
    }

    fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

fn misplaced(operator: BoolOperator) -> ErrorKind {
    match operator {
        BoolOperator::And => ErrorKind::MisplacedAnd,
        BoolOperator::Or => ErrorKind::MisplacedOr,
    }
}

/// G2: an unquoted term whose whole text is `and` or `or`, any ASCII case.
fn operator_word(text: &[char]) -> Option<BoolOperator> {
    let word: String = text.iter().collect();
    if word.eq_ignore_ascii_case("and") {
        Some(BoolOperator::And)
    } else if word.eq_ignore_ascii_case("or") {
        Some(BoolOperator::Or)
    } else {
        None
    }
}

impl Parser<'_> {
    fn search(&mut self) -> Result<NodeId, SearchError> {
        let lexer = self.lexer;
        let mut stack = vec![Sequence::new(None)];
        let mut i = 0;
        loop {
            i = lexer.skip_spaces(i);
            match lexer.at(i) {
                None => {
                    self.check_trailing(stack.last())?;
                    // E5: the outermost group left open.
                    if let Some(open) = stack.get(1).and_then(|s| s.group.as_ref()) {
                        return Err(lexer.error(ErrorKind::UnclosedGroup, open.open..open.open + 1));
                    }
                    let Some(root) = stack.pop() else { unreachable_root() };
                    return Ok(self.finish(root));
                }
                Some(')') => {
                    self.check_trailing(stack.last())?;
                    if stack.len() == 1 {
                        return Err(lexer.error(ErrorKind::UnopenedGroup, i..i + 1));
                    }
                    let Some(mut sequence) = stack.pop() else {
                        unreachable_root()
                    };
                    let Some(OpenGroup { open, negated_at }) = sequence.group.take() else {
                        unreachable_root()
                    };
                    if sequence.is_empty() {
                        return Err(lexer.error(ErrorKind::EmptyGroup, open..i + 1));
                    }
                    let inner = self.finish(sequence);
                    let group = self.push(NodeKind::Group(inner), open..i + 1);
                    let item = match negated_at {
                        Some(minus) => self.push(NodeKind::Not(group), minus..i + 1),
                        None => group,
                    };
                    self.add(&mut stack, item);
                    i += 1;
                }
                Some('(') => {
                    stack.push(Sequence::new(Some(OpenGroup {
                        open: i,
                        negated_at: None,
                    })));
                    i += 1;
                }
                // G4: `-` directly before a group or a term negates it.
                Some('-') if lexer.at(i + 1).is_some_and(|c| c != ' ' && c != ')') => {
                    if lexer.at(i + 1) == Some('(') {
                        stack.push(Sequence::new(Some(OpenGroup {
                            open: i + 1,
                            negated_at: Some(i),
                        })));
                        i += 2;
                        continue;
                    }
                    let raw = lexer.term(i + 1)?;
                    let end = raw.span.end;
                    let item = match self.unquoted_operator(&raw) {
                        Some(operator) => self.push(NodeKind::NegatedOperator(operator), i..end),
                        None => {
                            let term = self.term(&raw)?;
                            self.push(NodeKind::Not(term), i..end)
                        }
                    };
                    self.add(&mut stack, item);
                    i = end;
                }
                Some(_) => {
                    let raw = lexer.term(i)?;
                    i = raw.span.end;
                    match self.unquoted_operator(&raw) {
                        Some(operator) => {
                            let Some(sequence) = stack.last_mut() else {
                                unreachable_root()
                            };
                            // G2: at the start of a sequence or right after
                            // another operator word.
                            if sequence.is_empty() || sequence.pending.is_some() {
                                return Err(lexer.error(misplaced(operator), raw.span));
                            }
                            sequence.pending = Some((operator, raw.span));
                        }
                        None => {
                            let item = self.term(&raw)?;
                            self.add(&mut stack, item);
                        }
                    }
                }
            }
        }
    }

    fn push(&mut self, kind: NodeKind, span: Range<usize>) -> NodeId {
        let span = self.lexer.input_range(span);
        self.builder.push(kind, span)
    }

    fn unquoted_operator(&self, raw: &RawTerm) -> Option<BoolOperator> {
        match raw.form {
            Form::Unquoted => operator_word(self.lexer.chars(raw.span.clone())),
            _ => None,
        }
    }

    /// E4 point 2: a trailing operator word is reported first.
    fn check_trailing(&self, sequence: Option<&Sequence>) -> Result<(), SearchError> {
        match sequence.and_then(|s| s.pending.as_ref()) {
            Some((operator, at)) => Err(self.lexer.error(misplaced(*operator), at.clone())),
            None => Ok(()),
        }
    }

    /// Joins an item to the sequence on top: by OR after an `or`, else by AND.
    fn add(&mut self, stack: &mut [Sequence], item: NodeId) {
        let Some(sequence) = stack.last_mut() else {
            return;
        };
        match (sequence.pending.take(), sequence.runs.last_mut()) {
            (Some((BoolOperator::Or, _)), _) | (_, None) => sequence.runs.push(vec![item]),
            (_, Some(run)) => run.push(item),
        }
    }

    /// The node of a non-empty sequence.
    fn finish(&mut self, sequence: Sequence) -> NodeId {
        let mut runs = Vec::with_capacity(sequence.runs.len());
        for run in sequence.runs {
            runs.push(self.joined(run, NodeKind::And));
        }
        self.joined(runs, NodeKind::Or)
    }

    fn joined(&mut self, items: Vec<NodeId>, kind: fn(Vec<NodeId>) -> NodeKind) -> NodeId {
        if let [only] = items[..] {
            return only;
        }
        let first = items.first().map(|id| self.builder.span(*id)).unwrap_or_default();
        let last = items.last().map(|id| self.builder.span(*id)).unwrap_or_default();
        self.builder.push(kind(items), first.start..last.end)
    }

    /// G6-G8: the term node of a quoted, partly quoted or unquoted term.
    fn term(&mut self, raw: &RawTerm) -> Result<NodeId, SearchError> {
        let term = Reader {
            lexer: self.lexer,
            span: raw.span.clone(),
        }
        .term(raw)?;
        Ok(self.push(NodeKind::Term(term), raw.span.clone()))
    }
}

/// The sequence stack holds the whole search's sequence until the end, and
/// a group's sequence above it; finished sequences are never empty.
#[cold]
fn unreachable_root() -> ! {
    unreachable!("the parser's sequence stack lost its invariant")
}

/// G6-G9 for one term.
struct Reader<'a> {
    lexer: &'a Lexer,
    /// The whole term: the range of every error but `unknown_escape` (E5).
    span: Range<usize>,
}

/// Number forms are Rust's `FromStr` (G9).
fn number<T: std::str::FromStr>(text: &str) -> Option<T> {
    text.parse().ok()
}

/// G8: one or more decimal-digit runs separated by single commas, a digit
/// being any character of category Nd.
static ID_LIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+(?:,\d+)*$").expect("the id-list pattern compiles"));

impl Reader<'_> {
    fn error(&self, kind: ErrorKind) -> SearchError {
        self.lexer.error(kind, self.span.clone())
    }

    fn term(&self, raw: &RawTerm) -> Result<Term, SearchError> {
        if let Form::PartlyQuoted { key, inside } = &raw.form {
            return self.key_term(key.clone(), inside.clone());
        }
        let text = raw.text();
        // G6 step 1: the first colon no backslash escapes.
        let chars = self.lexer.chars(text.clone());
        let mut k = 0;
        let mut colon = None;
        while k < chars.len() {
            match chars[k] {
                '\\' => k += 2,
                ':' => {
                    colon = Some(k);
                    break;
                }
                _ => k += 1,
            }
        }
        match colon {
            None => Ok(Term::Text(self.unescaped(text)?)),
            Some(0) => Err(self.error(ErrorKind::MissingKey)),
            Some(k) => {
                let at = text.start + k;
                self.key_term(text.start..at, at + 1..text.end)
            }
        }
    }

    /// G6 step 4 and G8: a key and its value, both still escaped.
    fn key_term(&self, key: Range<usize>, value: Range<usize>) -> Result<Term, SearchError> {
        let typed_key = self.lexer.string(key.clone());
        let written = self.lexer.string(value.clone());
        let prefixed = |prefix: &str| {
            written
                .starts_with(prefix)
                .then(|| value.start + prefix.len()..value.end)
        };
        Ok(match typed_key.to_ascii_lowercase().as_str() {
            "deck" => Term::Deck(self.unescaped(value)?),
            "note" => Term::Notetype(self.unescaped(value)?),
            "tag" => match (prefixed("re:"), prefixed("nc:")) {
                (Some(rest), _) => Term::Tag {
                    mode: TextMode::Regex,
                    text: self.quotes_only(rest),
                },
                (_, Some(rest)) => Term::Tag {
                    mode: TextMode::NoCombining,
                    text: self.unescaped(rest)?,
                },
                _ => Term::Tag {
                    mode: TextMode::Normal,
                    text: self.unescaped(value)?,
                },
            },
            "card" => match number::<u16>(&written) {
                Some(n) => Term::Template(TemplateRef::Ordinal(n.saturating_sub(1))),
                None => Term::Template(TemplateRef::Name(self.unescaped(value)?)),
            },
            "flag" => match number::<u8>(&written).filter(|n| *n <= 7) {
                Some(n) => Term::Flag(n),
                None => return Err(self.error(ErrorKind::InvalidFlag)),
            },
            "resched" => Term::Rescheduled(self.positive(&written, "resched:")?),
            "added" => Term::Added(self.positive(&written, "added:")?.max(1)),
            "edited" => Term::Edited(self.positive(&written, "edited:")?.max(1)),
            "introduced" => Term::Introduced(self.positive(&written, "introduced:")?.max(1)),
            "rated" => {
                let (days, button) = split_once(&written, ':');
                let days = self.positive(days, "rated:")?.max(1);
                let button = button.map(|b| self.button(b, &written)).transpose()?;
                Term::Rated { days, button }
            }
            "prop" => self.property(&written)?,
            "is" => match CardState::ALL.into_iter().find(|s| s.word() == written) {
                Some(state) => Term::State(state),
                None => return Err(self.error(ErrorKind::InvalidState { provided: written })),
            },
            "did" | "nid" | "cid" => {
                if !ID_LIST.is_match(&written) {
                    let context = format!("{typed_key}:");
                    return Err(self.error(ErrorKind::InvalidIdList { context }));
                }
                let kind = match typed_key.to_ascii_lowercase().as_str() {
                    "did" => IdKind::Decks,
                    "nid" => IdKind::Notes,
                    _ => IdKind::Cards,
                };
                Term::Ids { kind, list: written }
            }
            "mid" => match number::<i64>(&written) {
                Some(id) => Term::NotetypeId(NotetypeId(id)),
                None => return Err(self.whole_number(&written, "mid:")),
            },
            "re" => Term::Regex(self.quotes_only(value)),
            "nc" => Term::NoCombining(self.unescaped(value)?),
            "sc" => Term::StripCloze(self.unescaped(value)?),
            "w" => Term::Word(self.unescaped(value)?),
            "dupe" => {
                let (id, text) = split_once(&written, ',');
                let Some(id) = number::<i64>(id) else {
                    return Err(self.whole_number(id, &written));
                };
                if text.is_none() {
                    return Err(self.error(ErrorKind::InvalidDupe));
                }
                let comma = written.chars().take_while(|c| *c != ',').count();
                Term::Duplicates {
                    notetype: NotetypeId(id),
                    text: self.dupe_text(value.start + comma + 1..value.end),
                }
            }
            "has-cd" => Term::HasCustomData(self.unescaped(value)?),
            "preset" => Term::Preset(written),
            // R1032: any other key is a field search.
            _ => {
                let name = self.unescaped(key)?;
                match (prefixed("re:"), prefixed("nc:")) {
                    (Some(rest), _) => Term::Field {
                        name,
                        mode: TextMode::Regex,
                        text: self.quotes_only(rest),
                    },
                    (_, Some(rest)) => Term::Field {
                        name,
                        mode: TextMode::NoCombining,
                        text: self.quotes_only(rest),
                    },
                    _ => Term::Field {
                        name,
                        mode: TextMode::Normal,
                        text: self.unescaped(value)?,
                    },
                }
            }
        })
    }

    fn positive(&self, text: &str, context: &str) -> Result<u32, SearchError> {
        number::<u32>(text).ok_or_else(|| {
            self.error(ErrorKind::InvalidPositiveWholeNumber {
                provided: text.to_owned(),
                context: context.to_owned(),
            })
        })
    }

    fn whole_number(&self, text: &str, context: &str) -> SearchError {
        self.error(ErrorKind::InvalidWholeNumber {
            provided: text.to_owned(),
            context: context.to_owned(),
        })
    }

    /// An answer button: unsigned 8-bit, 1 to 4.
    fn button(&self, text: &str, context: &str) -> Result<u8, SearchError> {
        number::<u8>(text).filter(|b| (1..=4).contains(b)).ok_or_else(|| {
            self.error(ErrorKind::InvalidAnswerButton {
                provided: text.to_owned(),
                context: context.to_owned(),
            })
        })
    }

    /// G8.1.
    fn property(&self, value: &str) -> Result<Term, SearchError> {
        const NAMES: [&str; 11] = [
            "ivl", "due", "reps", "lapses", "ease", "pos", "rated", "resched", "s", "d", "r",
        ];
        let custom = |prefix: &str| {
            let rest = value.strip_prefix(prefix)?;
            let key_len = rest.chars().take_while(char::is_ascii_alphanumeric).count();
            (key_len > 0).then(|| &value[..prefix.len() + key_len])
        };
        let name = NAMES
            .into_iter()
            .find(|n| value.starts_with(n))
            .or_else(|| custom("cdn:"))
            .or_else(|| custom("cds:"))
            .ok_or_else(|| {
                self.error(ErrorKind::InvalidProperty {
                    provided: value.to_owned(),
                })
            })?;
        let rest = &value[name.len()..];
        let comparison = Comparison::BY_LENGTH
            .into_iter()
            .find(|c| rest.starts_with(c.symbol()))
            .ok_or_else(|| {
                self.error(ErrorKind::InvalidOperator {
                    provided: name.to_owned(),
                })
            })?;
        let operand = &rest[comparison.symbol().len()..];
        let operand_error = |kind: fn(String, String) -> ErrorKind, provided: &str| {
            self.error(kind(provided.to_owned(), value.to_owned()))
        };
        let float = || {
            number::<f32>(operand).map(Float).ok_or_else(|| {
                operand_error(
                    |provided, context| ErrorKind::InvalidNumber { provided, context },
                    operand,
                )
            })
        };
        let unsigned = || {
            number::<u32>(operand).ok_or_else(|| {
                operand_error(
                    |provided, context| ErrorKind::InvalidPositiveWholeNumber { provided, context },
                    operand,
                )
            })
        };
        let not_positive = |text: &str| {
            number::<i32>(text).filter(|n| *n <= 0).ok_or_else(|| {
                operand_error(
                    |provided, context| ErrorKind::InvalidNegativeWholeNumber { provided, context },
                    text,
                )
            })
        };
        let property = match name {
            "ivl" => Property::Interval(unsigned()?),
            "due" => Property::Due(number::<i32>(operand).ok_or_else(|| {
                operand_error(
                    |provided, context| ErrorKind::InvalidWholeNumber { provided, context },
                    operand,
                )
            })?),
            "reps" => Property::Reps(unsigned()?),
            "lapses" => Property::Lapses(unsigned()?),
            "ease" => Property::Ease(float()?),
            "pos" => Property::Position(unsigned()?),
            "rated" => {
                let (days, button) = split_once(operand, ':');
                let days = not_positive(days)?;
                let button = button.map(|b| self.button(b, value)).transpose()?;
                Property::Rated { days, button }
            }
            "resched" => Property::Rescheduled(not_positive(operand)?),
            "s" => Property::Stability(float()?),
            "d" => Property::Difficulty(float()?),
            "r" => Property::Retrievability(float()?),
            custom if custom.starts_with("cdn:") => Property::CustomNumber {
                key: custom["cdn:".len()..].to_owned(),
                value: float()?,
            },
            custom => Property::CustomString {
                key: custom["cds:".len()..].to_owned(),
                value: operand.to_owned(),
            },
        };
        Ok(Term::Property { property, comparison })
    }

    /// G7 kind U: the first invalid escape is an error; `\"`, `\:`, `\(`,
    /// `\)` and `\-` become the bare character; `\\`, `\*` and `\_` stay.
    fn unescaped(&self, range: Range<usize>) -> Result<String, SearchError> {
        let chars = self.lexer.chars(range.clone());
        let mut out = String::with_capacity(chars.len());
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            if c != '\\' {
                out.push(c);
                k += 1;
                continue;
            }
            match chars.get(k + 1) {
                Some(&n @ ('"' | ':' | '(' | ')' | '-')) => out.push(n),
                Some(&n @ ('\\' | '*' | '_')) => {
                    out.push('\\');
                    out.push(n);
                }
                next => {
                    let width = if next.is_some() { 2 } else { 1 };
                    let at = range.start + k;
                    let provided = self.lexer.string(at..at + width);
                    return Err(self.lexer.error(ErrorKind::UnknownEscape { provided }, at..at + width));
                }
            }
            k += 2;
        }
        Ok(out)
    }

    /// G7 kind Q: every `\"` becomes `"`; nothing else changes.
    fn quotes_only(&self, range: Range<usize>) -> String {
        unquote(self.lexer.chars(range))
    }

    /// G7 kind D: every `\"` becomes `"`, then every `\\` becomes `\`.
    fn dupe_text(&self, range: Range<usize>) -> String {
        let quoted: Vec<char> = unquote(self.lexer.chars(range)).chars().collect();
        replace_pairs(&quoted, '\\', '\\')
    }
}

fn unquote(chars: &[char]) -> String {
    replace_pairs(chars, '\\', '"')
}

/// Every `first second` pair, read left to right without overlaps, becomes
/// `second`.
fn replace_pairs(chars: &[char], first: char, second: char) -> String {
    let mut out = String::with_capacity(chars.len());
    let mut k = 0;
    while k < chars.len() {
        if chars[k] == first && chars.get(k + 1) == Some(&second) {
            out.push(second);
            k += 2;
        } else {
            out.push(chars[k]);
            k += 1;
        }
    }
    out
}

/// The text before the first `separator`, and the text after it if there is
/// one.
fn split_once(text: &str, separator: char) -> (&str, Option<&str>) {
    match text.split_once(separator) {
        Some((before, after)) => (before, Some(after)),
        None => (text, None),
    }
}
