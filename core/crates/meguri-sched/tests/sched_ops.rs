use meguri_sched::ops::set_due::{Days, InvalidDays};
#[test]
fn parser_is_ascii_only_typed_and_accepts_reversed_ranges() {
    assert_eq!(
        Days::parse("5-2!"),
        Ok(Days {
            minimum: 2,
            maximum: 5,
            reset_interval: true
        })
    );
    assert_eq!(Days::parse("007").unwrap().minimum, 7);
    assert_eq!(Days::parse("4294967295").unwrap().maximum, u32::MAX);
    assert_eq!(Days::parse(""), Err(InvalidDays::Empty));
    for text in [
        "abc", "-1", "3--4", "1-2-3", "3-", "+3", "3.5", "3!!", " 3", "3\n", "٣", "!",
    ] {
        assert_eq!(Days::parse(text), Err(InvalidDays::Malformed), "{text:?}");
    }
    for text in ["4294967296", "1-4294967296", "999999999999999999999999999999"] {
        assert_eq!(Days::parse(text), Err(InvalidDays::TooLarge));
    }
}
#[test]
fn injected_ranges_are_repeatable_and_include_the_full_u32_domain() {
    let a = meguri_model::rng::SeededRandom::new(77);
    let b = meguri_model::rng::SeededRandom::new(77);
    let days = Days::parse("5-2!").unwrap();
    let draws = (0..100).map(|_| days.draw(&a)).collect::<Vec<_>>();
    assert!(draws.iter().all(|d| (2..=5).contains(d)));
    assert_eq!(draws, (0..100).map(|_| days.draw(&b)).collect::<Vec<_>>());
    Days::parse("0-4294967295").unwrap().draw(&a);
}
