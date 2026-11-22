//! The template editor's colouring of Styling (`notetypes/workspace` W6):
//! over the CSS Syntax Level 3 tokens, comments, at-rule keywords,
//! declaration properties and their values, and inside values each colour
//! W10.2 reads (a hex, a named colour, an `rgb()`, `rgba()`, `hsl()` or
//! `hsla()` function), with the colour, for its swatch. Selectors stay
//! plain. Pure, and it reads any text, well formed or not.

use std::ops::Range;

use super::Colour;
use super::colour;
use super::tokens::{Kind, Token, tokenize};

/// What a coloured range of the styling is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CssHighlight {
    Comment,
    /// `@media`, `@import` and the like: the keyword only.
    AtRule,
    /// A declaration's property name.
    Property,
    /// A declaration's value, from its first significant token to its last.
    Value,
    /// A colour inside a value, which its swatch shows and edits.
    Colour(Colour),
}

/// The coloured ranges of `css` in source order. The colour and comment
/// ranges of a value lie inside the value's range, after it; no other two
/// ranges overlap.
pub fn css_highlights(css: &str) -> Vec<(Range<usize>, CssHighlight)> {
    let tokens = tokenize(css).tokens;
    let mut spans = Vec::new();
    // Each open block: whether it holds declarations (a rule's block) or
    // rules (`@media`'s). Top level holds rules.
    let mut blocks: Vec<bool> = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        match &token.kind {
            Kind::Comment => spans.push((token.span.clone(), CssHighlight::Comment)),
            Kind::Whitespace | Kind::Semicolon => {}
            Kind::CloseCurly => {
                blocks.pop();
            }
            Kind::AtKeyword(_) => {
                spans.push((token.span.clone(), CssHighlight::AtRule));
                // Its prelude runs to `;` or `{`; a `{` opens a block of
                // rules for the conditional group rules, of declarations
                // for the others (`@font-face`, `@page`).
                let name = match &token.kind {
                    Kind::AtKeyword(name) => name.to_ascii_lowercase(),
                    _ => String::new(),
                };
                let (end, opens) = item_end(&tokens, at + 1);
                comments(&tokens, at + 1..end, &mut spans);
                if opens {
                    blocks.push(!matches!(name.as_str(), "media" | "supports" | "document" | "layer" | "container" | "scope"));
                }
                at = next(&tokens, end);
                continue;
            }
            _ => {
                let (end, opens) = item_end(&tokens, at);
                if opens || !blocks.last().copied().unwrap_or(false) {
                    // A rule's prelude (a selector), or stray tokens at a
                    // level that holds rules.
                    comments(&tokens, at..end, &mut spans);
                    if opens {
                        blocks.push(true);
                    }
                } else {
                    declaration(&tokens, at..end, &mut spans);
                }
                at = next(&tokens, end);
                continue;
            }
        }
        at += 1;
    }
    spans
}

/// Where the scan goes on after an item ending at `end`: past its `;` or
/// `{`, but onto a `}`, which closes the enclosing block.
fn next(tokens: &[Token], end: usize) -> usize {
    if tokens.get(end).is_some_and(|t| t.kind == Kind::CloseCurly) { end } else { end + 1 }
}

/// From `from`, the index of the token ending the item (its `;`, the `{`
/// opening its block, the `}` closing the enclosing block, or the end),
/// and whether that token is a `{`. Brackets nest; a `}` inside them still
/// ends the item, as W10.1 reads a `}` that closes no bracket.
fn item_end(tokens: &[Token], from: usize) -> (usize, bool) {
    let mut depth = 0usize;
    for (at, token) in tokens.iter().enumerate().skip(from) {
        match token.kind {
            Kind::OpenParen | Kind::OpenSquare | Kind::Function(_) => depth += 1,
            Kind::CloseParen | Kind::CloseSquare => depth = depth.saturating_sub(1),
            Kind::Semicolon if depth == 0 => return (at, false),
            Kind::OpenCurly => return (at, true),
            Kind::CloseCurly => return (at, false),
            _ => {}
        }
    }
    (tokens.len(), false)
}

fn comments(tokens: &[Token], range: Range<usize>, spans: &mut Vec<(Range<usize>, CssHighlight)>) {
    for token in &tokens[range.start..range.end.min(tokens.len())] {
        if token.kind == Kind::Comment {
            spans.push((token.span.clone(), CssHighlight::Comment));
        }
    }
}

/// One item of a declaration block: `property: value`, coloured when it
/// has that shape; anything else keeps only its comments coloured.
fn declaration(tokens: &[Token], range: Range<usize>, spans: &mut Vec<(Range<usize>, CssHighlight)>) {
    let items = &tokens[range.start..range.end.min(tokens.len())];
    let significant: Vec<usize> = (0..items.len()).filter(|i| !items[*i].is_trivia()).collect();
    let (Some(&name), Some(&colon)) = (significant.first(), significant.get(1)) else {
        comments(tokens, range, spans);
        return;
    };
    if !matches!(items[name].kind, Kind::Ident(_)) || items[colon].kind != Kind::Colon {
        comments(tokens, range, spans);
        return;
    }
    spans.push((items[name].span.clone(), CssHighlight::Property));
    comments(tokens, range.start + name + 1..range.start + colon, spans);
    let value: Vec<usize> = significant[2..].to_vec();
    if let (Some(&first), Some(&last)) = (value.first(), value.last()) {
        spans.push((items[first].span.start..items[last].span.end, CssHighlight::Value));
        let mut at = first;
        while at <= last {
            let token = &items[at];
            match &token.kind {
                Kind::Comment => spans.push((token.span.clone(), CssHighlight::Comment)),
                Kind::Hash(_) | Kind::Ident(_) => {
                    if let Some(c) = colour::read(std::slice::from_ref(token)) {
                        spans.push((token.span.clone(), CssHighlight::Colour(c)));
                    }
                }
                Kind::Function(name) if matches!(name.to_ascii_lowercase().as_str(), "rgb" | "rgba" | "hsl" | "hsla") => {
                    let close = closing_paren(items, at);
                    if let Some(close) = close.filter(|c| *c <= last) {
                        if let Some(c) = colour::read(&items[at..=close]) {
                            spans.push((token.span.start..items[close].span.end, CssHighlight::Colour(c)));
                        }
                        at = close + 1;
                        continue;
                    }
                }
                _ => {}
            }
            at += 1;
        }
    }
}

/// The `)` that closes the function or bracket opened at `open`.
fn closing_paren(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            Kind::OpenParen | Kind::Function(_) => depth += 1,
            Kind::CloseParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use CssHighlight::{AtRule, Comment, Property, Value};

    fn swatch(c: Colour) -> CssHighlight {
        CssHighlight::Colour(c)
    }

    fn coloured(css: &str) -> Vec<(&str, CssHighlight)> {
        css_highlights(css).into_iter().map(|(r, h)| (&css[r], h)).collect()
    }

    const RED: Colour = Colour {
        red: 255,
        green: 0,
        blue: 0,
        alpha: 255,
    };

    #[test]
    fn properties_values_and_colours_take_their_colours() {
        assert_eq!(
            coloured(".card {\n  color: #f00; /* note */\n  border: 1px solid red;\n}"),
            vec![
                ("color", Property),
                ("#f00", Value),
                ("#f00", swatch(RED)),
                ("/* note */", Comment),
                ("border", Property),
                ("1px solid red", Value),
                ("red", swatch(RED)),
            ]
        );
    }

    #[test]
    fn colour_functions_are_one_colour_and_selectors_stay_plain() {
        assert_eq!(
            coloured("a:hover, .x { background: rgb(255 0 0 / 50%) url(x.png) }"),
            vec![
                ("background", Property),
                ("rgb(255 0 0 / 50%) url(x.png)", Value),
                ("rgb(255 0 0 / 50%)", swatch(Colour { alpha: 128, ..RED })),
            ]
        );
    }

    #[test]
    fn at_rules_hold_rules_or_declarations() {
        assert_eq!(
            coloured("@media (prefers-color-scheme: dark) {\n  .card.nightMode a:link { color: white }\n}\n@font-face { font-family: X }"),
            vec![
                ("@media", AtRule),
                ("color", Property),
                ("white", Value),
                (
                    "white",
                    swatch(Colour {
                        red: 255,
                        green: 255,
                        blue: 255,
                        alpha: 255
                    })
                ),
                ("@font-face", AtRule),
                ("font-family", Property),
                ("X", Value),
            ]
        );
    }

    #[test]
    fn any_text_reads_without_failing() {
        assert_eq!(coloured(""), vec![]);
        assert_eq!(coloured("}}} color: red"), vec![]);
        assert_eq!(coloured(".card { color: rgb(1,2"), vec![("color", Property), ("rgb(1,2", Value)]);
        assert_eq!(coloured("/* open"), vec![("/* open", Comment)]);
    }
}
