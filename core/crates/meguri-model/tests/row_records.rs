//! Read tolerances of stored row values (specs/model/row-records.md R3 to R5).

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::note::Note;
use meguri_model::revlog::{RevlogEntry, RevlogKind};

#[test]
fn card_type_and_queue_take_the_low_byte() {
    assert_eq!(CardType::from_stored(2), Some(CardType::Review));
    assert_eq!(CardType::from_stored(258), Some(CardType::Review));
    assert_eq!(CardType::from_stored(7), None);
    assert_eq!(CardType::from_stored(-1), None);
    assert_eq!(CardQueue::from_stored(-3), Some(CardQueue::BuriedByUser));
    assert_eq!(CardQueue::from_stored(255), Some(CardQueue::Suspended));
    assert_eq!(CardQueue::from_stored(256 + 4), Some(CardQueue::Preview));
    assert_eq!(CardQueue::from_stored(9), None);
    assert_eq!(CardQueue::from_stored(-4), None);
    for queue in [-3, -2, -1, 0, 1, 2, 3, 4] {
        assert_eq!(CardQueue::from_stored(queue).unwrap().stored(), queue);
    }
}

#[test]
fn out_of_range_numbers_read_as_zero() {
    assert_eq!(Card::i32_or_zero(1 << 40), 0);
    assert_eq!(Card::i32_or_zero(-5), -5);
    assert_eq!(Note::checksum_from_stored(4_294_967_295), u32::MAX);
    assert_eq!(Note::checksum_from_stored(4_294_967_296), 0);
    assert_eq!(Note::checksum_from_stored(-1), 0);
    assert_eq!(RevlogEntry::answer_millis_from_stored(-1), 0);
    assert_eq!(RevlogEntry::answer_millis_from_stored(60_000), 60_000);
}

#[test]
fn revlog_kinds_take_the_low_byte_and_default_to_learning() {
    assert_eq!(RevlogKind::from_stored(3), RevlogKind::Filtered);
    assert_eq!(RevlogKind::from_stored(256 + 5), RevlogKind::Rescheduled);
    assert_eq!(RevlogKind::from_stored(6), RevlogKind::Learning);
    assert_eq!(RevlogKind::from_stored(-1), RevlogKind::Learning);
}

#[test]
fn note_fields_split_at_the_separator() {
    let note = Note {
        fields: "front\u{1f}back\u{1f}".to_owned(),
        ..Note::default()
    };
    assert_eq!(note.field_values().collect::<Vec<_>>(), ["front", "back", ""]);
}
