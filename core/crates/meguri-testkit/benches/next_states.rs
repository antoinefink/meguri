//! U45: warm pure state computation, including previews and finished labels.
use criterion::Criterion;
use meguri_model::{
    card::{Card, CardQueue, CardType},
    card_data::MemoryState,
    deck_config::DeckConfig,
    ids::{CardId, DeckConfigId, DeckId, NoteId},
    proto::{ProtoBlob, deck_config::DeckConfigMessage},
};
use meguri_sched::{
    model::preset::PresetView,
    states::{
        engine::{StateInput, next_states},
        fsrs_glue::FsrsCache,
    },
};
use meguri_testkit::bench::{
    peak_alloc::PeakAlloc,
    report::{self, Measurement, Suite},
    timing::Percentile,
};
use std::{hint::black_box, process::ExitCode, time::Duration};
#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;
fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let preset = PresetView::new(&DeckConfig {
        id: DeckConfigId(1),
        name: String::new(),
        mtime: Default::default(),
        usn: Default::default(),
        config: ProtoBlob::new(DeckConfigMessage::builtin_defaults()),
    });
    let mut card = Card {
        id: CardId(1894701600000),
        note_id: NoteId(1),
        deck_id: DeckId(1),
        ord: 0,
        mtime: Default::default(),
        usn: Default::default(),
        card_type: CardType::New,
        queue: CardQueue::New,
        due: 0,
        interval: 10,
        ease_factor: 2500,
        reps: 10,
        lapses: 2,
        remaining_steps: 1,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: Default::default(),
    };
    let mut cache = FsrsCache::default();
    let mut suite = Suite::wall_clock("next_states");
    for (name, kind, queue) in [
        ("new", CardType::New, CardQueue::New),
        ("learning", CardType::Learning, CardQueue::Learning),
        ("review", CardType::Review, CardQueue::Review),
        ("relearning", CardType::Relearning, CardQueue::Learning),
    ] {
        card.card_type = kind;
        card.queue = queue;
        let input = StateInput {
            card: &card,
            preset: &preset,
            memory_state: Some(MemoryState {
                stability: 10.0,
                difficulty: 5.0,
            }),
            last_review_time: Some(1894442400),
            desired_retention: 0.9,
            today: 100,
            next_day_at: 1894766400,
            now: 1894701600,
            short_term_with_steps: false,
            learn_ahead: 1200,
        };
        black_box(next_states(&mut cache, &input, None).unwrap());
        let mut measurement = Measurement::start(&format!("sched.next_states.{name}"), None);
        criterion.bench_function(&format!("next_states/{name}"), |b| {
            b.iter_custom(|iters| {
                measurement.run(iters, |_| {
                    black_box(next_states(&mut cache, black_box(&input), None).unwrap())
                })
            })
        });
        suite.push(measurement.finish());
    }
    criterion.final_summary();
    if let Err(e) = suite.write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")))) {
        eprintln!("next states report: {e}");
        return ExitCode::FAILURE;
    }
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let p95 = record.percentile(Percentile::P95);
        eprintln!("{} p95 {p95:?} (budget 5us)", record.name);
        if p95 > Duration::from_micros(5) {
            status = ExitCode::FAILURE;
        }
    }
    status
}
