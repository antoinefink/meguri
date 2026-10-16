//! The rewriting sink: the walker's events become the clone's text, with
//! characters drawn from their pools (A5.2) under the guards (A5.4).

use super::chars::{self, Class};
use super::keys::Stream;
use super::text::{self, Names, Sink};

/// Characters and entities replaced (A16).
#[derive(Clone, Copy, Debug, Default)]
pub struct CharCounts {
    pub chars: u64,
    pub entities: u64,
}

pub struct Rewriter<'r> {
    pub out: String,
    stream: &'r mut Stream,
    counts: &'r mut CharCounts,
    /// G3: the output position of the run's `&`, and each replaced
    /// character's output position and class.
    g3: Option<(usize, Vec<(usize, Class)>)>,
}

impl<'r> Rewriter<'r> {
    pub fn new(stream: &'r mut Stream, counts: &'r mut CharCounts, capacity: usize) -> Rewriter<'r> {
        Rewriter {
            out: String::with_capacity(capacity),
            stream,
            counts,
            g3: None,
        }
    }

    fn draw(&mut self, class: Class) -> char {
        let excluded = chars::guard_letter(class, &self.out);
        class.draw(self.stream, excluded)
    }

    fn push_drawn(&mut self, class: Class) {
        let at = self.out.len();
        let c = self.draw(class);
        if let Some((_, replaced)) = &mut self.g3 {
            replaced.push((at, class));
        }
        self.out.push(c);
    }
}

impl Sink for Rewriter<'_> {
    fn keep(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn name(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn replace(&mut self, c: char) {
        self.counts.chars += 1;
        self.push_drawn(Class::of(c));
    }

    fn entity(&mut self, _raw: &str, c: char) {
        self.counts.entities += 1;
        self.push_drawn(Class::of(c));
    }

    fn g3_begin(&mut self) {
        self.g3 = Some((self.out.len() - 1, Vec::new()));
    }

    fn g3_end(&mut self) {
        let Some((start, replaced)) = self.g3.take() else {
            return;
        };
        // Drawn again, in order, until the run is no entity. G3 runs hold
        // only ASCII, so each character is one byte in place.
        while !replaced.is_empty() && chars::is_entity(&self.out[start..]) {
            for &(at, class) in &replaced {
                let excluded = chars::guard_letter(class, &self.out[..at]);
                let c = class.draw(self.stream, excluded);
                let mut buf = [0u8; 4];
                self.out.replace_range(at..at + 1, c.encode_utf8(&mut buf));
            }
        }
    }
}

/// A field (or a deck description) rewritten (A6–A9).
pub fn field<N: Names>(src: &str, names: &mut N, stream: &mut Stream, counts: &mut CharCounts) -> String {
    let mut sink = Rewriter::new(stream, counts, src.len());
    text::walk_field(src, names, &mut sink);
    sink.out
}

/// A plain string replaced as text (A6.3 without tags).
pub fn plain<N: Names>(src: &str, names: &mut N, stream: &mut Stream, counts: &mut CharCounts) -> String {
    let mut sink = Rewriter::new(stream, counts, src.len());
    text::walk_text(src, names, &mut sink);
    sink.out
}
