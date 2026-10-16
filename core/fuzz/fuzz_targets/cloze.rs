//! Cloze parsing and rendering never panic, on any field text
//! (specs/render/cloze.md C12).
//!
//! Inputs are cut to 1 KiB: front output grows about 50,000-fold for a
//! character ten active levels deep (C12), exactly as in Anki, so a longer
//! input can take any amount of memory without being a finding.

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_template::cloze::{self, Side};

const MAX_INPUT: usize = 1024;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(&data[..data.len().min(MAX_INPUT)]);

    let numbers = cloze::numbers(&text);
    assert!(!numbers.contains(&0), "{text:?}: 0 is never a cloze number");
    if cloze::has_cloze(&text) {
        assert!(!numbers.is_empty(), "{text:?}: a top-level cloze has a number");
    }

    // A card with no cloze, and the cards of the text's first clozes.
    let cards = std::iter::once(65535u32).chain(numbers.iter().take(3).map(|&n| u32::from(n)));
    for card in cards {
        let active = numbers.iter().any(|&n| u32::from(n) == card);
        for side in [Side::Front, Side::Back] {
            let rendered = cloze::render(&text, card, side);
            // C12: spans are balanced unless C8 found math to strip. With no
            // `<` in the text, every tag in the output is a cloze span.
            if !text.contains('<') && !rendered.contains("\\(") && !rendered.contains("\\[") {
                assert_eq!(
                    rendered.matches("<span ").count(),
                    rendered.matches("</span>").count(),
                    "{text:?} card {card} {side:?}"
                );
            }
            let only = cloze::render_only(&text, card, side);
            if !active {
                assert!(rendered.is_empty() && only.is_empty(), "{text:?} card {card} {side:?}");
            }
        }
        let typed = cloze::type_answer(&text, card);
        if !active {
            assert!(typed.is_empty(), "{text:?} card {card}");
        }
    }
});
