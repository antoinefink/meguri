use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::ids::NoteId;
use meguri_storage::rows::cards::validated_note_id;
use meguri_storage::{Cell, Record, RowError, RowImage, Table};

fn image() -> RowImage {
    vec![
        Cell::Integer(101),
        Cell::Integer(202),
        Cell::Integer(303),
        Cell::Integer(4),
        Cell::Integer(505),
        Cell::Integer(-1),
        Cell::Integer(2),
        Cell::Integer(2),
        Cell::Integer(-606),
        Cell::Integer(7),
        Cell::Integer(2500),
        Cell::Integer(8),
        Cell::Integer(9),
        Cell::Integer(10),
        Cell::Integer(-707),
        Cell::Integer(808),
        Cell::Integer(255),
        Cell::text(r#"{"pos":3,"unknown":{"kept":true}}"#),
    ]
}

fn equivalent(cells: &[Cell]) -> Result<NoteId, RowError> {
    let projected = validated_note_id(cells);
    let full = Card::from_cells(cells).map(|card| card.note_id);
    assert_eq!(projected, full);
    projected
}

#[test]
fn compact_note_id_matches_every_scalar_storage_class() {
    assert_eq!(equivalent(&image()).unwrap(), NoteId(202));
    for index in 0..17 {
        for (cell, class) in [
            (Cell::Null, "NULL"),
            (Cell::Real(2.0), "REAL"),
            (Cell::text("2"), "TEXT"),
            (Cell::Blob(vec![2]), "BLOB"),
        ] {
            let mut cells = image();
            cells[index] = cell;
            let result = equivalent(&cells);
            if [8, 14].contains(&index) {
                assert_eq!(result.unwrap(), NoteId(202));
                assert_eq!(Card::from_cells(&cells).unwrap().to_cells()[index], Cell::Integer(0));
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.table, "cards");
                assert_eq!(error.column, Table::Cards.columns()[index]);
                assert_eq!(error.reason, format!("not an integer: {class}"));
            }
        }
    }
}

#[test]
fn compact_note_id_matches_scalar_boundaries_and_due_defaults() {
    for (index, min, max) in [
        (0, i64::MIN, i64::MAX),
        (1, i64::MIN, i64::MAX),
        (2, i64::MIN, i64::MAX),
        (3, 0, i64::from(u16::MAX)),
        (4, i64::MIN, i64::MAX),
        (5, i64::from(i32::MIN), i64::from(i32::MAX)),
        (8, i64::from(i32::MIN), i64::from(i32::MAX)),
        (9, 0, i64::from(u32::MAX)),
        (10, 0, i64::from(u16::MAX)),
        (11, 0, i64::from(u32::MAX)),
        (12, 0, i64::from(u32::MAX)),
        (13, 0, i64::from(u32::MAX)),
        (14, i64::from(i32::MIN), i64::from(i32::MAX)),
        (15, i64::MIN, i64::MAX),
        (16, 0, i64::from(u8::MAX)),
    ] {
        for value in [
            i64::MIN,
            min.saturating_sub(1),
            min,
            0,
            max,
            max.saturating_add(1),
            i64::MAX,
        ] {
            let mut cells = image();
            cells[index] = Cell::Integer(value);
            let result = equivalent(&cells);
            let in_range = (min..=max).contains(&value);
            if in_range || [8, 14].contains(&index) {
                let expected_note = if index == 1 { value } else { 202 };
                assert_eq!(result.unwrap(), NoteId(expected_note));
                assert_eq!(
                    Card::from_cells(&cells).unwrap().to_cells()[index],
                    Cell::Integer(if in_range { value } else { 0 }),
                );
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.table, "cards");
                assert_eq!(error.column, Table::Cards.columns()[index]);
                assert_eq!(error.key, "101");
                assert_eq!(error.reason, format!("{value} is out of range"));
            }
        }
    }
}

#[test]
fn compact_note_id_matches_low_byte_type_and_queue_rules() {
    for base in [i64::MIN, -256, 0, 256, i64::MAX - 255] {
        for low in 0..=255 {
            for index in [6, 7] {
                let mut cells = image();
                cells[index] = Cell::Integer(base + low);
                let result = equivalent(&cells);
                let valid = if index == 6 {
                    (0..=3).contains(&low)
                } else {
                    (0..=4).contains(&low) || (253..=255).contains(&low)
                };
                if valid {
                    assert_eq!(result.unwrap(), NoteId(202));
                    let card = Card::from_cells(&cells).unwrap();
                    if index == 6 {
                        let expected = [
                            CardType::New,
                            CardType::Learning,
                            CardType::Review,
                            CardType::Relearning,
                        ];
                        assert_eq!(card.card_type, expected[low as usize]);
                    } else {
                        let expected = match low {
                            0 => CardQueue::New,
                            1 => CardQueue::Learning,
                            2 => CardQueue::Review,
                            3 => CardQueue::InterdayLearning,
                            4 => CardQueue::Preview,
                            253 => CardQueue::BuriedByUser,
                            254 => CardQueue::BuriedByScheduler,
                            255 => CardQueue::Suspended,
                            _ => unreachable!(),
                        };
                        assert_eq!(card.queue, expected);
                    }
                } else {
                    assert_eq!(
                        result.unwrap_err(),
                        RowError {
                            table: "cards",
                            column: Table::Cards.columns()[index],
                            key: "101".into(),
                            reason: if index == 6 {
                                "not a card type"
                            } else {
                                "not a card queue"
                            }
                            .into(),
                        },
                    );
                }
            }
        }
    }
}

#[test]
fn compact_note_id_preserves_first_error_and_all_error_fields() {
    let order = [6, 7, 0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 15, 16];
    let original = image();
    let mut cells = original.clone();
    for index in order {
        cells[index] = Cell::Null;
    }
    for index in order {
        let error = equivalent(&cells).unwrap_err();
        assert_eq!(error.table, "cards");
        assert_eq!(error.column, Table::Cards.columns()[index]);
        assert_eq!(error.key, if cells[0] == Cell::Null { "[Null]" } else { "101" });
        assert_eq!(error.reason, "not an integer: NULL");
        cells[index] = original[index].clone();
    }
    assert_eq!(equivalent(&cells).unwrap(), NoteId(202));

    cells[6] = Cell::Integer(7);
    cells[7] = Cell::Integer(9);
    cells[1] = Cell::Null;
    let error = equivalent(&cells).unwrap_err();
    assert_eq!(error.column, "type");
    assert_eq!(error.reason, "not a card type");
    cells[6] = Cell::Integer(2);
    let error = equivalent(&cells).unwrap_err();
    assert_eq!(error.column, "queue");
    assert_eq!(error.reason, "not a card queue");
    cells[7] = Cell::Integer(2);
    assert_eq!(equivalent(&cells).unwrap_err().column, "nid");
}

#[test]
fn compact_note_id_accepts_tolerant_data_without_changing_full_decode() {
    for (data, expected) in [
        (Cell::text("{malformed"), "{}"),
        (Cell::Text(vec![0xff]), "{}"),
        (
            Cell::Text(b"{\"unknown\":\"\xff\"}".to_vec()),
            "{\"unknown\":\"\u{fffd}\"}",
        ),
        (Cell::Null, "{}"),
        (Cell::Integer(17), "{}"),
        (Cell::Real(17.5), "{}"),
        (Cell::Blob(b"{\"pos\":5}".to_vec()), "{}"),
        (
            Cell::text(r#"{"pos":5,"unknown":[1,true]}"#),
            r#"{"pos":5,"unknown":[1,true]}"#,
        ),
    ] {
        let mut cells = image();
        cells[17] = data;
        assert_eq!(equivalent(&cells).unwrap(), NoteId(202));
        assert_eq!(Card::from_cells(&cells).unwrap().data.to_text(), expected);
    }
}
