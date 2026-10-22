//! Neutral O3–O5/R2/R4 tables; no scheduler stand-in.
use meguri_model::{card::CardType, proto::deck_config::DeckConfigMessage};
use meguri_sched::options::{Code, Field, Severity, Value, remap, steps_parse, validate};
#[test]
fn parser_reports_one_typed_error_per_token_and_formats_exact_units() {
    let issues = steps_parse::parse(
        "0m -5 5x 1.5m 1M 999999999999999999999999999d",
        Field::LearnSteps,
        Some(7),
    )
    .unwrap_err();
    assert_eq!(
        issues.iter().map(|i| i.code).collect::<Vec<_>>(),
        [
            Code::StepZero,
            Code::StepNegative,
            Code::StepMalformed,
            Code::StepMalformed,
            Code::StepMalformed,
            Code::StepTooLong
        ]
    );
    for (index, issue) in issues.iter().enumerate() {
        assert_eq!(issue.preset, Some(7));
        assert_eq!(issue.params["index"], index.to_string());
    }
    for (text, minutes) in [
        ("1m 10m 1h 2h 1d", vec![1., 10., 60., 120., 1440.]),
        ("1s 5s 90s 400m", vec![1. / 60., 5. / 60., 1.5, 400.]),
        ("36h 365d", vec![2160., 525600.]),
    ] {
        assert_eq!(steps_parse::parse(text, Field::LearnSteps, None).unwrap(), minutes);
        assert_eq!(steps_parse::format(&minutes), text);
    }
    assert!(
        steps_parse::parse("\u{2003}\n\t", Field::LearnSteps, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        steps_parse::format(&[0., -1., f32::INFINITY, f32::NAN]),
        "invalid invalid invalid invalid"
    );
    assert!(steps_parse::parse("2147483648s", Field::LearnSteps, None).is_ok());
    assert_eq!(
        steps_parse::parse("2147483649s", Field::LearnSteps, None).unwrap_err()[0].code,
        Code::StepTooLong
    );
    assert_eq!(
        steps_parse::parse("-5x -0m", Field::LearnSteps, None)
            .unwrap_err()
            .iter()
            .map(|i| i.code)
            .collect::<Vec<_>>(),
        [Code::StepMalformed, Code::StepNegative]
    );
}
#[test]
fn remap_table_preserves_thousands_on_equal_and_handles_unsorted_equal_lists() {
    let old = [1., 10., 60.];
    let new = [5., 120.];
    for (left, expected) in [
        (3, Some(2)),
        (2, None),
        (1, None),
        (0, Some(1)),
        (7, Some(2)),
        (1002, None),
        (3003, Some(2)),
    ] {
        assert_eq!(remap::remaining(left, &old, &new), expected);
    }
    assert_eq!(remap::remaining(2, &new, &[]), Some(1));
    assert_eq!(remap::remaining(1, &new, &[]), None);
    assert_eq!(remap::remaining(1, &[], &old), None);
    assert_eq!(remap::remaining(1, &[10.], &[5., 30., 120.]), Some(3));
    assert_eq!(remap::remaining(0, &[10.], &[5., 30., 120.]), Some(2));
    let mut a = DeckConfigMessage::builtin_defaults();
    let mut b = a.clone();
    b.learn_steps = vec![1., 10., 100.];
    assert_eq!(remap::preset_change(CardType::Learning, 2, &a, &b), Some(3));
    assert_eq!(remap::preset_change(CardType::Learning, 1, &a, &b), Some(2));
    b.relearn_steps = vec![1., 10., 100.];
    assert_eq!(remap::preset_change(CardType::Relearning, 1, &a, &b), Some(3));
    assert_eq!(remap::preset_change(CardType::Review, 2, &a, &b), None);
    a.learn_steps = vec![1., 1., 10.];
    assert_eq!(remap::preset_change(CardType::Learning, 2, &a, &a), Some(1));
}
#[test]
fn validation_uses_changed_values_only_and_reports_all_editor_ranges() {
    for &field in Field::PRESET
        .iter()
        .chain(Field::DECK)
        .chain(&[Field::Rollover, Field::LearnAheadMinutes])
    {
        let m = field.metadata();
        if let Some(max) = m.maximum {
            let invalid = Value::Number(max + 1.0);
            assert!(
                validate::field(field, None, &invalid, Some(&invalid), "2030-01-15")
                    .iter()
                    .all(|i| i.severity != Severity::Error)
            );
            assert!(
                validate::field(field, None, &invalid, None, "2030-01-15")
                    .iter()
                    .any(|i| i.code == Code::OutOfRange)
            );
            assert!(
                validate::field(field, None, &Value::Number(f64::NAN), None, "2030-01-15")
                    .iter()
                    .any(|i| i.code == Code::OutOfRange)
            );
            if m.whole {
                assert!(
                    validate::field(field, None, &Value::Number(max - 0.5), None, "2030-01-15")
                        .iter()
                        .any(|i| i.code == Code::NotWhole)
                );
            }
        }
        if !m.choices.is_empty() {
            assert_eq!(
                validate::field(field, None, &Value::Number(42.), None, "2030-01-15")[0].code,
                Code::InvalidChoice
            );
        }
    }
    assert_eq!(
        validate::field(
            Field::IgnoreRevlogsBeforeDate,
            None,
            &Value::Text("bad".into()),
            None,
            "2030-01-15"
        )[0]
        .code,
        Code::InvalidDate
    );
    assert_eq!(
        validate::field(
            Field::IgnoreRevlogsBeforeDate,
            None,
            &Value::Text("2030-01-16".into()),
            None,
            "2030-01-15"
        )[0]
        .code,
        Code::DateInFuture
    );
    assert!(
        validate::field(
            Field::LearnSteps,
            None,
            &Value::Text("1d 1m".into()),
            None,
            "2030-01-15"
        )
        .iter()
        .any(|i| i.code == Code::StepsOneDayOrMore)
    );
}
#[test]
fn warning_bands_use_effective_limits_and_two_place_retention() {
    let mut values = meguri_sched::options::fields::read(&DeckConfigMessage::builtin_defaults());
    let mut deck = Field::DECK.iter().map(|f| (*f, Value::None)).collect();
    values.insert(Field::ReviewsPerDay, Value::Number(100.));
    values.insert(Field::DesiredRetention, Value::Number(0.96));
    let warnings = validate::effective_warnings(1, &values, &deck);
    assert!(
        warnings
            .iter()
            .any(|i| i.code == Code::ReviewsBelowTenTimesNew && i.params["expected"] == "200")
    );
    assert!(
        warnings
            .iter()
            .any(|i| i.code == Code::RetentionHigh && i.severity == Severity::Warning)
    );
    deck.insert(Field::NewLimitToday, Value::Number(0.));
    deck.insert(Field::DeckDesiredRetention, Value::Number(0.68));
    let warnings = validate::effective_warnings(1, &values, &deck);
    assert!(!warnings.iter().any(|i| i.code == Code::ReviewsBelowTenTimesNew));
    assert!(
        warnings
            .iter()
            .any(|i| i.code == Code::RetentionLow && i.severity == Severity::StrongWarning)
    );
}
