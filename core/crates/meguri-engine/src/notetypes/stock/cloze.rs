//! The stock Cloze kind (`specs/notetypes/stock-kinds.md` K4 to K6, R911):
//! `Text` (cloze field tag 0, protected) and `Back Extra` (tag 1), cloze
//! highlighting for light and dark appearance.

use super::{Definition, StockField, StockTemplate};
use crate::notetypes::BASE_CARD_CSS;

const FRONT: &str = include_str!("templates/cloze_front.html");
const BACK: &str = include_str!("templates/cloze_back.html");
const CSS: &str = include_str!("templates/cloze.css");

pub(crate) fn definition() -> Definition {
    Definition {
        fields: vec![
            StockField {
                name: "Text",
                tag: Some(0),
                protected: true,
            },
            StockField {
                name: "Back Extra",
                tag: Some(1),
                protected: false,
            },
        ],
        templates: vec![StockTemplate {
            name: "Cloze",
            front: FRONT,
            back: BACK,
        }],
        css: format!("{BASE_CARD_CSS}{CSS}"),
    }
}
