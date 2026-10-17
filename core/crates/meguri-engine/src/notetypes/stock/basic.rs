//! The four standard stock kinds (`specs/notetypes/stock-kinds.md` K4, K5):
//! Basic, Basic and reversed, Basic with optional reverse and Basic with
//! typing. Every back starts with the question and an `<hr id=answer>` line
//! (R910).

use super::{Definition, StockField, StockKind, StockTemplate};
use crate::notetypes::BASE_CARD_CSS;

const BASIC_FRONT: &str = include_str!("templates/basic_front.html");
const BASIC_BACK: &str = include_str!("templates/basic_back.html");
const REVERSE_FRONT: &str = include_str!("templates/reverse_front.html");
const REVERSE_BACK: &str = include_str!("templates/reverse_back.html");
const OPTIONAL_REVERSE_FRONT: &str = include_str!("templates/optional_reverse_front.html");
const TYPING_FRONT: &str = include_str!("templates/typing_front.html");
const TYPING_BACK: &str = include_str!("templates/typing_back.html");

/// `Card 1` of Basic and of both reversed kinds.
const CARD_1: StockTemplate = StockTemplate {
    name: "Card 1",
    front: BASIC_FRONT,
    back: BASIC_BACK,
};

/// One of the four standard kinds.
pub(crate) fn definition(kind: StockKind) -> Definition {
    let front_back = vec![StockField::plain("Front"), StockField::plain("Back")];
    let (fields, templates) = match kind {
        StockKind::BasicAndReversed => (
            front_back,
            vec![
                CARD_1,
                StockTemplate {
                    name: "Card 2",
                    front: REVERSE_FRONT,
                    back: REVERSE_BACK,
                },
            ],
        ),
        StockKind::BasicOptionalReversed => (
            vec![
                StockField::plain("Front"),
                StockField::plain("Back"),
                StockField::plain("Add Reverse"),
            ],
            vec![
                CARD_1,
                StockTemplate {
                    name: "Card 2",
                    front: OPTIONAL_REVERSE_FRONT,
                    back: REVERSE_BACK,
                },
            ],
        ),
        StockKind::BasicTyping => (
            front_back,
            vec![StockTemplate {
                name: "Card 1",
                front: TYPING_FRONT,
                back: TYPING_BACK,
            }],
        ),
        _ => (front_back, vec![CARD_1]),
    };
    Definition {
        fields,
        templates,
        css: BASE_CARD_CSS.to_owned(),
    }
}
