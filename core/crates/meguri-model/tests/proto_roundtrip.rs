//! Protobuf blobs (specs/model/protobuf-records.md): decoding, canonical
//! encoding with unknown fields kept in place (P3, P4), keeping the bytes a
//! record was read with (P5), and the decode errors (P9).
//!
//! The property tests build their blobs with this file's own wire writer and
//! field tables, written from P2, P3 and P6, never with the code under test.

mod common;

use common::{golden, hex, rows, unhex};
use meguri_model::proto::deck::{DeckCommon, DeckKind, DeckKindMessage};
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_model::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};
use meguri_model::proto::{BlobMessage, ProtoBlob};
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_model::{BlobColumn, BlobError, ModelError};

// ---------------------------------------------------------------------------
// An independent writer and the P6 field tables.

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn put_key(out: &mut Vec<u8>, field: u32, wire: u8) {
    put_varint(out, (u64::from(field) << 3) | u64::from(wire));
}

fn put_len(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
    put_key(out, field, 2);
    put_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

#[derive(Clone, Copy)]
enum Ty {
    Bool,
    Int32,
    Uint32,
    Int64,
    Float,
    Str,
    Bytes,
    OptInt64,
    OptUint32,
    OptFloat,
    Floats,
    Uint32s,
    /// A singular message (written when present).
    Msg(&'static Msg),
    /// A repeated message.
    Msgs(&'static Msg),
}

struct Msg {
    fields: &'static [(u32, Ty)],
    /// A one-of: exactly one member is written.
    one_of: bool,
}

use Ty::*;

static CARD_REQUIREMENT: Msg = Msg {
    fields: &[(1, Uint32), (2, Int32), (3, Uint32s)],
    one_of: false,
};
static NOTETYPE_CONFIG: Msg = Msg {
    fields: &[
        (1, Int32),
        (2, Uint32),
        (3, Str),
        (4, Int64),
        (5, Str),
        (6, Str),
        (7, Bool),
        (8, Msgs(&CARD_REQUIREMENT)),
        (9, Int32),
        (10, OptInt64),
        (255, Bytes),
    ],
    one_of: false,
};
static FIELD_CONFIG: Msg = Msg {
    fields: &[
        (1, Bool),
        (2, Bool),
        (3, Str),
        (4, Uint32),
        (5, Str),
        (6, Bool),
        (7, Bool),
        (8, Bool),
        (9, OptInt64),
        (10, OptUint32),
        (11, Bool),
        (255, Bytes),
    ],
    one_of: false,
};
static TEMPLATE_CONFIG: Msg = Msg {
    fields: &[
        (1, Str),
        (2, Str),
        (3, Str),
        (4, Str),
        (5, Int64),
        (6, Str),
        (7, Uint32),
        (8, OptInt64),
        (255, Bytes),
    ],
    one_of: false,
};
static DECK_COMMON: Msg = Msg {
    fields: &[
        (1, Bool),
        (2, Bool),
        (3, Uint32),
        (4, Int32),
        (5, Int32),
        (6, Int32),
        (7, Int32),
        (255, Bytes),
    ],
    one_of: false,
};
static DAY_LIMIT: Msg = Msg {
    fields: &[(1, Uint32), (2, Uint32)],
    one_of: false,
};
static NORMAL_DECK: Msg = Msg {
    fields: &[
        (1, Int64),
        (2, Uint32),
        (3, Uint32),
        (4, Str),
        (5, Bool),
        (6, OptUint32),
        (7, OptUint32),
        (8, Msg(&DAY_LIMIT)),
        (9, Msg(&DAY_LIMIT)),
        (10, OptFloat),
    ],
    one_of: false,
};
static SEARCH_TERM: Msg = Msg {
    fields: &[(1, Str), (2, Uint32), (3, Int32)],
    one_of: false,
};
static FILTERED_DECK: Msg = Msg {
    fields: &[
        (1, Bool),
        (2, Msgs(&SEARCH_TERM)),
        (3, Floats),
        (4, Uint32),
        (5, Uint32),
        (6, Uint32),
        (7, Uint32),
    ],
    one_of: false,
};
static DECK_KIND: Msg = Msg {
    fields: &[(1, Msg(&NORMAL_DECK)), (2, Msg(&FILTERED_DECK))],
    one_of: true,
};
static DECK_CONFIG: Msg = Msg {
    fields: &[
        (1, Floats),
        (2, Floats),
        (3, Floats),
        (4, Floats),
        (5, Floats),
        (6, Floats),
        (9, Uint32),
        (10, Uint32),
        (11, Float),
        (12, Float),
        (13, Float),
        (14, Float),
        (15, Float),
        (16, Uint32),
        (17, Uint32),
        (18, Uint32),
        (19, Uint32),
        (20, Int32),
        (21, Int32),
        (22, Uint32),
        (23, Bool),
        (24, Uint32),
        (25, Bool),
        (26, Bool),
        (27, Bool),
        (28, Bool),
        (29, Bool),
        (30, Int32),
        (31, Int32),
        (32, Int32),
        (33, Int32),
        (34, Int32),
        (35, Uint32),
        (36, Int32),
        (37, Float),
        (38, Bool),
        (40, Float),
        (41, Float),
        (42, Float),
        (43, Int32),
        (44, Bool),
        (45, Str),
        (46, Str),
        (255, Bytes),
    ],
    one_of: false,
};

struct Gen {
    rng: SeededRandom,
    /// Unknown fields written, by wire type 0, 1, 2, 3 (group), 5.
    unknown_wire_types: [usize; 6],
}

impl Gen {
    fn new(seed: u64) -> Gen {
        Gen {
            rng: SeededRandom::new(seed),
            unknown_wire_types: [0; 6],
        }
    }

    fn below(&mut self, n: u64) -> u64 {
        self.rng.next_u64() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn u64(&mut self) -> u64 {
        match self.below(4) {
            0 => self.below(128),
            1 => self.below(1 << 20),
            _ => self.rng.next_u64(),
        }
    }

    fn i32(&mut self) -> i32 {
        match self.below(3) {
            0 => self.below(10) as i32 - 5,
            _ => self.rng.next_u64() as i32,
        }
    }

    fn f32(&mut self) -> f32 {
        match self.below(4) {
            0 => [0.212, 1.0, 2.5, -1.25, 36500.0][self.below(5) as usize],
            _ => f32::from_bits(self.rng.next_u64() as u32),
        }
    }

    fn text(&mut self) -> String {
        let alphabet = ['a', 'Z', '0', ' ', '{', '"', 'é', '日', '😀', '\u{1f}', '\0'];
        (0..1 + self.below(12))
            .map(|_| alphabet[self.below(alphabet.len() as u64) as usize])
            .collect()
    }

    fn bytes(&mut self) -> Vec<u8> {
        (0..1 + self.below(12)).map(|_| self.rng.next_u64() as u8).collect()
    }

    /// One known field in canonical form, or nothing for a default.
    fn field(&mut self, out: &mut Vec<(u32, Vec<u8>)>, number: u32, ty: Ty, depth: usize) {
        let mut bytes = Vec::new();
        let absent = !self.chance(70);
        match ty {
            Bool => {
                if absent {
                    return;
                }
                put_key(&mut bytes, number, 0);
                bytes.push(1);
            }
            Int32 => {
                let value = self.i32();
                if absent || value == 0 {
                    return;
                }
                put_key(&mut bytes, number, 0);
                put_varint(&mut bytes, i64::from(value) as u64);
            }
            Uint32 => {
                let value = self.u64() as u32;
                if absent || value == 0 {
                    return;
                }
                put_key(&mut bytes, number, 0);
                put_varint(&mut bytes, u64::from(value));
            }
            Int64 => {
                let value = self.u64();
                if absent || value == 0 {
                    return;
                }
                put_key(&mut bytes, number, 0);
                put_varint(&mut bytes, value);
            }
            Float => {
                let value = self.f32();
                if absent || value == 0.0 {
                    return;
                }
                put_key(&mut bytes, number, 5);
                bytes.extend(value.to_bits().to_le_bytes());
            }
            Str => {
                if absent {
                    return;
                }
                let text = self.text();
                put_len(&mut bytes, number, text.as_bytes());
            }
            Bytes => {
                if absent {
                    return;
                }
                let payload = self.bytes();
                put_len(&mut bytes, number, &payload);
            }
            OptInt64 => {
                if absent {
                    return;
                }
                let value = if self.chance(20) { 0 } else { self.u64() };
                put_key(&mut bytes, number, 0);
                put_varint(&mut bytes, value);
            }
            OptUint32 => {
                if absent {
                    return;
                }
                let value = if self.chance(20) { 0 } else { self.u64() as u32 };
                put_key(&mut bytes, number, 0);
                put_varint(&mut bytes, u64::from(value));
            }
            OptFloat => {
                if absent {
                    return;
                }
                let value = if self.chance(20) { -0.0 } else { self.f32() };
                put_key(&mut bytes, number, 5);
                bytes.extend(value.to_bits().to_le_bytes());
            }
            Floats => {
                if absent {
                    return;
                }
                let payload: Vec<u8> = (0..1 + self.below(8))
                    .flat_map(|_| self.f32().to_bits().to_le_bytes())
                    .collect();
                put_len(&mut bytes, number, &payload);
            }
            Uint32s => {
                if absent {
                    return;
                }
                let mut payload = Vec::new();
                for _ in 0..1 + self.below(5) {
                    put_varint(&mut payload, u64::from(self.u64() as u32));
                }
                put_len(&mut bytes, number, &payload);
            }
            Msg(message) => {
                if absent || depth > 3 {
                    return;
                }
                let payload = self.message(message, depth + 1);
                put_len(&mut bytes, number, &payload);
            }
            Msgs(message) => {
                if depth > 3 {
                    return;
                }
                for _ in 0..self.below(3) {
                    let payload = self.message(message, depth + 1);
                    let mut entry = Vec::new();
                    put_len(&mut entry, number, &payload);
                    out.push((number, entry));
                }
                return;
            }
        }
        out.push((number, bytes));
    }

    /// An undeclared field of any wire type, its key sometimes padded (not
    /// minimal): unknown fields are kept as their exact bytes.
    fn unknown(&mut self, number: u32, depth: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        let wire = [0u8, 1, 2, 3, 5][self.below(5) as usize];
        let wire = if wire == 3 && depth > 4 { 0 } else { wire };
        self.unknown_wire_types[usize::from(wire)] += 1;
        let key = (u64::from(number) << 3) | u64::from(wire);
        if self.chance(10) && key < (1 << 28) {
            // A padded key: one extra continuation byte.
            put_varint(&mut bytes, key | (1 << 35));
            bytes.truncate(bytes.len() - 1);
            *bytes.last_mut().unwrap() |= 0x80;
            bytes.push(0);
        } else {
            put_varint(&mut bytes, key);
        }
        match wire {
            0 => put_varint(&mut bytes, self.u64()),
            1 => bytes.extend(self.rng.next_u64().to_le_bytes()),
            2 => {
                let payload = self.bytes();
                put_varint(&mut bytes, payload.len() as u64);
                bytes.extend(payload);
            }
            3 => {
                for _ in 0..self.below(3) {
                    let inner = 1 + self.below(40) as u32;
                    bytes.extend(self.unknown(inner, depth + 1));
                }
                put_key(&mut bytes, number, 4);
            }
            _ => bytes.extend((self.rng.next_u64() as u32).to_le_bytes()),
        }
        bytes
    }

    /// A message in canonical form with unknown fields placed by number.
    fn message(&mut self, message: &Msg, depth: usize) -> Vec<u8> {
        let mut entries: Vec<(u32, Vec<u8>)> = Vec::new();
        if message.one_of {
            let (number, ty) = message.fields[self.below(message.fields.len() as u64) as usize];
            let Msg(member) = ty else { unreachable!() };
            let payload = self.message(member, depth + 1);
            let mut bytes = Vec::new();
            put_len(&mut bytes, number, &payload);
            entries.push((number, bytes));
        } else {
            for &(number, ty) in message.fields {
                self.field(&mut entries, number, ty, depth);
            }
        }
        for _ in 0..self.below(4) {
            let number = loop {
                let candidate = match self.below(3) {
                    0 => 1 + self.below(64) as u32,
                    1 => 200 + self.below(100) as u32,
                    _ => 1 + self.below(536_870_911) as u32,
                };
                if !message.fields.iter().any(|(n, _)| *n == candidate) {
                    break candidate;
                }
            };
            entries.push((number, self.unknown(number, depth)));
        }
        entries.sort_by_key(|(number, _)| *number);
        entries.into_iter().flat_map(|(_, bytes)| bytes).collect()
    }
}

fn assert_round_trip<M: BlobMessage + std::fmt::Debug>(table: &Msg, seed: u64, cases: usize) -> Gen {
    let mut generator = Gen::new(seed);
    for case in 0..cases {
        let bytes = generator.message(table, 0);
        let decoded = M::decode(&bytes).unwrap_or_else(|e| panic!("case {case}: {e} for {}", hex(&bytes)));
        assert_eq!(hex(&decoded.encode()), hex(&bytes), "case {case}");
        let blob = ProtoBlob::<M>::decode(&bytes).unwrap();
        assert_eq!(&*blob.to_bytes(), bytes.as_slice(), "case {case}: unchanged blob");
    }
    generator
}

#[test]
fn every_blob_with_unknown_fields_round_trips() {
    let mut seen = [0usize; 6];
    let mut add = |g: Gen| {
        for (total, n) in seen.iter_mut().zip(g.unknown_wire_types) {
            *total += n;
        }
    };
    add(assert_round_trip::<NotetypeConfig>(&NOTETYPE_CONFIG, 1, 1500));
    add(assert_round_trip::<FieldConfig>(&FIELD_CONFIG, 2, 1500));
    add(assert_round_trip::<TemplateConfig>(&TEMPLATE_CONFIG, 3, 1500));
    add(assert_round_trip::<DeckCommon>(&DECK_COMMON, 4, 1500));
    add(assert_round_trip::<DeckKindMessage>(&DECK_KIND, 5, 1500));
    add(assert_round_trip::<DeckConfigMessage>(&DECK_CONFIG, 6, 1500));
    for (wire, count) in [(0, seen[0]), (1, seen[1]), (2, seen[2]), (3, seen[3]), (5, seen[5])] {
        assert!(count > 100, "wire type {wire} injected only {count} times");
    }
}

// ---------------------------------------------------------------------------
// Blobs the oracle wrote.

/// Every blob a model golden read back from the oracle, with its column.
fn oracle_blobs() -> Vec<(BlobColumn, Vec<u8>, String)> {
    let mut blobs = Vec::new();
    let mut add = |id: &str, step: usize, columns: &[(usize, BlobColumn)]| {
        let g = golden(id);
        for row in rows(&g, step) {
            for (index, column) in columns {
                let text = row[*index].as_str().unwrap();
                blobs.push((*column, unhex(text), format!("{id} step {step}")));
            }
        }
    };
    use BlobColumn as C;
    add("model/proto/preset-bytes", 1, &[(4, C::DeckConfig)]);
    add(
        "model/proto/deck-normal-bytes",
        1,
        &[(4, C::DeckCommon), (5, C::DeckKind)],
    );
    add(
        "model/proto/deck-filtered-bytes",
        3,
        &[(4, C::DeckCommon), (5, C::DeckKind)],
    );
    add("model/proto/notetype-bytes", 2, &[(3, C::NotetypeConfig)]);
    add("model/proto/notetype-bytes", 3, &[(3, C::FieldConfig)]);
    add("model/proto/notetype-bytes", 4, &[(5, C::TemplateConfig)]);
    add("model/schema11/notetype-values", 1, &[(0, C::NotetypeConfig)]);
    add("model/schema11/notetype-values", 2, &[(2, C::FieldConfig)]);
    add("model/schema11/notetype-values", 3, &[(2, C::TemplateConfig)]);
    add("model/schema11/notetype-other-reserved", 6, &[(0, C::NotetypeConfig)]);
    add("model/schema11/notetype-other-reserved", 7, &[(1, C::FieldConfig)]);
    add("model/schema11/notetype-other-reserved", 8, &[(1, C::TemplateConfig)]);
    add("model/schema11/deck-values", 1, &[(2, C::DeckCommon), (3, C::DeckKind)]);
    add("model/schema11/deck-values", 4, &[(2, C::DeckCommon), (3, C::DeckKind)]);
    add("model/schema11/deck-values", 7, &[(0, C::DeckKind)]);
    add("model/schema11/deck-values", 9, &[(0, C::DeckKind)]);
    add(
        "model/schema11/deck-other-roundtrip",
        3,
        &[(0, C::DeckCommon), (1, C::DeckKind)],
    );
    add(
        "model/schema11/deck-rejections",
        11,
        &[(2, C::DeckCommon), (3, C::DeckKind)],
    );
    add("model/schema11/preset-values", 1, &[(2, C::DeckConfig)]);
    add("model/schema11/preset-values", 4, &[(0, C::DeckConfig)]);
    add("model/schema11/preset-values", 9, &[(0, C::DeckConfig)]);
    add("model/schema11/preset-rejections", 10, &[(2, C::DeckConfig)]);
    blobs
}

fn rewrite<M: BlobMessage>(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let blob = ProtoBlob::<M>::decode(bytes).unwrap();
    (blob.to_bytes().into_owned(), blob.encode())
}

#[test]
fn unmodified_oracle_records_are_written_back_byte_identical() {
    let blobs = oracle_blobs();
    assert!(blobs.len() >= 40, "{}", blobs.len());
    for (column, bytes, what) in blobs {
        let (kept, canonical) = match column {
            BlobColumn::NotetypeConfig => rewrite::<NotetypeConfig>(&bytes),
            BlobColumn::FieldConfig => rewrite::<FieldConfig>(&bytes),
            BlobColumn::TemplateConfig => rewrite::<TemplateConfig>(&bytes),
            BlobColumn::DeckCommon => rewrite::<DeckCommon>(&bytes),
            BlobColumn::DeckKind => rewrite::<DeckKindMessage>(&bytes),
            BlobColumn::DeckConfig => rewrite::<DeckConfigMessage>(&bytes),
        };
        assert_eq!(hex(&kept), hex(&bytes), "{what}: kept bytes");
        // Anki's blobs are canonical, so encoding gives them back too (P3).
        assert_eq!(hex(&canonical), hex(&bytes), "{what}: canonical encoding");
    }
}

#[test]
fn spec_default_preset_decodes_to_its_values() {
    // specs/model/protobuf-records.md P6.6 example, the oracle's Default.
    let g = golden("model/proto/preset-bytes");
    let row = rows(&g, 1).iter().find(|row| row[1] == "Default").unwrap();
    let preset = DeckConfigMessage::decode(&unhex(row[4].as_str().unwrap())).unwrap();
    assert_eq!(preset.learn_steps, [1.0, 10.0]);
    assert_eq!(preset.relearn_steps, [10.0]);
    assert_eq!(preset.easy_days_percentages, [1.0; 7]);
    assert!(preset.fsrs_params_4().is_empty() && preset.fsrs_params_5().is_empty());
    assert!(preset.fsrs_params_6().is_empty());
    assert_eq!((preset.new_per_day, preset.reviews_per_day), (20, 200));
    assert_eq!(preset.initial_ease, 2.5);
    assert_eq!(preset.easy_multiplier, 1.3);
    assert_eq!(preset.hard_multiplier, 1.2);
    assert_eq!(preset.interval_multiplier, 1.0);
    assert_eq!(preset.lapse_multiplier, 0.0);
    assert_eq!(preset.maximum_review_interval, 36_500);
    assert_eq!(preset.minimum_lapse_interval, 1);
    assert_eq!(
        (preset.graduating_interval_good, preset.graduating_interval_easy),
        (1, 4)
    );
    assert_eq!((preset.new_card_insert_order, preset.leech_action), (0, 1));
    assert_eq!((preset.leech_threshold, preset.cap_answer_time_to_secs), (8, 60));
    assert_eq!((preset.desired_retention, preset.historical_retention), (0.9, 0.9));
    assert!(preset.wait_for_audio);
    assert!(preset.other().is_empty() && preset.unknown_fields().is_empty());
}

#[test]
fn scenario_preset_decodes_to_its_setup_values() {
    // model/proto/preset-bytes: the Daily preset's setup and patch.
    let g = golden("model/proto/preset-bytes");
    let row = rows(&g, 1).iter().find(|row| row[1] == "Daily").unwrap();
    let p = DeckConfigMessage::decode(&unhex(row[4].as_str().unwrap())).unwrap();
    assert_eq!(p.learn_steps, [1.0, 10.0, 60.0]);
    assert_eq!(p.relearn_steps, [10.0, 30.0]);
    assert_eq!((p.graduating_interval_good, p.graduating_interval_easy), (2, 5));
    assert_eq!(
        (p.new_per_day, p.reviews_per_day, p.maximum_review_interval),
        (15, 150, 3650)
    );
    assert_eq!((p.minimum_lapse_interval, p.leech_threshold, p.leech_action), (2, 6, 0));
    assert_eq!(
        (
            p.initial_ease,
            p.easy_multiplier,
            p.hard_multiplier,
            p.interval_multiplier,
            p.lapse_multiplier
        ),
        (2.3, 1.25, 1.1, 0.95, 0.1)
    );
    assert!(p.bury_new && p.bury_reviews && p.bury_interday_learning);
    assert_eq!(p.desired_retention, 0.85);
    assert_eq!(p.fsrs_params_4(), [0.4, 0.6, 2.4, 5.8]);
    assert_eq!(p.fsrs_params_5(), [0.40255, 1.18385, 3.173, 15.69105]);
    assert_eq!(p.fsrs_params_6().len(), 21);
    assert_eq!(p.fsrs_params_6()[0], 0.212);
    assert_eq!(p.easy_days_percentages, [1.0, 1.0, 0.5, 1.0, 1.0, 0.0, 1.0]);
    assert_eq!(p.ignore_revlogs_before_date, "2029-06-01");
    assert_eq!(p.param_search, "deck:Words");
    assert_eq!(p.historical_retention, 0.88);
    assert_eq!((p.new_mix, p.interday_learning_mix, p.review_order), (1, 2, 7));
    assert_eq!(
        (p.new_card_sort_order, p.new_card_gather_priority, p.new_per_day_minimum),
        (3, 5, 3)
    );
    assert_eq!((p.question_action, p.answer_action), (1, 3));
    assert_eq!((p.seconds_to_show_question, p.seconds_to_show_answer), (4.5, 2.25));
    assert!(p.stop_timer_on_answer && !p.wait_for_audio);
    assert_eq!(p.cap_answer_time_to_secs, 45);
    assert!(p.show_timer && p.disable_autoplay && p.skip_question_when_replaying_answer);
    // new.order 0 is random insertion.
    assert_eq!(p.new_card_insert_order, 1);
}

#[test]
fn scenario_notetype_and_filtered_deck_decode_to_their_values() {
    let g = golden("model/proto/notetype-bytes");
    let pair = rows(&g, 2).iter().find(|row| row[0] == "Pair").unwrap();
    let config = NotetypeConfig::decode(&unhex(pair[3].as_str().unwrap())).unwrap();
    assert_eq!(
        (config.kind, config.sort_field_idx, config.target_deck_id_unused),
        (0, 1, 1)
    );
    assert_eq!(config.css, ".card { margin: 0; }");
    assert_eq!(
        (config.latex_pre.as_str(), config.latex_post.as_str()),
        ("% pre\n", "% post")
    );
    assert!(config.latex_svg);
    assert_eq!(config.reqs.len(), 1);
    assert_eq!((config.reqs[0].card_ord, config.reqs[0].kind), (0, 1));
    assert_eq!(config.reqs[0].field_ords, [0]);
    assert_eq!(
        (config.original_stock_kind, config.original_id),
        (1, Some(1_234_567_890_123))
    );
    let back = rows(&g, 3)
        .iter()
        .find(|row| row[0] == "Pair" && row[2] == "Back")
        .unwrap();
    let field = FieldConfig::decode(&unhex(back[3].as_str().unwrap())).unwrap();
    assert!(!field.sticky && field.rtl && field.plain_text && field.exclude_from_search && field.prevent_deletion);
    assert_eq!((field.font_name.as_str(), field.font_size), ("Noto Sans", 18));
    assert_eq!((field.id, field.tag), (Some(1002), Some(3)));
    let forward = rows(&g, 4).iter().find(|row| row[0] == "Pair").unwrap();
    let template = TemplateConfig::decode(&unhex(forward[5].as_str().unwrap())).unwrap();
    assert_eq!(template.q_format_browser, "{{Front}} (browser)");
    assert_eq!(template.a_format_browser, "{{Back}} (browser)");
    assert_eq!(
        (template.target_deck_id, template.browser_font_size, template.id),
        (1, 12, Some(2001))
    );

    // The scenario's filtered deck, built from the spec: reschedule, one
    // search "is:due" limit 50 order 6, Again 60 s, Hard 600 s.
    let kind = DeckKindMessage::decode(&unhex("12150801120c0a0669733a6475651032180628d804383c")).unwrap();
    let DeckKind::Filtered(filtered) = &kind.kind else {
        panic!("{kind:?}")
    };
    assert!(filtered.reschedule);
    assert_eq!(filtered.search_terms.len(), 1);
    let term = &filtered.search_terms[0];
    assert_eq!((term.search.as_str(), term.limit, term.order), ("is:due", 50, 6));
    assert_eq!((filtered.preview_again_secs, filtered.preview_hard_secs), (60, 600));
}

// ---------------------------------------------------------------------------
// P3 to P5.

#[test]
fn spec_example_updating_fsrs_params_6_keeps_every_other_byte() {
    // specs/model/protobuf-records.md P4 example; field 7 is unknown.
    let stored = unhex("1a040000803f2a04000000403204000040403a04000080404814");
    let mut preset = ProtoBlob::<DeckConfigMessage>::decode(&stored).unwrap();
    assert_eq!(&*preset.to_bytes(), stored.as_slice());
    preset.set_fsrs_params_6(vec![0.5, 1.5]);
    assert_eq!(
        hex(&preset.to_bytes()),
        "1A040000803F2A040000004032080000003F0000C03F3A04000080404814"
    );
}

#[test]
fn updating_fsrs_params_6_leaves_the_other_lists_and_unknown_fields() {
    // The oracle's Daily preset, which has fsrsWeights, fsrsParams5 and
    // fsrsParams6, plus an unknown packed float list at 7 and a later one at
    // 47, as a newer Anki might write them.
    let g = golden("model/proto/preset-bytes");
    let row = rows(&g, 1).iter().find(|row| row[1] == "Daily").unwrap();
    let anki = unhex(row[4].as_str().unwrap());
    let field_7 = unhex("3a08cdcccc3dcdcc4c3e");
    let field_47 = unhex("fa0204cdcc4c3e");
    // Fields 1 to 6 hold 3, 2, 4, 7, 4 and 21 floats (the scenario's setup
    // and patch), each with a two-byte key and length.
    let six_end = (2 + 12) + (2 + 8) + (2 + 16) + (2 + 28) + (2 + 16) + (2 + 84);
    assert_eq!(&anki[six_end - 86..six_end - 84], [0x32, 84]);
    // 7 goes after 6; 47 after 46, the last field (Daily has no `other`).
    let mut stored = anki[..six_end].to_vec();
    stored.extend(&field_7);
    stored.extend(&anki[six_end..]);
    stored.extend(&field_47);
    let mut preset = ProtoBlob::<DeckConfigMessage>::decode(&stored).unwrap();
    assert_eq!(preset.unknown_fields().len(), 2);
    assert_eq!(preset.encode(), stored, "the stored blob is in canonical layout");

    let params = vec![0.3, 1.1, 2.2];
    preset.set_fsrs_params_6(params.clone());
    let written = preset.to_bytes().into_owned();
    let mut expected = anki[..six_end - 2 - 21 * 4].to_vec();
    expected.extend([0x32, 12]);
    for value in &params {
        expected.extend(f32::to_le_bytes(*value));
    }
    expected.extend(&field_7);
    expected.extend(&anki[six_end..]);
    expected.extend(&field_47);
    assert_eq!(hex(&written), hex(&expected));
    let reread = DeckConfigMessage::decode(&written).unwrap();
    assert_eq!(reread.fsrs_params_4(), [0.4, 0.6, 2.4, 5.8]);
    assert_eq!(reread.fsrs_params_5(), [0.40255, 1.18385, 3.173, 15.69105]);
    assert_eq!(reread.fsrs_params_6(), params);
}

#[test]
fn canonical_encoding_follows_p3() {
    let mut common = DeckCommon::default();
    common.new_studied = -1;
    // int32 below zero: sign-extended to 10 bytes.
    assert_eq!(hex(&common.encode()), "20FFFFFFFFFFFFFFFFFF01");
    common.new_studied = 0;
    assert_eq!(common.encode(), b"");

    let mut preset = DeckConfigMessage::default();
    preset.initial_ease = -0.0;
    preset.desired_retention = f32::from_bits(0x7fc0_1234);
    // -0.0 equals 0.0 and is left out; NaN is written bit for bit.
    assert_eq!(hex(&preset.encode()), "AD023412C07F");
    preset.desired_retention = 0.0;
    assert_eq!(preset.encode(), b"");

    // Explicit presence: written at the default; a present message with all
    // defaults is a key and a zero length.
    let mut kind = DeckKindMessage::new(DeckKind::Normal(Default::default()));
    assert_eq!(hex(&kind.encode()), "0A00");
    if let DeckKind::Normal(normal) = &mut kind.kind {
        normal.new_limit = Some(0);
        normal.new_limit_today = Some(meguri_model::proto::deck::DayLimit::new(0, 0));
        normal.desired_retention = Some(-0.0);
    }
    assert_eq!(hex(&kind.encode()), "0A0938004A005500000080");
}

#[test]
fn unchanged_content_keeps_non_canonical_bytes() {
    // Fields out of order, a padded varint, an unpacked float list and a
    // repeated singular field: decoded content equal to what is held.
    let stored = unhex("50c8014894800012040000204148140d0000803f");
    let mut preset = ProtoBlob::<DeckConfigMessage>::decode(&stored).unwrap();
    assert_eq!(preset.new_per_day, 20, "the last occurrence wins");
    assert_eq!(preset.reviews_per_day, 200);
    assert_eq!(preset.relearn_steps, [10.0]);
    assert_eq!(preset.learn_steps, [1.0], "unpacked element");
    assert_eq!(&*preset.to_bytes(), stored.as_slice());
    preset.reviews_per_day = 201;
    preset.reviews_per_day = 200;
    assert_eq!(&*preset.to_bytes(), stored.as_slice(), "changed back");
    preset.reviews_per_day = 100;
    assert_eq!(
        hex(&preset.to_bytes()),
        "0A040000803F120400002041481450 64".replace(' ', "")
    );
}

#[test]
fn messages_and_one_of_members_merge_as_p2_says() {
    // Normal deck, then a second normal occurrence: merged. Then filtered:
    // replaces. Then filtered again: merged.
    let kind = DeckKindMessage::decode(&unhex("0a0208070a021003")).unwrap();
    let DeckKind::Normal(normal) = &kind.kind else { panic!() };
    assert_eq!((normal.config_id, normal.extend_new), (7, 3));
    let kind = DeckKindMessage::decode(&unhex("0a020807120208011202280a")).unwrap();
    let DeckKind::Filtered(filtered) = &kind.kind else {
        panic!()
    };
    assert!(filtered.reschedule);
    assert_eq!(filtered.preview_hard_secs, 10);
    // A singular day limit twice: the later scalars win, the rest stays.
    let kind = DeckKindMessage::decode(&unhex("0a0a4204080510094202100b")).unwrap();
    let DeckKind::Normal(normal) = &kind.kind else { panic!() };
    let limit = normal.review_limit_today.as_ref().unwrap();
    assert_eq!((limit.limit, limit.today), (5, 11));
    // Packed and unpacked elements mix and append.
    let preset = DeckConfigMessage::decode(&unhex("0a080000803f000000400d00004040")).unwrap();
    assert_eq!(preset.learn_steps, [1.0, 2.0, 3.0]);
}

#[test]
fn unknown_fields_are_kept_at_every_level() {
    // Deck kind: unknown 3 (varint) at the top; in the normal deck an unknown
    // 11 (varint) and 12 (group holding a field 1); in its day limit an
    // unknown 5 (fixed32).
    let stored = unhex(concat!("0a11080142070809", "2d01000000", "5801", "63080164", "1801"));
    let mut kind = ProtoBlob::<DeckKindMessage>::decode(&stored).unwrap();
    assert_eq!(kind.encode(), stored);
    assert_eq!(
        kind.unknown_fields().iter().map(|f| f.number()).collect::<Vec<_>>(),
        [3]
    );
    let DeckKind::Normal(normal) = &mut kind.kind else {
        panic!()
    };
    let numbers: Vec<u32> = normal.unknown_fields().iter().map(|f| f.number()).collect();
    assert_eq!(numbers, [11, 12]);
    assert_eq!(
        normal.unknown_fields().iter().nth(1).unwrap().bytes(),
        unhex("63080164")
    );
    let limit = normal.review_limit_today.as_mut().unwrap();
    assert_eq!(
        limit.unknown_fields().iter().next().unwrap().bytes(),
        unhex("2d01000000")
    );
    limit.limit = 4;
    assert_eq!(
        hex(&kind.to_bytes()),
        concat!("0A11080142070804", "2D01000000", "5801", "63080164", "1801")
    );
}

// ---------------------------------------------------------------------------
// P9.

fn reason<M: BlobMessage + std::fmt::Debug>(hex_text: &str) -> BlobError {
    match M::decode(&unhex(hex_text)) {
        Err(ModelError::Blob { column, reason }) => {
            assert_eq!(column, M::COLUMN);
            reason
        }
        other => panic!("{hex_text}: {other:?}"),
    }
}

#[test]
fn spec_decode_error_examples() {
    use BlobError::*;
    // specs/model/protobuf-records.md P9 table; whole deck-config blobs
    // unless named otherwise.
    for (blob, expected) in [
        ("0a05", Truncated),
        ("ff", Truncated),
        ("0a020000", Truncated),
        ("3b", Truncated),
        ("38ffffffffffffffffff02", BadVarint),
        ("808080801000", BadVarint),
        ("0000", BadFieldNumber),
        ("0800", BadWireType),
        ("0f", BadWireType),
        ("0c", BadWireType),
        ("3b44", BadGroup),
    ] {
        assert_eq!(reason::<DeckConfigMessage>(blob), expected, "{blob}");
    }
    let groups_101 = format!("{}{}", "3b".repeat(101), "3c".repeat(101));
    assert_eq!(reason::<DeckConfigMessage>(&groups_101), TooDeep);
    assert_eq!(reason::<DeckKindMessage>("0a015b"), Truncated);
    assert_eq!(reason::<DeckKindMessage>("0a015b5c"), Truncated);
    let preset = DeckConfigMessage::decode(&unhex("38ffffffffffffffffff01")).unwrap();
    assert_eq!(preset.unknown_fields().iter().next().unwrap().number(), 7);
    let preset = DeckConfigMessage::decode(&unhex("fa0f02ffff")).unwrap();
    assert_eq!(preset.other(), [0xff, 0xff]);
}

/// The blobs of model/proto/decode-limits by list and record name: a
/// preset's config, or a deck's common and kind.
fn decode_limit_blobs() -> Vec<(&'static str, String, Vec<Vec<u8>>)> {
    let scenario = common::scenario("model/proto/decode-limits");
    let quoted = |sql: &str, after: &str| -> String {
        let rest = &sql[sql.find(after).unwrap() + after.len()..];
        let rest = &rest[rest.find('\'').unwrap() + 1..];
        rest[..rest.find('\'').unwrap()].to_owned()
    };
    let literals = |sql: &str| -> Vec<Vec<u8>> {
        sql.split("X'")
            .skip(1)
            .map(|rest| unhex(&rest[..rest.find('\'').unwrap()]))
            .collect()
    };
    let mut blobs = Vec::new();
    for step in scenario["steps"].as_array().unwrap() {
        let Some(sql) = step["db_exec"]["sql"].as_str() else {
            continue;
        };
        if sql.starts_with("UPDATE deck_config") {
            blobs.push(("presets", quoted(sql, "WHERE name ="), literals(sql)));
        } else if sql.starts_with("INSERT INTO decks") {
            blobs.push(("decks", quoted(sql, "VALUES"), literals(sql)));
        } else {
            panic!("unexpected step {sql}");
        }
    }
    blobs
}

/// specs/conformance/model-ops.md L4: `ok` when every blob of the record
/// decodes, `invalid_blob` for any P9 error but `no deck kind`.
fn decode_status<M: BlobMessage>(bytes: &[u8]) -> &'static str {
    match M::decode(bytes) {
        Ok(_) => "ok",
        Err(ModelError::Blob { reason, .. }) => {
            assert_ne!(reason, BlobError::NoDeckKind, "a run error, which scenarios avoid");
            "invalid_blob"
        }
        Err(other) => panic!("{other}"),
    }
}

#[test]
fn decode_limits_match_the_oracle() {
    let g = golden("model/proto/decode-limits");
    let observed = &g["observations"]["steps"].as_array().unwrap().last().unwrap()["blob_decode"];
    let blobs = decode_limit_blobs();
    let mut checked = 0;
    for (list, name, bytes) in &blobs {
        let status = match *list {
            "presets" => {
                assert_eq!(bytes.len(), 1, "{name}");
                decode_status::<DeckConfigMessage>(&bytes[0])
            }
            _ => {
                let [common, kind] = bytes.as_slice() else {
                    panic!("{name}: common and kind")
                };
                match (
                    decode_status::<DeckCommon>(common),
                    decode_status::<DeckKindMessage>(kind),
                ) {
                    ("ok", "ok") => "ok",
                    _ => "invalid_blob",
                }
            }
        };
        assert_eq!(observed[*list][name.as_str()], status, "{list} {name}");
        checked += 1;
    }
    let golden_count: usize = ["presets", "decks"]
        .iter()
        .map(|list| observed[*list].as_object().unwrap().len())
        .sum();
    assert_eq!(checked, golden_count, "every observed record has its blob");
    assert_eq!(checked, 32);
}

#[test]
fn when_several_reasons_apply_the_first_problem_met_decides() {
    use BlobError::*;
    let open = |n: usize| "3b".repeat(n);
    let groups = |n: usize, inner: &str| format!("{}{inner}{}", "3b".repeat(n), "3c".repeat(n));
    // P2 depth: 100 nested groups decode; the 101st start is at depth 100.
    assert!(DeckConfigMessage::decode(&unhex(&groups(100, ""))).is_ok());
    assert!(DeckConfigMessage::decode(&unhex(&groups(99, "0800"))).is_ok());
    assert_eq!(reason::<DeckConfigMessage>(&groups(100, "0800")), TooDeep);
    // The too-deep key comes before the end of the blob and before its own
    // short payload.
    assert_eq!(reason::<DeckConfigMessage>(&open(101)), TooDeep);
    assert_eq!(reason::<DeckConfigMessage>(&format!("{}0a05", open(100))), TooDeep);
    // A group-end key is not a field: at depth 100 it closes its group or
    // is a bad group.
    assert!(DeckConfigMessage::decode(&unhex(&groups(100, ""))).is_ok());
    assert_eq!(reason::<DeckConfigMessage>(&format!("{}44", open(100))), BadGroup);
    // The key's own checks come first: varint, field number, wire type.
    assert_eq!(
        reason::<DeckConfigMessage>(&format!("{}ffffffffffffffffff02", open(100))),
        BadVarint
    );
    assert_eq!(
        reason::<DeckConfigMessage>(&format!("{}0000", open(100))),
        BadFieldNumber
    );
    assert_eq!(reason::<DeckConfigMessage>(&format!("{}0f", open(100))), BadWireType);
    assert_eq!(reason::<DeckConfigMessage>("07"), BadFieldNumber);
    // A declared number's group end is a bad wire type; inside a group no
    // field is declared, so the same keys are an unknown varint and a bad
    // group.
    assert_eq!(reason::<DeckConfigMessage>("0c"), BadWireType);
    assert!(DeckConfigMessage::decode(&unhex("3b08003c")).is_ok());
    assert_eq!(reason::<DeckConfigMessage>("3b0c"), BadGroup);
    // In reading order: a bad wire type before a truncation, and the reverse.
    assert_eq!(reason::<DeckConfigMessage>("08000a05"), BadWireType);
    assert_eq!(reason::<DeckConfigMessage>("0a050800"), Truncated);
    // Nested messages count: the normal deck's fields are at depth 1 and its
    // day limit's at depth 2, so 98 groups fit in a day limit and 99 do not.
    let limit = |n: usize| {
        let inner = groups(n, "").replace("3b", "5b").replace("3c", "5c");
        let mut limit = Vec::new();
        put_len(&mut limit, 8, &unhex(&inner));
        let mut kind = Vec::new();
        put_len(&mut kind, 1, &limit);
        hex(&kind)
    };
    assert!(DeckKindMessage::decode(&unhex(&limit(98))).is_ok());
    assert_eq!(reason::<DeckKindMessage>(&limit(99)), TooDeep);
}

#[test]
fn the_tenth_varint_byte_limit_holds_for_every_varint() {
    use BlobError::*;
    // P2: a tenth byte of 00 or 01 only, for keys, lengths and values.
    let ten = |last: &str| format!("ffffffffffffffffff{last}");
    // A declared uint32 and an unknown varint value.
    assert_eq!(reason::<DeckCommon>(&format!("18{}", ten("02"))), BadVarint);
    assert!(DeckCommon::decode(&unhex(&format!("18{}", ten("01")))).is_ok());
    assert_eq!(reason::<DeckCommon>(&format!("40{}", ten("7f"))), BadVarint);
    // A length: refused as a varint before the payload is looked at.
    assert_eq!(reason::<FieldConfig>(&format!("1a{}", ten("02"))), BadVarint);
    assert_eq!(reason::<FieldConfig>(&format!("1a{}", ten("01"))), Truncated);
    // A packed varint inside a card requirement.
    assert_eq!(reason::<NotetypeConfig>(&format!("420c1a0a{}", ten("02"))), BadVarint);
    // An unknown varint inside a group, and a key of 2^63 or more.
    assert_eq!(reason::<DeckCommon>(&format!("5b08{}5c", ten("02"))), BadVarint);
    assert_eq!(reason::<DeckCommon>(&format!("{}00", ten("01"))), BadVarint);
}

#[test]
fn unknown_fields_are_written_in_field_number_order() {
    // specs/model/protobuf-records.md P4 example: unknown 8 then 7, before
    // new_per_day.
    let stored = unhex("42003a004814");
    let mut preset = ProtoBlob::<DeckConfigMessage>::decode(&stored).unwrap();
    assert_eq!(
        &*preset.to_bytes(),
        stored.as_slice(),
        "unchanged content keeps the read bytes"
    );
    preset.new_per_day = 25;
    assert_eq!(hex(&preset.to_bytes()), "3A0042004819");

    // Unknown fields read in any order equal the same fields in number order
    // (P4 equality) and encode in number order; those sharing a number keep
    // their read order.
    let mut generator = Gen::new(11);
    for case in 0..2000 {
        let mut fields: Vec<(u32, Vec<u8>)> = (0..1 + generator.below(6))
            .map(|_| {
                // Numbers deck config does not declare.
                let number = [7, 8, 39, 47, 300][generator.below(5) as usize];
                (number, generator.unknown(number, 0))
            })
            .collect();
        let read: Vec<u8> = fields.iter().flat_map(|(_, bytes)| bytes.clone()).collect();
        fields.sort_by_key(|(number, _)| *number);
        let sorted: Vec<u8> = fields.iter().flat_map(|(_, bytes)| bytes.clone()).collect();
        let as_read = DeckConfigMessage::decode(&read).unwrap();
        let in_order = DeckConfigMessage::decode(&sorted).unwrap();
        assert!(as_read.same_content(&in_order), "case {case}: {}", hex(&read));
        assert_eq!(hex(&as_read.encode()), hex(&sorted), "case {case}");
    }
    // Fields that share a number in another order are another record.
    let first = DeckConfigMessage::decode(&unhex("3a003a0101")).unwrap();
    let second = DeckConfigMessage::decode(&unhex("3a01013a00")).unwrap();
    assert!(!first.same_content(&second));
    assert!(!first.equivalent(&second));
}

#[test]
fn decode_errors_name_their_reason() {
    use BlobError::*;
    // A varint longer than 10 bytes; a key above 32 bits; field number 0.
    assert_eq!(reason::<DeckCommon>("38ffffffffffffffffffff01"), BadVarint);
    assert_eq!(reason::<DeckCommon>("8080808010"), BadVarint);
    assert_eq!(reason::<DeckCommon>("0000"), BadFieldNumber);
    // Wire types 6 and 7; a varint for a string; a fixed32 for a bool.
    assert_eq!(reason::<DeckCommon>("0e"), BadWireType);
    assert_eq!(reason::<TemplateConfig>("0805"), BadWireType);
    assert_eq!(reason::<DeckCommon>("0d00000000"), BadWireType);
    // Groups: an end with no start and an end of another number are bad
    // groups; a group left open at the end of a nested message's payload, or
    // of the blob, is truncated.
    assert_eq!(reason::<DeckCommon>("44"), BadGroup);
    assert_eq!(reason::<DeckCommon>("434c"), BadGroup);
    assert_eq!(reason::<DeckKindMessage>("0a015b"), Truncated);
    assert_eq!(reason::<DeckCommon>("5b0801"), Truncated);
    // Truncated fixed values and payloads.
    assert_eq!(reason::<DeckCommon>("490000"), Truncated);
    assert_eq!(reason::<DeckCommon>("4d00"), Truncated);
    assert_eq!(reason::<FieldConfig>("1a0541"), Truncated);
    // Not UTF-8 in a string; a deck kind with no member.
    assert_eq!(reason::<FieldConfig>("1a02c328"), BadUtf8);
    assert_eq!(reason::<NotetypeConfig>("42040a02ffff"), BadWireType);
    assert_eq!(reason::<DeckKindMessage>(""), NoDeckKind);
    assert_eq!(reason::<DeckKindMessage>("1803"), NoDeckKind);
    // Nested messages report the same reasons.
    assert_eq!(reason::<NotetypeConfig>("42031a0180"), Truncated);
    assert_eq!(reason::<DeckKindMessage>("12041a020000"), Truncated);
}

// ---------------------------------------------------------------------------
// Robustness: what the fuzz target checks, on deterministic inputs.

fn check_any<M: BlobMessage + std::fmt::Debug>(bytes: &[u8]) {
    if let Ok(message) = M::decode(bytes) {
        let encoded = message.encode();
        let again = M::decode(&encoded).unwrap_or_else(|e| panic!("{}: re-decode {e}", hex(bytes)));
        assert!(again.equivalent(&message), "{}", hex(bytes));
        assert_eq!(again.encode(), encoded, "{}", hex(bytes));
    }
}

fn check_all(bytes: &[u8]) {
    check_any::<NotetypeConfig>(bytes);
    check_any::<FieldConfig>(bytes);
    check_any::<TemplateConfig>(bytes);
    check_any::<DeckCommon>(bytes);
    check_any::<DeckKindMessage>(bytes);
    check_any::<DeckConfigMessage>(bytes);
}

#[test]
fn arbitrary_bytes_give_a_record_or_an_error() {
    let rng = SeededRandom::new(2026);
    for _ in 0..20_000 {
        let len = (rng.next_u64() % 40) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        check_all(&bytes);
    }
    let seeds: Vec<Vec<u8>> = oracle_blobs().into_iter().map(|(_, b, _)| b).collect();
    for seed in &seeds {
        for cut in 0..seed.len() {
            check_all(&seed[..cut]);
        }
        for _ in 0..200 {
            let mut bytes = seed.clone();
            for _ in 0..1 + rng.next_u64() % 3 {
                let at = (rng.next_u64() as usize) % bytes.len().max(1);
                match rng.next_u64() % 3 {
                    0 if !bytes.is_empty() => bytes[at] ^= 1 << (rng.next_u64() % 8),
                    1 => bytes.insert(at.min(bytes.len()), rng.next_u64() as u8),
                    _ if !bytes.is_empty() => {
                        bytes.remove(at);
                    }
                    _ => {}
                }
            }
            check_all(&bytes);
        }
    }
}
