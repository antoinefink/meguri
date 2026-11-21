//! W10.2's colour reading: hex forms of 3, 4, 6 or 8 digits, `rgb()`,
//! `rgba()`, `hsl()`, `hsla()` and the CSS named colours (CSS Color Level
//! 4, `transparent` included). Anything else reads as no colour.

use super::Colour;
use super::keywords::{GRADIANS, NAMED};
use super::tokens::{Kind, Token};

fn opaque(rgb: u32) -> Colour {
    Colour {
        red: (rgb >> 16) as u8,
        green: (rgb >> 8) as u8,
        blue: rgb as u8,
        alpha: 255,
    }
}

fn hex(digits: &str) -> Option<Colour> {
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |i: usize| u8::from_str_radix(&digits[i..=i], 16).ok();
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    let double = |n: u8| n * 17;
    match digits.len() {
        3 | 4 => Some(Colour {
            red: double(nibble(0)?),
            green: double(nibble(1)?),
            blue: double(nibble(2)?),
            alpha: if digits.len() == 4 { double(nibble(3)?) } else { 255 },
        }),
        6 | 8 => Some(Colour {
            red: byte(0)?,
            green: byte(2)?,
            blue: byte(4)?,
            alpha: if digits.len() == 8 { byte(6)? } else { 255 },
        }),
        _ => None,
    }
}

fn channel(value: f64) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

/// The arguments of a colour function, as numbers or percentages, and
/// whether they were separated by commas (the legacy form) or by spaces
/// with an optional `/ alpha`.
struct Arguments {
    values: Vec<Kind>,
    /// The position in `values` of the alpha value after `/`.
    slash: Option<usize>,
    commas: bool,
}

fn arguments(tokens: &[Token]) -> Option<Arguments> {
    let kinds: Vec<&Kind> = tokens.iter().filter(|t| !t.is_trivia()).map(|t| &t.kind).collect();
    let is_value = |kind: &Kind| matches!(kind, Kind::Number(_) | Kind::Percentage(_) | Kind::Dimension(..));
    let commas = kinds.iter().any(|kind| **kind == Kind::Comma);
    let mut parsed = Arguments {
        values: Vec::new(),
        slash: None,
        commas,
    };
    if commas {
        // value , value , value [, value]
        if kinds.len().is_multiple_of(2) {
            return None;
        }
        for (at, kind) in kinds.iter().enumerate() {
            match at % 2 {
                0 if is_value(kind) => parsed.values.push((*kind).clone()),
                1 if **kind == Kind::Comma => {}
                _ => return None,
            }
        }
    } else {
        // value value value [/ value]
        for kind in kinds {
            match kind {
                kind if is_value(kind) => parsed.values.push(kind.clone()),
                Kind::Delim('/') if parsed.slash.is_none() && !parsed.values.is_empty() => {
                    parsed.slash = Some(parsed.values.len());
                }
                _ => return None,
            }
        }
    }
    Some(parsed)
}

fn alpha(value: &Kind) -> Option<u8> {
    let fraction = match value {
        Kind::Number(n) => *n,
        Kind::Percentage(p) => p / 100.0,
        _ => return None,
    };
    Some((fraction.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The values and alpha of a function with three values and an optional
/// alpha, in either form.
fn three_and_alpha(args: &Arguments) -> Option<(&[Kind], Option<u8>)> {
    let count = args.values.len();
    match (args.commas, args.slash) {
        (true, None) if count == 3 || count == 4 => {
            let fourth = match args.values.get(3) {
                Some(value) => Some(alpha(value)?),
                None => None,
            };
            Some((&args.values[..3], fourth))
        }
        (false, None) if count == 3 => Some((&args.values[..3], None)),
        (false, Some(3)) if count == 4 => Some((&args.values[..3], Some(alpha(&args.values[3])?))),
        _ => None,
    }
}

fn rgb(args: &Arguments) -> Option<Colour> {
    let (values, alpha) = three_and_alpha(args)?;
    let all_numbers = values.iter().all(|v| matches!(v, Kind::Number(_)));
    let all_percentages = values.iter().all(|v| matches!(v, Kind::Percentage(_)));
    // The legacy form takes three numbers or three percentages.
    if args.commas && !all_numbers && !all_percentages {
        return None;
    }
    let mut channels = [0u8; 3];
    for (slot, value) in channels.iter_mut().zip(values) {
        *slot = match value {
            Kind::Number(n) => channel(*n),
            Kind::Percentage(p) => channel(p / 100.0 * 255.0),
            _ => return None,
        };
    }
    Some(Colour {
        red: channels[0],
        green: channels[1],
        blue: channels[2],
        alpha: alpha.unwrap_or(255),
    })
}

fn hsl(args: &Arguments) -> Option<Colour> {
    let (values, alpha) = three_and_alpha(args)?;
    let hue = match &values[0] {
        Kind::Number(n) => *n,
        Kind::Dimension(n, unit) => match unit.to_ascii_lowercase().as_str() {
            "deg" => *n,
            "rad" => n.to_degrees(),
            unit if unit == GRADIANS => n * 0.9,
            "turn" => n * 360.0,
            _ => return None,
        },
        _ => return None,
    };
    let fraction = |value: &Kind| match value {
        Kind::Percentage(p) => Some((p / 100.0).clamp(0.0, 1.0)),
        Kind::Number(n) if !args.commas => Some((n / 100.0).clamp(0.0, 1.0)),
        _ => None,
    };
    let saturation = fraction(&values[1])?;
    let lightness = fraction(&values[2])?;
    let hue = hue.rem_euclid(360.0);
    // CSS Color 4's hsl-to-rgb.
    let component = |n: f64| {
        let k = (n + hue / 30.0) % 12.0;
        let a = saturation * lightness.min(1.0 - lightness);
        lightness - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Some(Colour {
        red: channel(component(0.0) * 255.0),
        green: channel(component(8.0) * 255.0),
        blue: channel(component(4.0) * 255.0),
        alpha: alpha.unwrap_or(255),
    })
}

/// A value's significant tokens read as one colour: a hash, a named
/// colour, or one whole colour function. `None` for anything else.
pub(crate) fn read(value: &[Token]) -> Option<Colour> {
    let significant: Vec<&Token> = value.iter().filter(|t| !t.is_trivia()).collect();
    let (first, rest) = significant.split_first()?;
    match &first.kind {
        Kind::Hash(digits) if rest.is_empty() => hex(digits),
        Kind::Ident(name) if rest.is_empty() => {
            let name = name.to_ascii_lowercase();
            if name == "transparent" {
                return Some(Colour {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: 0,
                });
            }
            NAMED
                .iter()
                .find(|(named, _)| *named == name)
                .map(|(_, rgb)| opaque(*rgb))
        }
        Kind::Function(name) => {
            // The function must be the whole value: its `)` is the last
            // significant token, and nothing inside it nests.
            if rest.last().map(|t| &t.kind) != Some(&Kind::CloseParen) {
                return None;
            }
            let indexes: Vec<usize> = (0..value.len()).filter(|i| !value[*i].is_trivia()).collect();
            let inner = &value[indexes[0] + 1..indexes[indexes.len() - 1]];
            if inner.iter().any(|t| {
                matches!(
                    t.kind,
                    Kind::Function(_) | Kind::OpenParen | Kind::CloseParen | Kind::OpenSquare | Kind::OpenCurly
                )
            }) {
                return None;
            }
            let args = arguments(inner)?;
            match name.to_ascii_lowercase().as_str() {
                "rgb" | "rgba" => rgb(&args),
                "hsl" | "hsla" => hsl(&args),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::read;
    use crate::notetypes::card_text::keywords::NAMED;
    use crate::notetypes::card_text::tokens::tokenize;

    fn colour(text: &str) -> Option<(u8, u8, u8, u8)> {
        read(&tokenize(text).tokens).map(|c| (c.red, c.green, c.blue, c.alpha))
    }

    #[test]
    fn the_named_colours_are_css_colour_4s() {
        assert_eq!(NAMED.len(), 148);
        let mut names: Vec<&str> = NAMED.iter().map(|(n, _)| *n).collect();
        names.dedup();
        assert_eq!(names.len(), 148);
        assert!(names.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(colour("RebeccaPurple"), Some((0x66, 0x33, 0x99, 255)));
        assert_eq!(colour("transparent"), Some((0, 0, 0, 0)));
    }

    #[test]
    fn hex_and_functions_read_their_colour() {
        assert_eq!(colour("#0aF"), Some((0, 0xaa, 0xff, 255)));
        assert_eq!(colour("#0aF8"), Some((0, 0xaa, 0xff, 0x88)));
        assert_eq!(colour("#123456"), Some((0x12, 0x34, 0x56, 255)));
        assert_eq!(colour("#12345678"), Some((0x12, 0x34, 0x56, 0x78)));
        assert_eq!(colour("#12345"), None);
        assert_eq!(colour("#ggg"), None);
        assert_eq!(colour("rgb(255, 0, 128)"), Some((255, 0, 128, 255)));
        assert_eq!(colour("RGBA(100%, 0%, 50%, 0.5)"), Some((255, 0, 128, 128)));
        assert_eq!(colour("rgb(255 0 0 / 50%)"), Some((255, 0, 0, 128)));
        assert_eq!(colour("rgb( 300 , -5 , 0 )"), Some((255, 0, 0, 255)));
        assert_eq!(colour("rgb(255, 0%, 0)"), None);
        assert_eq!(colour("rgb(255, 0)"), None);
        assert_eq!(colour("rgb(1 2 3 4)"), None);
        assert_eq!(colour("hsl(120, 100%, 50%)"), Some((0, 255, 0, 255)));
        assert_eq!(colour("hsla(240deg 100% 50% / 0.25)"), Some((0, 0, 255, 64)));
        assert_eq!(colour("hsl(0.5turn, 100%, 25%)"), Some((0, 128, 128, 255)));
        assert_eq!(colour("hsl(0 0% 100%)"), Some((255, 255, 255, 255)));
    }

    #[test]
    fn other_values_read_as_no_colour() {
        for text in [
            "var(--x)",
            "currentColor",
            "inherit",
            "color-mix(in srgb, red, blue)",
            "red blue",
            "rgb(var(--r), 0, 0)",
            "rgb(1, 2, 3) red",
            "",
            "lab(50% 40 59)",
        ] {
            assert_eq!(colour(text), None, "{text:?}");
        }
    }
}
