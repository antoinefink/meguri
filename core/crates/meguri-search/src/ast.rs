//! The parsed query (`specs/search/query-model.md` Q1, Q3): AND, OR, NOT and
//! groups over typed terms, kept in an arena so that no walk, drop or clone
//! recurses on the input's nesting depth (`errors.md` E6).

use std::fmt;
use std::ops::Range;

use meguri_model::ids::NotetypeId;

/// A parsed (or built) search.
#[derive(Clone, Debug)]
pub struct Query {
    nodes: Vec<Node>,
    root: NodeId,
    /// The text it was read from, as the caller gave it; `None` for a built
    /// query.
    source: Option<String>,
}

/// A node's index in its [`Query`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(usize);

impl NodeId {
    pub fn index(self) -> usize {
        self.0
    }
}

/// One node and the characters of the source it was read from (offsets into
/// the caller's text, as error ranges, `errors.md` E5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// Two or more items joined by AND, implicit or explicit.
    And(Vec<NodeId>),
    /// Two or more runs joined by OR.
    Or(Vec<NodeId>),
    /// `-` before one item.
    Not(NodeId),
    /// A bracketed sequence, kept as written (also a group of one item).
    Group(NodeId),
    Term(Term),
    /// `-and` or `-or` (`grammar.md` G4): it has a canonical text but never
    /// runs (`errors.md` E3).
    NegatedOperator(BoolOperator),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoolOperator {
    And,
    Or,
}

/// A 32-bit float operand (`grammar.md` G9) compared bit for bit, so that two
/// readings of `nan` are the same term.
#[derive(Clone, Copy, Debug)]
pub struct Float(pub f32);

impl PartialEq for Float {
    fn eq(&self, other: &Float) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Float {}

impl fmt::Display for Float {
    /// `canonical-form.md` F6: Rust's `Display` for `f32`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// How a field or tag term matches its text (`grammar.md` G8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextMode {
    Normal,
    /// `re:` prefix.
    Regex,
    /// `nc:` prefix.
    NoCombining,
}

/// `is:` values (G8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardState {
    New,
    Review,
    Learn,
    Due,
    Buried,
    BuriedManually,
    BuriedSibling,
    Suspended,
}

impl CardState {
    pub const ALL: [CardState; 8] = [
        CardState::New,
        CardState::Review,
        CardState::Learn,
        CardState::Due,
        CardState::Buried,
        CardState::BuriedManually,
        CardState::BuriedSibling,
        CardState::Suspended,
    ];

    /// The word `is:` takes, exactly as written (R1031).
    pub fn word(self) -> &'static str {
        match self {
            CardState::New => "new",
            CardState::Review => "review",
            CardState::Learn => "learn",
            CardState::Due => "due",
            CardState::Buried => "buried",
            CardState::BuriedManually => "buried-manually",
            CardState::BuriedSibling => "buried-sibling",
            CardState::Suspended => "suspended",
        }
    }
}

/// The table an id list names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IdKind {
    /// `did:`
    Decks,
    /// `nid:`
    Notes,
    /// `cid:`
    Cards,
}

impl IdKind {
    pub fn key(self) -> &'static str {
        match self {
            IdKind::Decks => "did",
            IdKind::Notes => "nid",
            IdKind::Cards => "cid",
        }
    }
}

/// `card:` (G8).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TemplateRef {
    /// 0-based: `card:0` and `card:1` are both 0.
    Ordinal(u16),
    /// A name pattern, U-read.
    Name(String),
}

/// `prop:` comparisons (G8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Comparison {
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Equal,
    NotEqual,
}

impl Comparison {
    /// G8.1 step 2: tried in this order, so `<=` wins over `<`.
    pub const BY_LENGTH: [Comparison; 6] = [
        Comparison::LessOrEqual,
        Comparison::GreaterOrEqual,
        Comparison::NotEqual,
        Comparison::Equal,
        Comparison::Less,
        Comparison::Greater,
    ];

    pub fn symbol(self) -> &'static str {
        match self {
            Comparison::Less => "<",
            Comparison::LessOrEqual => "<=",
            Comparison::Greater => ">",
            Comparison::GreaterOrEqual => ">=",
            Comparison::Equal => "=",
            Comparison::NotEqual => "!=",
        }
    }
}

/// A `prop:` property with its typed operand (G8.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Property {
    Interval(u32),
    Due(i32),
    Reps(u32),
    Lapses(u32),
    Ease(Float),
    Position(u32),
    /// Days at most 0, then an optional answer button 1-4.
    Rated {
        days: i32,
        button: Option<u8>,
    },
    /// At most 0.
    Rescheduled(i32),
    Stability(Float),
    Difficulty(Float),
    Retrievability(Float),
    /// `cdn:<key>`: a number in the card's custom data.
    CustomNumber {
        key: String,
        value: Float,
    },
    /// `cds:<key>`: a string in the card's custom data, R-read.
    CustomString {
        key: String,
        value: String,
    },
}

/// One search term with the data `grammar.md` G8 reads from it (Q3). Texts
/// are stored as their reading kind (U, Q, D or R) leaves them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    /// The empty search (Q2).
    WholeCollection,
    /// Plain text, U-read.
    Text(String),
    /// `<field>:<text>`: the field name U-read, the text by mode.
    Field { name: String, mode: TextMode, text: String },
    /// `re:`, Q-read.
    Regex(String),
    /// `nc:`, U-read.
    NoCombining(String),
    /// `sc:`, U-read.
    StripCloze(String),
    /// `w:`, U-read.
    Word(String),
    /// `tag:`: regex Q-read, nc and normal U-read.
    Tag { mode: TextMode, text: String },
    /// `note:`
    Notetype(String),
    /// `mid:`
    NotetypeId(NotetypeId),
    /// `edited:`, days (1 or more).
    Edited(u32),
    /// `dupe:<id>,<text>`, the text D-read.
    Duplicates { notetype: NotetypeId, text: String },
    /// `deck:`
    Deck(String),
    /// `preset:`, R-read.
    Preset(String),
    /// `did:`, `nid:`, `cid:`: the list as written.
    Ids { kind: IdKind, list: String },
    /// `is:`
    State(CardState),
    /// `flag:` 0-7.
    Flag(u8),
    /// `added:`, days (1 or more).
    Added(u32),
    /// `introduced:`, days (1 or more).
    Introduced(u32),
    /// `rated:`, days (1 or more) and an optional button 1-4.
    Rated { days: u32, button: Option<u8> },
    /// `resched:`, days (0 or more).
    Rescheduled(u32),
    /// `prop:`
    Property { property: Property, comparison: Comparison },
    /// `has-cd:`, U-read.
    HasCustomData(String),
    /// `card:`
    Template(TemplateRef),
}

/// Which rows a term reads (Q5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Only what both rows hold (the note id) or nothing.
    Either,
    /// Card columns only.
    Card,
    /// Note columns only.
    Note,
    /// Both.
    CardAndNote,
}

impl Scope {
    /// Q5: "either" adds nothing; card with note gives card and note.
    #[must_use]
    pub fn combine(self, other: Scope) -> Scope {
        match (self, other) {
            (Scope::Either, s) | (s, Scope::Either) => s,
            (a, b) if a == b => a,
            _ => Scope::CardAndNote,
        }
    }
}

impl Term {
    /// Q3's scope column.
    pub fn scope(&self) -> Scope {
        match self {
            Term::WholeCollection
            | Term::Ids {
                kind: IdKind::Notes, ..
            } => Scope::Either,
            Term::Text(_)
            | Term::Field { .. }
            | Term::Regex(_)
            | Term::NoCombining(_)
            | Term::StripCloze(_)
            | Term::Word(_)
            | Term::Tag { .. }
            | Term::Notetype(_)
            | Term::NotetypeId(_)
            | Term::Edited(_)
            | Term::Duplicates { .. } => Scope::Note,
            Term::Deck(_)
            | Term::Preset(_)
            | Term::Ids { .. }
            | Term::State(_)
            | Term::Flag(_)
            | Term::Added(_)
            | Term::Introduced(_)
            | Term::Rated { .. }
            | Term::Rescheduled(_)
            | Term::Property { .. }
            | Term::HasCustomData(_) => Scope::Card,
            Term::Template(_) => Scope::CardAndNote,
        }
    }
}

impl Query {
    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The text the query was read from, as the caller gave it.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// The characters of the source a node spans; `None` for a built query.
    pub fn source_text(&self, span: &Range<usize>) -> Option<String> {
        let source = self.source.as_deref()?;
        Some(source.chars().skip(span.start).take(span.len()).collect())
    }

    /// The node ids in pre-order, children left to right: the order in which
    /// the source names them.
    pub fn pre_order(&self) -> Vec<NodeId> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            out.push(id);
            match &self.node(id).kind {
                NodeKind::And(children) | NodeKind::Or(children) => stack.extend(children.iter().rev()),
                NodeKind::Not(child) | NodeKind::Group(child) => stack.push(*child),
                NodeKind::Term(_) | NodeKind::NegatedOperator(_) => {}
            }
        }
        out
    }
}

/// Builds a [`Query`] bottom-up: children before their parent.
#[derive(Debug, Default)]
pub(crate) struct QueryBuilder {
    nodes: Vec<Node>,
}

impl QueryBuilder {
    pub(crate) fn push(&mut self, kind: NodeKind, span: Range<usize>) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node { kind, span });
        id
    }

    pub(crate) fn span(&self, id: NodeId) -> Range<usize> {
        self.nodes[id.index()].span.clone()
    }

    pub(crate) fn finish(self, root: NodeId, source: Option<String>) -> Query {
        Query {
            nodes: self.nodes,
            root,
            source,
        }
    }
}
