//! O4: pure typed issues, with stable codes and per-token diagnostics.
use super::{Field, Value, steps_parse};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    StrongWarning,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    OutOfRange,
    NotWhole,
    InvalidChoice,
    StepMalformed,
    StepZero,
    StepNegative,
    StepTooLong,
    InvalidDate,
    DateInFuture,
    StepsOneDayOrMore,
    ReviewsBelowTenTimesNew,
    RetentionLow,
    RetentionHigh,
    MaxIntervalShort,
    AnswerSecondsHigh,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub field: Field,
    pub preset: Option<i64>,
    pub code: Code,
    pub severity: Severity,
    pub params: BTreeMap<String, String>,
}
impl Issue {
    pub fn error(field: Field, preset: Option<i64>, code: Code) -> Self {
        Self {
            field,
            preset,
            code,
            severity: Severity::Error,
            params: BTreeMap::new(),
        }
    }
    pub fn warning(field: Field, preset: Option<i64>, code: Code, strong: bool) -> Self {
        Self {
            severity: if strong {
                Severity::StrongWarning
            } else {
                Severity::Warning
            },
            ..Self::error(field, preset, code)
        }
    }
}
pub fn field(field: Field, preset: Option<i64>, value: &Value, base: Option<&Value>, today: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    let changed = base.is_none_or(|b| !value.same(b, field));
    let meta = field.metadata();
    let issue = |code| Issue::error(field, preset, code);
    if matches!(field, Field::LearnSteps | Field::RelearnSteps) {
        let parsed = match value {
            Value::Text(text) => steps_parse::parse(text, field, preset),
            Value::Steps(v) => Ok(v.clone()),
            _ => {
                if changed {
                    issues.push(issue(Code::StepMalformed));
                }
                return issues;
            }
        };
        match parsed {
            Err(errors) if changed => issues.extend(errors),
            Ok(steps) => {
                if steps.iter().any(|&s| s >= 1440.0) {
                    issues.push(Issue::warning(field, preset, Code::StepsOneDayOrMore, false));
                }
                if changed && matches!(value, Value::Steps(_)) {
                    for (index, step) in steps.iter().enumerate() {
                        let code = if !step.is_finite() {
                            Some(Code::StepMalformed)
                        } else if *step < 0.0 {
                            Some(Code::StepNegative)
                        } else if *step == 0.0 {
                            Some(Code::StepZero)
                        } else if *step > (2_147_483_648.0_f64 / 60.0) as f32 {
                            Some(Code::StepTooLong)
                        } else {
                            None
                        };
                        if let Some(code) = code {
                            let mut i = issue(code);
                            i.params.insert("index".into(), index.to_string());
                            i.params.insert("token".into(), step.to_string());
                            issues.push(i);
                        }
                    }
                }
            }
            _ => (),
        }
        return issues;
    }
    if field == Field::IgnoreRevlogsBeforeDate {
        if changed {
            match value {
                Value::Text(text) => match crate::memory::ignore_before::cutoff_millis(text) {
                    Err(_) => issues.push(issue(Code::InvalidDate)),
                    Ok(date)
                        if !text.is_empty()
                            && date > crate::memory::ignore_before::cutoff_millis(today).expect("valid today") =>
                    {
                        let mut i = issue(Code::DateInFuture);
                        i.params.insert("today".into(), today.into());
                        issues.push(i);
                    }
                    _ => (),
                },
                _ => issues.push(issue(Code::InvalidDate)),
            }
        }
        return issues;
    }
    if changed {
        match value {
            Value::None if meta.optional => (),
            Value::Number(n) if !meta.choices.is_empty() => {
                if !n.is_finite() || n.fract() != 0.0 || !meta.choices.iter().any(|&v| f64::from(v) == *n) {
                    issues.push(issue(Code::InvalidChoice));
                }
            }
            Value::Number(n) if meta.minimum.is_some() => {
                let low = meta.minimum.unwrap();
                let high = meta.maximum.unwrap();
                let bounded = if field.is_float() {
                    (*n as f32) >= (low as f32) && (*n as f32) <= (high as f32)
                } else {
                    *n >= low && *n <= high
                };
                if !n.is_finite() || !bounded {
                    let mut i = issue(Code::OutOfRange);
                    i.params.insert("min".into(), low.to_string());
                    i.params.insert("max".into(), high.to_string());
                    issues.push(i);
                }
                if n.is_finite() && meta.whole && n.fract() != 0.0 {
                    issues.push(issue(Code::NotWhole));
                }
            }
            Value::Boolean(_) if matches!(meta.default, Value::Boolean(_)) => (),
            _ => issues.push(issue(if meta.choices.is_empty() {
                Code::OutOfRange
            } else {
                Code::InvalidChoice
            })),
        }
    }
    if let Value::Number(n) = value {
        if field == Field::MaximumReviewInterval && *n < 180.0 {
            issues.push(Issue::warning(field, preset, Code::MaxIntervalShort, *n < 50.0));
        }
        if field == Field::CapAnswerTimeToSecs && *n > 600.0 {
            issues.push(Issue::warning(field, preset, Code::AnswerSecondsHigh, false));
        }
    }
    issues
}
pub fn effective_warnings(preset: i64, values: &BTreeMap<Field, Value>, deck: &BTreeMap<Field, Value>) -> Vec<Issue> {
    let number = |f| match values[&f] {
        Value::Number(v) => v,
        _ => 0.0,
    };
    let effective = |a, b, p| {
        [a, b]
            .iter()
            .find_map(|f| match deck.get(f) {
                Some(Value::Number(v)) => Some(*v),
                _ => None,
            })
            .unwrap_or_else(|| number(p))
    };
    let new = effective(Field::NewLimitToday, Field::NewLimit, Field::NewPerDay);
    let reviews = effective(Field::ReviewLimitToday, Field::ReviewLimit, Field::ReviewsPerDay);
    let mut issues = Vec::new();
    let expected = (10.0 * new).min(9999.0);
    if reviews < expected {
        let mut i = Issue::warning(Field::ReviewsPerDay, Some(preset), Code::ReviewsBelowTenTimesNew, false);
        i.params.insert("expected".into(), expected.to_string());
        issues.push(i);
    }
    let (field, retention) = match deck.get(&Field::DeckDesiredRetention) {
        Some(Value::Number(r)) => (Field::DeckDesiredRetention, *r),
        _ => (Field::DesiredRetention, number(Field::DesiredRetention)),
    };
    let rounded = (retention * 100.0).round() / 100.0;
    if rounded < 0.8 {
        issues.push(Issue::warning(field, Some(preset), Code::RetentionLow, rounded < 0.7));
    }
    if rounded > 0.95 {
        issues.push(Issue::warning(field, Some(preset), Code::RetentionHigh, rounded > 0.97));
    }
    issues
}
