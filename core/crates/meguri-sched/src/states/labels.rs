//! sched/answer-labels A2–A6. Callers apply the day-boundary conversion
//! first; the same resulting delay feeds both labels and answer previews.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Months,
    Years,
    End,
}
impl TimeUnit {
    pub fn name(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Minutes => "minutes",
            Self::Hours => "hours",
            Self::Days => "days",
            Self::Months => "months",
            Self::Years => "years",
            Self::End => "end",
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Self::Seconds => "s",
            Self::Minutes => "m",
            Self::Hours => "h",
            Self::Days => "d",
            Self::Months => "mo",
            Self::Years => "y",
            Self::End => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerLabel {
    pub less_than: bool,
    pub amount: String,
    pub unit: TimeUnit,
}
impl AnswerLabel {
    pub fn text(&self) -> String {
        if self.unit == TimeUnit::End {
            "(end)".into()
        } else {
            format!(
                "{}{}{}",
                if self.less_than { "<" } else { "" },
                self.amount,
                self.unit.suffix()
            )
        }
    }
}

pub fn format_seconds(seconds: u32, learn_ahead: u32) -> AnswerLabel {
    if seconds == 0 {
        return AnswerLabel {
            less_than: false,
            amount: String::new(),
            unit: TimeUnit::End,
        };
    }
    let value = seconds as f32;
    let (unit, length, tenths) = if value < 60.0 {
        (TimeUnit::Seconds, 1.0, false)
    } else if value < 3600.0 {
        (TimeUnit::Minutes, 60.0, false)
    } else if value < 86400.0 {
        (TimeUnit::Hours, 3600.0, true)
    } else if value < 2628000.0 {
        (TimeUnit::Days, 86400.0, false)
    } else if value < 31536000.0 {
        (TimeUnit::Months, 2628000.0, true)
    } else {
        (TimeUnit::Years, 31536000.0, true)
    };
    let quotient = value / length;
    let amount = if tenths {
        (quotient * 10.0).round() / 10.0
    } else {
        quotient.round()
    };
    let digit = (amount * 10.0).round() as u32;
    let amount = if digit.is_multiple_of(10) {
        (digit / 10).to_string()
    } else {
        format!("{}.{}", digit / 10, digit % 10)
    };
    AnswerLabel {
        less_than: seconds < learn_ahead,
        amount,
        unit,
    }
}
