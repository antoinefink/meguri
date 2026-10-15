//! Field text (`specs/fixtures/anonymizer.md` A6–A9.2): the tokenizer, the
//! text rules and where media references are found.
//!
//! One walker reads a field and reports what it finds to a [`Sink`]: kept
//! text, media names with what they become, characters to replace and
//! entities to replace. The rewriter's sink writes the clone field; V1's
//! sink lists the same events for the source and for the clone field so
//! the two can be compared.
//!
//! Media references are located before the field is tokenized (A9.2): the
//! name of every M2 and M3 match is an *atom*, and so are a sound tag's
//! `[sound:` and `]`. The tokenizer treats atoms as opaque: no syntax
//! character inside a name (a space, a quote) ends a tag, a value or a run.

use std::collections::BTreeMap;

use meguri_text::media_refs;

use super::chars::{self, is_replaced};

/// What a raw media name found by M2, M3 or in a CSS `url()` becomes.
pub enum Reference {
    /// A well-formed local name (A9.1): replaced by this text.
    Local(String),
    /// A remote name (M5).
    Remote,
    /// Any other name: not a reference.
    Malformed,
}

/// The media names a walker can resolve.
pub trait Names {
    /// A raw name found by M2, M3 or in a CSS `url()`.
    fn reference(&mut self, raw: &str) -> Reference;
    /// A file of the media folder whose key is the fold of `candidate`'s NFC
    /// form (decoded first when `decode`); with `need_extension`, only a
    /// name with an extension (A9.1) counts. Returns what it becomes.
    fn file(&mut self, candidate: &str, decode: bool, need_extension: bool) -> Option<String>;
}

/// What a walker reports, in field order.
pub trait Sink {
    /// Text copied as it is.
    fn keep(&mut self, text: &str);
    /// A media name, as it is written in the result.
    fn name(&mut self, text: &str);
    /// A character replaced by the character rule (A5).
    fn replace(&mut self, c: char);
    /// An entity whose character is replaced (A5.3): `raw` as written, `c`
    /// its character.
    fn entity(&mut self, raw: &str, c: char);
    /// G3: an `&` that is no entity was just kept, and a run of ASCII
    /// letters, digits and `#` up to a `;` follows.
    fn g3_begin(&mut self);
    /// G3: the run's `;` was just kept.
    fn g3_end(&mut self);
}

enum Atom {
    Name(String),
    Keep,
}

/// A6.2: attributes whose values are kept (compared ASCII
/// case-insensitively).
const KEPT_ATTRIBUTES: &[&str] = &[
    "class",
    "id",
    "dir",
    "lang",
    "align",
    "valign",
    "width",
    "height",
    "size",
    "color",
    "bgcolor",
    "face",
    "border",
    "cellpadding",
    "cellspacing",
    "colspan",
    "rowspan",
    "span",
    "start",
    "type",
    "role",
    "tabindex",
    "target",
    "rel",
    "controls",
    "autoplay",
    "loop",
    "muted",
    "preload",
    "contenteditable",
    "draggable",
    "hidden",
    "spellcheck",
    "translate",
];

/// A6.3: the LaTeX markers that are kept.
const LATEX_MARKERS: &[&str] = &["[latex]", "[/latex]", "[$$]", "[/$$]", "[$]", "[/$]"];

const SOUND_OPEN: &str = "[sound:";
const IMAGE_OCCLUSION: &str = "image-occlusion:";

pub struct Walker<'t, 'n, 's, N: Names, S: Sink> {
    src: &'t str,
    atoms: BTreeMap<usize, (usize, Atom)>,
    names: &'n mut N,
    sink: &'s mut S,
}

/// A6: walks a field (or a deck description) through the tokenizer.
pub fn walk_field<N: Names, S: Sink>(src: &str, names: &mut N, sink: &mut S) {
    let atoms = locate(src, names);
    let mut walker = Walker {
        src,
        atoms,
        names,
        sink,
    };
    walker.field();
}

/// A6.3 alone, on a plain string (search values, saved search names, flag
/// labels): no tags and no located references.
pub fn walk_text<N: Names, S: Sink>(src: &str, names: &mut N, sink: &mut S) {
    let mut walker = Walker {
        src,
        atoms: BTreeMap::new(),
        names,
        sink,
    };
    walker.text(0, src.len());
}

/// A start tag's name and its attributes' names and values; returns whether
/// to go on.
pub type TagVisitor<'a> = dyn FnMut(&str, &[(&str, &str)]) -> bool + 'a;

/// Visits the start tags of `src` as A6.1 reads them, with their attribute
/// values (names as written), until `visit` returns false. Comment, style
/// and script contents are skipped.
pub fn start_tags(src: &str, visit: &mut TagVisitor<'_>) {
    struct Null;
    impl Names for Null {
        fn reference(&mut self, _: &str) -> Reference {
            Reference::Malformed
        }
        fn file(&mut self, _: &str, _: bool, _: bool) -> Option<String> {
            None
        }
    }
    impl Sink for Null {
        fn keep(&mut self, _: &str) {}
        fn name(&mut self, _: &str) {}
        fn replace(&mut self, _: char) {}
        fn entity(&mut self, _: &str, _: char) {}
        fn g3_begin(&mut self) {}
        fn g3_end(&mut self) {}
    }
    let (mut names, mut sink) = (Null, Null);
    let walker = Walker {
        src,
        atoms: BTreeMap::new(),
        names: &mut names,
        sink: &mut sink,
    };
    let mut p = 0;
    while p < src.len() {
        if src.as_bytes()[p] == b'<'
            && let Some(markup) = walker.markup(p)
        {
            p = match markup {
                Markup::Comment { content, close } => close.map_or(content.1, |at| at + 3),
                Markup::End(end) => end,
                Markup::Declaration(gt) => gt + 1,
                Markup::Start(tag) => {
                    let name = &src[tag.name.0..tag.name.1];
                    let attrs: Vec<(&str, &str)> = tag
                        .parts
                        .iter()
                        .filter_map(|part| match *part {
                            Part::Value { attr, value } => Some((&src[attr.0..attr.1], &src[value.0..value.1])),
                            Part::Kept(..) => None,
                        })
                        .collect();
                    if !visit(name, &attrs) {
                        return;
                    }
                    let raw_end = |close: &str| find_ignore_ascii_case(src, tag.end, close).unwrap_or(src.len());
                    if name.eq_ignore_ascii_case("style") {
                        raw_end("</style")
                    } else if name.eq_ignore_ascii_case("script") {
                        raw_end("</script")
                    } else {
                        tag.end
                    }
                }
            };
            continue;
        }
        p += walker.char_at(p).len_utf8();
    }
}

/// A9.2: the atoms of `src`: the names of M2 and M3 matches that are
/// well-formed local names, and every sound tag's `[sound:` and `]`.
fn locate<N: Names>(src: &str, names: &mut N) -> BTreeMap<usize, (usize, Atom)> {
    let base = src.as_ptr() as usize;
    let offset = |part: &str| part.as_ptr() as usize - base;
    let mut found: Vec<(usize, usize, Atom)> = Vec::new();
    for tag in media_refs::media_tags(src) {
        if let Reference::Local(text) = names.reference(tag.raw_name) {
            let start = offset(tag.raw_name);
            found.push((start, start + tag.raw_name.len(), Atom::Name(text)));
        }
    }
    for sound in media_refs::sound_tags(src) {
        let name_start = sound.start + SOUND_OPEN.len();
        let name_end = name_start + sound.raw_name.len();
        found.push((sound.start, name_start, Atom::Keep));
        if let Reference::Local(text) = names.reference(sound.raw_name) {
            found.push((name_start, name_end, Atom::Name(text)));
        }
        found.push((name_end, name_end + 1, Atom::Keep));
    }
    found.sort_by_key(|&(start, end, _)| (start, end));
    let mut atoms = BTreeMap::new();
    let mut reached = 0;
    for (start, end, atom) in found {
        // Atoms never overlap; a later one that would is dropped.
        if start >= reached && end > start {
            atoms.insert(start, (end, atom));
            reached = end;
        }
    }
    atoms
}

/// A parsed start tag.
struct StartTag {
    name: (usize, usize),
    parts: Vec<Part>,
    /// After its `>`.
    end: usize,
}

enum Part {
    Kept(usize, usize),
    Value {
        attr: (usize, usize),
        value: (usize, usize),
    },
}

enum Markup {
    /// `<!--`, content, and the `-->` when there is one.
    Comment {
        content: (usize, usize),
        close: Option<usize>,
    },
    Start(StartTag),
    /// An end tag up to and including its `>`.
    End(usize),
    /// `<!` or `<?`: the position of its `>`.
    Declaration(usize),
}

fn starts_with_ignore_ascii_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len() && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// The first position of `needle` in `text` from `from`, ASCII
/// case-insensitive.
fn find_ignore_ascii_case(text: &str, from: usize, needle: &str) -> Option<usize> {
    let hay = text.as_bytes();
    let n = needle.as_bytes();
    (from..=hay.len().saturating_sub(n.len()))
        .find(|&i| hay.len() >= i + n.len() && hay[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// A7: the length of the opener shape at the start of `text`: `{{c`, ASCII
/// digits or commas, `::`.
pub fn opener_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix("{{c")?;
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit() || *b == b',').count();
    (digits > 0 && rest[digits..].starts_with("::")).then_some(3 + digits + 2)
}

impl<N: Names, S: Sink> Walker<'_, '_, '_, N, S> {
    fn char_at(&self, p: usize) -> char {
        self.src[p..].chars().next().expect("a position inside the text")
    }

    fn atom_end(&self, p: usize) -> Option<usize> {
        self.atoms.get(&p).map(|(end, _)| *end)
    }

    /// Reports the atom at `p` and returns its end.
    fn emit_atom(&mut self, p: usize) -> usize {
        let (end, atom) = self.atoms.get(&p).expect("an atom starts here");
        let end = *end;
        match atom {
            Atom::Name(text) => {
                let text = text.clone();
                self.sink.name(&text);
            }
            Atom::Keep => self.sink.keep(&self.src[p..end]),
        }
        end
    }

    /// Kept text: copied, except located names.
    fn kept(&mut self, start: usize, end: usize) {
        let mut run = start;
        let mut p = start;
        while p < end {
            if self.atoms.contains_key(&p) {
                self.sink.keep(&self.src[run..p]);
                p = self.emit_atom(p);
                run = p;
            } else {
                p += self.char_at(p).len_utf8();
            }
        }
        if run < end {
            self.sink.keep(&self.src[run..end.max(run)]);
        }
    }

    /// The position after the character or atom at `p`.
    fn step(&self, p: usize) -> usize {
        self.atom_end(p).unwrap_or_else(|| p + self.char_at(p).len_utf8())
    }

    /// A6.1: the field, token by token.
    fn field(&mut self) {
        let len = self.src.len();
        let mut run = 0;
        let mut p = 0;
        while p < len {
            if self.atoms.contains_key(&p) {
                p = self.step(p);
                continue;
            }
            if self.src.as_bytes()[p] == b'<'
                && let Some(markup) = self.markup(p)
            {
                self.text(run, p);
                p = self.emit_markup(p, markup);
                run = p;
                continue;
            }
            p = self.step(p);
        }
        self.text(run, len);
    }

    /// The markup form starting at the `<` at `p`, if one does.
    fn markup(&self, p: usize) -> Option<Markup> {
        let rest = &self.src[p..];
        let bytes = rest.as_bytes();
        if rest.starts_with("<!--") {
            let content_start = p + 4;
            return Some(match self.src[content_start..].find("-->") {
                Some(i) => Markup::Comment {
                    content: (content_start, content_start + i),
                    close: Some(content_start + i),
                },
                None => Markup::Comment {
                    content: (content_start, self.src.len()),
                    close: None,
                },
            });
        }
        if bytes.len() >= 2 && bytes[1].is_ascii_alphabetic() {
            return self.start_tag(p).map(Markup::Start);
        }
        if bytes.len() >= 3 && bytes[1] == b'/' && bytes[2].is_ascii_alphabetic() {
            return rest.find('>').map(|i| Markup::End(p + i + 1));
        }
        if bytes.len() >= 2 && (bytes[1] == b'!' || bytes[1] == b'?') {
            return rest.find('>').map(|i| Markup::Declaration(p + i));
        }
        None
    }

    /// A6.1: a start tag at `p` (a `<` and an ASCII letter); `None` when no
    /// `>` ends it before the end of the field.
    fn start_tag(&self, p: usize) -> Option<StartTag> {
        let len = self.src.len();
        let is_space = |c: char| c.is_whitespace();
        let mut q = p + 1;
        while q < len {
            if self.atoms.contains_key(&q) {
                q = self.step(q);
                continue;
            }
            let c = self.char_at(q);
            if is_space(c) || c == '/' || c == '>' {
                break;
            }
            q += c.len_utf8();
        }
        let name = (p + 1, q);
        let mut parts = Vec::new();
        loop {
            let separators = q;
            while q < len && !self.atoms.contains_key(&q) {
                let c = self.char_at(q);
                if !(is_space(c) || c == '/') {
                    break;
                }
                q += c.len_utf8();
            }
            if q > separators {
                parts.push(Part::Kept(separators, q));
            }
            if q >= len {
                return None;
            }
            if !self.atoms.contains_key(&q) && self.src.as_bytes()[q] == b'>' {
                parts.push(Part::Kept(q, q + 1));
                return Some(StartTag {
                    name,
                    parts,
                    end: q + 1,
                });
            }
            let attr_start = q;
            while q < len {
                if self.atoms.contains_key(&q) {
                    q = self.step(q);
                    continue;
                }
                let c = self.char_at(q);
                if is_space(c) || c == '/' || c == '>' || c == '=' {
                    break;
                }
                q += c.len_utf8();
            }
            let attr = (attr_start, q);
            if q < len && !self.atoms.contains_key(&q) && self.src.as_bytes()[q] == b'=' {
                parts.push(Part::Kept(attr_start, q + 1));
                q += 1;
                let quote = self
                    .src
                    .as_bytes()
                    .get(q)
                    .copied()
                    .filter(|b| *b == b'"' || *b == b'\'');
                if let Some(quote) = quote.filter(|_| !self.atoms.contains_key(&q)) {
                    let value_start = q + 1;
                    let mut r = value_start;
                    loop {
                        if r >= len {
                            return None;
                        }
                        if self.atoms.contains_key(&r) {
                            r = self.step(r);
                            continue;
                        }
                        if self.src.as_bytes()[r] == quote {
                            break;
                        }
                        r += self.char_at(r).len_utf8();
                    }
                    parts.push(Part::Kept(q, q + 1));
                    parts.push(Part::Value {
                        attr,
                        value: (value_start, r),
                    });
                    parts.push(Part::Kept(r, r + 1));
                    q = r + 1;
                } else {
                    let value_start = q;
                    while q < len {
                        if self.atoms.contains_key(&q) {
                            q = self.step(q);
                            continue;
                        }
                        let c = self.char_at(q);
                        if is_space(c) || c == '>' {
                            break;
                        }
                        q += c.len_utf8();
                    }
                    parts.push(Part::Value {
                        attr,
                        value: (value_start, q),
                    });
                }
            } else {
                parts.push(Part::Kept(attr_start, q));
            }
        }
    }

    /// Reports a markup form and its raw content; returns where the field
    /// goes on.
    fn emit_markup(&mut self, p: usize, markup: Markup) -> usize {
        match markup {
            Markup::Comment { content, close } => {
                self.sink.keep("<!--");
                self.text(content.0, content.1);
                match close {
                    Some(at) => {
                        self.sink.keep("-->");
                        at + 3
                    }
                    None => content.1,
                }
            }
            Markup::End(end) => {
                self.kept(p, end);
                end
            }
            Markup::Declaration(gt) => {
                self.sink.keep(&self.src[p..p + 2]);
                self.text(p + 2, gt);
                self.sink.keep(">");
                gt + 1
            }
            Markup::Start(tag) => {
                self.sink.keep("<");
                self.kept(tag.name.0, tag.name.1);
                for part in &tag.parts {
                    match *part {
                        Part::Kept(a, b) => self.kept(a, b),
                        Part::Value { attr, value } => self.value(attr, value),
                    }
                }
                let name = &self.src[tag.name.0..tag.name.1];
                if name.eq_ignore_ascii_case("style") {
                    let end = find_ignore_ascii_case(self.src, tag.end, "</style").unwrap_or(self.src.len());
                    self.css(tag.end, end);
                    end
                } else if name.eq_ignore_ascii_case("script") {
                    let end = find_ignore_ascii_case(self.src, tag.end, "</script").unwrap_or(self.src.len());
                    self.text(tag.end, end);
                    end
                } else {
                    tag.end
                }
            }
        }
    }

    /// A6.2: an attribute value.
    fn value(&mut self, attr: (usize, usize), value: (usize, usize)) {
        let attr_name = &self.src[attr.0..attr.1];
        let (start, end) = value;
        if attr_name.eq_ignore_ascii_case("style") {
            self.css(start, end);
            return;
        }
        if KEPT_ATTRIBUTES.iter().any(|kept| attr_name.eq_ignore_ascii_case(kept)) {
            self.kept(start, end);
            return;
        }
        let raw = &self.src[start..end];
        if !raw.is_empty()
            && let Some(name) = self.names.file(raw, true, false)
        {
            self.sink.name(&name);
            return;
        }
        let prefix = if starts_with_ignore_ascii_case(raw, "http://") {
            7
        } else if starts_with_ignore_ascii_case(raw, "https://") {
            8
        } else if starts_with_ignore_ascii_case(raw, "data:") {
            raw.find(',').map_or(raw.len(), |i| i + 1)
        } else {
            0
        };
        self.kept(start, start + prefix);
        self.text(start + prefix, end);
    }

    /// Kept CSS (a `style` value or block): `url()` names per A9.2.
    fn css(&mut self, start: usize, end: usize) {
        let mut run = start;
        let mut p = start;
        while p < end {
            if self.atoms.contains_key(&p) {
                p = self.step(p);
                continue;
            }
            if starts_with_ignore_ascii_case(&self.src[p..end], "url(")
                && let Some((name_start, name_end, after)) = self.url_name(p + 4, end)
            {
                self.kept(run, name_start);
                self.css_name(name_start, name_end);
                self.kept(name_end, after);
                p = after;
                run = p;
                continue;
            }
            p = self.step(p);
        }
        self.kept(run, end);
    }

    /// The name inside `url(` ... `)` from `q` (after the `(`): its span and
    /// the position after the `)`.
    fn url_name(&self, q: usize, end: usize) -> Option<(usize, usize, usize)> {
        let text = &self.src[q..end];
        let skip_ws = |s: &str| s.len() - s.trim_start().len();
        let mut i = skip_ws(text);
        let quote = text[i..].chars().next().filter(|c| *c == '"' || *c == '\'');
        let (name_start, name_end);
        if let Some(quote) = quote {
            i += 1;
            name_start = i;
            let close = text[i..].find(quote)?;
            name_end = i + close;
            i = name_end + 1;
        } else {
            name_start = i;
            let stop = text[i..].find(|c: char| c == ')' || c.is_whitespace())?;
            name_end = i + stop;
            i = name_end;
        }
        i += skip_ws(&text[i..]);
        if !text[i..].starts_with(')') || name_end == name_start {
            return None;
        }
        // A name holding an atom is left as kept text.
        if (q + name_start..q + name_end).any(|at| self.atoms.contains_key(&at)) {
            return None;
        }
        Some((q + name_start, q + name_end, q + i + 1))
    }

    /// A9.2's CSS rule for one `url()` name.
    fn css_name(&mut self, start: usize, end: usize) {
        let raw = &self.src[start..end];
        if starts_with_ignore_ascii_case(raw, "data:") {
            let comma = raw.find(',').map_or(raw.len(), |i| i + 1);
            self.kept(start, start + comma);
            self.text(start + comma, end);
            return;
        }
        match self.names.reference(raw) {
            Reference::Local(text) => self.sink.name(&text),
            Reference::Remote => {
                let scheme = raw.find("://").map_or(raw.len(), |i| i + 3);
                self.kept(start, start + scheme);
                self.text(start + scheme, end);
            }
            Reference::Malformed => self.kept(start, end),
        }
    }

    /// A6.3: text, rule by rule at each position.
    fn text(&mut self, start: usize, end: usize) {
        let mut p = start;
        while p < end {
            if self.atoms.contains_key(&p) {
                p = self.emit_atom(p);
                continue;
            }
            let rest = &self.src[p..end];
            // A7: cloze opener shapes, and image-occlusion data after one.
            if let Some(len) = opener_len(rest) {
                self.sink.keep(&rest[..len]);
                p += len;
                if self.src[p..end].starts_with(IMAGE_OCCLUSION) {
                    p = self.io_data(p, end);
                }
                continue;
            }
            // LaTeX markers.
            if let Some(marker) = LATEX_MARKERS.iter().find(|m| starts_with_ignore_ascii_case(rest, m)) {
                self.sink.keep(&rest[..marker.len()]);
                p += marker.len();
                continue;
            }
            // A9.2: a bare media name.
            if let Some(run_end) = self.bare_run(p, start, end)
                && let Some(name) = self.names.file(&self.src[p..run_end], false, true)
            {
                self.sink.name(&name);
                p = run_end;
                continue;
            }
            // A8: bracket annotation codes.
            if rest.starts_with('[')
                && let Some(next) = self.annotation(p, end)
            {
                p = next;
                continue;
            }
            // A5.3: entities, and G3 after an `&` that is none.
            if rest.starts_with('&') {
                match chars::entity_at(rest) {
                    Some((len, c)) => {
                        if is_replaced(c) {
                            self.sink.entity(&rest[..len], c);
                        } else {
                            self.sink.keep(&rest[..len]);
                        }
                        p += len;
                    }
                    None => {
                        self.sink.keep("&");
                        p += 1;
                        if let Some(run) = chars::g3_run(&self.src[p..end])
                            && !(p..p + run).any(|at| self.atoms.contains_key(&at))
                        {
                            self.sink.g3_begin();
                            for c in self.src[p..p + run - 1].chars() {
                                if is_replaced(c) {
                                    self.sink.replace(c);
                                } else {
                                    let mut buf = [0u8; 4];
                                    self.sink.keep(c.encode_utf8(&mut buf));
                                }
                            }
                            self.sink.keep(";");
                            self.sink.g3_end();
                            p += run;
                        }
                    }
                }
                continue;
            }
            // A5: the character rule.
            let c = self.char_at(p);
            if is_replaced(c) {
                self.sink.replace(c);
            } else {
                self.sink.keep(&rest[..c.len_utf8()]);
            }
            p += c.len_utf8();
        }
    }

    /// When `p` starts a maximal run of non-whitespace characters in
    /// `start..end` that holds no atom, the run's end.
    fn bare_run(&self, p: usize, start: usize, end: usize) -> Option<usize> {
        if p > start && !self.src[..p].chars().next_back().is_some_and(char::is_whitespace) {
            return None;
        }
        let run = self.src[p..end].find(char::is_whitespace).map_or(end, |i| p + i);
        if run == p || (p..run).any(|at| self.atoms.contains_key(&at)) {
            return None;
        }
        Some(run)
    }

    /// A8: a bracket annotation at `p` whose part after its first `;` or `,`
    /// is a code: reports it and returns the position after its `]`.
    fn annotation(&mut self, p: usize, end: usize) -> Option<usize> {
        let inner = &self.src[p + 1..end];
        let close = inner.find([']', '[', '\n', '\r'])?;
        if inner.as_bytes()[close] != b']' {
            return None;
        }
        let close = p + 1 + close;
        if (p + 1..close).any(|at| self.atoms.contains_key(&at)) {
            return None;
        }
        let content = &self.src[p + 1..close];
        let sep = content.find([';', ','])?;
        let code = &content[sep + 1..];
        let is_code = code.split([';', ',']).all(|item| {
            (1..=5).contains(&item.len()) && item.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
        if !is_code {
            return None;
        }
        self.sink.keep("[");
        self.text(p + 1, p + 1 + sep);
        self.sink.keep(&self.src[p + 1 + sep..close]);
        self.sink.keep("]");
        Some(close + 1)
    }

    /// A7: image-occlusion shape data from `p` (at `image-occlusion:`) to
    /// the next opener shape or `}}`: kept, except `text` property values.
    fn io_data(&mut self, p: usize, end: usize) -> usize {
        let mut region_end = end;
        let mut i = p;
        while i < end {
            let rest = &self.src[i..end];
            if rest.starts_with("}}") || opener_len(rest).is_some() {
                region_end = i;
                break;
            }
            i = self.step(i);
        }
        let mut q = p + IMAGE_OCCLUSION.len();
        self.kept(p, q);
        let shape_end = self.unescaped_colon(q, region_end);
        self.kept(q, shape_end);
        q = shape_end;
        while q < region_end {
            // At a ':' that starts a property.
            self.kept(q, q + 1);
            q += 1;
            let segment_end = self.unescaped_colon(q, region_end);
            match self.src[q..segment_end].find('=') {
                Some(eq) if &self.src[q..q + eq] == "text" => {
                    self.kept(q, q + eq + 1);
                    self.text(q + eq + 1, segment_end);
                }
                _ => self.kept(q, segment_end),
            }
            q = segment_end;
        }
        region_end
    }

    /// The next `:` from `q` not escaped by `\`, or `end`.
    fn unescaped_colon(&self, q: usize, end: usize) -> usize {
        let mut i = q;
        while i < end {
            if self.atoms.contains_key(&i) {
                i = self.step(i);
                continue;
            }
            match self.src.as_bytes()[i] {
                b'\\' => {
                    i += 1;
                    if i < end {
                        i = self.step(i);
                    }
                }
                b':' => return i,
                _ => i = self.step(i),
            }
        }
        end
    }
}
