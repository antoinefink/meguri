//! The forgetting curve (`stats/card-info` C7): the curve reviews, the
//! offered ranges and, per range, the points the view draws (C7.5, Meguri's
//! own sampling) with their tooltip texts (C7.6).

use crate::text::{SpanForm, time_span, two_decimal_percent, whole_percent};

/// The four ranges of C7.3, in offering order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RangeKind {
    FirstWeek,
    FirstMonth,
    FirstYear,
    AllTime,
}

impl RangeKind {
    fn limit(self) -> f64 {
        match self {
            RangeKind::FirstWeek => 7.0,
            RangeKind::FirstMonth => 30.0,
            RangeKind::FirstYear => 365.0,
            RangeKind::AllTime => f64::INFINITY,
        }
    }
}

/// One review the curve starts a segment at (C7.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveReview {
    /// Epoch seconds.
    pub instant: i64,
    /// The review's interval seconds (C6.1).
    pub interval_secs: i64,
    /// The stability of the review's memory state, widened from f32.
    pub stability: f64,
}

/// One drawn point (C7.5) and its tooltip texts (C7.6).
#[derive(Clone, Debug, PartialEq)]
pub struct CurvePoint {
    /// Epoch seconds.
    pub instant: f64,
    pub days_since_first: f64,
    /// Days since the review whose segment the point belongs to.
    pub days_since_review: f64,
    /// Retrievability in percent.
    pub retrievability: f64,
    /// The stability the point follows, in days.
    pub stability: f64,
    /// Drawn dashed: after `now`, and the second of the two `now` points.
    pub future: bool,
    /// The long time span of the days since the review.
    pub elapsed_text: String,
    /// The two-decimal percent of the retrievability.
    pub retrievability_text: String,
    /// The long, unrounded time span of the stability.
    pub stability_text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveRange {
    pub kind: RangeKind,
    /// min(`M`, the range's limit), days.
    pub span_days: f64,
    pub points: Vec<CurvePoint>,
    /// `max(0, 100 − 1.2 × (100 − the smallest retrievability))`.
    pub y_min: f64,
    /// Whether the desired-retention line is drawn: retention × 100 above
    /// `y_min`.
    pub shows_retention: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    /// The offered ranges, First Week first; at least one.
    pub ranges: Vec<CurveRange>,
    /// The default range: the last one offered.
    pub default_range: RangeKind,
    /// `M`, days (C7.3).
    pub maximum_days: f64,
    /// The retention in force for the card's home deck (C7.4).
    pub desired_retention: f32,
    /// Its whole percent, for the tooltip near the line (C7.6).
    pub desired_retention_text: String,
}

/// C7.4's retrievability, `x` days after a review with stability `s`.
fn retrievability(x: f64, stability: f64, decay: f64) -> f64 {
    if x == 0.0 {
        return 100.0;
    }
    let factor = 0.9f64.powf(1.0 / -decay) - 1.0;
    (x / stability * factor + 1.0).powf(-decay) * 100.0
}

struct Points<'a> {
    reviews: &'a [CurveReview],
    first: f64,
    now: f64,
    decay: f64,
    out: Vec<CurvePoint>,
}

impl Points<'_> {
    fn push(&mut self, instant: f64, review: usize, future: bool) {
        let follows = &self.reviews[review];
        let days_since_review = (instant - follows.instant as f64) / 86_400.0;
        let value = retrievability(days_since_review, follows.stability, self.decay);
        self.out.push(CurvePoint {
            instant,
            days_since_first: (instant - self.first) / 86_400.0,
            days_since_review,
            retrievability: value,
            stability: follows.stability,
            future,
            elapsed_text: time_span(days_since_review * 86_400.0, SpanForm::Long, false, false),
            retrievability_text: two_decimal_percent(value),
            stability_text: time_span(follows.stability * 86_400.0, SpanForm::Long, false, false),
        });
    }

    fn at(&mut self, instant: f64, review: usize) {
        let future = instant > self.now;
        self.push(instant, review, future);
    }

    fn now_pair(&mut self, review: usize) {
        self.push(self.now, review, false);
        self.push(self.now, review, true);
    }
}

/// C7.5 for one range. `reviews` are the curve reviews, oldest first, with
/// same-instant reviews already counted as one.
fn range_points(reviews: &[CurveReview], span: f64, now: i64, decay: f64) -> Vec<CurvePoint> {
    let first = reviews[0].instant as f64;
    let end = first + span * 86_400.0;
    let step = (span / 1_000.0).min(1.0);
    let inside = reviews.iter().take_while(|review| review.instant as f64 <= end).count();
    let now = now as f64;
    let mut points = Points {
        reviews: &reviews[..inside],
        first,
        now,
        decay,
        out: Vec::new(),
    };
    let mut now_done = !(first <= now && now <= end);
    for index in 0..inside {
        let start = reviews[index].instant as f64;
        if index > 0 {
            points.at(start, index - 1);
        }
        points.at(start, index);
        if !now_done && now == start {
            points.now_pair(index);
            now_done = true;
        }
        let last = index + 1 == inside;
        let segment_end = if last {
            end
        } else {
            reviews[index + 1].instant as f64
        };
        if step > 0.0 {
            let mut k = 1u64;
            loop {
                let sample = start + (k as f64 * step) * 86_400.0;
                if sample >= segment_end {
                    break;
                }
                k += 1;
                if !now_done && now <= sample {
                    points.now_pair(index);
                    now_done = true;
                    if now == sample {
                        continue;
                    }
                }
                points.at(sample, index);
            }
        }
        if !last {
            if !now_done && now < segment_end {
                points.now_pair(index);
                now_done = true;
            }
        } else if start < end {
            if !now_done && now == end {
                points.now_pair(index);
            } else {
                if !now_done && now < end {
                    points.now_pair(index);
                }
                points.at(end, index);
            }
            now_done = true;
        }
    }
    debug_assert!(now_done || inside == 0);
    points.out
}

/// C7.3 to C7.6. `reviews` are the curve reviews of C7.2, oldest first
/// (same-instant ones not yet merged); `None` when there is none (C7.1).
pub fn build(reviews: &[CurveReview], now: i64, decay: f32, retention: f32) -> Option<Curve> {
    let (oldest, newest) = (reviews.first()?, reviews.last()?);
    let first = (now - oldest.instant) as f64 / 86_400.0;
    let since = (now - newest.instant) as f64 / 86_400.0;
    let last = newest.interval_secs as f64 / 86_400.0;
    let preview = (last * 1.5 - since).max(last * 0.5);
    let maximum = first + preview;
    // C7.5: reviews in the same second count as one, the newest of them.
    let mut merged: Vec<CurveReview> = Vec::with_capacity(reviews.len());
    for review in reviews {
        match merged.last_mut() {
            Some(previous) if previous.instant == review.instant => *previous = *review,
            _ => merged.push(*review),
        }
    }
    let offered = [
        (RangeKind::FirstWeek, true),
        (RangeKind::FirstMonth, maximum > 7.0),
        (RangeKind::FirstYear, maximum > 30.0),
        (RangeKind::AllTime, maximum > 365.0),
    ];
    let decay = f64::from(decay);
    let line = f64::from(retention) * 100.0;
    let ranges: Vec<CurveRange> = offered
        .into_iter()
        .filter(|(_, offered)| *offered)
        .map(|(kind, _)| {
            let span = maximum.min(kind.limit());
            let points = range_points(&merged, span, now, decay);
            let lowest = points.iter().map(|p| p.retrievability).fold(f64::INFINITY, f64::min);
            let y_min = (100.0 - 1.2 * (100.0 - lowest)).max(0.0);
            CurveRange {
                kind,
                span_days: span,
                points,
                y_min,
                shows_retention: line > y_min,
            }
        })
        .collect();
    Some(Curve {
        default_range: ranges.last().expect("First Week is always offered").kind,
        ranges,
        maximum_days: maximum,
        desired_retention: retention,
        desired_retention_text: whole_percent(line),
    })
}
