//! C10 audio display is applied before H6; C7.4 uses complete deck names.
use meguri_search::browser::{
    deck_cell,
    qa::{self, Question},
};
use meguri_template::card::{CardRender, Outcome};
#[test]
fn question_uses_the_shared_av_grammar_and_decodes_entities_only_in_the_text_step() {
    for (html, expected) in [
        ("<b>猫</b><br>[sound:neko.mp3]", "猫 🔉neko.mp3🔉"),
        ("[sound:a&amp;lt;b.mp3]", "🔉a&lt;b.mp3🔉"),
        ("[anki:tts]猫[/anki:tts]", "💬猫💬"),
        ("[anki:tts lang=\"\"][/anki:tts]", "💬💬"),
        ("[anki:TTS lang=ja]猫[/anki:TTS]", "[anki:TTS lang=ja]猫[/anki:TTS]"),
        (
            "[anki:other  x=\"a b\" x=last]猫[/anki:other]",
            "[anki:other x=last]猫[/anki:other]",
        ),
        ("<div>犬</div><img src=\"inu.jpg\">[[type:Back]]", "犬 inu.jpg"),
    ] {
        let rendered = CardRender {
            question: Some(html.into()),
            answer: Some(String::new()),
            question_av: vec![],
            answer_av: vec![],
            outcome: Outcome::Rendered,
        };
        assert_eq!(qa::question(&rendered), Question::Text(expected.into()));
    }
}
#[test]
fn cards_deck_cell_has_no_elision_or_state_dependent_home_filter() {
    assert_eq!(deck_cell::cards("Japanese::Mining", None), "Japanese::Mining");
    assert_eq!(
        deck_cell::cards("Filtered 1", Some("Japanese::Mining")),
        "Filtered 1 (Japanese::Mining)"
    );
    assert_eq!(
        deck_cell::cards("Ordinary::Current", Some("Home::Full")),
        "Ordinary::Current (Home::Full)"
    );
}

mod cells {
    //! `browser/columns` C4 to C8, C11, C12's time spans and C13, from the
    //! spec's tables and examples.
    use meguri_model::card::{Card, CardQueue, CardType};
    use meguri_model::card_data::CardData;
    use meguri_model::card_data::extractors::Fields;
    use meguri_model::ids::{CardId, DeckId, NoteId};
    use meguri_model::time::EpochSecs;
    use meguri_model::usn::Usn;
    use meguri_search::browser::cells::{
        self, Cell, Font, Percent, RowState, RowTime, SpanUnit, Tint, notes, span_amount_text, span_parts,
    };
    use meguri_search::config::{Alignment, Column, Elision};
    use meguri_search::SearchMode;

    /// 2030-01-15 10:00 UTC, today 0 (the C7.5 table).
    const TIME: RowTime = RowTime {
        now: 1_894_701_600,
        today: 0,
    };

    fn card(card_type: CardType, queue: CardQueue, due: i32) -> Card {
        Card {
            id: CardId(1),
            note_id: NoteId(1),
            deck_id: DeckId(1),
            ord: 0,
            mtime: EpochSecs(0),
            usn: Usn(0),
            card_type,
            queue,
            due,
            interval: 0,
            ease_factor: 0,
            reps: 0,
            lapses: 0,
            remaining_steps: 0,
            original_due: 0,
            original_deck_id: DeckId(0),
            flags: 0,
            data: CardData::default(),
        }
    }

    fn filtered(mut card: Card, original_due: i32) -> Card {
        card.original_deck_id = DeckId(5);
        card.original_due = original_due;
        card
    }

    fn instant(secs: i64, wrapped: bool) -> Cell {
        Cell::Instant { secs, wrapped }
    }

    #[test]
    fn due_follows_the_c7_5_table() {
        use CardQueue as Q;
        use CardType as T;
        let day = 86_400;
        let now = TIME.now;
        for (card, expected) in [
            (card(T::New, Q::New, 5), Cell::NewPosition { position: 5, wrapped: false }),
            (card(T::New, Q::Suspended, 5), Cell::NewPosition { position: 5, wrapped: true }),
            (card(T::Learning, Q::Learning, 1_894_702_800), instant(1_894_702_800, false)),
            (card(T::Relearning, Q::InterdayLearning, 1), instant(now + day, false)),
            (card(T::Review, Q::Review, -3), instant(now - 3 * day, false)),
            (card(T::Review, Q::Suspended, 2), instant(now + 2 * day, true)),
            (card(T::Learning, Q::Suspended, 1_894_702_800), Cell::Empty),
            (filtered(card(T::Review, Q::Review, -100_000), 4), Cell::Filtered { wrapped: false }),
            (filtered(card(T::New, Q::BuriedByUser, -99_999), 4), Cell::Filtered { wrapped: true }),
            (card(T::Review, Q::Review, 3_000_000), instant(now + 3_000_000 * day, false)),
        ] {
            assert_eq!(cells::due(&card, TIME), expected, "{card:?}");
        }
    }

    #[test]
    fn ease_interval_and_counts_c7_6_to_c7_8() {
        let mut review = card(CardType::Review, CardQueue::Review, 0);
        for (factor, percent) in [(2500, 250), (2505, 250), (1300, 130)] {
            review.ease_factor = factor;
            assert_eq!(cells::ease(&review), Cell::Percent(Percent::Integer(percent)));
        }
        assert_eq!(cells::ease(&card(CardType::New, CardQueue::New, 1)), Cell::NewMarker);
        assert_eq!(
            cells::ease(&card(CardType::Learning, CardQueue::Learning, 1)),
            Cell::Percent(Percent::Integer(0))
        );
        assert_eq!(cells::interval(&card(CardType::New, CardQueue::New, 1)), Cell::NewMarker);
        assert_eq!(
            cells::interval(&card(CardType::Learning, CardQueue::Learning, 1)),
            Cell::LearningMarker
        );
        let mut relearning = card(CardType::Relearning, CardQueue::Learning, 1);
        assert_eq!(cells::interval(&relearning), Cell::TimeSpan(0.0));
        relearning.interval = 60_000;
        // 60,000 days of seconds, reduced modulo 2^32.
        assert_eq!(cells::interval(&relearning), Cell::TimeSpan(889_032_704.0));
    }

    #[test]
    fn created_truncates_toward_zero_and_tags_split_at_both_spaces_c7_9_c7_10() {
        assert_eq!(cells::created(-60_500), Cell::DateTime(-60));
        assert_eq!(cells::created(1_894_701_600_999), Cell::DateTime(1_894_701_600));
        assert_eq!(cells::tags(" a  b\u{3000}c "), "a b c");
        assert_eq!(cells::tags(""), "");
        assert_eq!(cells::card_type("Cloze", true, 9), "Cloze 10");
        assert_eq!(cells::card_type("Card 1", false, 9), "Card 1");
    }

    #[test]
    fn position_and_memory_state_cells_c7_11_c7_12() {
        let new = card(CardType::New, CardQueue::New, -100_000);
        let review = card(CardType::Review, CardQueue::Review, 4);
        let none = Fields::default();
        assert_eq!(cells::position(&new, &none), Cell::Position(-100_000));
        assert_eq!(cells::position(&review, &none), Cell::Empty);
        let pos = Fields::parse(r#"{"pos":3}"#);
        assert_eq!(cells::position(&review, &pos), Cell::Position(3));
        // A non-string `cd` makes the whole text read as no keys (C1).
        let broken = Fields::parse(r#"{"pos":5,"s":10,"d":5,"cd":5}"#);
        assert_eq!(cells::position(&card(CardType::New, CardQueue::New, 7), &broken), Cell::Position(7));
        assert_eq!(cells::stability(&broken), Cell::Empty);
        assert_eq!(cells::stability(&Fields::parse(r#"{"s":1}"#)), Cell::Empty);
        assert_eq!(
            cells::stability(&Fields::parse(r#"{"s":-1,"d":5}"#)),
            Cell::TimeSpan(-86_400.0)
        );
        assert_eq!(
            cells::stability(&Fields::parse(r#"{"s":1e40,"d":5}"#)),
            Cell::TimeSpan(f32::INFINITY)
        );
        for (d, expected) in [("5.0", 44.0f32), ("3.0", 22.0), ("9.0", 89.0)] {
            let Cell::Percent(Percent::Real(value)) =
                cells::difficulty(&Fields::parse(&format!(r#"{{"s":1,"d":{d}}}"#)))
            else {
                panic!("a percent");
            };
            assert_eq!(f64::from(value).round_ties_even() as f32, expected, "{d}");
        }
        let Cell::Percent(Percent::Real(value)) = cells::difficulty(&Fields::parse(r#"{"s":1,"d":0.99}"#)) else {
            panic!("a percent");
        };
        assert!(value < 0.0 && value > -0.5, "{value}");
    }

    fn retrievability(card: &Card, data: &str) -> f32 {
        match cells::retrievability(card, &Fields::parse(data), TIME) {
            Cell::Percent(Percent::Real(value)) => value,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn retrievability_uses_the_cells_own_elapsed_time_c11() {
        let mut card = card(CardType::Review, CardQueue::Review, 3);
        card.interval = 10;
        // Due in 3 days with an interval of 10: 7 days elapsed.
        assert_eq!(retrievability(&card, r#"{"s":10,"d":3}"#).round(), 93.0);
        card.due = 4;
        card.interval = 4;
        assert_eq!(retrievability(&card, r#"{"s":10,"d":3}"#), 100.0);
        assert_eq!(cells::retrievability(&card, &Fields::parse(r#"{"s":10}"#), TIME), Cell::Empty);
        // `s` 0 with no time elapsed is NaN.
        assert!(retrievability(&card, r#"{"s":0,"d":3}"#).is_nan());
        // `lrt` present: now − lrt.
        let day_ago = format!(r#"{{"s":10,"d":3,"lrt":{}}}"#, TIME.now - 86_400);
        let one_day = retrievability(&card, &day_ago);
        assert!(one_day < 100.0 && one_day > 90.0, "{one_day}");
        // An `lrt` after now wraps to a very large elapsed time (about
        // 49,710 days: 2.9%).
        let ahead = format!(r#"{{"s":10,"d":3,"lrt":{}}}"#, TIME.now + 10);
        assert!(retrievability(&card, &ahead) < 5.0);
    }

    #[test]
    fn state_and_tint_c4_c5() {
        let mut card = card(CardType::Review, CardQueue::Suspended, 1);
        card.flags = 1;
        let state = cells::card_state(&card, " Marked leech::old ");
        assert_eq!(
            state,
            RowState {
                flag: 1,
                marked: true,
                suspended: true,
                ..RowState::default()
            }
        );
        // AE44: flag 1 wins over marked and suspended.
        assert_eq!(cells::card_tint(&card, &state), Tint::Flag(1));
        assert_eq!(cells::note_state(" Marked Leech "), RowState { marked: true, leech: true, ..RowState::default() });
        assert_eq!(cells::note_tint(&cells::note_state(" Marked ")), Tint::Marked);
        // Flags 9 is flag 1 in the state but no flag tint.
        card.flags = 9;
        let state = cells::card_state(&card, " unmarked märked marked::x ");
        assert_eq!(state.flag, 1);
        assert!(!state.marked);
        assert_eq!(cells::card_tint(&card, &state), Tint::Suspended);
        let mut buried = filtered(super::cells_card(CardQueue::BuriedByScheduler), 1);
        buried.flags = 0;
        let state = cells::card_state(&buried, "");
        assert!(state.buried && state.filtered);
        assert_eq!(cells::card_tint(&buried, &state), Tint::Buried);
    }

    #[test]
    fn notes_mode_aggregates_wrap_as_c13_says_c8() {
        let review = |factor: u16, interval: u32| {
            let mut card = card(CardType::Review, CardQueue::Review, 2);
            card.ease_factor = factor;
            card.interval = interval;
            card
        };
        let learning = card(CardType::Learning, CardQueue::Learning, 1_894_702_800);
        let pair = [review(2500, 10), learning.clone()];
        assert_eq!(notes::ease(&pair), Cell::Percent(Percent::Integer(125)));
        assert_eq!(notes::interval(&pair), Cell::TimeSpan(864_000.0));
        // The earliest instant over eligible cards: the learning card.
        assert_eq!(notes::due(&pair, TIME), instant(1_894_702_800, false));
        let new = [card(CardType::New, CardQueue::New, 1)];
        assert_eq!(notes::ease(&new), Cell::NewMarker);
        assert_eq!(notes::interval(&new), Cell::Empty);
        assert_eq!(notes::due(&new, TIME), Cell::Empty);
        // Suspended and filtered cards do not count for Due; never wrapped.
        let suspended = card(CardType::Review, CardQueue::Suspended, 1);
        assert_eq!(notes::due(&[suspended], TIME), Cell::Empty);
        let many: Vec<Card> = (0..27).map(|_| review(2500, 1)).collect();
        assert_eq!(notes::ease(&many), Cell::Percent(Percent::Integer(7)));
        let long = [review(2500, 30_000), review(2500, 30_000)];
        let Cell::TimeSpan(span) = notes::interval(&long) else {
            panic!("a span");
        };
        assert_eq!(span_parts(span), (SpanUnit::Years, 14.1));
        let mut heavy = [review(2500, 1), review(2500, 1)];
        for card in &mut heavy {
            card.reps = 3_000_000_000;
            card.lapses = 3_000_000_000;
        }
        assert_eq!(notes::reviews(&heavy), Cell::Count(1_705_032_704));
        assert_eq!(notes::lapses(&heavy), Cell::Count(1_705_032_704));
        let mut decks = [review(2500, 1), review(2500, 1)];
        decks[1].deck_id = DeckId(99);
        assert_eq!(notes::distinct_decks(&decks), 2);
    }

    #[test]
    fn time_spans_take_the_c12_unit_and_amount() {
        let day = 86_400.0f32;
        for (seconds, unit, amount) in [
            (59.6f32, SpanUnit::Seconds, "60"),
            (0.9999 * day, SpanUnit::Hours, "24"),
            (90.0, SpanUnit::Minutes, "1.5"),
            (10.0 * day, SpanUnit::Days, "10"),
            (12.5 * day, SpanUnit::Days, "13"),
            (40.0 * day, SpanUnit::Months, "1.3"),
            (45.0 * day, SpanUnit::Months, "1.5"),
            (364.0 * day, SpanUnit::Months, "12"),
            (365.0 * day, SpanUnit::Years, "1"),
            (30_000.0 * day, SpanUnit::Years, "82.2"),
            (0.0, SpanUnit::Seconds, "0"),
            (-0.0, SpanUnit::Seconds, "-0"),
            (-day, SpanUnit::Days, "-1"),
            (f32::INFINITY, SpanUnit::Years, "inf"),
        ] {
            let (found_unit, found) = span_parts(seconds);
            assert_eq!((found_unit, span_amount_text(found).as_str()), (unit, amount), "{seconds}");
        }
        // Two decimals, an exact half to the even digit.
        assert_eq!(span_amount_text(0.125), "0.12");
        assert_eq!(span_amount_text(0.375), "0.38");
    }

    #[test]
    fn the_catalog_holds_labels_alignment_elision_and_font_c2() {
        assert_eq!(Column::CardType.label(SearchMode::Cards), "Card Type");
        assert_eq!(Column::CardType.label(SearchMode::Notes), "Cards");
        assert_eq!(Column::Ease.label(SearchMode::Notes), "Avg. Ease");
        assert_eq!(Column::Interval.label(SearchMode::Notes), "Avg. Interval");
        assert_eq!(Column::Due.alignment(), Alignment::Center);
        assert_eq!(Column::Deck.alignment(), Alignment::Start);
        assert_eq!(Column::Deck.elision(), Elision::Middle);
        assert_eq!(Column::Tags.elision(), Elision::End);
        let fonted: Vec<_> = Column::ALL.into_iter().filter(|c| c.uses_row_font()).collect();
        assert_eq!(fonted, [Column::SortField, Column::Question, Column::Answer]);
        assert_eq!(Font::of("", 0), Font::default());
        assert_eq!(Font::of("Arial", 0).size, None);
        assert_eq!(Font::of("", 14).name, None);
    }
}

fn cells_card(queue: meguri_model::card::CardQueue) -> meguri_model::card::Card {
    use meguri_model::{card::*, card_data::CardData, ids::*, time::EpochSecs, usn::Usn};
    Card {
        id: CardId(2),
        note_id: NoteId(1),
        deck_id: DeckId(1),
        ord: 0,
        mtime: EpochSecs(0),
        usn: Usn(0),
        card_type: CardType::New,
        queue,
        due: 1,
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: CardData::default(),
    }
}
