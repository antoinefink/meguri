//! The Card Text row over FFI (`notetypes/workspace` W10, W10.1, W10.2,
//! U268): reading the plain `.card` rule's five values and editing one,
//! both pure functions of the CSS text (no collection is read).

use meguri_engine::notetypes::card_text::{self, Alignment, CardText, CardTextEdit, CardTextRefusal, Colour, Font};

use crate::{CoreError, callback};

/// An 8-bit sRGB colour with alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardTextColour {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl From<Colour> for CardTextColour {
    fn from(c: Colour) -> Self {
        Self {
            red: c.red,
            green: c.green,
            blue: c.blue,
            alpha: c.alpha,
        }
    }
}

impl From<CardTextColour> for Colour {
    fn from(c: CardTextColour) -> Self {
        Self {
            red: c.red,
            green: c.green,
            blue: c.blue,
            alpha: c.alpha,
        }
    }
}

/// The Font control: "System", or a family name.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardTextFont {
    System,
    Family { name: String },
}

impl From<Font> for CardTextFont {
    fn from(f: Font) -> Self {
        match f {
            Font::System => Self::System,
            Font::Family(name) => Self::Family { name },
        }
    }
}

impl From<CardTextFont> for Font {
    fn from(f: CardTextFont) -> Self {
        match f {
            CardTextFont::System => Self::System,
            CardTextFont::Family { name } => Self::Family(name),
        }
    }
}

/// The Alignment control, in the interface's direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardTextAlignment {
    Leading,
    Center,
    Trailing,
}

impl From<Alignment> for CardTextAlignment {
    fn from(a: Alignment) -> Self {
        match a {
            Alignment::Leading => Self::Leading,
            Alignment::Center => Self::Center,
            Alignment::Trailing => Self::Trailing,
        }
    }
}

impl From<CardTextAlignment> for Alignment {
    fn from(a: CardTextAlignment) -> Self {
        match a {
            CardTextAlignment::Leading => Self::Leading,
            CardTextAlignment::Center => Self::Center,
            CardTextAlignment::Trailing => Self::Trailing,
        }
    }
}

/// What the row's five controls show; none is a control's empty state.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CardTextRow {
    pub font: Option<CardTextFont>,
    /// Points (a `px` value's number, a `pt` value × 4/3 rounded).
    pub size: Option<f64>,
    pub alignment: Option<CardTextAlignment>,
    pub text_colour: Option<CardTextColour>,
    pub background: Option<CardTextColour>,
}

impl From<CardText> for CardTextRow {
    fn from(t: CardText) -> Self {
        Self {
            font: t.font.map(Into::into),
            size: t.size,
            alignment: t.alignment.map(Into::into),
            text_colour: t.text_colour.map(Into::into),
            background: t.background.map(Into::into),
        }
    }
}

/// One control's change.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum CardTextChange {
    Font { font: CardTextFont },
    /// Points, above 0; written `<n>px`.
    Size { points: f64 },
    Alignment { alignment: CardTextAlignment },
    /// Written `#rrggbb`, or `#rrggbbaa` when not opaque.
    TextColour { colour: CardTextColour },
    /// Written `#rrggbb`; the alpha is not read.
    Background { colour: CardTextColour },
}

impl From<CardTextChange> for CardTextEdit {
    fn from(c: CardTextChange) -> Self {
        match c {
            CardTextChange::Font { font } => Self::Font(font.into()),
            CardTextChange::Size { points } => Self::Size(points),
            CardTextChange::Alignment { alignment } => Self::Alignment(alignment.into()),
            CardTextChange::TextColour { colour } => Self::TextColour(colour.into()),
            CardTextChange::Background { colour } => Self::Background(colour.into()),
        }
    }
}

/// An edit's whole new CSS, and the UTF-8 byte range in it of the value
/// written (its line is the one to highlight).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardTextEdited {
    pub css: String,
    pub value_start: u64,
    pub value_end: u64,
}

/// W10: the Card Text row of `css`, or none when the row is absent (no
/// single plain `.card` rule, or CSS that is not well formed, W10.1).
#[uniffi::export]
pub fn notetype_card_text(css: String, right_to_left: bool) -> Result<Option<CardTextRow>, CoreError> {
    callback::enter("notetype_card_text");
    Ok(card_text::read(&css, right_to_left).map(Into::into))
}

/// W10: one control's change. Only that declaration's value characters
/// change, or one declaration is added at the end of the rule; every other
/// byte stays. `InvalidArgument` when the row is absent or the value is
/// not one the control writes.
#[uniffi::export]
pub fn notetype_card_text_edit(
    css: String,
    change: CardTextChange,
    right_to_left: bool,
) -> Result<CardTextEdited, CoreError> {
    callback::enter("notetype_card_text_edit");
    match card_text::edit(&css, &change.into(), right_to_left) {
        Ok(edited) => Ok(CardTextEdited {
            css: edited.css,
            value_start: edited.value.start as u64,
            value_end: edited.value.end as u64,
        }),
        Err(CardTextRefusal::NoRow) => Err(CoreError::InvalidArgument {
            message: "the styling has no Card Text row".into(),
        }),
        Err(CardTextRefusal::InvalidValue) => Err(CoreError::InvalidArgument {
            message: "not a value the Card Text row writes".into(),
        }),
        Err(CardTextRefusal::NotApplied) => Err(CoreError::InvalidArgument {
            message: "the styling around the rule's end would absorb the new value".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_and_its_edits_cross_the_bridge() {
        let css = ".card {\n  color: red;\n}\n".to_owned();
        let row = notetype_card_text(css.clone(), false).unwrap().unwrap();
        assert_eq!(
            row.text_colour,
            Some(CardTextColour {
                red: 255,
                green: 0,
                blue: 0,
                alpha: 255
            })
        );
        assert_eq!(row.font, None);
        let edited = notetype_card_text_edit(css, CardTextChange::Size { points: 18.0 }, false).unwrap();
        assert_eq!(edited.css, ".card {\n  color: red;\n  font-size: 18px;\n}\n");
        assert_eq!(&edited.css[edited.value_start as usize..edited.value_end as usize], "18px");
        assert_eq!(notetype_card_text(".x {}".into(), false).unwrap(), None);
        assert!(matches!(
            notetype_card_text_edit(".card {".into(), CardTextChange::Size { points: 18.0 }, false),
            Err(CoreError::InvalidArgument { .. })
        ));
        assert!(matches!(
            notetype_card_text_edit(".card {}".into(), CardTextChange::Size { points: 0.0 }, false),
            Err(CoreError::InvalidArgument { .. })
        ));
    }
}
