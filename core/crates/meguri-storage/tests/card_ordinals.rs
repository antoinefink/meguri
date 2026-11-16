//! Destination membership/ordinal reads do not decode note text or scheduling.
mod common;

use common::{card, clock, empty_collection, note, open};
use meguri_model::ids::{CardId, NoteId, NotetypeId};
use meguri_storage::{Cell, ReadRows, Record, RowKey, StampMode, Table};

#[test]
fn notetype_card_ordinals_are_bounded_complete_strict_and_bound_to_the_scope() {
    let (_directory, path) = empty_collection();
    let collection = open(&path).unwrap();
    let clock = clock();
    let writer = collection.writer();
    let target = NotetypeId(10);
    let first_note = NoteId(1);
    let second_note = NoteId(2);
    let unrelated_note = NoteId(3);
    assert!(writer.card_ordinals_of_notetype_page(target, None).unwrap().is_empty());
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    for (id, notetype) in [(first_note, target), (second_note, target), (unrelated_note, NotetypeId(20))] {
        let mut note = note(notetype.0, &["Own ordinal fixture"]);
        note.id = id;
        let mut image = note.to_cells();
        image[1] = Cell::Text(vec![0xff]);
        scope.restore(Table::Notes, &RowKey::Id(id.0), Some(&image)).unwrap();
        assert!(scope.note(id).is_err(), "text cannot be decoded, even for target members");
    }

    let mut expected = Vec::new();
    for id in std::iter::once(i64::MIN).chain(1..=1_000).chain(std::iter::once(i64::MAX)) {
        let member = if id % 2 == 0 { first_note } else { second_note };
        let ord = if id == i64::MAX { u16::MAX } else { 0 };
        let mut card = card(member, ord);
        card.id = CardId(id);
        let mut image = card.to_cells();
        image[7] = Cell::Integer(999); // Irrelevant scheduling is unreadable.
        scope.restore(Table::Cards, &RowKey::Id(id), Some(&image)).unwrap();
        expected.push((CardId(id), ord));
    }
    // Neither an unrelated member nor an orphan contributes an ordinal.
    // Their invalid ordinals must never be decoded for the target.
    for (id, member) in [(2_000, unrelated_note), (3_000, NoteId(99))] {
        let mut card = card(member, 0);
        card.id = CardId(id);
        let mut image = card.to_cells();
        image[3] = Cell::text("unreadable ordinal");
        scope.restore(Table::Cards, &RowKey::Id(id), Some(&image)).unwrap();
    }
    assert!(scope.cards_of_note(first_note).is_err());
    assert!(scope.cards_of_note(second_note).is_err());
    let first = scope.card_ordinals_of_notetype_page(target, None).unwrap();
    assert_eq!(first.len(), 1_000);
    assert_eq!(first, expected[..1_000]);
    let cursor = first.last().unwrap().0;
    let second = scope.card_ordinals_of_notetype_page(target, Some(cursor)).unwrap();
    assert_eq!(second, expected[1_000..]);
    assert!(scope.card_ordinals_of_notetype_page(target, Some(CardId(i64::MAX))).unwrap().is_empty());
    assert!(scope.card_ordinals_of_notetype_page(NotetypeId(30), None).unwrap().is_empty());

    // Errors on a later page remain errors, including wrong storage classes
    // and integers outside the complete u16 ordinal domain.
    let key = RowKey::Id(i64::MAX);
    let mut last = scope.row_image(Table::Cards, &key).unwrap().unwrap();
    for invalid in [Cell::Integer(-1), Cell::Integer(65_536), Cell::text("bad"), Cell::Real(0.5), Cell::Blob(vec![0])] {
        last[3] = invalid;
        scope.restore(Table::Cards, &key, Some(&last)).unwrap();
        assert!(scope.card_ordinals_of_notetype_page(target, Some(cursor)).is_err());
    }

    // Every row above was uncommitted and visible through this scope only.
    scope.rollback().unwrap();
    assert!(writer.card_ordinals_of_notetype_page(target, None).unwrap().is_empty());
    collection.close().unwrap();
}
