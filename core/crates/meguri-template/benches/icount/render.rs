//! R1864: large card-template rendering, including AV and display transforms.
use gungraun::prelude::*;
use meguri_template::{
    CardSpecials, FieldMap, ParsedTemplate,
    card::{self, RenderMode},
    display::{self, RenderHost},
};
use std::hint::black_box;
struct Input {
    source: String,
    question: ParsedTemplate,
    answer: ParsedTemplate,
    fields: FieldMap,
}
fn input(size: usize) -> Input {
    let suffix = "{{Front}}[sound:file]";
    let source = if size == 30 * 1024 {
        let prefix = "<script>/*";
        let close = "*/</script>";
        format!(
            "{prefix}{}{close}{suffix}",
            "x".repeat(size - prefix.len() - close.len() - suffix.len())
        )
    } else {
        let mut body = "<p>template</p>".repeat(size / 15);
        body.truncate(size - suffix.len());
        body.push_str(suffix);
        body
    };
    assert_eq!(source.len(), size);
    let fields = FieldMap::new(
        [("Front", "text")],
        &CardSpecials {
            ord: 0,
            notetype_name: "Render",
            template_name: "Card",
            deck_name: None,
            flags: 0,
            card_id: 1,
            tags: &[],
        },
    );
    Input {
        question: ParsedTemplate::parse(&source),
        answer: ParsedTemplate::parse("{{FrontSide}}"),
        source,
        fields,
    }
}
#[library_benchmark]
#[bench::script_30kb(args = (30 * 1024), setup = input)]
#[bench::template_5mb(args = (5 * 1024 * 1024), setup = input)]
fn warm(input: Input) -> display::DisplaySides {
    let rendered = card::render(
        black_box(&input.question),
        &input.answer,
        &input.fields,
        false,
        RenderMode::Review,
    );
    black_box(display::compose(&rendered, "", 0, RenderHost::default()))
}
#[library_benchmark]
#[bench::script_30kb(args = (30 * 1024), setup = input)]
#[bench::template_5mb(args = (5 * 1024 * 1024), setup = input)]
fn cold(input: Input) -> display::DisplaySides {
    let question = ParsedTemplate::parse(black_box(&input.source));
    let rendered = card::render(&question, &input.answer, &input.fields, false, RenderMode::Review);
    black_box(display::compose(&rendered, "", 0, RenderHost::default()))
}
library_benchmark_group!(name = render, benchmarks = [warm, cold]);
main!(library_benchmark_groups = render);
