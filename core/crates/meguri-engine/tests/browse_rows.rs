//! `browser/columns` C3 to C10 through the engine's row records: what the
//! scenarios do not cover (C15's last paragraph): unreadable rows, the
//! reading order of the row errors, a deck count over a missing deck,
//! fonts and right-to-left sort fields, template errors and blank cloze
//! cards in Question and Answer, card data with a non-string `cd`.
mod common;
use meguri_engine::{
    browse::{self, Answer, Cell, Font, Question, Row, RowFailure, RowRequest, RowTime, Tint},
    notes,
    notetypes::{self, Draft, SaveOptions},
};
use meguri_model::{
    changes::OpSource,
    ids::{CardId, DeckId, NoteId, NotetypeId},
};
use meguri_search::SearchMode;
use meguri_search::config::Column;
use meguri_storage::{ReadRows, ReaderRole};
use meguri_template::{ErrorKind, av_tags::AvSide, card::BlankKind};

const TIME: RowTime = RowTime {
    now: 1_894_701_600,
    today: 0,
};

struct Fixture {
    h: common::Harness,
    kind: NotetypeId,
}

/// A note type with two templates (the second reads Back) or a cloze one.
fn fixture(cloze: bool, front: &str, back: &str, browser_front: &str) -> Fixture {
    let h = common::fresh();
    let template = |name: &str, front: &str, back: &str| notetypes::NewTemplate {
        name: name.into(),
        front: front.into(),
        back: back.into(),
        browser_front: browser_front.into(),
        browser_back: String::new(),
    };
    let templates = if cloze {
        vec![template("Cloze", front, back)]
    } else {
        vec![
            template("Recall", front, back),
            template("Reverse", "{{Back}}", "{{Front}}"),
        ]
    };
    let kind = notetypes::add(
        &h.collection,
        notetypes::NewNotetype {
            name: "Pair".into(),
            kind: if cloze {
                notetypes::NotetypeKind::Cloze
            } else {
                notetypes::NotetypeKind::Standard
            },
            fields: vec!["Front".into(), "Back".into()],
            templates,
            css: None,
        },
    )
    .unwrap()
    .value
    .id;
    Fixture { h, kind }
}

impl Fixture {
    fn note(&self, fields: &[&str], tags: &[&str]) -> (NoteId, Vec<CardId>) {
        let note = notes::add(
            &self.h.collection,
            notes::AddNote {
                notetype_id: self.kind,
                fields: fields.iter().map(|v| (*v).to_owned()).collect(),
                tags: tags.iter().map(|v| (*v).to_owned()).collect(),
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap()
        .value;
        let mut cards: Vec<_> = self
            .h
            .collection
            .reader(ReaderRole::Query)
            .unwrap()
            .cards_of_note(note.id)
            .unwrap()
            .into_iter()
            .map(|card| (card.ord, card.id))
            .collect();
        cards.sort();
        (note.id, cards.into_iter().map(|(_, id)| id).collect())
    }

    fn raw(&self, sql: &str) {
        common::tagging::raw(&self.h.collection, sql, &[]);
    }

    fn rows(&self, mode: SearchMode, ids: &[i64], columns: &[Column]) -> Vec<Result<Row, RowFailure>> {
        browse::rows(
            &self.h.collection,
            &RowRequest {
                mode,
                ids,
                columns,
                time: TIME,
            },
        )
        .unwrap()
    }

    fn cells(&self, mode: SearchMode, id: i64, columns: &[Column]) -> Vec<Cell> {
        self.rows(mode, &[id], columns).remove(0).unwrap().cells
    }
}

fn text(value: &str) -> Cell {
    Cell::Text(value.into())
}

#[test]
fn cards_mode_cells_come_in_request_order_with_the_answer_prefix_removed() {
    let f = fixture(false, "{{Front}}", "{{Front}}<br>{{Back}}", "");
    let (_, cards) = f.note(&["<b>Baum</b>", "tree"], &["Marked", "b"]);
    let columns = [
        Column::Tags,
        Column::Answer,
        Column::SortField,
        Column::Question,
        Column::CardType,
        Column::Deck,
        Column::Notetype,
    ];
    let row = f.rows(SearchMode::Cards, &[cards[0].0], &columns).remove(0).unwrap();
    assert_eq!(
        row.cells,
        vec![
            // A note save stores the tags in unicase order (tags T4).
            text("b Marked"),
            Cell::Answer(Answer::Text("tree".into())),
            text("Baum"),
            Cell::Question(Question::Text("Baum".into())),
            text("Recall"),
            text("Default"),
            text("Pair"),
        ]
    );
    assert_eq!(row.tint, Tint::Marked);
    assert!(row.state.marked && !row.state.leech);
    assert_eq!(row.font, Font::default());
    assert!(!row.sort_field_rtl);
    // Notes mode: Cards counts the note's cards, and no row error for a
    // request without Question or Answer.
    let (note, _) = f.note(&["a", "b"], &[]);
    assert_eq!(
        f.cells(SearchMode::Notes, note.0, &[Column::CardType, Column::Deck]),
        vec![Cell::Count(2), text("Default")]
    );
}

#[test]
fn row_errors_follow_the_c3_1_reading_order_and_never_stop_other_rows() {
    let f = fixture(false, "{{Front}}", "{{Back}}", "");
    let (_, a) = f.note(&["a", "1"], &[]);
    let (b_note, b) = f.note(&["b", "2"], &[]);
    let (c_note, c) = f.note(&["c", "3"], &[]);
    let (_, d) = f.note(&["d", "4"], &[]);
    let (_, e) = f.note(&["e", "5"], &[]);
    let (_, g) = f.note(&["g", "6"], &[]);
    f.raw(&format!("UPDATE cards SET did = 999 WHERE id = {}", a[0].0));
    f.raw(&format!("UPDATE cards SET odid = 998, odue = 1 WHERE id = {}", d[0].0));
    f.raw(&format!("UPDATE cards SET ord = 7 WHERE id = {}", e[0].0));
    f.raw(&format!("UPDATE notes SET mid = 4242 WHERE id = {}", c_note.0));
    // An undecodable card, and a note that does not decode on a card whose
    // deck is missing: the note is read before the deck.
    f.raw(&format!("UPDATE cards SET ivl = -1 WHERE id = {}", g[0].0));
    f.raw(&format!("UPDATE notes SET tags = X'00' WHERE id = {}", b_note.0));
    f.raw(&format!("UPDATE cards SET did = 999 WHERE id = {}", b[0].0));
    let ids = [a[0].0, -1, b[0].0, c[0].0, d[0].0, e[0].0, g[0].0, a[1].0];
    let rows = f.rows(SearchMode::Cards, &ids, &[Column::Deck, Column::Question]);
    let failures: Vec<_> = rows.iter().map(|row| row.as_ref().err().cloned()).collect();
    assert_eq!(failures[0], Some(RowFailure::Damaged));
    assert_eq!(failures[1], Some(RowFailure::Deleted));
    assert!(
        matches!(&failures[2], Some(RowFailure::Unreadable { table: "notes", column: "tags", .. })),
        "{:?}",
        failures[2]
    );
    assert_eq!(failures[3], Some(RowFailure::Damaged));
    assert_eq!(failures[4], Some(RowFailure::Damaged));
    assert_eq!(failures[5], Some(RowFailure::Damaged));
    assert!(
        matches!(&failures[6], Some(RowFailure::Unreadable { table: "cards", column: "ivl", row }) if *row == g[0].0.to_string()),
        "{:?}",
        failures[6]
    );
    assert_eq!(failures[7], None);
}

#[test]
fn notes_mode_reads_every_card_and_counts_decks_that_do_not_exist() {
    let f = fixture(false, "{{Front}}", "{{Back}}", "");
    let (spread, spread_cards) = f.note(&["a", "1"], &[]);
    // Only the second card's deck is missing: the deck count is 2 and the
    // row is no error (only the first card's records count, C3.1).
    f.raw(&format!("UPDATE cards SET did = 999 WHERE id = {}", spread_cards[1].0));
    assert_eq!(
        f.cells(SearchMode::Notes, spread.0, &[Column::Deck]),
        vec![Cell::DeckCount(2)]
    );
    let (bare, _) = f.note(&["b", "2"], &[]);
    f.raw(&format!("DELETE FROM cards WHERE nid = {}", bare.0));
    let (broken, broken_cards) = f.note(&["c", "3"], &[]);
    f.raw(&format!("UPDATE cards SET reps = -4 WHERE id = {}", broken_cards[1].0));
    let rows = f.rows(SearchMode::Notes, &[bare.0, 12, broken.0], &[Column::Reviews]);
    assert_eq!(rows[0], Err(RowFailure::Damaged));
    assert_eq!(rows[1], Err(RowFailure::Deleted));
    assert!(matches!(
        &rows[2],
        Err(RowFailure::Unreadable { table: "cards", column: "reps", .. })
    ));
}

#[test]
fn first_card_values_in_notes_mode_come_from_the_lowest_card_id() {
    let f = fixture(false, "{{Front}}", "{{Back}}", "");
    let (note, cards) = f.note(&["a", "b"], &[]);
    // Swap the ids so the Reverse card (ordinal 1) is the first card.
    for (from, to) in [(cards[0].0, -1), (cards[1].0, cards[0].0), (-1, cards[1].0)] {
        f.raw(&format!("UPDATE cards SET id = {to} WHERE id = {from}"));
    }
    f.raw(&format!(
        "UPDATE cards SET type = 2, queue = 2, due = 3, ivl = 5, factor = 2500, reps = 7, data = '{{\"pos\":4}}' WHERE nid = {} AND ord = 1",
        note.0
    ));
    assert_eq!(
        f.cells(
            SearchMode::Notes,
            note.0,
            &[Column::Question, Column::Position, Column::Reviews, Column::Due]
        ),
        vec![
            Cell::Question(Question::Text("b".into())),
            Cell::Position(4),
            Cell::Count(7),
            Cell::Instant {
                secs: TIME.now + 3 * 86_400,
                wrapped: false
            },
        ]
    );
}

#[test]
fn fonts_and_a_right_to_left_sort_field_come_from_the_note_type_c6() {
    let f = fixture(false, "{{Front}}", "{{Back}}", "");
    let (note, cards) = f.note(&["שלום", "b"], &[]);
    let stored = f
        .h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(f.kind)
        .unwrap()
        .unwrap();
    let mut draft = Draft::of(stored);
    draft.notetype.templates[0].config.browser_font_name = "Hiragino".into();
    draft.notetype.templates[1].config.browser_font_size = 18;
    draft.notetype.fields[0].config.rtl = true;
    notetypes::save(&f.h.collection, draft, SaveOptions::default()).unwrap();
    let rows = f.rows(SearchMode::Cards, &[cards[0].0, cards[1].0], &[Column::SortField]);
    let (first, second) = (rows[0].as_ref().unwrap(), rows[1].as_ref().unwrap());
    assert_eq!(
        first.font,
        Font {
            name: Some("Hiragino".into()),
            size: None
        }
    );
    assert_eq!(second.font, Font { name: None, size: Some(18) });
    assert!(first.sort_field_rtl && second.sort_field_rtl);
    let note_row = f.rows(SearchMode::Notes, &[note.0], &[Column::SortField]).remove(0).unwrap();
    assert_eq!(note_row.font, first.font);
    let mut draft = Draft::of(
        f.h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .notetype(f.kind)
            .unwrap()
            .unwrap(),
    );
    draft.notetype.fields[0].config.rtl = false;
    notetypes::save(&f.h.collection, draft, SaveOptions::default()).unwrap();
    assert!(!f.rows(SearchMode::Cards, &[cards[0].0], &[Column::SortField])[0]
        .as_ref()
        .unwrap()
        .sort_field_rtl);
}

#[test]
fn template_errors_and_blank_cloze_cards_are_typed_in_both_cells_c10() {
    let f = fixture(false, "{{Front}}", "{{Back}}", "{{Absent}}");
    let (_, cards) = f.note(&["a", "b"], &[]);
    assert_eq!(
        f.cells(SearchMode::Cards, cards[0].0, &[Column::Question, Column::Answer]),
        vec![
            Cell::Question(Question::TemplateError {
                kind: ErrorKind::UnknownField,
                side: AvSide::Question
            }),
            Cell::Answer(Answer::TemplateError {
                kind: ErrorKind::UnknownField,
                side: AvSide::Question
            }),
        ]
    );
    let f = fixture(true, "{{cloze:Front}}", "{{cloze:Front}}", "");
    let (_, cards) = f.note(&["{{c1::犬}} と 猫", ""], &[]);
    assert_eq!(
        f.cells(SearchMode::Cards, cards[0].0, &[Column::Question, Column::Answer, Column::CardType]),
        vec![
            Cell::Question(Question::Text("[...] と 猫".into())),
            Cell::Answer(Answer::Text("犬 と 猫".into())),
            text("Cloze 1"),
        ]
    );
    f.raw("UPDATE cards SET ord = 8");
    assert_eq!(
        f.cells(SearchMode::Cards, cards[0].0, &[Column::Question, Column::Answer, Column::CardType]),
        vec![
            Cell::Question(Question::MissingCloze {
                kind: BlankKind::MissingCloze,
                text: String::new()
            }),
            Cell::Answer(Answer::MissingCloze {
                kind: BlankKind::MissingCloze
            }),
            text("Cloze 9"),
        ]
    );
}

#[test]
fn card_data_with_a_non_string_cd_reads_as_no_keys_in_the_cells_c1() {
    let f = fixture(false, "{{Front}}", "{{Back}}", "");
    let (_, cards) = f.note(&["a", "b"], &[]);
    f.raw(&format!(
        "UPDATE cards SET due = 9, data = '{{\"pos\":5,\"s\":10,\"d\":5,\"cd\":5}}' WHERE id = {}",
        cards[0].0
    ));
    assert_eq!(
        f.cells(
            SearchMode::Cards,
            cards[0].0,
            &[Column::Position, Column::Stability, Column::Difficulty, Column::Retrievability]
        ),
        vec![Cell::Position(9), Cell::Empty, Cell::Empty, Cell::Empty]
    );
}

#[test]
fn a_row_request_writes_nothing() {
    let f = fixture(false, "{{Front}}", "{{Back}}", "");
    let (note, cards) = f.note(&["a", "b"], &[]);
    let stamps = |f: &Fixture| {
        (
            common::col_mod(&f.h.collection),
            common::query_int(&f.h.collection, "SELECT sum(mod) + sum(usn) FROM cards"),
            common::query_int(&f.h.collection, "SELECT sum(mod) + sum(usn) FROM notes"),
        )
    };
    let before = stamps(&f);
    f.rows(SearchMode::Cards, &[cards[0].0], &Column::ALL);
    f.rows(SearchMode::Notes, &[note.0], &Column::ALL);
    assert_eq!(stamps(&f), before);
}
