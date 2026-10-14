//! The `cards.data` text (`specs/model/card-data-json.md`).

use serde_json::value::RawValue;

use crate::json::{self, Kind, ObjectWriter};

/// The seven keys Anki knows, in the order they are written (C2, C4).
const KNOWN_KEYS: [&str; 7] = ["pos", "s", "d", "dr", "decay", "lrt", "cd"];

/// FSRS stability and difficulty: both present or both absent (C3 rule 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemoryState {
    pub stability: f32,
    pub difficulty: f32,
}

/// The values of a card's `data` column. Reading never fails (C3); writing
/// rebuilds the text from the values every time (C4).
#[derive(Clone, Debug, Default)]
pub struct CardData {
    /// `pos`: the card's position while it was new.
    pub original_position: Option<u32>,
    /// `s` and `d`.
    pub memory_state: Option<MemoryState>,
    /// `dr`: the desired retention the card was last scheduled with.
    pub desired_retention: Option<f32>,
    /// `decay`: the FSRS decay the card was last scheduled with.
    pub decay: Option<f32>,
    /// `lrt`: last review time, epoch seconds.
    pub last_review_secs: Option<i64>,
    custom_data: Option<String>,
    unknown: Vec<(String, String)>,
}

impl CardData {
    /// C3: text that is empty, not JSON, or JSON other than an object or an
    /// array reads as no keys at all. So does a number beyond the 64-bit
    /// float range anywhere but in the value of an unknown key, which Anki
    /// skips without reading (rule 1, `model/json-numbers` N10).
    pub fn parse(text: &str) -> CardData {
        let Some(raw) = json::parse_skipping_range(text) else {
            return CardData::default();
        };
        match json::kind(raw) {
            Kind::Object => CardData::from_object(raw),
            Kind::Array if json::in_range(raw) => CardData::from_array(raw),
            _ => CardData::default(),
        }
    }

    fn from_object(raw: &RawValue) -> CardData {
        let Some(members) = json::parse_members(raw.get()) else {
            return CardData::default();
        };
        // C3 rule 2: a known key twice means no keys at all. Rule 1: so does
        // a known key's value holding a number out of range.
        let known = |key: &str| KNOWN_KEYS.contains(&key);
        if KNOWN_KEYS
            .iter()
            .any(|known| members.iter().filter(|(key, _)| key == known).count() > 1)
            || members.iter().any(|(key, value)| known(key) && !json::in_range(value))
        {
            return CardData::default();
        }
        let value = |name: &str| members.iter().find(|(key, _)| key == name).map(|(_, raw)| *raw);
        let mut data = CardData::read(
            value("pos"),
            value("s"),
            value("d"),
            value("dr"),
            value("decay"),
            value("lrt"),
            value("cd"),
        );
        // C3 rule 7: unknown keys kept, a repeated one at its last place.
        let unknown: Vec<json::Member<'_>> = members.into_iter().filter(|(key, _)| !known(key)).collect();
        data.unknown = json::last_wins(unknown)
            .into_iter()
            .map(|(key, raw)| (key, raw.get().to_owned()))
            .collect();
        data
    }

    /// C3 rule 6: by position, `null` meaning absent; more than 7 elements
    /// read as no keys at all.
    fn from_array(raw: &RawValue) -> CardData {
        let Some(items) = json::array(raw) else {
            return CardData::default();
        };
        if items.len() > KNOWN_KEYS.len() {
            return CardData::default();
        }
        let at = |i: usize| items.get(i).copied();
        CardData::read(at(0), at(1), at(2), at(3), at(4), at(5), at(6))
    }

    /// C3 rules 3 to 5.
    fn read(
        pos: Option<&RawValue>,
        s: Option<&RawValue>,
        d: Option<&RawValue>,
        dr: Option<&RawValue>,
        decay: Option<&RawValue>,
        lrt: Option<&RawValue>,
        cd: Option<&RawValue>,
    ) -> CardData {
        let stability = s.and_then(json::f32_value);
        let difficulty = d.and_then(json::f32_value);
        CardData {
            original_position: pos.and_then(json::u32_value),
            memory_state: stability
                .zip(difficulty)
                .map(|(stability, difficulty)| MemoryState { stability, difficulty }),
            desired_retention: dr.and_then(json::f32_value),
            decay: decay.and_then(json::f32_value),
            last_review_secs: lrt.and_then(json::i64_value),
            custom_data: cd
                .and_then(json::string)
                .filter(|text| !text.is_empty() && text != "{}"),
            unknown: Vec::new(),
        }
    }

    /// `cd`: custom scheduling data, a string holding JSON text. Meguri only
    /// carries it (R591).
    pub fn custom_data(&self) -> Option<&str> {
        self.custom_data.as_deref()
    }

    /// Keys Anki does not know, with their exact value text, in kept order
    /// (R1461).
    pub fn unknown_keys(&self) -> impl Iterator<Item = (&str, &str)> {
        self.unknown.iter().map(|(key, text)| (key.as_str(), text.as_str()))
    }

    /// C4: the known members present, rounded, in order, then the unknown
    /// ones; `{}` when there is none.
    pub fn to_text(&self) -> String {
        let mut out = ObjectWriter::new();
        if let Some(pos) = self.original_position {
            out.int("pos", pos);
        }
        if let Some(memory) = self.memory_state {
            out.f32("s", round(memory.stability, 10_000.0));
            out.f32("d", round(memory.difficulty, 1_000.0));
        }
        if let Some(dr) = self.desired_retention {
            out.f32("dr", round(dr, 100.0));
        }
        if let Some(decay) = self.decay {
            out.f32("decay", round(decay, 1_000.0));
        }
        if let Some(lrt) = self.last_review_secs {
            out.int("lrt", lrt);
        }
        if let Some(cd) = &self.custom_data {
            out.string("cd", cd);
        }
        for (key, text) in &self.unknown {
            out.raw(key, text);
        }
        out.finish()
    }
}

/// C4 rule 2, in `f32` arithmetic: scale, round half away from zero, unscale.
fn round(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}
