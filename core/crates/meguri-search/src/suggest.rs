//! Suggestions for the term at the cursor (`specs/search/editing-services.md`
//! D6): Browse shows them while Anki syntax is typed at the caret
//! (`search/filter-tokens` T12.2). The call reads the name index it is
//! handed (D6.7) and its arguments only; offsets are characters of the text
//! as the caller gave it (D1).

use std::collections::HashSet;
use std::ops::Range;

use meguri_text::fold::{compare_folded, folded};
use meguri_text::normalize::nfc;

use crate::ast::{NodeKind, Term};
use crate::builder::{BuiltItem, build, escape_name, search_field_text};
use crate::canonical::{canonical_text, quoted};
use crate::filter_tokens::{DeckValue, NotetypeValue, Value};
use crate::fold::simple_fold;
use crate::lexer::{Form, Lexer};
use crate::name_index::{NameIndex, NotetypeNames};
use crate::parser::parse;
use crate::pattern;
use crate::tail::InvalidOffset;

/// R1104: the most candidates a call returns.
pub const MOST: usize = 50;

/// D6.1's kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    AsTyped,
    Keyword,
    Deck,
    Tag,
    Notetype,
    Template,
    Field,
    Flag,
    State,
    Property,
    Operator,
    ValueHint,
    Days,
    Button,
    Preset,
    SavedSearch,
}

/// A property's unit (D6.3.1 step 3); Swift writes the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    /// `ivl`, `s`.
    Days,
    /// `due`: days from today, negative when overdue.
    DaysFromToday,
    /// `reps`, `lapses`.
    Count,
    /// `ease`: an ease factor such as 2.5.
    EaseFactor,
    /// `pos`.
    Position,
    /// `rated`, `resched`: days ago, 0 or less.
    DaysAgo,
    /// `d`, `r`: 0 to 1.
    ZeroToOne,
    /// `cdn:`.
    Number,
    /// `cds:`.
    Text,
}

/// D6.1's `detail`, by kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detail {
    /// A filtered deck.
    Filtered,
    /// A flag's shown name.
    FlagName(String),
    Unit(Unit),
    Days(u32),
    Button(u8),
}

/// D6.6: what accepting a candidate puts in the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insertion {
    /// Replaced by `text`.
    pub range: Range<usize>,
    pub text: String,
    /// The cursor after the insertion.
    pub cursor_after: usize,
}

/// One candidate (D6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub kind: CandidateKind,
    /// The text to show: a name as stored, with `::` between levels.
    pub label: String,
    pub detail: Option<Detail>,
    /// Absent for `as_typed` and `value_hint`.
    pub insert: Option<Insertion>,
    /// Accepting it commits the search (D6.6).
    pub completes: bool,
}

/// The keywords of D6.3, in order.
const KEYWORDS: [&str; 23] = [
    "deck:",
    "tag:",
    "note:",
    "card:",
    "flag:",
    "is:",
    "prop:",
    "preset:",
    "added:",
    "edited:",
    "rated:",
    "introduced:",
    "resched:",
    "dupe:",
    "nid:",
    "cid:",
    "did:",
    "mid:",
    "re:",
    "nc:",
    "sc:",
    "w:",
    "has-cd:",
];

/// `grammar.md` G8's state words, in its order.
const STATES: [&str; 8] = [
    "new",
    "review",
    "learn",
    "due",
    "buried",
    "buried-manually",
    "buried-sibling",
    "suspended",
];

/// G8.1's properties, in order.
const PROPERTIES: [&str; 13] = [
    "ivl", "due", "reps", "lapses", "ease", "pos", "rated", "resched", "s", "d", "r", "cdn:", "cds:",
];

/// D6.3.1's operators, in order.
const OPERATORS: [&str; 6] = ["<", "<=", ">", ">=", "=", "!="];

/// D6.3's day counts.
const DAYS: [u32; 4] = [1, 7, 30, 365];

/// The term at the cursor (D6.2).
#[derive(Debug)]
enum Current {
    /// No term holds the cursor or ends at it.
    Empty,
    /// G3 cannot read the term at the cursor: no candidates.
    Unreadable,
    Term(At),
}

/// A current term: its range in the caller's characters (without a
/// negating `-`) and its typed part.
#[derive(Debug)]
struct At {
    range: Range<usize>,
    /// From its start to the cursor, without an opening `"`, `\"` read as
    /// `"`.
    typed: String,
    /// Partly quoted: the value is inside the quotes.
    partly_quoted: bool,
}

/// One item of the scan, for D6.4's scope.
#[derive(Debug)]
struct Scanned {
    current: Current,
    /// The note types D6.4 scopes to; `None` when no term names any.
    scope: Option<Vec<usize>>,
}

/// D6.2: whether the term at the caret has a key before a colon, and that
/// key with ASCII letters lowered (`search/filter-tokens` T12.2).
pub fn key_at(text: &str, cursor: usize) -> Option<String> {
    let text = search_field_text(text);
    if cursor > text.chars().count() {
        return None;
    }
    let Current::Term(at) = scan(&text, cursor, None).current else {
        return None;
    };
    split_key(&at.typed).map(|(key, _)| key.to_ascii_lowercase())
}

/// D6.1: the candidates for `text` with the cursor at character `cursor`,
/// at most `limit` (50 at most), ranked with `recent` (insert texts the user
/// accepted, newest first).
pub fn suggest(
    text: &str,
    cursor: usize,
    limit: usize,
    recent: &[String],
    names: &NameIndex,
) -> Result<Vec<Candidate>, InvalidOffset> {
    let length = text.chars().count();
    if cursor > length {
        return Err(InvalidOffset);
    }
    let limit = limit.min(MOST);
    if limit == 0 {
        return Ok(Vec::new());
    }
    let text = search_field_text(text);
    let mut out = Ranked {
        recent_names: recent_names(recent),
        ..Ranked::default()
    };
    if text.chars().all(crate::lexer::is_white_space) {
        // The empty field: every saved search, replacing the whole text.
        for saved in names.saved_searches() {
            if let Some(c) = saved_text(&saved.text) {
                out.name(
                    Class::Prefix,
                    0,
                    candidate(CandidateKind::SavedSearch, &saved.name, None, 0..length, c, true),
                );
            }
        }
        return Ok(out.finish(recent, limit));
    }
    let scanned = scan(&text, cursor, Some(names));
    let scope: Vec<&NotetypeNames> = match &scanned.scope {
        None => names.notetypes().iter().collect(),
        Some(found) => found.iter().map(|k| &names.notetypes()[*k]).collect(),
    };
    let at = match scanned.current {
        Current::Unreadable => return Ok(Vec::new()),
        Current::Empty => {
            let here = cursor..cursor;
            for keyword in KEYWORDS {
                out.fixed(candidate(CandidateKind::Keyword, keyword, None, here.clone(), keyword.to_owned(), false));
            }
            saved_searches(&mut out, names, &Matcher::new(""), here);
            return Ok(out.finish(recent, limit));
        }
        Current::Term(at) => at,
    };
    out.as_typed = Some(Candidate {
        kind: CandidateKind::AsTyped,
        label: at.typed.clone(),
        detail: None,
        insert: None,
        completes: true,
    });
    let range = at.range.clone();
    let Some((key, value)) = split_key(&at.typed) else {
        // A word W.
        let word = exact(&at.typed);
        let matcher = Matcher::new(&word);
        for keyword in KEYWORDS {
            if starts_with_ascii_folded(keyword, &at.typed) {
                out.fixed(candidate(CandidateKind::Keyword, keyword, None, range.clone(), keyword.to_owned(), false));
            }
        }
        for (field, _) in field_names(&scope) {
            if let Some(class) = matcher.substring(&field)
                && let Some(insert) = field_key_text(&field)
            {
                out.name(class, 0, candidate(CandidateKind::Field, &field, None, range.clone(), insert, false));
            }
        }
        saved_searches(&mut out, names, &matcher, range);
        return Ok(out.finish(recent, limit));
    };
    let mut value = value;
    if at.partly_quoted {
        value = value.strip_prefix('"').unwrap_or(&value).to_owned();
    }
    let reserved = key.to_ascii_lowercase();
    let v = exact(&value);
    let matcher = Matcher::new(&v);
    match reserved.as_str() {
        "deck" => decks(&mut out, names, &value, &matcher, range),
        "tag" if value.starts_with("re:") || value.starts_with("nc:") => {}
        "tag" => tags(&mut out, names, &value, &matcher, range),
        "note" => {
            for notetype in names.notetypes() {
                let Some(class) = matcher.substring(&notetype.name) else { continue };
                let value = Value::Notetype(NotetypeValue::of(notetype));
                if let Some(insert) = value.item().and_then(|item| build(&item).ok()) {
                    out.name(
                        class,
                        0,
                        candidate(CandidateKind::Notetype, &notetype.name, None, range.clone(), insert, true),
                    );
                }
            }
        }
        "card" => {
            let most = scope.iter().map(|n| n.templates.len()).max().unwrap_or(0);
            for k in 1..=most {
                let number = k.to_string();
                if number.starts_with(value.as_str()) {
                    out.fixed(candidate(
                        CandidateKind::Template,
                        &number,
                        None,
                        range.clone(),
                        format!("card:{number}"),
                        true,
                    ));
                }
            }
            for (name, _) in template_names(&scope) {
                if let Some(class) = matcher.substring(&name)
                    && let Some(insert) = template_text(&name)
                {
                    out.name(class, 0, candidate(CandidateKind::Template, &name, None, range.clone(), insert, true));
                }
            }
        }
        "flag" => {
            for flag in 0..=7u8 {
                let number = flag.to_string();
                let shown = (flag > 0).then(|| names.flag_names()[usize::from(flag) - 1].clone());
                let by_name = shown.as_deref().is_some_and(|shown| matcher.substring(shown).is_some());
                if number.starts_with(value.as_str()) || by_name {
                    out.fixed(candidate(
                        CandidateKind::Flag,
                        &number,
                        shown.map(Detail::FlagName),
                        range.clone(),
                        format!("flag:{number}"),
                        true,
                    ));
                }
            }
        }
        "is" => {
            for state in STATES {
                if starts_with_ascii_folded(state, &value) {
                    out.fixed(candidate(CandidateKind::State, state, None, range.clone(), format!("is:{state}"), true));
                }
            }
        }
        "prop" => properties(&mut out, &value, range),
        "added" | "edited" | "introduced" | "resched" => days(&mut out, &reserved, &value, range),
        "rated" => match value.split_once(':') {
            None => days(&mut out, "rated", &value, range),
            // G8: the days must read as an unsigned 32-bit number before a
            // button can follow them.
            Some((typed_days, button)) if typed_days.parse::<u32>().is_ok() => {
                for b in 1..=4u8 {
                    let digit = b.to_string();
                    if digit.starts_with(button) {
                        out.fixed(candidate(
                            CandidateKind::Button,
                            &digit,
                            Some(Detail::Button(b)),
                            range.clone(),
                            format!("rated:{typed_days}:{digit}"),
                            true,
                        ));
                    }
                }
            }
            Some(_) => {}
        },
        "preset" => {
            for preset in names.presets() {
                let Some(class) = matcher.substring(&preset.name) else { continue };
                let insert = quoted(&format!("preset:{}", preset.name));
                if reads_as_preset(&insert, &preset.name) {
                    out.name(class, 0, candidate(CandidateKind::Preset, &preset.name, None, range.clone(), insert, true));
                }
            }
        }
        "did" | "nid" | "cid" | "mid" | "dupe" | "re" | "nc" | "sc" | "w" | "has-cd" => {}
        _ => {
            // A field key: read like a word, then compared with the scoped
            // note types' fields with letter case folded.
            let field = exact(&key);
            if let Some((stored, _)) = field_names(&scope)
                .into_iter()
                .find(|(name, _)| compare_folded(name, &field).is_eq())
                && let Some(present) = build(&BuiltItem::FieldPresent(stored.clone())).ok()
            {
                let name = field_name_text(&stored);
                let forms = [
                    ("_*", present, true),
                    ("", quoted(&format!("{name}:")), true),
                    ("re:", quoted(&format!("{name}:re:")), false),
                    ("nc:", quoted(&format!("{name}:nc:")), false),
                ];
                for (part, insert, completes) in forms {
                    if part.starts_with(v.as_str()) {
                        let label = format!("{stored}:{part}");
                        out.fixed(candidate(CandidateKind::Field, &label, None, range.clone(), insert, completes));
                    }
                }
            }
        }
    }
    Ok(out.finish(recent, limit))
}

/// D6.6's candidate with its insertion.
fn candidate(
    kind: CandidateKind,
    label: &str,
    detail: Option<Detail>,
    range: Range<usize>,
    text: String,
    completes: bool,
) -> Candidate {
    let mut cursor_after = range.start + text.chars().count();
    if !completes && text.ends_with('"') {
        cursor_after -= 1;
    }
    Candidate {
        kind,
        label: label.to_owned(),
        detail,
        insert: Some(Insertion {
            range,
            text,
            cursor_after,
        }),
        completes,
    }
}

/// D6.4's two classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Class {
    Prefix,
    Inner,
}

/// Candidates in D6.5's groups.
#[derive(Default)]
struct Ranked<'n> {
    as_typed: Option<Candidate>,
    fixed: Vec<Candidate>,
    /// Name candidates with their class and source (field keys 0, saved
    /// searches 1), in each source's name order; each made only when the
    /// list needs it (a deck or tag's insert text is built then).
    names: Vec<(Class, u8, Lazy<'n>)>,
    /// What the recent texts name ([`recent_names`]).
    recent_names: HashSet<String>,
}

/// A name candidate, or what makes it (a deck gives its `Name::*` too),
/// with the names it would show, folded.
enum Lazy<'n> {
    Ready(Candidate),
    Make {
        names: Vec<String>,
        make: Box<dyn FnOnce() -> Vec<Candidate> + 'n>,
    },
}

impl Lazy<'_> {
    fn make(self) -> Vec<Candidate> {
        match self {
            Lazy::Ready(candidate) => vec![candidate],
            Lazy::Make { make, .. } => make(),
        }
    }

    /// Whether a recent text can be one of its insert texts: a ready one
    /// always may, a deck or tag only when it names it.
    fn may_be_recent(&self, recent: &HashSet<String>) -> bool {
        match self {
            Lazy::Ready(_) => true,
            Lazy::Make { names, .. } => names.iter().any(|name| recent.contains(name)),
        }
    }
}

impl<'n> Ranked<'n> {
    fn fixed(&mut self, candidate: Candidate) {
        self.fixed.push(candidate);
    }

    fn name(&mut self, class: Class, source: u8, candidate: Candidate) {
        self.names.push((class, source, Lazy::Ready(candidate)));
    }

    fn lazy(&mut self, class: Class, source: u8, names: Vec<String>, make: impl FnOnce() -> Vec<Candidate> + 'n) {
        self.names.push((
            class,
            source,
            Lazy::Make {
                names,
                make: Box::new(make),
            },
        ));
    }

    /// D6.5: `as_typed`, the fixed candidates, the prefix class, the inner
    /// class; recent ones first inside each group; cut at `limit`. A deck or
    /// tag past the limit is made only when a recent text names it.
    fn finish(self, recent: &[String], limit: usize) -> Vec<Candidate> {
        let recent_names = &self.recent_names;
        let rank = |candidate: &Candidate| {
            candidate
                .insert
                .as_ref()
                .and_then(|insert| recent.iter().position(|text| *text == insert.text))
                .unwrap_or(usize::MAX)
        };
        let mut out: Vec<Candidate> = self.as_typed.into_iter().collect();
        let mut fixed = self.fixed;
        // A stable sort keeps the others' order.
        fixed.sort_by_key(|candidate| rank(candidate));
        out.extend(fixed);
        let mut names = self.names;
        names.sort_by_key(|(class, source, _)| (*class, *source));
        let mut names = names.into_iter().peekable();
        for class in [Class::Prefix, Class::Inner] {
            let mut group: Vec<Candidate> = Vec::new();
            while let Some((_, _, lazy)) = names.next_if(|(c, _, _)| *c == class) {
                if out.len() + group.len() >= limit && !lazy.may_be_recent(recent_names) {
                    continue;
                }
                group.extend(lazy.make());
            }
            group.sort_by_key(|candidate| rank(candidate));
            out.extend(group);
        }
        out.truncate(limit);
        out
    }
}

/// The deck and tag names recent insert texts name, folded: a recent text
/// that reads as one `deck:` or `tag:` term in normal mode gives its exact
/// text; a `did:` list gives [`BY_IDS`], which every deck answers to.
const BY_IDS: &str = "\u{0}did";
/// What a `tag:re:` text gives: J3.2 writes a tag holding other White_Space
/// that way.
const BY_REGEX: &str = "\u{0}re";

fn recent_names(recent: &[String]) -> HashSet<String> {
    let mut names = HashSet::new();
    for text in recent {
        let Ok(query) = parse(text) else { continue };
        let value = match &query.node(query.root()).kind {
            NodeKind::Term(Term::Ids {
                kind: crate::ast::IdKind::Decks,
                ..
            }) => {
                names.insert(BY_IDS.to_owned());
                continue;
            }
            NodeKind::Term(Term::Deck(value)) => value,
            NodeKind::Term(Term::Tag {
                mode: crate::ast::TextMode::Normal,
                text,
            }) => text,
            NodeKind::Term(Term::Tag {
                mode: crate::ast::TextMode::Regex,
                ..
            }) => {
                names.insert(BY_REGEX.to_owned());
                continue;
            }
            _ => continue,
        };
        names.insert(simple_fold(&pattern::exact_text(value)));
    }
    names
}

/// A word's matcher (D6.4; `search/filter-tokens` T12.2): names by
/// component (decks, tags; `tags/complete` C2, C3 with C5's colon removal)
/// or by substring, both on simple foldings. In the prefix class when the
/// word starts the name (or every filter starts its own component).
pub(crate) struct Matcher {
    /// The word folded, for substrings.
    word: String,
    /// The word's filters (C2) after C5's colon removal, folded.
    filters: Vec<String>,
}

impl Matcher {
    pub(crate) fn new(word: &str) -> Matcher {
        let trimmed = trim_single_colons(word);
        Matcher {
            word: simple_fold(word),
            filters: trimmed.split("::").map(simple_fold).collect(),
        }
    }

    pub(crate) fn substring(&self, name: &str) -> Option<Class> {
        match find_folded(name, &self.word)? {
            0 => Some(Class::Prefix),
            _ => Some(Class::Inner),
        }
    }

    /// C3 over `components`: each filter takes the first component after
    /// the previous one's that contains it.
    pub(crate) fn components<'n>(&self, components: impl Iterator<Item = &'n str>) -> Option<Class> {
        let mut parts = components.enumerate();
        let mut prefix = true;
        for (k, filter) in self.filters.iter().enumerate() {
            let (index, offset) = parts.find_map(|(index, part)| find_folded(part, filter).map(|at| (index, at)))?;
            prefix &= index == k && offset == 0;
        }
        Some(if prefix { Class::Prefix } else { Class::Inner })
    }
}

/// Where `folded` (a simple folding) occurs in `text` simply folded: 0 when
/// at its start. ASCII text is searched in place, without a folded copy;
/// such text holds no folding of a character beyond ASCII.
fn find_folded(text: &str, folded: &str) -> Option<usize> {
    if !text.is_ascii() {
        return simple_fold(text).find(folded);
    }
    if !folded.is_ascii() {
        return None;
    }
    let (text, folded) = (text.as_bytes(), folded.as_bytes());
    if folded.is_empty() {
        return Some(0);
    }
    text.windows(folded.len())
        .position(|window| window.iter().zip(folded).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

/// C5: one leading `:` when the text begins with a single colon, one
/// trailing `:` when it ends with a single colon.
fn trim_single_colons(text: &str) -> &str {
    let mut text = text;
    if text.starts_with(':') && !text.starts_with("::") {
        text = &text[1..];
    }
    if text.ends_with(':') && !text.ends_with("::") {
        text = &text[..text.len() - 1];
    }
    text
}

/// D6.2: a typed part's first unescaped colon splits it into a key and a
/// value; `None` for a word, or a text whose colon comes first.
fn split_key(typed: &str) -> Option<(String, String)> {
    let chars: Vec<char> = typed.chars().collect();
    let mut k = 0;
    while k < chars.len() {
        match chars[k] {
            '\\' => k += 2,
            ':' if k > 0 => {
                return Some((chars[..k].iter().collect(), chars[k + 1..].iter().collect()));
            }
            ':' => return None,
            _ => k += 1,
        }
    }
    None
}

/// D6.2: a word, value or field key read as G7's kind U without its errors,
/// then as N1's exact text.
fn exact(text: &str) -> String {
    let mut unescaped = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            unescaped.push(c);
            continue;
        }
        match chars.next() {
            Some(next @ ('"' | ':' | '(' | ')' | '-')) => unescaped.push(next),
            Some(next) => {
                unescaped.push('\\');
                unescaped.push(next);
            }
            None => unescaped.push('\\'),
        }
    }
    pattern::exact_text(&unescaped).into_owned()
}

fn starts_with_ascii_folded(word: &str, typed: &str) -> bool {
    word.len() >= typed.len() && word.is_char_boundary(typed.len()) && word[..typed.len()].eq_ignore_ascii_case(typed)
}

/// D6.6: a template name's insert text, Q(`card:` + E(s)), when it reads
/// back as a template-name term holding that name (not a number).
fn template_text(name: &str) -> Option<String> {
    // A name not in NFC is normalized by the search and then finds nothing.
    let nfc = matches!(nfc(name), std::borrow::Cow::Borrowed(_));
    if name.is_empty() || !nfc || name.chars().any(crate::builder::splits_a_term) {
        return None;
    }
    let escaped = escape_name(name);
    let text = crate::canonical::term_text(&Term::Template(crate::ast::TemplateRef::Name(escaped.clone())));
    let query = parse(&text).ok()?;
    match &query.node(query.root()).kind {
        NodeKind::Term(Term::Template(crate::ast::TemplateRef::Name(read))) if *read == escaped => Some(text),
        _ => None,
    }
}

/// D6.6: a field key's insert text, Q(f′ + `:`); `None` for a field J3.3
/// refuses.
fn field_key_text(field: &str) -> Option<String> {
    build(&BuiltItem::FieldPresent(field.to_owned())).ok()?;
    Some(quoted(&format!("{}:", field_name_text(field))))
}

/// f′: E(f) with every `:` written `\:` (F5).
fn field_name_text(field: &str) -> String {
    escape_name(field).replace(':', "\\:")
}

/// Field names of `scope`, one per name under unicase equality, spelled as
/// the note type with the lowest id spells it, in D6.5 step 3's order; with
/// the ids of the note types that have each.
pub(crate) fn field_names(scope: &[&NotetypeNames]) -> Vec<(String, Vec<meguri_model::ids::NotetypeId>)> {
    grouped(scope, |notetype| notetype.fields.as_slice())
}

/// Template names of `scope`, grouped as [`field_names`].
pub(crate) fn template_names(scope: &[&NotetypeNames]) -> Vec<(String, Vec<meguri_model::ids::NotetypeId>)> {
    grouped(scope, |notetype| notetype.templates.as_slice())
}

fn grouped(
    scope: &[&NotetypeNames],
    names_of: impl Fn(&NotetypeNames) -> &[String],
) -> Vec<(String, Vec<meguri_model::ids::NotetypeId>)> {
    let mut groups: Vec<(String, String, meguri_model::ids::NotetypeId, Vec<meguri_model::ids::NotetypeId>)> = Vec::new();
    let mut by_key: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for notetype in scope {
        for name in names_of(notetype) {
            let key = folded(name);
            match by_key.get(&key) {
                Some(&k) => {
                    let group = &mut groups[k];
                    if !group.3.contains(&notetype.id) {
                        group.3.push(notetype.id);
                    }
                    if notetype.id < group.2 {
                        group.1.clone_from(name);
                        group.2 = notetype.id;
                    }
                }
                None => {
                    by_key.insert(key.clone(), groups.len());
                    groups.push((key, name.clone(), notetype.id, vec![notetype.id]));
                }
            }
        }
    }
    groups.sort_by(|a, b| compare_folded(&a.1, &b.1).then_with(|| a.1.cmp(&b.1)));
    groups.into_iter().map(|(_, name, _, ids)| (name, ids)).collect()
}

/// D6.6: a saved search's text as inserted, `canonicalize(s)`; `None` when
/// it does not parse (not offered).
fn saved_text(stored: &str) -> Option<String> {
    canonical_text(&search_field_text(stored)).ok()
}

fn saved_searches(out: &mut Ranked<'_>, names: &NameIndex, matcher: &Matcher, range: Range<usize>) {
    for saved in names.saved_searches() {
        let Some(class) = matcher.substring(&saved.name) else { continue };
        if let Some(c) = saved_text(&saved.text) {
            out.name(
                class,
                1,
                candidate(CandidateKind::SavedSearch, &saved.name, None, range.clone(), format!("({c})"), true),
            );
        }
    }
}

fn decks<'n>(out: &mut Ranked<'n>, names: &'n NameIndex, value: &str, matcher: &Matcher, range: Range<usize>) {
    for special in ["current", "filtered", "*"] {
        if starts_with_ascii_folded(special, value) {
            out.fixed(candidate(CandidateKind::Deck, special, None, range.clone(), format!("deck:{special}"), true));
        }
    }
    for deck in names.decks() {
        let Some(class) = matcher.components(deck.native.split('\u{1f}')) else {
            continue;
        };
        let range = range.clone();
        let mut shown = vec![simple_fold(&deck.name), simple_fold(&format!("{}::*", deck.name))];
        // Only a deck J3.5 writes by its ids answers to a recent `did:`.
        if out.recent_names.contains(BY_IDS)
            && (deck.native.split('\u{1f}').any(|level| level.contains("::"))
                || crate::builder::build_query(&BuiltItem::Deck(deck.name.clone())).is_err())
        {
            shown.push(BY_IDS.to_owned());
        }
        out.lazy(class, 0, shown, move || {
            let Some(value) = DeckValue::of(names, deck.id) else { return Vec::new() };
            let Some(item) = Value::Deck(value).item() else { return Vec::new() };
            let Ok(insert) = build(&item) else { return Vec::new() };
            let detail = deck.filtered.then_some(Detail::Filtered);
            let mut made = vec![candidate(CandidateKind::Deck, &deck.name, detail.clone(), range.clone(), insert, true)];
            // `Name::*` follows a deck with subdecks written by name.
            if deck.has_subdecks && matches!(item, BuiltItem::Deck(_)) {
                let any = quoted(&format!("deck:{}::*", escape_name(&deck.name)));
                let label = format!("{}::*", deck.name);
                made.push(candidate(CandidateKind::Deck, &label, detail, range, any, true));
            }
            made
        });
    }
}

fn tags<'n>(out: &mut Ranked<'n>, names: &'n NameIndex, value: &str, matcher: &Matcher, range: Range<usize>) {
    if starts_with_ascii_folded("none", value) {
        out.fixed(candidate(CandidateKind::Tag, "none", None, range.clone(), "tag:none".to_owned(), true));
    }
    for tag in names.tags() {
        let Some(class) = matcher.components(tag.split("::")) else {
            continue;
        };
        let range = range.clone();
        let mut shown = vec![simple_fold(tag)];
        if tag.chars().any(crate::builder::splits_a_term) {
            shown.push(BY_REGEX.to_owned());
        }
        out.lazy(class, 0, shown, move || match build(&BuiltItem::Tag(tag.clone())) {
            Ok(insert) => vec![candidate(CandidateKind::Tag, tag, None, range, insert, true)],
            Err(_) => Vec::new(),
        });
    }
}

fn days(out: &mut Ranked<'_>, key: &str, value: &str, range: Range<usize>) {
    for d in DAYS {
        let text = d.to_string();
        if text.starts_with(value) {
            out.fixed(candidate(
                CandidateKind::Days,
                &text,
                Some(Detail::Days(d)),
                range.clone(),
                format!("{key}:{text}"),
                true,
            ));
        }
    }
}

/// D6.3.1.
fn properties(out: &mut Ranked<'_>, value: &str, range: Range<usize>) {
    for property in PROPERTIES {
        if property != value && starts_with_ascii_folded(property, value) {
            out.fixed(candidate(
                CandidateKind::Property,
                property,
                None,
                range.clone(),
                format!("prop:{property}"),
                false,
            ));
        }
    }
    let Some(property) = property_prefix(value) else { return };
    let rest = &value[property.len()..];
    if OPERATORS.iter().any(|o| o.starts_with(rest)) {
        for operator in OPERATORS {
            if operator.starts_with(rest) {
                out.fixed(candidate(
                    CandidateKind::Operator,
                    operator,
                    None,
                    range.clone(),
                    format!("prop:{property}{operator}"),
                    false,
                ));
            }
        }
    }
    let operator = ["<=", ">=", "!=", "=", "<", ">"].into_iter().find(|o| rest.starts_with(o));
    if let Some(operator) = operator
        && rest.len() > operator.len()
    {
        out.fixed(Candidate {
            kind: CandidateKind::ValueHint,
            label: property.to_owned(),
            detail: Some(Detail::Unit(unit(property))),
            insert: None,
            completes: false,
        });
    }
}

/// G8.1 step 1: the property `value` starts with (a custom data key needs
/// one ASCII letter or digit at least).
fn property_prefix(value: &str) -> Option<&str> {
    for property in PROPERTIES {
        if property.ends_with(':') {
            if let Some(rest) = value.strip_prefix(property) {
                let key = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
                if key > 0 {
                    return Some(&value[..property.len() + key]);
                }
            }
        } else if value.starts_with(property) {
            return Some(property);
        }
    }
    None
}

fn unit(property: &str) -> Unit {
    match property {
        "ivl" | "s" => Unit::Days,
        "due" => Unit::DaysFromToday,
        "reps" | "lapses" => Unit::Count,
        "ease" => Unit::EaseFactor,
        "pos" => Unit::Position,
        "rated" | "resched" => Unit::DaysAgo,
        "d" | "r" => Unit::ZeroToOne,
        p if p.starts_with("cdn:") => Unit::Number,
        _ => Unit::Text,
    }
}

/// D6.6: a preset's insert text reads back as a preset term holding exactly
/// its name.
fn reads_as_preset(insert: &str, name: &str) -> bool {
    let Ok(query) = parse(insert) else { return false };
    matches!(&query.node(query.root()).kind, NodeKind::Term(Term::Preset(read)) if read == name)
}

/// One group being read, for D6.4.
struct Frame {
    /// Each AND-run's note type terms (their note type indexes, or none
    /// found), and whether it holds the current term or a group around it.
    runs: Vec<(Vec<Option<Vec<usize>>>, bool)>,
}

/// D6.2's current term and D6.4's scope, read left to right; items read
/// after the text's first parse error count for neither.
fn scan(text: &str, cursor: usize, names: Option<&NameIndex>) -> Scanned {
    let lexer = Lexer::new(text);
    let lead = lexer.input_range(0..0).start;
    // An unclosed group is reported at its `(` once everything after it
    // was read: every item counts then.
    let cutoff = match parse(text) {
        Ok(_) => usize::MAX,
        Err(error) if matches!(error.kind(), crate::error::ErrorKind::UnclosedGroup) => usize::MAX,
        Err(error) => error.range().start,
    };
    let mut frames = vec![Frame {
        runs: vec![(Vec::new(), false)],
    }];
    let mut closed: Vec<Frame> = Vec::new();
    let mut current = None;
    let mut i = 0;
    let len = lexer.len();
    loop {
        i = lexer.skip_spaces(i);
        if i >= len {
            break;
        }
        match lexer.at(i) {
            Some('(') => {
                frames.push(Frame {
                    runs: vec![(Vec::new(), false)],
                });
                i += 1;
                continue;
            }
            Some(')') => {
                if frames.len() > 1 {
                    closed.extend(frames.pop());
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        let mut start = i;
        let mut negated = false;
        if lexer.at(i) == Some('-') {
            match lexer.at(i + 1) {
                Some('(') => {
                    i += 1;
                    continue;
                }
                Some(' ' | ')') | None => {}
                Some(_) => {
                    negated = true;
                    start = i + 1;
                }
            }
        }
        let unclosed = matches!(lexer.term(start), Err(ref error) if matches!(error.kind(), crate::error::ErrorKind::UnclosedQuote));
        let (end, form, readable) = match lexer.term(start) {
            Ok(raw) => (raw.span.end, Some(raw.form), true),
            Err(error) => match error.kind() {
                crate::error::ErrorKind::UnclosedQuote => (len, None, true),
                crate::error::ErrorKind::EmptyQuote => (error.range().end - lead, None, true),
                crate::error::ErrorKind::UnknownEscape { provided } if provided == "\\" => (len, None, true),
                _ => (len, None, false),
            },
        };
        let mut range = lexer.input_range(start..end);
        // An unclosed quote runs to the end of the text, the spaces G1 trims
        // after it included.
        let holds = range.start < cursor && (cursor <= range.end || unclosed);
        // D6.2: a term with an unclosed quote ends at the cursor.
        if holds && form.is_none() && unclosed {
            range.end = cursor;
        }
        let word = lexer.string(start..end);
        let operator = matches!(form, Some(Form::Unquoted)) && (word.eq_ignore_ascii_case("and") || word.eq_ignore_ascii_case("or"));
        if holds && current.is_none() {
            current = Some(if !readable {
                Current::Unreadable
            } else if word == "-" && range.end == cursor && !negated {
                Current::Empty
            } else {
                Current::Term(typed_part(&lexer, start, end, cursor - lead, form.as_ref(), range.clone()))
            });
            for frame in &mut frames {
                if let Some(run) = frame.runs.last_mut() {
                    run.1 = true;
                }
            }
        } else if operator && word.eq_ignore_ascii_case("or") && !negated {
            if let Some(frame) = frames.last_mut() {
                frame.runs.push((Vec::new(), false));
            }
        } else if !negated
            && !operator
            && range.end <= cutoff
            && let Some(names) = names
            && let Some(found) = notetype_term(&word, names)
            && let Some(frame) = frames.last_mut()
            && let Some(run) = frame.runs.last_mut()
        {
            run.0.push(found);
        }
        if !readable {
            break;
        }
        i = end.max(start + 1);
    }
    let current = current.unwrap_or(Current::Empty);
    let mut terms: Vec<Option<Vec<usize>>> = Vec::new();
    for frame in frames.iter().chain(closed.iter()) {
        for (found, holds) in &frame.runs {
            if *holds {
                terms.extend(found.iter().cloned());
            }
        }
    }
    let scope = if terms.is_empty() {
        None
    } else {
        let mut indexes: Vec<usize> = terms.into_iter().flatten().flatten().collect();
        indexes.sort_unstable();
        indexes.dedup();
        Some(indexes)
    };
    Scanned { current, scope }
}

/// D6.2's typed part of the term at lexer positions `start..end` (`range`
/// in the caller's characters), the cursor at lexer position `cursor`.
fn typed_part(lexer: &Lexer, start: usize, end: usize, cursor: usize, form: Option<&Form>, range: Range<usize>) -> At {
    let quoted_term = lexer.at(start) == Some('"');
    let from = if quoted_term { start + 1 } else { start };
    let mut to = cursor.max(from);
    // After a closed quote the typed part ends before it.
    if matches!(form, Some(Form::Quoted { .. } | Form::PartlyQuoted { .. }))
        && to == end
        && to > from
        && lexer.at(to - 1) == Some('"')
    {
        to -= 1;
    }
    // Past the trimmed text (spaces inside an unclosed quote), spaces.
    let inside = to.min(lexer.len());
    let mut typed = lexer.string(from.min(inside)..inside);
    typed.extend(std::iter::repeat_n(' ', to - inside));
    let typed = typed.replace("\\\"", "\"");
    // G3 form 2: a `"` right after the key's colon.
    let chars = lexer.chars(start..lexer.len());
    let mut k = 0;
    let mut partly_quoted = false;
    while k < chars.len() {
        match chars[k] {
            '\\' => k += 2,
            ':' => {
                partly_quoted = !quoted_term && k > 0 && chars.get(k + 1) == Some(&'"');
                break;
            }
            ' ' | '"' | '(' | ')' => break,
            _ => k += 1,
        }
    }
    At {
        range,
        typed,
        partly_quoted,
    }
}

/// A `note:` or `mid:` term's note types (D6.4): `Some(list)`, possibly
/// empty when it names none; `None` for any other item.
fn notetype_term(word: &str, names: &NameIndex) -> Option<Option<Vec<usize>>> {
    let query = parse(word).ok()?;
    let found: Vec<usize> = match &query.node(query.root()).kind {
        NodeKind::Term(Term::Notetype(value)) => {
            let value = nfc(value);
            if value.is_empty() {
                Vec::new()
            } else if pattern::has_wildcards(&value) {
                let expression = format!("(?i)^{}$", glob_to_regex_text(&value));
                let Ok(regex) = regex::Regex::new(&expression) else {
                    return Some(Some(Vec::new()));
                };
                matching(names, |n| regex.is_match(&n.name))
            } else {
                let exact = pattern::exact_text(&value);
                matching(names, |n| compare_folded(&n.name, &exact).is_eq())
            }
        }
        NodeKind::Term(Term::NotetypeId(id)) => matching(names, |n| n.id == *id),
        _ => return None,
    };
    Some(Some(found))
}

fn glob_to_regex_text(value: &str) -> String {
    pattern::glob_to_regex(value, pattern::Wildcards::TEXT)
}

fn matching(names: &NameIndex, test: impl Fn(&NotetypeNames) -> bool) -> Vec<usize> {
    names
        .notetypes()
        .iter()
        .enumerate()
        .filter(|(_, n)| test(n))
        .map(|(k, _)| k)
        .collect()
}
