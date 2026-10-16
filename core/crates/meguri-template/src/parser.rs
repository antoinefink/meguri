//! From template text to nodes (`specs/render/template-syntax.md` T2-T6,
//! T12, T13).
//!
//! The text is split once, left to right, into tags, comments and text. A
//! tag or comment may start only where its closing delimiter occurs later
//! on; the last occurrence of each closer answers that in constant time, and
//! each opener's next occurrence is looked up once and reused until the scan
//! passes it, so the scan stays linear (T13). Nodes stay flat, in source
//! order: sections are an open node, the nodes inside and a close node, so
//! nothing here or in rendering recurses.

use std::ops::Range;

use crate::error::{ErrorKind, TemplateError};

/// T2: the directive that switches a template to `<% %>` tags.
const ALTERNATE_DIRECTIVE: &str = "{{=<% %>=}}";
const COMMENT_OPEN: &str = "<!--";
const COMMENT_CLOSE: &str = "-->";

/// The tag delimiters of one template text (T2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delimiters {
    /// `{{` and `}}`.
    Braces,
    /// `<%` and `%>`, after a leading `{{=<% %>=}}`.
    Alternate,
}

impl Delimiters {
    pub const fn open(self) -> &'static str {
        match self {
            Delimiters::Braces => "{{",
            Delimiters::Alternate => "<%",
        }
    }

    pub const fn close(self) -> &'static str {
        match self {
            Delimiters::Braces => "}}",
            Delimiters::Alternate => "%>",
        }
    }
}

/// One node and its byte range in the template source (T12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// For a tag, its opening delimiter through its closing one; for a
    /// comment, `<!--` through `-->`. The nodes' ranges tile the source.
    pub span: Range<usize>,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// T2: leading white space and the alternate-delimiter directives. It
    /// renders nothing.
    Prefix,
    /// Output unchanged.
    Text,
    /// `<!--…-->`: output unchanged, never evaluated (T3, T7).
    Comment,
    /// T5: `filters` in source order, so the first one written is applied
    /// last. Neither the pieces nor the key are trimmed.
    Replacement { key: String, filters: Vec<String> },
    /// T4: `{{#key}}`, or `{{^key}}` when `negated`.
    Open { key: String, negated: bool },
    /// T4: `{{/key}}`.
    Close { key: String },
}

/// A parsed template: the source, its nodes in order and the first
/// structural error, if any (T6). Parsing accepts any text.
#[derive(Debug, Clone)]
pub struct ParsedTemplate {
    source: String,
    delimiters: Delimiters,
    nodes: Vec<Node>,
    structure_error: Option<TemplateError>,
}

impl ParsedTemplate {
    pub fn parse(source: &str) -> ParsedTemplate {
        let (delimiters, body) = delimiters_of(source);
        let mut nodes = Vec::new();
        if body > 0 {
            nodes.push(Node {
                span: 0..body,
                kind: NodeKind::Prefix,
            });
        }
        tokenize(source, body, delimiters, &mut nodes);
        let structure_error = check_structure(&nodes);
        ParsedTemplate {
            source: source.to_owned(),
            delimiters,
            nodes,
            structure_error,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn delimiters(&self) -> Delimiters {
        self.delimiters
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The source text of a node of this template.
    pub fn text_of(&self, node: &Node) -> &str {
        &self.source[node.span.clone()]
    }

    /// The first structural error in source order (T6): an unopened or
    /// mismatched close, or else the innermost section left open at the end.
    pub fn structure_error(&self) -> Option<&TemplateError> {
        self.structure_error.as_ref()
    }
}

/// T2: the delimiters, and where the body starts after the prefix.
fn delimiters_of(source: &str) -> (Delimiters, usize) {
    let mut rest = source.trim_start();
    if !rest.starts_with(ALTERNATE_DIRECTIVE) {
        return (Delimiters::Braces, 0);
    }
    while let Some(after) = rest.strip_prefix(ALTERNATE_DIRECTIVE) {
        rest = after;
    }
    (Delimiters::Alternate, source.len() - rest.len())
}

/// The next occurrence of a needle at or after a position that only moves
/// forward. A found occurrence is kept until the position passes it, and
/// "none" stays none, so the text is searched about once overall.
struct NextOccurrence<'a> {
    haystack: &'a str,
    needle: &'static str,
    found: Option<Option<usize>>,
}

impl<'a> NextOccurrence<'a> {
    fn new(haystack: &'a str, needle: &'static str) -> NextOccurrence<'a> {
        NextOccurrence {
            haystack,
            needle,
            found: None,
        }
    }

    fn at_or_after(&mut self, from: usize) -> Option<usize> {
        match self.found {
            Some(None) => return None,
            Some(Some(at)) if at >= from => return Some(at),
            _ => {}
        }
        let found = self.haystack[from..].find(self.needle).map(|i| from + i);
        self.found = Some(found);
        found
    }
}

/// T3 over `source[body..]`, appending text, comment and tag nodes.
fn tokenize(source: &str, body: usize, delimiters: Delimiters, nodes: &mut Vec<Node>) {
    let (open, close) = (delimiters.open(), delimiters.close());
    // A tag (comment) can start at `p` only if its closer occurs at or after
    // the end of its opener; the closer's last occurrence decides that.
    let last_close = source.rfind(close);
    let last_comment_close = source.rfind(COMMENT_CLOSE);
    let closes_after = |last: Option<usize>, from: usize| last.is_some_and(|last| last >= from);
    let mut opens = NextOccurrence::new(source, open);
    let mut comments = NextOccurrence::new(source, COMMENT_OPEN);
    let mut at = body;
    while at < source.len() {
        let tag = opens
            .at_or_after(at)
            .filter(|&p| closes_after(last_close, p + open.len()));
        let comment = comments
            .at_or_after(at)
            .filter(|&p| closes_after(last_comment_close, p + COMMENT_OPEN.len()));
        // Openers never share a first byte, so starts never tie.
        let (start, opener, closer, is_tag) = match (tag, comment) {
            (Some(t), Some(c)) if c < t => (c, COMMENT_OPEN, COMMENT_CLOSE, false),
            (Some(t), _) => (t, open, close, true),
            (None, Some(c)) => (c, COMMENT_OPEN, COMMENT_CLOSE, false),
            (None, None) => break,
        };
        let inner = start + opener.len();
        let Some(closer_at) = source[inner..].find(closer).map(|i| inner + i) else {
            break;
        };
        if start > at {
            nodes.push(Node {
                span: at..start,
                kind: NodeKind::Text,
            });
        }
        let end = closer_at + closer.len();
        let kind = if is_tag {
            classify(&source[inner..closer_at])
        } else {
            NodeKind::Comment
        };
        nodes.push(Node { span: start..end, kind });
        at = end;
    }
    if at < source.len() {
        nodes.push(Node {
            span: at..source.len(),
            kind: NodeKind::Text,
        });
    }
}

/// T4: the kind of a tag from its content.
fn classify(content: &str) -> NodeKind {
    let name = content.trim_start_matches('{').trim();
    if name.len() >= 2 {
        let key = || name[1..].trim_start().to_owned();
        match name.as_bytes()[0] {
            b'#' => {
                return NodeKind::Open {
                    key: key(),
                    negated: false,
                };
            }
            b'^' => {
                return NodeKind::Open {
                    key: key(),
                    negated: true,
                };
            }
            b'/' => return NodeKind::Close { key: key() },
            _ => {}
        }
    }
    // T5: the last piece is the key, the ones before it filter names.
    let mut pieces: Vec<String> = name.split(':').map(str::to_owned).collect();
    let key = pieces.pop().unwrap_or_default();
    NodeKind::Replacement { key, filters: pieces }
}

/// T6: the first structural error, reading from the start.
fn check_structure(nodes: &[Node]) -> Option<TemplateError> {
    let mut open: Vec<&Node> = Vec::new();
    for node in nodes {
        let NodeKind::Close { key } = &node.kind else {
            if matches!(node.kind, NodeKind::Open { .. }) {
                open.push(node);
            }
            continue;
        };
        match open.last() {
            None => return Some(error(ErrorKind::UnopenedConditional, node, None)),
            Some(innermost) if open_key(innermost) != key => {
                return Some(error(ErrorKind::MismatchedConditional, node, Some(innermost)));
            }
            Some(_) => {
                open.pop();
            }
        }
    }
    open.last()
        .map(|innermost| error(ErrorKind::UnclosedConditional, innermost, None))
}

fn open_key(node: &Node) -> &str {
    match &node.kind {
        NodeKind::Open { key, .. } => key,
        _ => "",
    }
}

/// An error naming `node`'s tag, and for a mismatch the open section.
pub(crate) fn error(kind: ErrorKind, node: &Node, open: Option<&Node>) -> TemplateError {
    TemplateError {
        kind,
        span: node.span.clone(),
        tag: describe(&node.kind),
        open: open.map(|o| describe(&o.kind)),
    }
}

/// A tag as `{{…}}`, whatever the template's delimiters (T2).
fn describe(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Replacement { key, filters } => {
            let mut name = String::new();
            for filter in filters {
                name.push_str(filter);
                name.push(':');
            }
            name.push_str(key);
            format!("{{{{{name}}}}}")
        }
        NodeKind::Open { key, negated } => format!("{{{{{}{key}}}}}", if *negated { '^' } else { '#' }),
        NodeKind::Close { key } => format!("{{{{/{key}}}}}"),
        NodeKind::Prefix | NodeKind::Text | NodeKind::Comment => String::new(),
    }
}
