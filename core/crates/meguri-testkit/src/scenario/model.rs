//! The typed scenario model (specs/conformance/dsl.md D4-D7). It is read from
//! the parsed JSON value, which stays the source of the scenario hash and of
//! what the oracle receives.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::FormatError;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub clock: ClockSpec,
    #[serde(default)]
    pub compare: CompareSpec,
    #[serde(default)]
    pub setup: Setup,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockSpec {
    pub tz: String,
    pub start: DateTimeText,
    #[serde(default)]
    pub mode: ClockMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockMode {
    #[default]
    Frozen,
    Step,
}

impl ClockMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ClockMode::Frozen => "frozen",
            ClockMode::Step => "step",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareSpec {
    /// Pointer patterns of arrays compared as multisets.
    #[serde(default)]
    pub set: Vec<String>,
}

/// Setup sections; `None` when the scenario does not use a section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub config: Option<Map<String, Value>>,
    pub presets: Option<Vec<Preset>>,
    pub decks: Option<Vec<DeckSpec>>,
    pub notetypes: Option<Vec<NotetypeSpec>>,
    pub notes: Option<Vec<NoteSpec>>,
    pub cards: Option<Vec<CardOverride>>,
    pub revlog: Option<Vec<RevlogRow>>,
}

impl Setup {
    /// Section names in the order setup runs them (D6).
    pub const ORDER: [&'static str; 7] = ["config", "presets", "decks", "notetypes", "notes", "cards", "revlog"];

    /// The sections present, in run order.
    pub fn sections(&self) -> Vec<&'static str> {
        let present = [
            self.config.is_some(),
            self.presets.is_some(),
            self.decks.is_some(),
            self.notetypes.is_some(),
            self.notes.is_some(),
            self.cards.is_some(),
            self.revlog.is_some(),
        ];
        Setup::ORDER
            .iter()
            .zip(present)
            .filter(|(_, p)| *p)
            .map(|(n, _)| *n)
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    pub learn_steps: Option<Vec<Duration>>,
    pub relearn_steps: Option<Vec<Duration>>,
    pub graduating_interval_days: Option<i64>,
    pub easy_interval_days: Option<i64>,
    pub new_per_day: Option<i64>,
    pub reviews_per_day: Option<i64>,
    pub max_interval_days: Option<i64>,
    pub min_lapse_interval_days: Option<i64>,
    pub starting_ease: Option<f64>,
    pub easy_bonus: Option<f64>,
    pub hard_interval: Option<f64>,
    pub interval_modifier: Option<f64>,
    pub lapse_new_interval: Option<f64>,
    pub leech_threshold: Option<i64>,
    pub leech_action: Option<LeechAction>,
    pub bury_new: Option<bool>,
    pub bury_reviews: Option<bool>,
    pub bury_interday_learning: Option<bool>,
    pub desired_retention: Option<f64>,
    pub fsrs_params: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeechAction {
    Suspend,
    TagOnly,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DeckSpec {
    Name(String),
    Full(DeckWithPreset),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckWithPreset {
    pub name: String,
    pub preset: Option<String>,
}

impl DeckSpec {
    pub fn name(&self) -> &str {
        match self {
            DeckSpec::Name(name) => name,
            DeckSpec::Full(deck) => &deck.name,
        }
    }

    pub fn preset(&self) -> Option<&str> {
        match self {
            DeckSpec::Name(_) => None,
            DeckSpec::Full(deck) => deck.preset.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotetypeKind {
    #[default]
    Standard,
    Cloze,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotetypeSpec {
    pub name: String,
    #[serde(default)]
    pub kind: NotetypeKind,
    pub fields: Vec<String>,
    pub templates: Vec<TemplateSpec>,
    #[serde(default)]
    pub css: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateSpec {
    pub name: String,
    #[serde(default)]
    pub front: String,
    #[serde(default)]
    pub back: String,
}

/// Field values in field order, or by field name.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Fields {
    List(Vec<String>),
    Map(BTreeMap<String, String>),
}

impl Default for Fields {
    fn default() -> Fields {
        Fields::List(Vec::new())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteSpec {
    pub notetype: String,
    pub deck: Option<String>,
    #[serde(default)]
    pub fields: Fields,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Draft (D6.6): stored card columns written after the notes are added.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardOverride {
    pub card: CardRef,
    #[serde(rename = "type")]
    pub kind: Option<i64>,
    pub queue: Option<i64>,
    pub due: Option<i64>,
    pub ivl: Option<i64>,
    pub factor: Option<i64>,
    pub reps: Option<i64>,
    pub lapses: Option<i64>,
    pub left: Option<i64>,
    pub odue: Option<i64>,
    pub flags: Option<i64>,
    pub data: Option<Map<String, Value>>,
}

/// Draft (D6.7): one review-history row.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevlogRow {
    pub card: CardRef,
    pub id: i64,
    pub ease: i64,
    pub ivl: i64,
    #[serde(rename = "lastIvl")]
    pub last_ivl: i64,
    pub factor: i64,
    pub time: i64,
    #[serde(rename = "type")]
    pub kind: i64,
}

/// One step: a mapping with one key, the op name (D7).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Advance(Duration),
    SetClock(DateTimeText),
    AddNote(NoteSpec),
    Answer(AnswerSpec),
    Observe(Vec<ObservationRequest>),
    EditNote(DraftArgs),
    DeleteNotes(DraftArgs),
    Bury(DraftArgs),
    Suspend(DraftArgs),
    Forget(DraftArgs),
    SetDue(DraftArgs),
    Reposition(DraftArgs),
    UpdatePreset(DraftArgs),
    BuildFiltered(DraftArgs),
    Import(DraftArgs),
    Export(DraftArgs),
    CheckDb(DraftArgs),
    DbExec(DraftArgs),
    Sync(DraftArgs),
    /// Live scenarios only (D9): the collection moves to this side.
    Handoff(Side),
}

impl Step {
    /// Every op of the language (D7.2); `observe` included.
    pub const NAMES: [&'static str; 20] = [
        "advance",
        "set_clock",
        "add_note",
        "answer",
        "observe",
        "edit_note",
        "delete_notes",
        "bury",
        "suspend",
        "forget",
        "set_due",
        "reposition",
        "update_preset",
        "build_filtered",
        "import",
        "export",
        "check_db",
        "db_exec",
        "sync",
        "handoff",
    ];

    /// The op name as written in scenarios.
    pub fn name(&self) -> &'static str {
        match self {
            Step::Advance(_) => "advance",
            Step::SetClock(_) => "set_clock",
            Step::AddNote(_) => "add_note",
            Step::Answer(_) => "answer",
            Step::Observe(_) => "observe",
            Step::EditNote(_) => "edit_note",
            Step::DeleteNotes(_) => "delete_notes",
            Step::Bury(_) => "bury",
            Step::Suspend(_) => "suspend",
            Step::Forget(_) => "forget",
            Step::SetDue(_) => "set_due",
            Step::Reposition(_) => "reposition",
            Step::UpdatePreset(_) => "update_preset",
            Step::BuildFiltered(_) => "build_filtered",
            Step::Import(_) => "import",
            Step::Export(_) => "export",
            Step::CheckDb(_) => "check_db",
            Step::DbExec(_) => "db_exec",
            Step::Sync(_) => "sync",
            Step::Handoff(_) => "handoff",
        }
    }

    /// The arguments of a draft op.
    pub fn draft_args(&self) -> Option<&DraftArgs> {
        match self {
            Step::EditNote(a)
            | Step::DeleteNotes(a)
            | Step::Bury(a)
            | Step::Suspend(a)
            | Step::Forget(a)
            | Step::SetDue(a)
            | Step::Reposition(a)
            | Step::UpdatePreset(a)
            | Step::BuildFiltered(a)
            | Step::Import(a)
            | Step::Export(a)
            | Step::CheckDb(a)
            | Step::DbExec(a)
            | Step::Sync(a) => Some(a),
            _ => None,
        }
    }
}

/// Who runs a part of a live scenario (D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Meguri,
    Oracle,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Meguri => "meguri",
            Side::Oracle => "oracle",
        }
    }

    pub fn other(self) -> Side {
        match self {
            Side::Meguri => Side::Oracle,
            Side::Oracle => Side::Meguri,
        }
    }
}

/// One part of a live scenario: the steps one side runs, from setup (the first
/// segment) or from the previous handoff, up to the next handoff step (D9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub side: Side,
    /// Step indices this side runs.
    pub steps: std::ops::Range<usize>,
    /// The handoff step that ends the segment; `None` for the last one.
    pub handoff: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerSpec {
    pub card: CardRef,
    pub rating: Rating,
    #[serde(default)]
    pub time_ms: u64,
}

/// Arguments of a draft op: an object whose shape the owning area fixes
/// (D7.2). `Scenario::from_value` checks the keys the schema requires.
#[derive(Debug, Clone)]
pub struct DraftArgs(pub Map<String, Value>);

impl<'de> Deserialize<'de> for DraftArgs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<DraftArgs, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Object(map) => Ok(DraftArgs(map)),
            other => Err(de::Error::custom(format!(
                "expected a mapping of arguments, found {other}"
            ))),
        }
    }
}

/// Elapsed time in whole seconds: `20m`, `1d`, `23h40m` (D7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duration(i64);

impl Duration {
    pub fn seconds(self) -> i64 {
        self.0
    }
}

impl FromStr for Duration {
    type Err = FormatError;

    fn from_str(text: &str) -> Result<Duration, FormatError> {
        let bad = || FormatError::new(format!("duration {text:?} must look like 20m, 1d or 23h40m"));
        let mut total: i64 = 0;
        let mut digits = String::new();
        for c in text.chars() {
            if c.is_ascii_digit() {
                digits.push(c);
                continue;
            }
            let unit = match c {
                'd' => 86_400,
                'h' => 3_600,
                'm' => 60,
                's' => 1,
                _ => return Err(bad()),
            };
            if digits.is_empty() {
                return Err(bad());
            }
            let n: i64 = digits.parse().map_err(|_| bad())?;
            total = n.checked_mul(unit).and_then(|v| total.checked_add(v)).ok_or_else(bad)?;
            digits.clear();
        }
        if text.is_empty() || !digits.is_empty() {
            return Err(bad());
        }
        Ok(Duration(total))
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

/// An ISO 8601 date-time in whole seconds, optionally with `Z` or `±HH:MM`
/// (D5). Only its shape is checked here; the scenario clock resolves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateTimeText(String);

impl DateTimeText {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for DateTimeText {
    type Err = FormatError;

    fn from_str(text: &str) -> Result<DateTimeText, FormatError> {
        let b = text.as_bytes();
        let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
        let shape = b.len() >= 19
            && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
                .iter()
                .all(|&i| digit(i))
            && b[4] == b'-'
            && b[7] == b'-'
            && b[10] == b'T'
            && b[13] == b':'
            && b[16] == b':';
        let zone = &b[b.len().min(19)..];
        let zone_ok = zone.is_empty()
            || zone == b"Z"
            || (zone.len() == 6
                && (zone[0] == b'+' || zone[0] == b'-')
                && zone[1].is_ascii_digit()
                && zone[2].is_ascii_digit()
                && zone[3] == b':'
                && zone[4].is_ascii_digit()
                && zone[5].is_ascii_digit());
        if shape && zone_ok {
            Ok(DateTimeText(text.to_owned()))
        } else {
            Err(FormatError::new(format!(
                "date-time {text:?} must look like 2030-01-15T10:00:00, optionally with Z or +05:45"
            )))
        }
    }
}

impl<'de> Deserialize<'de> for DateTimeText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<DateTimeText, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

/// Notes or cards, numbered from 1 in creation order (D7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefKind {
    Note,
    Card,
}

/// A logical id: `#nN` or `#cN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalRef {
    pub kind: RefKind,
    pub n: u32,
}

impl LogicalRef {
    /// Position in the creation-order list (0-based).
    pub fn index(self) -> usize {
        usize::try_from(self.n - 1).unwrap_or(usize::MAX)
    }
}

impl fmt::Display for LogicalRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let letter = match self.kind {
            RefKind::Note => 'n',
            RefKind::Card => 'c',
        };
        write!(f, "#{letter}{}", self.n)
    }
}

impl FromStr for LogicalRef {
    type Err = FormatError;

    fn from_str(text: &str) -> Result<LogicalRef, FormatError> {
        let bad = || FormatError::new(format!("{text:?} is not a #nN or #cN reference"));
        let rest = text.strip_prefix('#').ok_or_else(bad)?;
        let (kind, digits) = match rest.split_at_checked(1) {
            Some(("n", d)) => (RefKind::Note, d),
            Some(("c", d)) => (RefKind::Card, d),
            _ => return Err(bad()),
        };
        if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let n = digits.parse().map_err(|_| bad())?;
        Ok(LogicalRef { kind, n })
    }
}

impl<'de> Deserialize<'de> for LogicalRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<LogicalRef, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

/// A `#cN` reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CardRef(pub LogicalRef);

impl<'de> Deserialize<'de> for CardRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<CardRef, D::Error> {
        let r = LogicalRef::deserialize(deserializer)?;
        if r.kind != RefKind::Card {
            return Err(de::Error::custom(format!("{r} is not a card reference")));
        }
        Ok(CardRef(r))
    }
}

/// Observation kinds (observations.md O4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservationKind {
    Ids,
    Timing,
    Card,
    Revlog,
    NextStates,
    FuzzTable,
    NetProbe,
    Queue,
    DeckTree,
    Render,
    Search,
    DbDump,
    Stats,
    Digest,
}

/// How a kind takes arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ArgShape {
    /// No arguments.
    None,
    /// Optional note or card references.
    AnyRefs,
    /// Optional card references.
    CardRefs,
    /// A required mapping.
    Mapping,
    /// No arguments or a mapping.
    OptionalMapping,
}

impl ObservationKind {
    pub const ALL: [ObservationKind; 14] = [
        ObservationKind::Ids,
        ObservationKind::Timing,
        ObservationKind::Card,
        ObservationKind::Revlog,
        ObservationKind::NextStates,
        ObservationKind::FuzzTable,
        ObservationKind::NetProbe,
        ObservationKind::Queue,
        ObservationKind::DeckTree,
        ObservationKind::Render,
        ObservationKind::Search,
        ObservationKind::DbDump,
        ObservationKind::Stats,
        ObservationKind::Digest,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ObservationKind::Ids => "ids",
            ObservationKind::Timing => "timing",
            ObservationKind::Card => "card",
            ObservationKind::Revlog => "revlog",
            ObservationKind::NextStates => "next_states",
            ObservationKind::FuzzTable => "fuzz_table",
            ObservationKind::NetProbe => "net_probe",
            ObservationKind::Queue => "queue",
            ObservationKind::DeckTree => "deck_tree",
            ObservationKind::Render => "render",
            ObservationKind::Search => "search",
            ObservationKind::DbDump => "db_dump",
            ObservationKind::Stats => "stats",
            ObservationKind::Digest => "digest",
        }
    }

    pub fn from_name(name: &str) -> Option<ObservationKind> {
        ObservationKind::ALL.into_iter().find(|k| k.name() == name)
    }

    pub(super) fn shape(self) -> ArgShape {
        match self {
            ObservationKind::Ids => ArgShape::AnyRefs,
            ObservationKind::Timing | ObservationKind::DeckTree | ObservationKind::Digest => ArgShape::None,
            ObservationKind::Card | ObservationKind::Revlog | ObservationKind::NextStates | ObservationKind::Render => {
                ArgShape::CardRefs
            }
            ObservationKind::FuzzTable | ObservationKind::NetProbe | ObservationKind::Search => ArgShape::Mapping,
            ObservationKind::Queue | ObservationKind::DbDump | ObservationKind::Stats => ArgShape::OptionalMapping,
        }
    }
}

/// The arguments of one observation request.
#[derive(Debug, Clone, PartialEq)]
pub enum ObservationArgs {
    /// None given: for per-card kinds, every card.
    None,
    Refs(Vec<LogicalRef>),
    Mapping(Map<String, Value>),
}

/// One entry of an `observe` list (observations.md O2).
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationRequest {
    pub kind: ObservationKind,
    pub args: ObservationArgs,
}

impl ObservationRequest {
    fn parse_text(text: &str) -> Result<ObservationRequest, FormatError> {
        let bad = || FormatError::new(format!("observation {text:?} must look like name or name(#c1, ...)"));
        let trimmed = text.trim();
        let (name, inner) = match trimmed.find('(') {
            Some(open) => {
                let inner = trimmed[open + 1..].strip_suffix(')').ok_or_else(bad)?;
                (&trimmed[..open], Some(inner))
            }
            None => (trimmed, None),
        };
        let kind = ObservationKind::from_name(name).ok_or_else(|| unknown_kind(name))?;
        let args = match inner {
            None => ObservationArgs::None,
            Some(inner) => {
                let refs: Vec<LogicalRef> = inner
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                if refs.is_empty() {
                    ObservationArgs::None
                } else {
                    ObservationArgs::Refs(refs)
                }
            }
        };
        ObservationRequest::checked(kind, args)
    }

    fn parse_mapping(map: Map<String, Value>) -> Result<ObservationRequest, FormatError> {
        let mut entries = map.into_iter();
        let (Some((name, value)), None) = (entries.next(), entries.next()) else {
            return Err(FormatError::new("an observation mapping has exactly one key"));
        };
        let kind = ObservationKind::from_name(&name).ok_or_else(|| unknown_kind(&name))?;
        let args = match value {
            // Only kinds without required arguments take null (the schema).
            Value::Null if matches!(kind.shape(), ArgShape::None | ArgShape::OptionalMapping) => ObservationArgs::None,
            Value::Object(map) => ObservationArgs::Mapping(map),
            // `{ids: []}` is every id, like `ids`; a card list is never empty.
            Value::Array(items) if items.is_empty() && kind.shape() == ArgShape::AnyRefs => ObservationArgs::None,
            Value::Array(items) if items.is_empty() => {
                return Err(FormatError::new(format!(
                    "observation {name} needs at least one reference"
                )));
            }
            Value::Array(items) => ObservationArgs::Refs(
                items
                    .into_iter()
                    .map(|v| match v {
                        Value::String(s) => s.parse(),
                        other => Err(FormatError::new(format!("{other} is not a reference"))),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            other => return Err(FormatError::new(format!("observation {name} cannot take {other}"))),
        };
        ObservationRequest::checked(kind, args)
    }

    fn checked(kind: ObservationKind, args: ObservationArgs) -> Result<ObservationRequest, FormatError> {
        let name = kind.name();
        let ok = match (kind.shape(), &args) {
            (_, ObservationArgs::None) => !matches!(kind.shape(), ArgShape::Mapping),
            (ArgShape::AnyRefs, ObservationArgs::Refs(_)) => true,
            (ArgShape::CardRefs, ObservationArgs::Refs(refs)) => refs.iter().all(|r| r.kind == RefKind::Card),
            (ArgShape::Mapping | ArgShape::OptionalMapping, ObservationArgs::Mapping(_)) => true,
            _ => false,
        };
        if !ok {
            return Err(FormatError::new(format!(
                "observation {name} does not take these arguments"
            )));
        }
        if let ObservationArgs::Mapping(map) = &args {
            super::validate::observation_mapping(kind, map)?;
        }
        Ok(ObservationRequest { kind, args })
    }
}

fn unknown_kind(name: &str) -> FormatError {
    FormatError::new(format!("unknown observation kind {name:?}"))
}

impl<'de> Deserialize<'de> for ObservationRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ObservationRequest, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::String(text) => ObservationRequest::parse_text(&text),
            Value::Object(map) => ObservationRequest::parse_mapping(map),
            other => Err(FormatError::new(format!("{other} is not an observation request"))),
        }
        .map_err(de::Error::custom)
    }
}

/// What a scenario uses: the capabilities an executor needs to run it (D8).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Needs {
    pub setup: BTreeSet<String>,
    pub ops: BTreeSet<String>,
    pub observations: BTreeSet<String>,
}

impl Needs {
    pub fn is_empty(&self) -> bool {
        self.setup.is_empty() && self.ops.is_empty() && self.observations.is_empty()
    }

    /// One line for reports: `ops: a, b; observations: c; setup: d`.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = [
            ("ops", &self.ops),
            ("observations", &self.observations),
            ("setup", &self.setup),
        ]
        .into_iter()
        .filter(|(_, set)| !set.is_empty())
        .map(|(what, set)| format!("{what}: {}", set.iter().cloned().collect::<Vec<_>>().join(", ")))
        .collect();
        parts.join("; ")
    }
}

impl Scenario {
    /// Reads the typed model from a parsed scenario value and checks what
    /// the schema requires beyond the types (scenario.schema.json).
    pub fn from_value(value: &Value) -> Result<Scenario, FormatError> {
        let scenario = Scenario::deserialize(value).map_err(|e| FormatError::new(e.to_string()))?;
        super::validate::scenario(&scenario)?;
        Ok(scenario)
    }

    /// The setup sections, ops and observation kinds the scenario uses.
    pub fn needs(&self) -> Needs {
        let mut needs = Needs::default();
        needs.setup.extend(self.setup.sections().into_iter().map(str::to_owned));
        add_step_needs(&mut needs, &self.steps);
        needs
    }

    /// A live scenario has handoff steps (D9).
    pub fn is_live(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Handoff(_)))
    }

    /// The segments of the scenario, split at its handoff steps; one segment
    /// on the oracle when it has none. Handoffs alternate (checked on load),
    /// so the first segment runs on the side the first handoff leaves.
    pub fn segments(&self) -> Vec<Segment> {
        let first = self
            .steps
            .iter()
            .find_map(|s| match s {
                Step::Handoff(to) => Some(to.other()),
                _ => None,
            })
            .unwrap_or(Side::Oracle);
        let mut segments = Vec::new();
        let (mut side, mut start) = (first, 0);
        for (i, step) in self.steps.iter().enumerate() {
            if let Step::Handoff(to) = step {
                segments.push(Segment {
                    side,
                    steps: start..i,
                    handoff: Some(i),
                });
                side = *to;
                start = i + 1;
            }
        }
        segments.push(Segment {
            side,
            steps: start..self.steps.len(),
            handoff: None,
        });
        segments
    }

    /// What one side needs to run its segments of a live scenario: the setup
    /// sections when it runs the first segment, the ops and observations of
    /// its segments, and `handoff`.
    pub fn needs_on(&self, side: Side) -> Needs {
        let mut needs = Needs::default();
        for (i, segment) in self.segments().iter().enumerate().filter(|(_, s)| s.side == side) {
            if i == 0 {
                needs.setup.extend(self.setup.sections().into_iter().map(str::to_owned));
            }
            add_step_needs(&mut needs, &self.steps[segment.steps.clone()]);
        }
        if self.is_live() {
            needs.ops.insert("handoff".to_owned());
        }
        needs
    }
}

fn add_step_needs(needs: &mut Needs, steps: &[Step]) {
    for step in steps {
        match step {
            Step::Observe(requests) => {
                needs
                    .observations
                    .extend(requests.iter().map(|r| r.kind.name().to_owned()));
            }
            other => {
                needs.ops.insert(other.name().to_owned());
            }
        }
    }
}
