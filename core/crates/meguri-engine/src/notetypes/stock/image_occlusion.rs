//! The stock Image Occlusion kind (`specs/notetypes/stock-kinds.md` K4,
//! K7, R912): fields tagged 0 to 4 for the image-occlusion code (U63, U68),
//! templates that work with Anki's image-occlusion runtime and Meguri's, and
//! the mask colours the runtime reads from the canvas. `Comments` is on no
//! template.

use super::{Definition, StockField, StockTemplate};
use crate::notetypes::BASE_CARD_CSS;

const FRONT: &str = include_str!("templates/image_occlusion_front.html");
const BACK: &str = include_str!("templates/image_occlusion_back.html");
const CSS: &str = include_str!("templates/image_occlusion.css");

pub(crate) fn definition() -> Definition {
    let protected = |name, tag| StockField {
        name,
        tag: Some(tag),
        protected: true,
    };
    Definition {
        fields: vec![
            protected("Occlusion", 0),
            protected("Image", 1),
            protected("Header", 2),
            protected("Back Extra", 3),
            StockField {
                name: "Comments",
                tag: Some(4),
                protected: false,
            },
        ],
        templates: vec![StockTemplate {
            name: "Image Occlusion",
            front: FRONT,
            back: BACK,
        }],
        css: format!("{BASE_CARD_CSS}{CSS}"),
    }
}
