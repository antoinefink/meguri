//! `stats/card-info` C14.2.

use meguri_stats::text::{SpanForm, time_span};

#[test]
fn c5_2_time_spans() {
    assert_eq!(time_span(70.0, SpanForm::Long, false, false), "1.17 minutes");
}
