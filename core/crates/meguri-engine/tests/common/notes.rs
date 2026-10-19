//! U27 fixtures use real engine note types and ops, no card-generation stand-in.
use super::Harness;
use meguri_engine::{notes, notetypes};
use meguri_model::{
    changes::OpSource,
    ids::{DeckId, NotetypeId},
};

pub fn kind(h: &Harness, cloze: bool, fields: &[&str], fronts: &[&str]) -> NotetypeId {
    notetypes::add(
        &h.collection,
        notetypes::NewNotetype {
            name: "U27 synthetic".to_owned(),
            kind: if cloze {
                notetypes::NotetypeKind::Cloze
            } else {
                notetypes::NotetypeKind::Standard
            },
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            templates: fronts
                .iter()
                .enumerate()
                .map(|(i, front)| notetypes::NewTemplate {
                    name: format!("View {i}"),
                    front: (*front).to_owned(),
                    back: String::new(),
                    ..Default::default()
                })
                .collect(),
            css: Some(String::new()),
        },
    )
    .unwrap()
    .value
    .id
}
pub fn input(mid: NotetypeId, fields: &[&str]) -> notes::AddNote {
    notes::AddNote {
        notetype_id: mid,
        fields: fields.iter().map(|s| (*s).to_owned()).collect(),
        tags: Vec::new(),
        deck_id: DeckId(1),
        guid: None,
        update_add_defaults: false,
    }
}
pub fn add(h: &Harness, input: notes::AddNote) -> notes::Added {
    notes::add(&h.collection, input, OpSource::UserUndoable).unwrap().value
}
