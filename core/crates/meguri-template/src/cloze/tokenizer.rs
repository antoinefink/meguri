//! C2-C4: markers, tokens and the cloze tree.
//!
//! Nesting is at most [`MAX_OPEN`] deep, so the tree and every walk over it
//! may recurse.

/// The prefix that makes a cloze an image-occlusion cloze (C11), and keeps
/// hint splitting off a run (C4).
const IMAGE_OCCLUSION_PREFIX: &str = "image-occlusion:";

/// C4: at most this many clozes are open; a further opening marker is
/// dropped (R972).
const MAX_OPEN: usize = 10;

const OPEN: &str = "{{c";
const SEPARATOR: &str = "::";
const CLOSE: &str = "}}";

/// A top-level item, or a child of a cloze.
#[derive(Debug)]
pub(super) enum Item<'a> {
    Text(&'a str),
    Cloze(Cloze<'a>),
}

/// A finished cloze (C4).
#[derive(Debug)]
pub(super) struct Cloze<'a> {
    /// Ascending and distinct (C2); 0 is kept.
    pub numbers: Vec<u16>,
    pub items: Vec<Item<'a>>,
    pub hint: Option<&'a str>,
}

impl<'a> Cloze<'a> {
    fn new(numbers: Vec<u16>) -> Cloze<'a> {
        Cloze {
            numbers,
            items: Vec::new(),
            hint: None,
        }
    }

    /// C4, a text run inside this cloze: split at its first `::` unless it
    /// starts with the image-occlusion prefix; the part after replaces the
    /// hint.
    fn add_run(&mut self, run: &'a str) {
        let split = (!run.starts_with(IMAGE_OCCLUSION_PREFIX))
            .then(|| run.split_once(SEPARATOR))
            .flatten();
        match split {
            Some((child, hint)) => {
                self.items.push(Item::Text(child));
                self.hint = Some(hint);
            }
            None => self.items.push(Item::Text(run)),
        }
    }

    /// C6: active on card `card` when `card` is in the number list.
    pub fn is_active(&self, card: u32) -> bool {
        self.numbers.iter().any(|&n| u32::from(n) == card)
    }

    /// C1: the number list L, `1,2,3`.
    pub fn number_list(&self) -> String {
        let numbers: Vec<String> = self.numbers.iter().map(u16::to_string).collect();
        numbers.join(",")
    }

    /// C11: the shape text of an image-occlusion cloze, `None` for any
    /// other cloze.
    pub fn image_occlusion_shape(&self) -> Option<&'a str> {
        match self.items.first() {
            Some(Item::Text(first)) => first.strip_prefix(IMAGE_OCCLUSION_PREFIX),
            _ => None,
        }
    }

    /// C9: the concatenation of every text inside, nested clozes' texts
    /// included, hints left out.
    pub fn plain_content(&self) -> String {
        let mut out = String::new();
        self.push_plain_content(&mut out);
        out
    }

    fn push_plain_content(&self, out: &mut String) {
        for item in &self.items {
            match item {
                Item::Text(text) => out.push_str(text),
                Item::Cloze(cloze) => cloze.push_plain_content(out),
            }
        }
    }
}

/// C3's tokens.
#[derive(Debug, PartialEq)]
enum Token<'a> {
    /// An opening marker's numbers (never empty).
    Open(Vec<u16>),
    Close,
    Text(&'a str),
}

/// C2: the numbers of the opening marker at the start of `rest` and its
/// length, or `None` when no opening marker starts there.
fn opening_marker(rest: &str) -> Option<(Vec<u16>, usize)> {
    let after_open = rest.strip_prefix(OPEN)?;
    let run_len = after_open
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b',')
        .count();
    let (run, after_run) = after_open.split_at(run_len);
    if !after_run.starts_with(SEPARATOR) {
        return None;
    }
    let mut numbers: Vec<u16> = run.split(',').filter_map(piece_number).collect();
    if numbers.is_empty() {
        return None;
    }
    numbers.sort_unstable();
    numbers.dedup();
    Some((numbers, OPEN.len() + run_len + SEPARATOR.len()))
}

/// C2: a piece of digits with a value from 0 to 65535; leading zeros are
/// allowed and the piece may be longer than any integer type.
fn piece_number(piece: &str) -> Option<u16> {
    if piece.is_empty() {
        return None;
    }
    match piece.trim_start_matches('0') {
        "" => Some(0),
        significant if significant.len() > 5 => None,
        significant => significant.parse::<u32>().ok().and_then(|n| u16::try_from(n).ok()),
    }
}

/// C3: whether an opening or closing marker starts at the start of `rest`.
fn marker_at(rest: &str) -> bool {
    rest.starts_with(CLOSE) || opening_marker(rest).is_some()
}

struct Tokens<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        let rest = &self.text[self.at..];
        if rest.is_empty() {
            return None;
        }
        if let Some((numbers, len)) = opening_marker(rest) {
            self.at += len;
            return Some(Token::Open(numbers));
        }
        if rest.starts_with(CLOSE) {
            self.at += CLOSE.len();
            return Some(Token::Close);
        }
        // The first character joins the run; the run ends where a marker
        // starts. Markers start with an ASCII brace, always a character
        // boundary.
        let bytes = rest.as_bytes();
        let end = (1..bytes.len())
            .find(|&i| matches!(bytes[i], b'{' | b'}') && marker_at(&rest[i..]))
            .unwrap_or(bytes.len());
        self.at += end;
        Some(Token::Text(&rest[..end]))
    }
}

fn tokens(text: &str) -> Tokens<'_> {
    Tokens { text, at: 0 }
}

/// C4: the top-level items of `text`. Clozes still open at the end are
/// discarded with everything inside them.
pub(super) fn parse(text: &str) -> Vec<Item<'_>> {
    let mut top: Vec<Item<'_>> = Vec::new();
    let mut open: Vec<Cloze<'_>> = Vec::new();
    for token in tokens(text) {
        match token {
            Token::Open(numbers) => {
                if open.len() < MAX_OPEN {
                    open.push(Cloze::new(numbers));
                }
            }
            Token::Text(run) => match open.last_mut() {
                Some(cloze) => cloze.add_run(run),
                None => top.push(Item::Text(run)),
            },
            Token::Close => match open.pop() {
                Some(finished) => match open.last_mut() {
                    Some(parent) => parent.items.push(Item::Cloze(finished)),
                    None => top.push(Item::Cloze(finished)),
                },
                None => top.push(Item::Text(CLOSE)),
            },
        }
    }
    top
}

/// Every cloze of the tree, a cloze before the clozes nested in it.
pub(super) fn for_each_cloze<'t, 'a>(items: &'t [Item<'a>], f: &mut impl FnMut(&'t Cloze<'a>)) {
    for item in items {
        if let Item::Cloze(cloze) = item {
            f(cloze);
            for_each_cloze(&cloze.items, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(text: &str) -> Vec<Token<'_>> {
        tokens(text).collect()
    }

    #[test]
    fn tokens_follow_c3() {
        assert_eq!(all("}}}"), [Token::Close, Token::Text("}")]);
        assert_eq!(
            all("{{{c1::x"),
            [Token::Text("{"), Token::Open(vec![1]), Token::Text("x")]
        );
        assert_eq!(
            all("a{{c::b}}"),
            [Token::Text("a{{c::b"), Token::Close],
            "a failed marker does not end the run"
        );
        assert_eq!(
            all("é{{c2,1::ü"),
            [Token::Text("é"), Token::Open(vec![1, 2]), Token::Text("ü")]
        );
        assert_eq!(all(""), []);
    }

    #[test]
    fn pieces_follow_c2() {
        assert_eq!(piece_number("0"), Some(0));
        assert_eq!(piece_number("000"), Some(0));
        assert_eq!(piece_number("007"), Some(7));
        assert_eq!(piece_number("65535"), Some(65535));
        assert_eq!(piece_number("0000000000065535"), Some(65535));
        assert_eq!(piece_number("65536"), None);
        assert_eq!(piece_number("99999999999999999999999"), None);
        assert_eq!(piece_number(""), None);
    }

    #[test]
    fn opening_markers_measure_their_length() {
        assert_eq!(opening_marker("{{c1::x"), Some((vec![1], 6)));
        assert_eq!(opening_marker("{{c3,1,3,2:::x"), Some((vec![1, 2, 3], 12)));
        assert_eq!(opening_marker("{{c0,65536::"), Some((vec![0], 12)));
        assert_eq!(opening_marker("{{c65536::"), None);
        assert_eq!(opening_marker("{{c1:"), None);
        assert_eq!(opening_marker("{{c1"), None);
    }
}
