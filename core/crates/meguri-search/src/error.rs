//! Search errors (`specs/search/errors.md`): the closed list of kinds (E1),
//! the texts each carries (E2), the run errors (E3), ranges (E5) and Meguri's
//! messages (E7).

use std::fmt;
use std::ops::Range;

/// A search that cannot be read or run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchError {
    kind: ErrorKind,
    range: Range<usize>,
}

/// E1: Anki's parse error categories, then the two run errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    MisplacedAnd,
    MisplacedOr,
    EmptyGroup,
    UnopenedGroup,
    UnclosedGroup,
    EmptyQuote,
    UnclosedQuote,
    MissingKey,
    UnknownEscape {
        provided: String,
    },
    InvalidState {
        provided: String,
    },
    InvalidFlag,
    InvalidProperty {
        provided: String,
    },
    InvalidOperator {
        provided: String,
    },
    InvalidNumber {
        provided: String,
        context: String,
    },
    InvalidWholeNumber {
        provided: String,
        context: String,
    },
    InvalidPositiveWholeNumber {
        provided: String,
        context: String,
    },
    InvalidNegativeWholeNumber {
        provided: String,
        context: String,
    },
    InvalidAnswerButton {
        provided: String,
        context: String,
    },
    InvalidIdList {
        context: String,
    },
    InvalidDupe,
    /// Anki's fallback; no input is known to reach it, and Meguri never
    /// raises it (E1).
    Malformed,
    /// Raised by the text-matching unit (U81) while a plan is built; the
    /// pattern and reason are defined there (`text-matching.md`).
    InvalidRegex {
        pattern: String,
        reason: String,
    },
    SearchFailed(SearchFailure),
}

/// E3: why a search that parses cannot run. Each cause has its own message
/// (E7); none is observed (`specs/conformance/search-ops.md` S3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchFailure {
    /// A negated operator word (`-and`, `-or`; `grammar.md` G4).
    NegatedOperator,
    /// An id list with a digit other than ASCII `0`-`9`; `context` is the
    /// key as typed, then `:`.
    IdList { context: String },
    /// The database refused the statement built for the search.
    DatabaseRefusal,
    /// A text pattern exceeds T3.3's byte limit.
    PatternTooLong,
    /// T12.1's compiled expression limit; `pattern` is T16's user text.
    ExpressionTooLarge { pattern: String },
    /// Every note type excludes every field (T7.2).
    NoSearchableField,
    /// An `sc:` term without any cloze note type (T9.1).
    NoClozeNoteType,
    /// A `re:` or `w:` term; T16 selects the first excluded note type.
    AllFieldsExcluded { notetype: String },
    /// A non-finite property operand (P14); the whole term as E7 defines it.
    NumberOutOfRange { term: String },
}

impl ErrorKind {
    /// The kind's name in observations and the schema (`invalid_state`).
    pub fn code(&self) -> &'static str {
        match self {
            ErrorKind::MisplacedAnd => "misplaced_and",
            ErrorKind::MisplacedOr => "misplaced_or",
            ErrorKind::EmptyGroup => "empty_group",
            ErrorKind::UnopenedGroup => "unopened_group",
            ErrorKind::UnclosedGroup => "unclosed_group",
            ErrorKind::EmptyQuote => "empty_quote",
            ErrorKind::UnclosedQuote => "unclosed_quote",
            ErrorKind::MissingKey => "missing_key",
            ErrorKind::UnknownEscape { .. } => "unknown_escape",
            ErrorKind::InvalidState { .. } => "invalid_state",
            ErrorKind::InvalidFlag => "invalid_flag",
            ErrorKind::InvalidProperty { .. } => "invalid_property",
            ErrorKind::InvalidOperator { .. } => "invalid_operator",
            ErrorKind::InvalidNumber { .. } => "invalid_number",
            ErrorKind::InvalidWholeNumber { .. } => "invalid_whole_number",
            ErrorKind::InvalidPositiveWholeNumber { .. } => "invalid_positive_whole_number",
            ErrorKind::InvalidNegativeWholeNumber { .. } => "invalid_negative_whole_number",
            ErrorKind::InvalidAnswerButton { .. } => "invalid_answer_button",
            ErrorKind::InvalidIdList { .. } => "invalid_id_list",
            ErrorKind::InvalidDupe => "invalid_dupe",
            ErrorKind::Malformed => "malformed",
            ErrorKind::InvalidRegex { .. } => "invalid_regex",
            ErrorKind::SearchFailed(_) => "search_failed",
        }
    }

    /// E2's `provided`, as the text holds it (backticks not yet replaced).
    pub fn provided(&self) -> Option<&str> {
        match self {
            ErrorKind::UnknownEscape { provided }
            | ErrorKind::InvalidState { provided }
            | ErrorKind::InvalidProperty { provided }
            | ErrorKind::InvalidOperator { provided }
            | ErrorKind::InvalidNumber { provided, .. }
            | ErrorKind::InvalidWholeNumber { provided, .. }
            | ErrorKind::InvalidPositiveWholeNumber { provided, .. }
            | ErrorKind::InvalidNegativeWholeNumber { provided, .. }
            | ErrorKind::InvalidAnswerButton { provided, .. } => Some(provided),
            _ => None,
        }
    }

    /// E2's `context`. A failed id list keeps its key for the message only.
    pub fn context(&self) -> Option<&str> {
        match self {
            ErrorKind::InvalidNumber { context, .. }
            | ErrorKind::InvalidWholeNumber { context, .. }
            | ErrorKind::InvalidPositiveWholeNumber { context, .. }
            | ErrorKind::InvalidNegativeWholeNumber { context, .. }
            | ErrorKind::InvalidAnswerButton { context, .. }
            | ErrorKind::InvalidIdList { context } => Some(context),
            _ => None,
        }
    }

    /// Raised while the text is read (so also by canonicalization), not when
    /// it runs (E1).
    pub fn is_parse_error(&self) -> bool {
        !matches!(self, ErrorKind::InvalidRegex { .. } | ErrorKind::SearchFailed(_))
    }

    /// E7's text for this kind: the whole message, with no prefix.
    fn message_text(&self) -> String {
        let p = apostrophes;
        match self {
            ErrorKind::MisplacedAnd => {
                r#"`and` has to sit between two search terms. To find the word itself, quote it: `"and"`."#.to_owned()
            }
            ErrorKind::MisplacedOr => {
                r#"`or` has to sit between two search terms. To find the word itself, quote it: `"or"`."#.to_owned()
            }
            ErrorKind::EmptyGroup => {
                r#"The brackets `( )` hold nothing to look for. To find brackets themselves, quote them: `"( )"`."#
                    .to_owned()
            }
            ErrorKind::UnopenedGroup => {
                r"This `)` has no `(` before it. To find the bracket itself, quote it or write `\)`.".to_owned()
            }
            ErrorKind::UnclosedGroup => {
                r"This `(` is never closed by a `)`. To find the bracket itself, quote it or write `\(`.".to_owned()
            }
            ErrorKind::EmptyQuote => {
                r#"The quote marks `""` hold nothing to look for. To find quote marks themselves, write `\"\"`."#
                    .to_owned()
            }
            ErrorKind::UnclosedQuote => {
                r#"This quote mark `"` has no partner to close it. To find the quote mark itself, write `\"`."#
                    .to_owned()
            }
            ErrorKind::MissingKey => {
                r"A colon `:` must follow a keyword or a field name. To find the colon itself, write `\:`.".to_owned()
            }
            ErrorKind::UnknownEscape { provided } => {
                format!(
                    r"`{}` is not a valid escape. To find a backslash itself, write `\\`.",
                    p(provided)
                )
            }
            ErrorKind::InvalidState { provided } => {
                format!("`{}` is not a card state that `is:` knows.", p(provided))
            }
            ErrorKind::InvalidFlag => {
                "`flag:` takes 0 for no flag, or 1 to 7 for red, orange, green, blue, pink, turquoise and purple."
                    .to_owned()
            }
            ErrorKind::InvalidProperty { provided } => format!(
                "`prop:{}` does not begin with a card property that can be compared.",
                p(provided)
            ),
            ErrorKind::InvalidOperator { provided } => format!(
                "Follow `prop:{}` with a comparison: `<`, `<=`, `=`, `!=`, `>=` or `>`.",
                p(provided)
            ),
            ErrorKind::InvalidNumber { provided, context } => not_one(context, "a number", provided),
            ErrorKind::InvalidWholeNumber { provided, context } => not_one(context, "a whole number", provided),
            ErrorKind::InvalidPositiveWholeNumber { provided, context } => {
                not_one(context, "a whole number of zero or more", provided)
            }
            ErrorKind::InvalidNegativeWholeNumber { provided, context } => {
                not_one(context, "a whole number of zero or less", provided)
            }
            ErrorKind::InvalidAnswerButton { provided, context } => {
                not_one(context, "an answer button from 1 to 4", provided)
            }
            ErrorKind::InvalidIdList { context } => format!(
                "`{}` takes ids made of digits, with a single comma between two ids.",
                p(context)
            ),
            ErrorKind::InvalidDupe => "`dupe:` takes a note type id, then a comma, then the text.".to_owned(),
            ErrorKind::Malformed => "This search could not be read. Look for a typing slip.".to_owned(),
            ErrorKind::InvalidRegex { pattern, reason } => {
                format!("`{}` is not a valid regular expression: {}", p(pattern), p(reason))
            }
            ErrorKind::SearchFailed(SearchFailure::NegatedOperator) => {
                r#"A `-` cannot negate `and` or `or`. To leave out the word itself, quote it: `-"and"`."#.to_owned()
            }
            ErrorKind::SearchFailed(SearchFailure::IdList { context }) => {
                format!("`{}` takes ids written with the digits 0 to 9 only.", p(context))
            }
            ErrorKind::SearchFailed(SearchFailure::DatabaseRefusal) => {
                "This search is too large to run. Split it into smaller searches.".to_owned()
            }
            ErrorKind::SearchFailed(SearchFailure::PatternTooLong) => {
                "This search term is too long. Shorten it.".to_owned()
            }
            ErrorKind::SearchFailed(SearchFailure::ExpressionTooLarge { pattern }) => format!(
                "The regular expression `{}` is too large to search with. Make it shorter.",
                p(pattern)
            ),
            ErrorKind::SearchFailed(SearchFailure::NoSearchableField) => {
                "Every note type leaves all its fields out of searches without a field name. Search a named field instead, as in `front:text`.".to_owned()
            }
            ErrorKind::SearchFailed(SearchFailure::NoClozeNoteType) => {
                "`sc:` searches cloze note types, and this collection has none.".to_owned()
            }
            ErrorKind::SearchFailed(SearchFailure::AllFieldsExcluded { notetype }) => format!(
                "The note type `{}` leaves all its fields out of searches without a field name, so `re:` and `w:` cannot run. Search a named field instead, as in `front:re:text`.",
                p(notetype)
            ),
            ErrorKind::SearchFailed(SearchFailure::NumberOutOfRange { term }) => format!(
                "`{}` needs a finite number, and this one is out of range or not a number.",
                p(term)
            ),
        }
    }
}

/// E7's number messages share one sentence: what the `context` needs, and
/// that `provided` is not it.
fn not_one(context: &str, needed: &str, provided: &str) -> String {
    format!(
        "`{}` needs {needed}, and `{}` is not one.",
        apostrophes(context),
        apostrophes(provided)
    )
}

impl SearchError {
    /// `range` is a half-open interval of character offsets into the text as
    /// the caller gave it (E5).
    pub fn new(kind: ErrorKind, range: Range<usize>) -> SearchError {
        SearchError { kind, range }
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// E5: characters (Unicode scalar values) of the caller's text, before
    /// whitespace normalization. The FFI converts them to UTF-16 offsets.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// E2: `provided` when the kind has it, otherwise the characters of
    /// `text` (the search this error came from) that the range covers.
    pub fn offending_text(&self, text: &str) -> String {
        match self.kind.provided() {
            Some(provided) => provided.to_owned(),
            None => text.chars().skip(self.range.start).take(self.range.len()).collect(),
        }
    }

    /// E7: the kind's text, naming the offending text. The browser and the
    /// AnkiConnect server show this string.
    pub fn message(&self) -> String {
        self.kind.message_text()
    }
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for SearchError {}

/// Every backtick (U+0060) replaced by an apostrophe (U+0027): how E2's texts
/// appear in messages (E7) and observations (search-ops.md S3).
pub fn apostrophes(text: &str) -> String {
    text.replace('`', "'")
}
