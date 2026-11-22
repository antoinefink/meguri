//! The Card Text row over a note type's styling (`notetypes/workspace` W10,
//! W10.1, W10.2): reading the one plain `.card` rule's font, size,
//! alignment, text colour and background, and editing one of them.
//!
//! Both are pure functions of the CSS text. An edit changes only the
//! characters of one declaration's value, or adds one declaration at the
//! end of the rule's block as W10 says; every other byte of the text stays
//! as it was. The user's styling is never reformatted or repaired: when
//! the text is not well formed (W10.1) there is no row, so nothing reads
//! or edits it.

mod colour;
mod highlight;
mod keywords;
mod tokens;

use std::ops::Range;

pub use highlight::{CssHighlight, css_highlights};
use tokens::{Kind, Token, tokenize};

/// An 8-bit sRGB colour with alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colour {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// The Font control (W10.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Font {
    /// `system-ui`, `-apple-system` or `BlinkMacSystemFont` first; writes
    /// `system-ui, -apple-system, sans-serif`.
    System,
    /// A family name, generic families included.
    Family(String),
}

/// The Alignment control, in the interface's direction (W10.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Leading,
    Center,
    Trailing,
}

/// What the Card Text row's five controls show; `None` is a control's
/// empty state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardText {
    pub font: Option<Font>,
    /// Points as pixels: a `px` value's number, a `pt` value × 4/3 rounded.
    pub size: Option<f64>,
    pub alignment: Option<Alignment>,
    pub text_colour: Option<Colour>,
    pub background: Option<Colour>,
}

/// One control's change.
#[derive(Clone, Debug, PartialEq)]
pub enum CardTextEdit {
    Font(Font),
    /// Points, written as `<n>px`; finite and above 0.
    Size(f64),
    Alignment(Alignment),
    /// Written `#rrggbb`, or `#rrggbbaa` when not opaque.
    TextColour(Colour),
    /// Written `#rrggbb`; the alpha is not read.
    Background(Colour),
}

/// An edit's result: the whole new text, and the byte range in it of the
/// value written (the declaration's line is the one to highlight, W10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardTextEdited {
    pub css: String,
    pub value: Range<usize>,
}

/// Why an edit wrote nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardTextRefusal {
    /// The row is absent (W10.1): no single plain `.card` rule, or the
    /// text is not well formed.
    NoRow,
    /// A size that is not a finite number above 0, or a family name that
    /// is empty or holds a control character or a line break.
    InvalidValue,
    /// The edit would not take effect as written: read again, the rule
    /// would not show the value (the text around it would absorb it).
    /// Nothing is written.
    NotApplied,
}

/// One declaration of the rule's block.
struct Declaration {
    /// The property, ASCII lower case.
    name: String,
    /// Token indexes of the value: from the first significant token after
    /// `:` to the last before `!important` or the end, exclusive end. Empty
    /// when the value has no significant token.
    value: Range<usize>,
    /// The bytes those tokens cover; for an empty value, the point right
    /// after `:` and the white space that follows it.
    value_bytes: Range<usize>,
}

/// The plain `.card` rule's block: its declarations, and the byte of its
/// closing `}`.
struct Rule {
    declarations: Vec<Declaration>,
    /// The block's last item, whatever its shape (an invalid declaration
    /// or an at-rule statement included): the new declaration goes after it.
    tail: Option<Tail>,
    close: usize,
}

/// The last item of a block.
struct Tail {
    /// The byte after its last significant token.
    end: usize,
    /// It ends with neither its own `;` nor a `{}` block.
    open: bool,
    /// Its last token would absorb a `;` written right after it: a string
    /// broken by a line break, or a lone `\`.
    absorbs: bool,
}

/// The block-closing token's index for each opening one, or `None` when
/// the text is not well formed (W10.1): it ends inside a comment, a string,
/// a `url(` or a block, or a `}` closes no block.
fn match_blocks(tokens: &[Token]) -> Option<Vec<Option<usize>>> {
    let mut closes = vec![None; tokens.len()];
    let mut open: Vec<usize> = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let opener = |i: &usize| &tokens[*i].kind;
        match token.kind {
            Kind::OpenCurly | Kind::OpenParen | Kind::OpenSquare | Kind::Function(_) => open.push(at),
            Kind::CloseCurly => match open.last().map(opener) {
                Some(Kind::OpenCurly) => closes[open.pop()?] = Some(at),
                _ => return None,
            },
            Kind::CloseParen => {
                if matches!(open.last().map(opener), Some(Kind::OpenParen | Kind::Function(_))) {
                    closes[open.pop()?] = Some(at);
                }
            }
            Kind::CloseSquare => {
                if matches!(open.last().map(opener), Some(Kind::OpenSquare)) {
                    closes[open.pop()?] = Some(at);
                }
            }
            _ => {}
        }
    }
    open.is_empty().then_some(closes)
}

/// The index after the component value that starts at `at` (a block or a
/// function as a whole).
fn after_component(at: usize, closes: &[Option<usize>]) -> usize {
    closes[at].map_or(at + 1, |close| close + 1)
}

/// The top-level rules whose prelude, without comments and trimmed of
/// White_Space, is exactly `.card` (W10.1): their `{` and `}` indexes.
fn plain_card_rules(source: &str, tokens: &[Token], closes: &[Option<usize>]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        match tokens[at].kind {
            Kind::Whitespace | Kind::Comment | Kind::Cdo | Kind::Cdc => at += 1,
            // An at-rule ends at its `;` or after its block.
            Kind::AtKeyword(_) => {
                while at < tokens.len() {
                    match tokens[at].kind {
                        Kind::Semicolon => {
                            at += 1;
                            break;
                        }
                        Kind::OpenCurly => {
                            at = after_component(at, closes);
                            break;
                        }
                        _ => at = after_component(at, closes),
                    }
                }
            }
            // A qualified rule: its prelude runs to the `{` of its block.
            _ => {
                let prelude_start = at;
                while at < tokens.len() && tokens[at].kind != Kind::OpenCurly {
                    at = after_component(at, closes);
                }
                if at >= tokens.len() {
                    break;
                }
                let prelude: String = tokens[prelude_start..at]
                    .iter()
                    .filter(|t| t.kind != Kind::Comment)
                    .map(|t| &source[t.span.clone()])
                    .collect();
                let close = closes[at].unwrap_or(at);
                if prelude.trim() == ".card" {
                    found.push((at, close));
                }
                at = close + 1;
            }
        }
    }
    found
}

fn significant(tokens: &[Token], range: Range<usize>) -> Vec<usize> {
    range.filter(|i| !tokens[*i].is_trivia()).collect()
}

/// The declarations of the block between `open` and `close` (CSS's list
/// of declarations; a nested rule or an at-rule is skipped whole).
fn declarations(
    tokens: &[Token],
    closes: &[Option<usize>],
    open: usize,
    close: usize,
) -> (Vec<Declaration>, Option<Tail>) {
    let mut found = Vec::new();
    let mut tail = None;
    let mut at = open + 1;
    while at < close {
        match tokens[at].kind {
            Kind::Whitespace | Kind::Comment | Kind::Semicolon => {
                at += 1;
                continue;
            }
            _ => {}
        }
        let start = at;
        let mut nested = matches!(tokens[at].kind, Kind::AtKeyword(_));
        // A custom property's value may hold a `{}` block (CSS Syntax): it
        // runs to its `;` like any declaration, never as a nested rule.
        let custom = matches!(&tokens[start].kind, Kind::Ident(name) if name.starts_with("--"))
            && (start + 1..close)
                .find(|i| !tokens[*i].is_trivia())
                .is_some_and(|i| tokens[i].kind == Kind::Colon);
        let mut terminated = false;
        let mut block = false;
        while at < close {
            match tokens[at].kind {
                Kind::Semicolon => {
                    terminated = true;
                    break;
                }
                Kind::OpenCurly if !custom => {
                    nested = true;
                    block = true;
                    at = after_component(at, closes);
                    break;
                }
                _ => at = after_component(at, closes),
            }
        }
        let end = at;
        if terminated {
            at += 1;
        }
        if let Some(last) = (start..end).rev().find(|i| !tokens[*i].is_trivia()) {
            tail = Some(Tail {
                end: tokens[last].span.end,
                open: !terminated && !block,
                absorbs: matches!(tokens[last].kind, Kind::BadString | Kind::Delim('\\')),
            });
        }
        if nested {
            continue;
        }
        let Kind::Ident(name) = &tokens[start].kind else {
            continue;
        };
        let Some(colon) = (start + 1..end).find(|i| !tokens[*i].is_trivia()) else {
            continue;
        };
        if tokens[colon].kind != Kind::Colon {
            continue;
        }
        let mut value = significant(tokens, colon + 1..end);
        // `!important` stays where it is.
        if let [.., mark, word] = value.as_slice()
            && tokens[*mark].kind == Kind::Delim('!')
            && matches!(&tokens[*word].kind, Kind::Ident(text) if text.eq_ignore_ascii_case(keywords::IMPORTANT))
        {
            let mark = *mark;
            value.retain(|i| *i < mark);
        }
        let (value, value_bytes) = match (value.first(), value.last()) {
            (Some(first), Some(last)) => (*first..last + 1, tokens[*first].span.start..tokens[*last].span.end),
            _ => {
                let mut point = colon + 1;
                while point < end && tokens[point].kind == Kind::Whitespace {
                    point += 1;
                }
                let byte = tokens.get(point).map_or(tokens[colon].span.end, |t| t.span.start);
                (point..point, byte..byte)
            }
        };
        found.push(Declaration {
            name: name.to_ascii_lowercase(),
            value,
            value_bytes,
        });
    }
    (found, tail)
}

/// W10.1: the one plain `.card` rule of a well-formed text, or `None`.
fn the_rule(source: &str, tokens: &[Token], unclosed: bool) -> Option<Rule> {
    if unclosed {
        return None;
    }
    let closes = match_blocks(tokens)?;
    let rules = plain_card_rules(source, tokens, &closes);
    let [(open, close)] = rules.as_slice() else {
        return None;
    };
    let (declarations, tail) = declarations(tokens, &closes, *open, *close);
    Some(Rule {
        declarations,
        tail,
        close: tokens[*close].span.start,
    })
}

/// The last declaration of `property` (CSS's cascade within a rule).
fn last<'a>(rule: &'a Rule, property: &str) -> Option<&'a Declaration> {
    rule.declarations.iter().rev().find(|d| d.name == property)
}

fn value_tokens<'a>(tokens: &'a [Token], declaration: &Declaration) -> &'a [Token] {
    &tokens[declaration.value.clone()]
}

/// The one significant token of a value, if it has exactly one.
fn single<'a>(tokens: &'a [Token], declaration: &Declaration) -> Option<&'a Kind> {
    let value = value_tokens(tokens, declaration);
    let mut significant = value.iter().filter(|t| !t.is_trivia());
    let first = significant.next()?;
    significant.next().is_none().then_some(&first.kind)
}

fn read_font(tokens: &[Token], declaration: &Declaration) -> Option<Font> {
    let value = value_tokens(tokens, declaration);
    let first: Vec<&Token> = value
        .iter()
        .take_while(|t| t.kind != Kind::Comma)
        .filter(|t| !t.is_trivia())
        .collect();
    match first.as_slice() {
        [Token {
            kind: Kind::String(name),
            ..
        }] => Some(Font::Family(name.clone())),
        [Token {
            kind: Kind::Ident(name),
            ..
        }] if is_system_alias(name) => {
            Some(Font::System)
        }
        idents if !idents.is_empty() => {
            let names: Option<Vec<&str>> = idents
                .iter()
                .map(|t| match &t.kind {
                    Kind::Ident(name) => Some(name.as_str()),
                    _ => None,
                })
                .collect();
            names.map(|names| Font::Family(names.join(" ")))
        }
        _ => None,
    }
}

fn read_size(tokens: &[Token], declaration: &Declaration) -> Option<f64> {
    match single(tokens, declaration)? {
        Kind::Dimension(n, unit) if unit.eq_ignore_ascii_case("px") => Some(*n),
        Kind::Dimension(n, unit) if unit.eq_ignore_ascii_case("pt") => Some((n * 4.0 / 3.0).round()),
        _ => None,
    }
}

fn read_alignment(tokens: &[Token], declaration: &Declaration, rtl: bool) -> Option<Alignment> {
    let Kind::Ident(word) = single(tokens, declaration)? else {
        return None;
    };
    let (left, right) = if rtl {
        (Alignment::Trailing, Alignment::Leading)
    } else {
        (Alignment::Leading, Alignment::Trailing)
    };
    match word.to_ascii_lowercase().as_str() {
        "left" => Some(left),
        "right" => Some(right),
        "center" => Some(Alignment::Center),
        "start" => Some(Alignment::Leading),
        "end" => Some(Alignment::Trailing),
        _ => None,
    }
}

/// The declaration and token range the Background control reads and
/// edits: the last `background-color`, else the last `background` when
/// its value is a single colour (W10.2).
fn background(rule: &Rule, tokens: &[Token]) -> Option<(usize, Colour)> {
    if let Some(at) = rule.declarations.iter().rposition(|d| d.name == "background-color") {
        return colour::read(value_tokens(tokens, &rule.declarations[at])).map(|c| (at, c));
    }
    let at = rule.declarations.iter().rposition(|d| d.name == "background")?;
    colour::read(value_tokens(tokens, &rule.declarations[at])).map(|c| (at, c))
}

/// W10: the Card Text row of a note type's CSS, or `None` when the row is
/// absent. `rtl`: the interface runs right to left (W10.2's Alignment).
pub fn read(css: &str, rtl: bool) -> Option<CardText> {
    let lexed = tokenize(css);
    let tokens = &lexed.tokens;
    let rule = the_rule(css, tokens, lexed.unclosed)?;
    Some(CardText {
        font: last(&rule, "font-family").and_then(|d| read_font(tokens, d)),
        size: last(&rule, "font-size").and_then(|d| read_size(tokens, d)),
        alignment: last(&rule, "text-align").and_then(|d| read_alignment(tokens, d, rtl)),
        text_colour: last(&rule, "color").and_then(|d| colour::read(value_tokens(tokens, d))),
        background: background(&rule, tokens).map(|(_, c)| c),
    })
}

fn hex(colour: Colour, alpha: bool) -> String {
    let Colour {
        red,
        green,
        blue,
        alpha: a,
    } = colour;
    if alpha && a != 255 {
        format!("#{red:02x}{green:02x}{blue:02x}{a:02x}")
    } else {
        format!("#{red:02x}{green:02x}{blue:02x}")
    }
}

/// W10.2's names that the Font control shows as "System".
fn is_system_alias(name: &str) -> bool {
    ["system-ui", "-apple-system", "blinkmacsystemfont"].contains(&name.to_ascii_lowercase().as_str())
}

/// A family name as W10.2 writes it: quoted when it holds a space. It is
/// also quoted when it holds a character an unquoted name cannot (one
/// outside letters, digits, `-`, `_` and non-ASCII), so the written value
/// never ends the declaration or the rule.
fn family(name: &str) -> Result<String, CardTextRefusal> {
    if name.is_empty() || name.chars().any(char::is_control) || name.chars().any(|c| c == '\u{2028}' || c == '\u{2029}') {
        return Err(CardTextRefusal::InvalidValue);
    }
    // Unquoted only when it reads back as one identifier that is not a
    // CSS-wide keyword (which would change the value's meaning).
    let mut chars = name.chars();
    let starts = match (chars.next(), chars.next()) {
        (Some('-'), Some(second)) => second.is_ascii_alphabetic() || second == '_' || second == '-' || !second.is_ascii(),
        (Some(first), _) => first.is_ascii_alphabetic() || first == '_' || !first.is_ascii(),
        (None, _) => false,
    };
    // A family named like a System alias stays a family only when quoted.
    let keyword = ["inherit", "initial", "unset", "revert", "revert-layer", "default"]
        .iter()
        .any(|k| name.eq_ignore_ascii_case(k))
        || is_system_alias(name);
    let plain = starts
        && !keyword
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii());
    if plain {
        return Ok(name.to_owned());
    }
    let mut quoted = String::from("\"");
    for c in name.chars() {
        if c == '"' || c == '\\' {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    Ok(quoted)
}

fn size(points: f64) -> Result<String, CardTextRefusal> {
    if !points.is_finite() || points <= 0.0 {
        return Err(CardTextRefusal::InvalidValue);
    }
    Ok(format!("{points}px"))
}

/// The newline that ends the line before byte `at`'s line.
fn newline_before(css: &str, line_start: usize) -> &'static str {
    let before = &css.as_bytes()[..line_start];
    match before {
        [.., b'\r', b'\n'] => "\r\n",
        [.., b'\r'] => "\r",
        _ => "\n",
    }
}

/// Whether the `}` at byte `close` stands alone on its line, and the byte
/// where that line starts.
fn alone_on_its_line(css: &str, close: usize) -> Option<usize> {
    let bytes = css.as_bytes();
    let line_start = bytes[..close]
        .iter()
        .rposition(|b| matches!(b, b'\n' | b'\r' | b'\x0c'))
        .map(|at| at + 1)?;
    let blank = |b: &u8| matches!(b, b' ' | b'\t');
    let before = bytes[line_start..close].iter().all(blank);
    let after = bytes[close + 1..]
        .iter()
        .take_while(|b| !matches!(b, b'\n' | b'\r' | b'\x0c'))
        .all(blank);
    (before && after).then_some(line_start)
}

/// W10: one control's change to the CSS. The value replaces exactly the
/// characters of the last declaration of its property in the plain `.card`
/// rule (`!important` and every other byte stay); with no such declaration
/// one is added at the end of the rule's block.
pub fn edit(css: &str, change: &CardTextEdit, rtl: bool) -> Result<CardTextEdited, CardTextRefusal> {
    let edited = write(css, change, rtl)?;
    // Read the result again: the row must still show, and the value must be
    // the characters of its property's last declaration.
    let lexed = tokenize(&edited.css);
    let rule = the_rule(&edited.css, &lexed.tokens, lexed.unclosed).ok_or(CardTextRefusal::NotApplied)?;
    let property = match change {
        CardTextEdit::Font(_) => "font-family",
        CardTextEdit::Size(_) => "font-size",
        CardTextEdit::Alignment(_) => "text-align",
        CardTextEdit::TextColour(_) => "color",
        CardTextEdit::Background(_) => "background-color",
    };
    let shows = |name: &str| last(&rule, name).is_some_and(|d| d.value_bytes == edited.value);
    if shows(property) || (property == "background-color" && shows("background")) {
        Ok(edited)
    } else {
        Err(CardTextRefusal::NotApplied)
    }
}

fn write(css: &str, change: &CardTextEdit, rtl: bool) -> Result<CardTextEdited, CardTextRefusal> {
    let lexed = tokenize(css);
    let tokens = &lexed.tokens;
    let rule = the_rule(css, tokens, lexed.unclosed).ok_or(CardTextRefusal::NoRow)?;
    let (property, value) = match change {
        CardTextEdit::Font(Font::System) => ("font-family", "system-ui, -apple-system, sans-serif".to_owned()),
        CardTextEdit::Font(Font::Family(name)) => ("font-family", family(name)?),
        CardTextEdit::Size(points) => ("font-size", size(*points)?),
        CardTextEdit::Alignment(alignment) => {
            let word = match (alignment, rtl) {
                (Alignment::Center, _) => "center",
                (Alignment::Leading, false) | (Alignment::Trailing, true) => "left",
                (Alignment::Leading, true) | (Alignment::Trailing, false) => "right",
            };
            ("text-align", word.to_owned())
        }
        CardTextEdit::TextColour(colour) => ("color", hex(*colour, true)),
        CardTextEdit::Background(colour) => ("background-color", hex(*colour, false)),
    };
    let target = match change {
        // The shorthand's single colour is edited only when the rule has no
        // `background-color` (W10.2).
        CardTextEdit::Background(_) => background(&rule, tokens)
            .map(|(at, _)| &rule.declarations[at])
            .or_else(|| last(&rule, "background-color")),
        _ => last(&rule, property),
    };
    if let Some(declaration) = target {
        let range = declaration.value_bytes.clone();
        let mut out = String::with_capacity(css.len() + value.len());
        out.push_str(&css[..range.start]);
        out.push_str(&value);
        // Comments between the old value's tokens stay, each after a space,
        // in their order.
        for comment in tokens[declaration.value.clone()].iter().filter(|t| t.kind == Kind::Comment) {
            out.push(' ');
            out.push_str(&css[comment.span.clone()]);
        }
        out.push_str(&css[range.end..]);
        return Ok(CardTextEdited {
            css: out,
            value: range.start..range.start + value.len(),
        });
    }
    // A new declaration at the end of the block.
    let mut out = String::with_capacity(css.len() + value.len() + property.len() + 8);
    let mut copied = 0;
    if let Some(tail) = &rule.tail
        && tail.open
    {
        // A `;` there would join the broken string or the escape: the new
        // declaration would not be one.
        if tail.absorbs {
            return Err(CardTextRefusal::NotApplied);
        }
        out.push_str(&css[..tail.end]);
        out.push(';');
        copied = tail.end;
    }
    let (insert_at, before, after) = match alone_on_its_line(css, rule.close) {
        Some(line_start) => (line_start, "  ", format!(";{}", newline_before(css, line_start))),
        None => (rule.close, " ", "; ".to_owned()),
    };
    out.push_str(&css[copied..insert_at]);
    out.push_str(before);
    out.push_str(property);
    out.push_str(": ");
    let start = out.len();
    out.push_str(&value);
    let end = out.len();
    out.push_str(&after);
    out.push_str(&css[insert_at..]);
    Ok(CardTextEdited {
        css: out,
        value: start..end,
    })
}
