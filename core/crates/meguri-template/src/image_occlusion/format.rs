//! F3–F11: the compatibility reading, safe editor reading and writer.
use super::{Document, Error, ErrorKind, Shape, ShapeKind};
use crate::cloze::tokenizer::{self, Item};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawShape {
    pub shape: String,
    pub props: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawNumber {
    pub number: u16,
    pub shapes: Vec<RawShape>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawField {
    pub hide_all: bool,
    pub numbers: Vec<RawNumber>,
}

/// F4, also returning the number of bytes successfully consumed.
pub(super) fn raw_shape(text: &str) -> Option<(RawShape, usize)> {
    let colon = text.find(':')?;
    let mut shape = RawShape {
        shape: text[..colon].to_owned(),
        props: Vec::new(),
    };
    let mut at = colon;
    while at < text.len() {
        let Some(eq) = text[at + 1..].find('=').map(|i| at + 1 + i) else {
            break;
        };
        if eq == at + 1 {
            break;
        }
        let key = &text[at + 1..eq];
        let mut value = String::new();
        let mut chars = text[eq + 1..].char_indices();
        let mut end = text.len();
        let mut valid = true;
        while let Some((offset, c)) = chars.next() {
            match c {
                ':' => {
                    end = eq + 1 + offset;
                    break;
                }
                '\\' => match chars.next() {
                    Some((_, c @ ('\\' | ':'))) => value.push(c),
                    _ => {
                        valid = false;
                        break;
                    }
                },
                _ => value.push(c),
            }
        }
        if !valid || (value.is_empty() && end < text.len()) {
            break;
        }
        shape.props.push((key.to_owned(), value));
        at = end;
    }
    Some((shape, at))
}

fn marks_hide_all(shape: &RawShape) -> bool {
    shape
        .props
        .iter()
        .find(|(k, _)| k == "oi")
        .is_some_and(|(_, v)| v == "1")
}

pub fn parse_raw(field: &str) -> RawField {
    let mut by_number: BTreeMap<u16, Vec<RawShape>> = BTreeMap::new();
    let mut hide_all = false;
    for item in tokenizer::parse(field) {
        let Item::Cloze(cloze) = item else { continue };
        let Some((shape, _)) = cloze.image_occlusion_shape().and_then(raw_shape) else {
            continue;
        };
        hide_all |= marks_hide_all(&shape);
        for number in cloze.numbers {
            by_number.entry(number).or_default().push(shape.clone());
        }
    }
    RawField {
        hide_all,
        numbers: by_number
            .into_iter()
            .map(|(number, shapes)| RawNumber { number, shapes })
            .collect(),
    }
}

/// F7.1 validates syntax before Rust's correctly rounded decimal conversion.
pub fn parse_number(text: &str) -> Option<f64> {
    static NUMBER: OnceLock<Regex> = OnceLock::new();
    let syntax = NUMBER.get_or_init(|| {
        Regex::new(r"\A[+-]?(?:(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?|\.)\z").expect("number grammar")
    });
    if !syntax.is_match(text) {
        return None;
    }
    if matches!(text, "." | "+." | "-.") {
        return Some(0.);
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn points(text: &str) -> Option<Vec<(f64, f64)>> {
    text.split(' ')
        .map(|p| {
            let (x, y) = p.split_once(',')?;
            Some((parse_number(x)?, parse_number(y)?))
        })
        .collect()
}

fn separator(text: &str) -> bool {
    static SEPARATOR: OnceLock<Regex> = OnceLock::new();
    SEPARATOR
        .get_or_init(|| Regex::new(r"\A(?:[ \t\n\r\x0c]|(?i:<br>|<br/>|<br />))*\z").expect("separator grammar"))
        .is_match(text)
}

fn known(kind: &str, key: &str) -> bool {
    matches!(key, "left" | "top" | "angle" | "fill" | "oi")
        || match kind {
            "rect" => matches!(key, "width" | "height"),
            "ellipse" => matches!(key, "rx" | "ry"),
            "polygon" => key == "points",
            "text" => matches!(key, "text" | "scale" | "fs"),
            _ => false,
        }
}

pub fn parse(field: &str) -> Result<Document, Error> {
    let tree = tokenizer::parse(field);
    let clozes: Vec<_> = tree
        .iter()
        .filter_map(|item| match item {
            Item::Cloze(c) if c.image_occlusion_shape().is_some() => Some(c),
            _ => None,
        })
        .collect();
    // Coverage is checked against the original text: C4 drops unclosed
    // clozes, so looking only at surviving text nodes would lose that error.
    let mut end = 0;
    for cloze in &clozes {
        if !separator(&field[end..cloze.source.start]) {
            return Err(Error {
                kind: ErrorKind::ExtraContent,
                shape: None,
            });
        }
        end = cloze.source.end;
    }
    if !separator(&field[end..]) {
        return Err(Error {
            kind: ErrorKind::ExtraContent,
            shape: None,
        });
    }
    let mut shapes = Vec::with_capacity(clozes.len());
    let mut hide_all = false;
    for (index, cloze) in clozes.into_iter().enumerate() {
        let fail = |kind| Error {
            kind,
            shape: Some(index),
        };
        if cloze.numbers.len() > 1 {
            return Err(fail(ErrorKind::MultipleNumbers));
        }
        if cloze.items.iter().any(|i| matches!(i, Item::Cloze(_))) {
            return Err(fail(ErrorKind::NestedCloze));
        }
        let text = cloze.image_occlusion_shape().expect("IO cloze");
        let (raw, consumed) = raw_shape(text).ok_or_else(|| fail(ErrorKind::MalformedShape))?;
        if consumed != text.len() {
            return Err(fail(ErrorKind::MalformedShape));
        }
        if !matches!(raw.shape.as_str(), "rect" | "ellipse" | "polygon" | "text") {
            return Err(fail(ErrorKind::UnknownShape));
        }
        let mut keys = BTreeSet::new();
        for (k, _) in &raw.props {
            if !keys.insert(k) {
                return Err(fail(ErrorKind::DuplicateKey));
            }
        }
        for (k, v) in &raw.props {
            if !known(&raw.shape, k) {
                continue;
            }
            let valid = match k.as_str() {
                "text" | "fill" | "oi" => true,
                "points" => points(v).is_some(),
                _ => parse_number(v).is_some(),
            };
            if !valid {
                return Err(fail(ErrorKind::InvalidValue));
            }
        }
        let get = |key: &str| raw.props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let required: &[&str] = match raw.shape.as_str() {
            "rect" => &["left", "top", "width", "height"],
            "ellipse" => &["left", "top", "rx", "ry"],
            "polygon" => &["left", "top", "points"],
            _ => &["left", "top", "text"],
        };
        if required.iter().any(|key| get(key).is_none()) {
            return Err(fail(ErrorKind::MissingKey));
        }
        let num = |key: &str| get(key).and_then(parse_number).expect("validated required number");
        let kind = match raw.shape.as_str() {
            "rect" => ShapeKind::Rect {
                width: num("width"),
                height: num("height"),
            },
            "ellipse" => ShapeKind::Ellipse {
                rx: num("rx"),
                ry: num("ry"),
            },
            "polygon" => ShapeKind::Polygon {
                points: points(get("points").expect("points")).expect("validated points"),
            },
            _ => ShapeKind::Text {
                text: get("text").expect("text").to_owned(),
                scale: get("scale").and_then(parse_number).unwrap_or(1.),
                fs: get("fs").and_then(parse_number),
            },
        };
        let mut shape = Shape::new(cloze.numbers[0], num("left"), num("top"), kind);
        shape.angle = get("angle").and_then(parse_number).map(read_angle).unwrap_or(0.);
        shape.fill = get("fill").filter(|s| !s.is_empty()).map(str::to_owned);
        let shape_mode = marks_hide_all(&raw);
        hide_all |= shape_mode;
        shape.unknown_props = raw
            .props
            .into_iter()
            .filter(|(k, _)| !known(shape.kind.name(), k))
            .collect();
        shape.retain_source(&field[cloze.source.clone()], shape_mode);
        shapes.push(shape);
    }
    Ok(Document {
        original: field.to_owned(),
        original_shapes: shapes.clone(),
        original_mode: hide_all,
        shapes,
        hide_all,
    })
}

fn unwritable() -> Error {
    Error {
        kind: ErrorKind::UnwritableShape,
        shape: None,
    }
}

/// F8: multiply the binary significand by 625 and shift by exponent+4.
/// This is x*10000 as an exact integer times a power of two, with no
/// intermediate floating-point product and its boundary-rounding error.
pub fn format_number(x: f64) -> Result<String, Error> {
    if !x.is_finite() {
        return Err(unwritable());
    }
    if x == 0. {
        return Ok(".0000".to_owned());
    }
    let bits = x.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (mantissa, shift) = if exponent == 0 {
        (fraction, -1070)
    } else {
        (fraction | (1_u64 << 52), exponent - 1023 - 48)
    };
    let product = mantissa * 625;
    let mut digits = if shift < 0 {
        let down = -shift as u32;
        let rounded = if down >= 64 {
            0
        } else {
            (product + (1_u64 << (down - 1))) >> down
        };
        rounded.to_string()
    } else {
        // At most 313 decimal digits, including f64::MAX scaled by 10000.
        let mut decimal: Vec<u8> = product.to_string().bytes().map(|b| b - b'0').rev().collect();
        for _ in 0..shift {
            let mut carry = 0;
            for d in &mut decimal {
                let twice = *d * 2 + carry;
                *d = twice % 10;
                carry = twice / 10;
            }
            if carry != 0 {
                decimal.push(carry);
            }
        }
        decimal.into_iter().rev().map(|d| char::from(b'0' + d)).collect()
    };
    if digits.len() < 5 {
        digits = format!("{:0>5}", digits);
    }
    digits.insert(digits.len() - 4, '.');
    if x.is_sign_negative() {
        digits.insert(0, '-');
    }
    Ok(digits.trim_start_matches('0').trim_end_matches('0').to_owned())
}

pub fn read_angle(stored: f64) -> f64 {
    ((stored % 10000.) / 10000.) * 360.
}

pub fn write_angle(degrees: f64) -> Result<Option<i32>, Error> {
    if !degrees.is_finite() {
        return Err(unwritable());
    }
    let r = ((degrees % 360.) / 360.) * 10000.;
    // Avoid r+0.5, whose rounding can erase a near-half distinction.
    let floor = r.floor();
    let step = if r - floor >= 0.5 { floor + 1. } else { floor } as i32;
    Ok((step != 0).then_some(step))
}

pub fn write_shape(shape: &Shape, hide_all: bool) -> Result<String, Error> {
    let mut props = vec![
        ("left".to_owned(), format_number(shape.left)?),
        ("top".to_owned(), format_number(shape.top)?),
    ];
    if let Some(angle) = write_angle(shape.angle)? {
        props.push(("angle".into(), angle.to_string()));
    }
    let mut number_prop = |key: &str, value: f64| -> Result<(), Error> {
        props.push((key.to_owned(), format_number(value)?));
        Ok(())
    };
    match &shape.kind {
        ShapeKind::Rect { width, height } => {
            number_prop("width", *width)?;
            number_prop("height", *height)?;
        }
        ShapeKind::Ellipse { rx, ry } => {
            number_prop("rx", *rx)?;
            number_prop("ry", *ry)?;
        }
        ShapeKind::Polygon { points } => {
            let pairs = points
                .iter()
                .map(|(x, y)| Ok(format!("{},{}", format_number(*x)?, format_number(*y)?)))
                .collect::<Result<Vec<_>, Error>>()?;
            props.push(("points".into(), pairs.join(" ")));
        }
        ShapeKind::Text { text, scale, fs } => {
            props.push(("text".into(), text.clone()));
            props.push(("scale".into(), format_number(*scale)?));
            if let Some(fs) = fs {
                props.push(("fs".into(), format_number(*fs)?));
            }
        }
    }
    let default = if shape.kind.is_text() { "#000000" } else { "#ffeba2" };
    if let Some(fill) = &shape.fill
        && fill != default
    {
        props.push(("fill".into(), fill.clone()));
    }
    props.extend(shape.unknown_props.iter().filter(|(_, v)| !v.is_empty()).cloned());
    if hide_all {
        props.push(("oi".into(), "1".into()));
    }
    let number = if shape.kind.is_text() { 0 } else { shape.number };
    let mut out = format!("{{{{c{number}::image-occlusion:{}", shape.kind.name());
    for (key, value) in &props {
        out.push(':');
        out.push_str(key);
        out.push('=');
        for c in value.chars() {
            if matches!(c, '\\' | ':') {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out.push_str("}}");
    // F10's representability test uses the permissive reading, and also
    // requires one complete, unnested cloze covering the entire output.
    let tree = tokenizer::parse(&out);
    let representable = match tree.as_slice() {
        [Item::Cloze(c)] if c.source == (0..out.len()) && c.numbers == [number] && c.items.len() == 1 => c
            .image_occlusion_shape()
            .and_then(raw_shape)
            .is_some_and(|(raw, consumed)| {
                consumed == c.image_occlusion_shape().expect("IO").len()
                    && raw.shape == shape.kind.name()
                    && raw.props == props
            }),
        _ => false,
    };
    if !representable {
        return Err(unwritable());
    }
    Ok(out)
}

pub(super) fn check_size(width: f64, height: f64) -> Result<(), Error> {
    if width.is_finite() && width > 0. && height.is_finite() && height > 0. {
        Ok(())
    } else {
        Err(Error {
            kind: ErrorKind::InvalidSize,
            shape: None,
        })
    }
}

fn drop_shape(shape: &Shape, width: f64, height: f64) -> bool {
    if shape.fill.as_deref() == Some("transparent") {
        return true;
    }
    match &shape.kind {
        ShapeKind::Rect { width: w, height: h } => w * width < 5. || h * height < 5.,
        ShapeKind::Ellipse { rx, ry } => 2. * rx * width < 5. || 2. * ry * height < 5.,
        ShapeKind::Polygon { points } => {
            let minx = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let maxx = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let miny = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let maxy = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            (maxx - minx) * width < 5. || (maxy - miny) * height < 5.
        }
        ShapeKind::Text { text, .. } => text.is_empty(),
    }
}

pub fn write(shapes: &[Shape], hide_all: bool, width: f64, height: f64) -> Result<String, Error> {
    check_size(width, height)?;
    let mut out = String::new();
    for (i, shape) in shapes.iter().enumerate() {
        if let Some(source) = &shape.source
            && shape == &source.shape
            && source.hide_all == hide_all
        {
            out.push_str(&source.text);
            out.push_str("<br>");
            continue;
        }
        if drop_shape(shape, width, height) {
            continue;
        }
        out.push_str(&write_shape(shape, hide_all).map_err(|mut e| {
            e.shape = Some(i);
            e
        })?);
        out.push_str("<br>");
    }
    Ok(out)
}
