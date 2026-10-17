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
    #[serde(default, deserialize_with = "present")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub clock: ClockSpec,
    #[serde(default)]
    pub compare: CompareSpec,
    #[serde(default)]
    pub setup: Setup,
    /// The clients of an actor scenario (sync-ops.md S2.1); the first runs
    /// the setup.
    #[serde(default, deserialize_with = "present")]
    pub actors: Option<Vec<ActorName>>,
    /// The scenario's sync server (sync-ops.md S3.2); only with actors.
    #[serde(default, deserialize_with = "present")]
    pub server: Option<ServerSpec>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// An actor of an actor scenario (sync-ops.md S2.1): a Meguri actor
/// (`meguri`, `meguri_2`, ...) or an oracle actor (`client_a` to `client_z`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ActorName(String);

impl ActorName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// In T1, Meguri's executor plays it; the oracle plays every other actor.
    pub fn is_meguri(&self) -> bool {
        self.0.starts_with("meguri")
    }
}

impl fmt::Display for ActorName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ActorName {
    type Err = FormatError;

    fn from_str(name: &str) -> Result<ActorName, FormatError> {
        let numbered =
            |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0') && n != "1";
        let letter = |c: &str| c.len() == 1 && c.bytes().all(|b| b.is_ascii_lowercase());
        let ok = name == "meguri"
            || name.strip_prefix("meguri_").is_some_and(numbered)
            || name.strip_prefix("client_").is_some_and(letter);
        if ok {
            Ok(ActorName(name.to_owned()))
        } else {
            Err(FormatError::new(format!(
                "actor name {name:?} must be meguri, meguri_<n> with n >= 2, or client_a to client_z"
            )))
        }
    }
}

impl<'de> Deserialize<'de> for ActorName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ActorName, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

/// `server` (sync-ops.md S3.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerSpec {
    /// Seconds the server's clock runs ahead of the scenario clock (behind
    /// when negative).
    pub clock_offset_secs: i64,
}

impl ServerSpec {
    pub const MAX_OFFSET_SECS: i64 = 86_400;
}

impl<'de> Deserialize<'de> for ServerSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ServerSpec, D::Error> {
        let map = mapping(deserializer, "server")?;
        let mut spec = ServerSpec::default();
        for (key, value) in map {
            if key != "clock_offset_secs" {
                return Err(de::Error::custom(format!(
                    "server: unknown key {key:?}, expected clock_offset_secs"
                )));
            }
            spec.clock_offset_secs = value
                .as_i64()
                .filter(|v| v.abs() <= ServerSpec::MAX_OFFSET_SECS)
                .ok_or_else(|| {
                    de::Error::custom(format!(
                        "server.clock_offset_secs must be an integer from -86400 to 86400, found {value}"
                    ))
                })?;
        }
        Ok(spec)
    }
}

/// The value of an op that takes a mapping, as the mapping: serde's derived
/// structs would also accept a list of their fields, which the schema
/// forbids.
fn mapping<'de, D: Deserializer<'de>>(deserializer: D, what: &str) -> Result<Map<String, Value>, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::Object(map) => Ok(map),
        other => Err(de::Error::custom(format!("{what} takes a mapping, found {other}"))),
    }
}

/// An op argument read through its mapping, for a variant's or a member's
/// `deserialize_with`: serde's derived struct alone would also read the list
/// of its values, which the schema forbids.
fn op_mapping<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: de::DeserializeOwned,
{
    let map = mapping(deserializer, "an op argument")?;
    serde_json::from_value(Value::Object(map)).map_err(de::Error::custom)
}

/// The argument `{}` of `reopen`, `converge` and `check_db`: a mapping with
/// no keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoArgs;

impl<'de> Deserialize<'de> for NoArgs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<NoArgs, D::Error> {
        let map = mapping(deserializer, "this op")?;
        match map.keys().next() {
            None => Ok(NoArgs),
            Some(key) => Err(de::Error::custom(format!("unknown key {key:?}: the op takes {{}}"))),
        }
    }
}

/// `sync: {kind, credentials?}` (sync-ops.md S4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncSpec {
    pub kind: SyncKind,
    /// What a `login` sends (S4.5); `Account` for every other kind.
    pub credentials: Credentials,
}

impl SyncSpec {
    /// The rejections a sync step records (S4.4, dsl.md D7.2): the list a
    /// Meguri sync adapter registers with `Registry::op_rejecting`.
    pub const REJECTIONS: [&'static str; 3] = ["auth_failed", "clock_incorrect", "network"];
}

/// How a `login` derives its credentials from the scenario's account
/// (sync-ops.md S4.5). A scenario never names a credential.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Credentials {
    /// The account's user name and password.
    #[default]
    Account,
    /// The account's user name with its password followed by `-wrong`: the
    /// server refuses it (`auth_failed`).
    WrongPassword,
}

impl Credentials {
    /// What `wrong_password` appends to the account's password (S4.5).
    pub const WRONG_PASSWORD_SUFFIX: &'static str = "-wrong";

    pub fn as_str(self) -> &'static str {
        match self {
            Credentials::Account => "account",
            Credentials::WrongPassword => "wrong_password",
        }
    }

    /// The password a login with these credentials sends for an account
    /// whose password is `password`.
    pub fn password(self, password: &str) -> String {
        match self {
            Credentials::Account => password.to_owned(),
            Credentials::WrongPassword => format!("{password}{}", Credentials::WRONG_PASSWORD_SUFFIX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncKind {
    Login,
    Normal,
    FullUpload,
    FullDownload,
    /// The sync status (S4.1; `specs/sync/meta-status.md` M6): results
    /// `no_changes`, `normal_sync` or `full_sync`; it syncs nothing.
    Status,
}

impl SyncKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncKind::Login => "login",
            SyncKind::Normal => "normal",
            SyncKind::FullUpload => "full_upload",
            SyncKind::FullDownload => "full_download",
            SyncKind::Status => "status",
        }
    }
}

impl<'de> Deserialize<'de> for SyncSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<SyncSpec, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            kind: SyncKind,
        }
        let mut map = mapping(deserializer, "sync")?;
        // Read by hand, so its errors name the argument and a present
        // `credentials: null` is not taken for a missing one.
        let credentials = map.remove("credentials");
        let raw: Raw = serde_json::from_value(Value::Object(map)).map_err(de::Error::custom)?;
        let credentials = match credentials {
            None => Credentials::Account,
            Some(_) if raw.kind != SyncKind::Login => {
                return Err(de::Error::custom(format!(
                    "sync {}: credentials are allowed only with kind login (sync-ops.md S4.5)",
                    raw.kind.as_str()
                )));
            }
            Some(Value::String(text)) if text == "account" => Credentials::Account,
            Some(Value::String(text)) if text == "wrong_password" => Credentials::WrongPassword,
            Some(other) => {
                return Err(de::Error::custom(format!(
                    "sync credentials {other} is not account or wrong_password (sync-ops.md S4.5)"
                )));
            }
        };
        Ok(SyncSpec {
            kind: raw.kind,
            credentials,
        })
    }
}

/// `net_fault` (sync-ops.md S5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetFault {
    /// `reset_peer` on the data flowing in this direction.
    Reset(Direction),
    /// Removes every fault.
    Clear,
}

/// Which way the data a fault hits flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ToClient,
    ToServer,
}

impl<'de> Deserialize<'de> for NetFault {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<NetFault, D::Error> {
        let map = mapping(deserializer, "net_fault")?;
        let text = |key: &str| {
            map.get(key)
                .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
        };
        let fault = match text("kind").as_deref() {
            Some("reset") => {
                let direction = match text("direction").as_deref() {
                    Some("to_client") => Direction::ToClient,
                    Some("to_server") => Direction::ToServer,
                    Some(other) => {
                        return Err(de::Error::custom(format!(
                            "net_fault direction {other:?} is not to_client or to_server"
                        )));
                    }
                    None => {
                        return Err(de::Error::custom(
                            "net_fault reset needs a direction: to_client or to_server",
                        ));
                    }
                };
                NetFault::Reset(direction)
            }
            Some("clear") => NetFault::Clear,
            Some(other) => {
                return Err(de::Error::custom(format!(
                    "net_fault kind {other:?} is not reset or clear"
                )));
            }
            None => return Err(de::Error::custom("net_fault needs a kind: reset or clear")),
        };
        let allowed: &[&str] = match fault {
            NetFault::Reset(_) => &["kind", "direction"],
            NetFault::Clear => &["kind"],
        };
        if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(de::Error::custom(format!(
                "net_fault {} takes no {key:?}",
                text("kind").unwrap_or_default()
            )));
        }
        Ok(fault)
    }
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

/// Setup sections; `None` when the scenario does not use a section. Optional
/// members may be left out but never be null (scenario.schema.json): see
/// `present`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    #[serde(default, deserialize_with = "present")]
    pub config: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    pub presets: Option<Vec<Preset>>,
    #[serde(default, deserialize_with = "present")]
    pub decks: Option<Vec<DeckSpec>>,
    #[serde(default, deserialize_with = "present")]
    pub notetypes: Option<Vec<NotetypeSpec>>,
    #[serde(default, deserialize_with = "present")]
    pub notes: Option<Vec<NoteSpec>>,
    #[serde(default, deserialize_with = "present")]
    pub cards: Option<Vec<CardOverride>>,
    #[serde(default, deserialize_with = "present")]
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
    #[serde(default, deserialize_with = "present")]
    pub learn_steps: Option<Vec<Duration>>,
    #[serde(default, deserialize_with = "present")]
    pub relearn_steps: Option<Vec<Duration>>,
    #[serde(default, deserialize_with = "present")]
    pub graduating_interval_days: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub easy_interval_days: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub new_per_day: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub reviews_per_day: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub max_interval_days: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub min_lapse_interval_days: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub starting_ease: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    pub easy_bonus: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    pub hard_interval: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    pub interval_modifier: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    pub lapse_new_interval: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    pub leech_threshold: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub leech_action: Option<LeechAction>,
    #[serde(default, deserialize_with = "present")]
    pub bury_new: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub bury_reviews: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub bury_interday_learning: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub desired_retention: Option<f64>,
    #[serde(default, deserialize_with = "present")]
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
    #[serde(default, deserialize_with = "present")]
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    #[serde(default, deserialize_with = "present")]
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
    #[serde(rename = "type", default, deserialize_with = "present")]
    pub kind: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub queue: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub due: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub ivl: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub factor: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub reps: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub lapses: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub left: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub odue: Option<i64>,
    #[serde(default, deserialize_with = "present")]
    pub flags: Option<i64>,
    #[serde(default, deserialize_with = "present")]
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

/// `legacy_update` (model-ops.md L2): merge `patch` into the schema-11 object
/// of exactly one named note type, deck or preset and save it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyUpdate {
    #[serde(default, deserialize_with = "present")]
    pub notetype: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub deck: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub preset: Option<String>,
    pub patch: Map<String, Value>,
}

/// The six stock kinds of `add_stock_notetype` (notetype-ops.md T3;
/// `notetypes/stock-kinds` K1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockKind {
    Basic,
    BasicAndReversed,
    BasicOptionalReversed,
    BasicTyping,
    Cloze,
    ImageOcclusion,
}

/// `add_stock_notetype: {kind, name?}` (notetype-ops.md T3): `name` not
/// empty (checked on load), the kind's name when missing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddStockNotetypeArgs {
    pub kind: StockKind,
    #[serde(default, deserialize_with = "present")]
    pub name: Option<String>,
}

/// `clone_notetype: {notetype, name?}` (notetype-ops.md T4): both not empty
/// (checked on load); `<notetype> copy` when `name` is missing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloneNotetypeArgs {
    pub notetype: String,
    #[serde(default, deserialize_with = "present")]
    pub name: Option<String>,
}

/// `update_notetype: {notetype, edits, skip_checks?}` (notetype-ops.md T5):
/// `notetype` not empty (checked on load); the edits apply in order to one
/// draft, saved once.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateNotetypeArgs {
    pub notetype: String,
    pub edits: Vec<NotetypeEdit>,
    #[serde(default)]
    pub skip_checks: bool,
}

/// One edit of `update_notetype` (notetype-ops.md T5): a mapping with exactly
/// one key. Field and template names refer to the draft as the earlier edits
/// left it; they are resolved when the step runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotetypeEdit {
    Rename(String),
    Css(String),
    SortField(String),
    AddField(String),
    RemoveField(String),
    #[serde(deserialize_with = "op_mapping")]
    RenameField(RenamePair),
    #[serde(deserialize_with = "op_mapping")]
    MoveField(MoveFieldEdit),
    #[serde(deserialize_with = "op_mapping")]
    FieldOptions(FieldOptionsEdit),
    /// `{name, front?, back?}`, texts empty when missing.
    #[serde(deserialize_with = "op_mapping")]
    AddTemplate(TemplateSpec),
    RemoveTemplate(String),
    #[serde(deserialize_with = "op_mapping")]
    RenameTemplate(RenamePair),
    #[serde(deserialize_with = "op_mapping")]
    MoveTemplate(MoveTemplateEdit),
    #[serde(deserialize_with = "op_mapping")]
    Template(TemplateEdit),
}

impl NotetypeEdit {
    /// The edit's key as written in scenarios.
    pub fn key(&self) -> &'static str {
        match self {
            NotetypeEdit::Rename(_) => "rename",
            NotetypeEdit::Css(_) => "css",
            NotetypeEdit::SortField(_) => "sort_field",
            NotetypeEdit::AddField(_) => "add_field",
            NotetypeEdit::RemoveField(_) => "remove_field",
            NotetypeEdit::RenameField(_) => "rename_field",
            NotetypeEdit::MoveField(_) => "move_field",
            NotetypeEdit::FieldOptions(_) => "field_options",
            NotetypeEdit::AddTemplate(_) => "add_template",
            NotetypeEdit::RemoveTemplate(_) => "remove_template",
            NotetypeEdit::RenameTemplate(_) => "rename_template",
            NotetypeEdit::MoveTemplate(_) => "move_template",
            NotetypeEdit::Template(_) => "template",
        }
    }

    /// The capability the edit needs (notetype-ops.md T10.1):
    /// `update_notetype.<key>`, and `update_notetype.template_text` for a
    /// `template` edit that sets `front` or `back`.
    pub fn capability(&self) -> String {
        match self {
            NotetypeEdit::Template(t) if t.front.is_some() || t.back.is_some() => {
                "update_notetype.template_text".to_owned()
            }
            other => format!("update_notetype.{}", other.key()),
        }
    }
}

/// `{from, to}` of `rename_field` and `rename_template`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenamePair {
    pub from: String,
    pub to: String,
}

/// `move_field: {field, to}`: `to` a 0-based position of the draft's
/// fields, checked when the step runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveFieldEdit {
    pub field: String,
    pub to: u64,
}

/// `move_template: {template, to}`: `to` a 0-based position.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveTemplateEdit {
    pub template: String,
    pub to: u64,
}

/// `field_options: {field, ...}`: the options given (`model/protobuf-records`
/// P6.2 names); missing ones stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldOptionsEdit {
    pub field: String,
    #[serde(default, deserialize_with = "present")]
    pub sticky: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub rtl: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub font_name: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub font_size: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub plain_text: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub collapsed: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub exclude_from_search: Option<bool>,
}

/// `template: {name, ...}`: the values given for the named template; missing
/// ones stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateEdit {
    pub name: String,
    #[serde(default, deserialize_with = "present")]
    pub front: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub back: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub browser_front: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub browser_back: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub browser_font_name: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub browser_font_size: Option<u64>,
    /// The deck override: `None` when missing (kept), `Some(None)` for null
    /// (cleared), `Some(Some(name))` to set it.
    #[serde(default, deserialize_with = "present")]
    pub deck: Option<Option<String>>,
}

/// `open: {fixture}` (storage-ops.md P1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpec {
    /// A path relative to `conformance/fixtures/`, ending in `.anki2`.
    pub fixture: FixturePath,
}

impl<'de> Deserialize<'de> for OpenSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<OpenSpec, D::Error> {
        let map = mapping(deserializer, "open")?;
        let mut fixture = None;
        for (key, value) in map {
            match (key.as_str(), value) {
                ("fixture", Value::String(path)) => fixture = Some(path.parse().map_err(de::Error::custom)?),
                ("fixture", other) => {
                    return Err(de::Error::custom(format!("open.fixture is a path, found {other}")));
                }
                (other, _) => {
                    return Err(de::Error::custom(format!(
                        "open: unknown key {other:?}, expected fixture"
                    )));
                }
            }
        }
        let fixture = fixture.ok_or_else(|| de::Error::custom("open needs a fixture"))?;
        Ok(OpenSpec { fixture })
    }
}

/// A fixture file below `conformance/fixtures/` (scenario.schema.json
/// `open.fixture`): `/`-separated segments of `[a-z0-9._-]` that start with
/// a letter or digit, ending in `.anki2`, so no `..`, leading `/` or empty
/// segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixturePath(String);

impl FixturePath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for FixturePath {
    type Err = FormatError;

    fn from_str(text: &str) -> Result<FixturePath, FormatError> {
        let segment_ok = |segment: &str| {
            segment
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
        };
        if text.ends_with(".anki2") && text.split('/').all(segment_ok) {
            Ok(FixturePath(text.to_owned()))
        } else {
            Err(FormatError::new(format!(
                "fixture {text:?} must be a path below conformance/fixtures/ ending in .anki2 (segments of a-z, 0-9, '.', '_' and '-')"
            )))
        }
    }
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
    /// record-ops.md W2.
    EditNote(EditNoteArgs),
    /// record-ops.md W3.
    DeleteNotes(DeleteNotesArgs),
    /// record-ops.md W4; `undoable`: undo-ops.md V3.1.
    SetConfig(SetConfigArgs),
    /// undo-ops.md V3.2.
    RemoveConfig(RemoveConfigArgs),
    /// undo-ops.md V1: the top step of the undo history.
    Undo(NoArgs),
    /// undo-ops.md V1: the top step of the redo history.
    Redo(NoArgs),
    /// tag-ops.md G1.
    AddNoteTags(NoteTagsArgs),
    /// tag-ops.md G2.
    RemoveNoteTags(NoteTagsArgs),
    /// tag-ops.md G3.
    DeleteTags(DeleteTagsArgs),
    /// tag-ops.md G4.
    RenameTag(RenameTagArgs),
    /// tag-ops.md G5.
    ReparentTags(ReparentTagsArgs),
    /// tag-ops.md G6.
    ClearUnusedTags(NoArgs),
    /// tag-ops.md G7.
    SetTagCollapsed(SetTagCollapsedArgs),
    /// tag-ops.md G8.
    SetFlag(SetFlagArgs),
    /// browser-config-ops.md B1.
    BrowserConfigSet(BrowserConfigSetArgs),
    Bury(DraftArgs),
    Suspend(DraftArgs),
    Forget(DraftArgs),
    SetDue(DraftArgs),
    Reposition(DraftArgs),
    UpdatePreset(DraftArgs),
    /// model-ops.md L2.
    LegacyUpdate(LegacyUpdate),
    /// notetype-ops.md T2: shaped as a `setup.notetypes` entry.
    #[serde(deserialize_with = "op_mapping")]
    AddNotetype(NotetypeSpec),
    /// notetype-ops.md T3.
    #[serde(deserialize_with = "op_mapping")]
    AddStockNotetype(AddStockNotetypeArgs),
    /// notetype-ops.md T4.
    #[serde(deserialize_with = "op_mapping")]
    CloneNotetype(CloneNotetypeArgs),
    /// notetype-ops.md T5.
    #[serde(deserialize_with = "op_mapping")]
    UpdateNotetype(UpdateNotetypeArgs),
    /// notetype-ops.md T6.
    EnsureIoNotetype(NoArgs),
    BuildFiltered(DraftArgs),
    Import(DraftArgs),
    Export(DraftArgs),
    /// collection-check.md K10: Anki's Check Database, its K5 counts.
    CheckDb(NoArgs),
    /// record-ops.md W1.
    DbExec(DbExecArgs),
    /// Closes the collection and opens it again (sync-ops.md S6).
    Reopen(NoArgs),
    /// storage-ops.md P1: replaces the collection with a copy of a fixture
    /// file and opens it.
    Open(OpenSpec),
    /// Actor scenarios (sync-ops.md S2.3): the actor that runs the next steps.
    Actor(ActorName),
    /// Actor scenarios (sync-ops.md S4).
    Sync(SyncSpec),
    /// Actor scenarios (sync-ops.md S5).
    NetFault(NetFault),
    /// Actor scenarios (sync-ops.md S7).
    Converge(NoArgs),
    /// Live scenarios only (D9): the collection moves to this side.
    Handoff(Side),
}

impl Step {
    /// Every op of the language (D7.2); `observe` included.
    pub const NAMES: [&'static str; 44] = [
        "advance",
        "set_clock",
        "add_note",
        "answer",
        "observe",
        "edit_note",
        "delete_notes",
        "set_config",
        "remove_config",
        "undo",
        "redo",
        "add_note_tags",
        "remove_note_tags",
        "delete_tags",
        "rename_tag",
        "reparent_tags",
        "clear_unused_tags",
        "set_tag_collapsed",
        "set_flag",
        "browser_config_set",
        "bury",
        "suspend",
        "forget",
        "set_due",
        "reposition",
        "update_preset",
        "legacy_update",
        "add_notetype",
        "add_stock_notetype",
        "clone_notetype",
        "update_notetype",
        "ensure_io_notetype",
        "build_filtered",
        "import",
        "export",
        "check_db",
        "db_exec",
        "reopen",
        "open",
        "actor",
        "sync",
        "net_fault",
        "converge",
        "handoff",
    ];

    /// The ops only actor scenarios have (sync-ops.md S1).
    pub const ACTOR_OPS: [&'static str; 4] = ["actor", "sync", "net_fault", "converge"];

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
            Step::SetConfig(_) => "set_config",
            Step::RemoveConfig(_) => "remove_config",
            Step::Undo(_) => "undo",
            Step::Redo(_) => "redo",
            Step::AddNoteTags(_) => "add_note_tags",
            Step::RemoveNoteTags(_) => "remove_note_tags",
            Step::DeleteTags(_) => "delete_tags",
            Step::RenameTag(_) => "rename_tag",
            Step::ReparentTags(_) => "reparent_tags",
            Step::ClearUnusedTags(_) => "clear_unused_tags",
            Step::SetTagCollapsed(_) => "set_tag_collapsed",
            Step::SetFlag(_) => "set_flag",
            Step::BrowserConfigSet(_) => "browser_config_set",
            Step::Bury(_) => "bury",
            Step::Suspend(_) => "suspend",
            Step::Forget(_) => "forget",
            Step::SetDue(_) => "set_due",
            Step::Reposition(_) => "reposition",
            Step::UpdatePreset(_) => "update_preset",
            Step::LegacyUpdate(_) => "legacy_update",
            Step::AddNotetype(_) => "add_notetype",
            Step::AddStockNotetype(_) => "add_stock_notetype",
            Step::CloneNotetype(_) => "clone_notetype",
            Step::UpdateNotetype(_) => "update_notetype",
            Step::EnsureIoNotetype(_) => "ensure_io_notetype",
            Step::BuildFiltered(_) => "build_filtered",
            Step::Import(_) => "import",
            Step::Export(_) => "export",
            Step::CheckDb(_) => "check_db",
            Step::DbExec(_) => "db_exec",
            Step::Reopen(_) => "reopen",
            Step::Open(_) => "open",
            Step::Actor(_) => "actor",
            Step::Sync(_) => "sync",
            Step::NetFault(_) => "net_fault",
            Step::Converge(_) => "converge",
            Step::Handoff(_) => "handoff",
        }
    }

    /// A step that runs whatever the current actor of an actor scenario
    /// (sync-ops.md S2.3): it moves the shared clock, changes every actor's
    /// network path, reads every actor or picks the current one. The current
    /// actor runs every other step.
    pub fn is_global(&self) -> bool {
        matches!(
            self,
            Step::Advance(_) | Step::SetClock(_) | Step::NetFault(_) | Step::Converge(_) | Step::Actor(_)
        )
    }

    /// The arguments of a draft op.
    pub fn draft_args(&self) -> Option<&DraftArgs> {
        match self {
            Step::Bury(a)
            | Step::Suspend(a)
            | Step::Forget(a)
            | Step::SetDue(a)
            | Step::Reposition(a)
            | Step::UpdatePreset(a)
            | Step::BuildFiltered(a)
            | Step::Import(a)
            | Step::Export(a) => Some(a),
            _ => None,
        }
    }
}

/// An optional member that is present: its value, never null. With
/// `#[serde(default)]` a missing member stays `None`; serde alone would read
/// an explicit null as missing, which the schema forbids for these members.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
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

/// `db_exec: {sql, params?}` (record-ops.md W1).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbExecArgs {
    /// Exactly one statement; not empty (checked on load).
    pub sql: String,
    #[serde(default, deserialize_with = "present")]
    pub params: Option<Vec<SqlParam>>,
}

/// A `db_exec` parameter (W1): a string binds as TEXT, an integer as
/// INTEGER, any other number as REAL, null as NULL. Booleans, lists,
/// mappings and integers outside the signed 64-bit range are invalid.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlParam {
    Text(String),
    Integer(i64),
    Real(f64),
    Null,
}

impl<'de> Deserialize<'de> for SqlParam {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<SqlParam, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Null => Ok(SqlParam::Null),
            Value::String(s) => Ok(SqlParam::Text(s)),
            Value::Number(n) => match (n.as_i64(), n.is_f64(), n.as_f64()) {
                (Some(i), _, _) => Ok(SqlParam::Integer(i)),
                (None, true, Some(f)) => Ok(SqlParam::Real(f)),
                _ => Err(de::Error::custom(format!(
                    "db_exec parameter {n} is outside the signed 64-bit range"
                ))),
            },
            other => Err(de::Error::custom(format!(
                "db_exec parameters are strings, numbers or null, found {other}"
            ))),
        }
    }
}

/// `edit_note: {note, fields?, tags?}` (record-ops.md W2).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditNoteArgs {
    pub note: NoteRef,
    /// A list replaces the fields from the first one; a mapping the named
    /// ones.
    #[serde(default, deserialize_with = "present")]
    pub fields: Option<Fields>,
    /// Replaces every tag, in this order.
    #[serde(default, deserialize_with = "present")]
    pub tags: Option<Vec<String>>,
}

/// `delete_notes: {notes}` (record-ops.md W3); at least one (checked on
/// load).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteNotesArgs {
    pub notes: Vec<NoteRef>,
}

/// `set_config: {key, value, undoable?}` (record-ops.md W4): `key` not
/// empty (checked on load), `value` any JSON value, null included.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetConfigArgs {
    pub key: String,
    pub value: Value,
    /// undo-ops.md V3.1: true makes the write an "Update Config" undo step;
    /// false when missing (an explicit null is invalid).
    #[serde(default)]
    pub undoable: bool,
}

/// `remove_config: {key}` (undo-ops.md V3.2): `key` not empty (checked on
/// load).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveConfigArgs {
    pub key: String,
}

/// `add_note_tags` and `remove_note_tags: {notes, tags}` (tag-ops.md G1,
/// G2): at least one note (checked on load); `tags` the tag text as a caller
/// gives it, which the executor splits.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteTagsArgs {
    pub notes: Vec<NoteRef>,
    pub tags: String,
}

/// `delete_tags: {tags}` (tag-ops.md G3): the tag text of the names to
/// delete.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteTagsArgs {
    pub tags: String,
}

/// `rename_tag: {from, to}` (tag-ops.md G4): `from` one non-empty name
/// without U+0020 or U+3000 (checked on load); `to` the new name as typed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameTagArgs {
    pub from: String,
    pub to: String,
}

/// `reparent_tags: {tags, parent}` (tag-ops.md G5): at least one source,
/// each non-empty (checked on load); `parent` the target, empty for the top
/// level.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReparentTagsArgs {
    pub tags: Vec<String>,
    pub parent: String,
}

/// `set_tag_collapsed: {tags, collapsed}` (tag-ops.md G7): at least one
/// name, each non-empty (checked on load).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTagCollapsedArgs {
    pub tags: Vec<String>,
    pub collapsed: bool,
}

/// `set_flag: {cards, flag}` (tag-ops.md G8): at least one card (checked on
/// load); `flag` an integer from 0, a value above 7 being the executor's
/// `invalid_flag` rejection.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetFlagArgs {
    pub cards: Vec<CardRef>,
    pub flag: u64,
}

/// `browser_config_set: {key, value}` (browser-config-ops.md B1): `key` one
/// of the 22 browser config keys, `value` of its shape (both checked on
/// load).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfigSetArgs {
    pub key: String,
    pub value: Value,
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

/// A `#nN` reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteRef(pub LogicalRef);

impl<'de> Deserialize<'de> for NoteRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<NoteRef, D::Error> {
        let r = LogicalRef::deserialize(deserializer)?;
        if r.kind != RefKind::Note {
            return Err(de::Error::custom(format!("{r} is not a note reference")));
        }
        Ok(NoteRef(r))
    }
}

/// The tables of a schema-18 collection file that `db_dump` reports
/// (observations.md O8).
pub const DUMP_TABLES: [&str; 12] = [
    "col",
    "notes",
    "cards",
    "revlog",
    "graves",
    "decks",
    "deck_config",
    "notetypes",
    "fields",
    "templates",
    "tags",
    "config",
];

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
    /// render-ops.md Q4.
    RenderStatus,
    /// render-ops.md Q1.
    Template,
    /// search-ops.md S1.
    Search,
    /// search-ops.md S2.
    CanonicalSearch,
    DbDump,
    /// storage-ops.md P3.
    SchemaObjects,
    Stats,
    Digest,
    LegacyJson,
    BlobDecode,
    PackageCol,
    Text,
    /// undo-ops.md V2.
    UndoStatus,
    /// cloze-ops.md Z1.
    Cloze,
    /// float-determinism.md F5.
    FsrsNext,
    /// tag-ops.md G9.
    TagTree,
    /// browser-config-ops.md B2.
    BrowserConfig,
    /// notetype-ops.md T8.
    NotetypeStructure,
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
    /// A required non-empty list of values that are not references.
    List,
}

impl ObservationKind {
    pub const ALL: [ObservationKind; 28] = [
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
        ObservationKind::RenderStatus,
        ObservationKind::Template,
        ObservationKind::Search,
        ObservationKind::CanonicalSearch,
        ObservationKind::DbDump,
        ObservationKind::SchemaObjects,
        ObservationKind::Stats,
        ObservationKind::Digest,
        ObservationKind::LegacyJson,
        ObservationKind::BlobDecode,
        ObservationKind::PackageCol,
        ObservationKind::Text,
        ObservationKind::UndoStatus,
        ObservationKind::Cloze,
        ObservationKind::FsrsNext,
        ObservationKind::TagTree,
        ObservationKind::BrowserConfig,
        ObservationKind::NotetypeStructure,
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
            ObservationKind::RenderStatus => "render_status",
            ObservationKind::Template => "template",
            ObservationKind::Search => "search",
            ObservationKind::CanonicalSearch => "canonical_search",
            ObservationKind::DbDump => "db_dump",
            ObservationKind::SchemaObjects => "schema_objects",
            ObservationKind::Stats => "stats",
            ObservationKind::Digest => "digest",
            ObservationKind::LegacyJson => "legacy_json",
            ObservationKind::BlobDecode => "blob_decode",
            ObservationKind::PackageCol => "package_col",
            ObservationKind::Text => "text",
            ObservationKind::UndoStatus => "undo_status",
            ObservationKind::Cloze => "cloze",
            ObservationKind::FsrsNext => "fsrs_next",
            ObservationKind::TagTree => "tag_tree",
            ObservationKind::BrowserConfig => "browser_config",
            ObservationKind::NotetypeStructure => "notetype_structure",
        }
    }

    pub fn from_name(name: &str) -> Option<ObservationKind> {
        ObservationKind::ALL.into_iter().find(|k| k.name() == name)
    }

    pub(super) fn shape(self) -> ArgShape {
        match self {
            ObservationKind::Ids => ArgShape::AnyRefs,
            ObservationKind::Timing
            | ObservationKind::DeckTree
            | ObservationKind::Digest
            | ObservationKind::SchemaObjects
            | ObservationKind::UndoStatus
            | ObservationKind::TagTree
            | ObservationKind::BrowserConfig => ArgShape::None,
            ObservationKind::Card
            | ObservationKind::Revlog
            | ObservationKind::NextStates
            | ObservationKind::Render
            | ObservationKind::RenderStatus => ArgShape::CardRefs,
            ObservationKind::FuzzTable
            | ObservationKind::NetProbe
            | ObservationKind::LegacyJson
            | ObservationKind::BlobDecode
            | ObservationKind::PackageCol
            | ObservationKind::Template
            | ObservationKind::Text
            | ObservationKind::Cloze
            | ObservationKind::FsrsNext => ArgShape::Mapping,
            ObservationKind::Queue
            | ObservationKind::DbDump
            | ObservationKind::Stats
            | ObservationKind::NotetypeStructure => ArgShape::OptionalMapping,
            ObservationKind::Search | ObservationKind::CanonicalSearch => ArgShape::List,
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
    /// The items of a kind that takes a list (`search`, `canonical_search`).
    List(Vec<Value>),
}

/// One entry of an `observe` list (observations.md O2).
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationRequest {
    pub kind: ObservationKind,
    pub args: ObservationArgs,
}

impl ObservationRequest {
    /// D8: the kinds that never read a collection, `text`, `cloze`,
    /// `fsrs_next` and `canonical_search`, and `template` when every case is
    /// a new-note case.
    pub fn reads_no_collection(&self) -> bool {
        match self.kind {
            ObservationKind::Text
            | ObservationKind::Cloze
            | ObservationKind::FsrsNext
            | ObservationKind::CanonicalSearch => true,
            ObservationKind::Template => match &self.args {
                ObservationArgs::Mapping(map) => TemplateRequest::from_mapping(map)
                    .is_ok_and(|request| request.cases.iter().all(|c| matches!(c, TemplateCase::NewNote(_)))),
                _ => false,
            },
            _ => false,
        }
    }

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
            Value::Array(items) if kind.shape() == ArgShape::List => ObservationArgs::List(items),
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
            (_, ObservationArgs::None) => !matches!(kind.shape(), ArgShape::Mapping | ArgShape::List),
            (ArgShape::AnyRefs, ObservationArgs::Refs(_)) => true,
            (ArgShape::CardRefs, ObservationArgs::Refs(refs)) => refs.iter().all(|r| r.kind == RefKind::Card),
            (ArgShape::Mapping | ArgShape::OptionalMapping, ObservationArgs::Mapping(_)) => true,
            (ArgShape::List, ObservationArgs::List(_)) => true,
            _ => false,
        };
        if !ok {
            return Err(FormatError::new(format!(
                "observation {name} does not take these arguments"
            )));
        }
        match &args {
            ObservationArgs::Mapping(map) => super::validate::observation_mapping(kind, map)?,
            ObservationArgs::List(items) => super::validate::observation_list(kind, items)?,
            ObservationArgs::None | ObservationArgs::Refs(_) => {}
        }
        Ok(ObservationRequest { kind, args })
    }
}

fn unknown_kind(name: &str) -> FormatError {
    FormatError::new(format!("unknown observation kind {name:?}"))
}

/// The mode of one `search` request (search-ops.md S1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SearchKind {
    Cards,
    Notes,
}

/// One request of a `search` observation: `{query, kind?}`, `kind` cards when
/// missing (search-ops.md S1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub query: String,
    pub kind: SearchKind,
}

impl SearchRequest {
    /// Reads a `search` list with scenario.schema.json's rules: at least one
    /// request, each exactly `query` (a string) and an optional `kind`.
    pub fn from_list(items: &[Value]) -> Result<Vec<SearchRequest>, FormatError> {
        let bad = |why: String| FormatError::new(format!("observation search: {why}"));
        if items.is_empty() {
            return Err(bad("needs at least one request".to_owned()));
        }
        items
            .iter()
            .enumerate()
            .map(|(k, item)| {
                let map = item
                    .as_object()
                    .ok_or_else(|| bad(format!("request {k} is a {{query, kind?}} mapping")))?;
                if let Some(key) = map.keys().find(|key| !matches!(key.as_str(), "query" | "kind")) {
                    return Err(bad(format!("request {k} takes {{query, kind?}}, not {key:?}")));
                }
                let query = map
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad(format!("request {k} needs a query string")))?;
                let kind = match map.get("kind") {
                    None => SearchKind::Cards,
                    Some(kind) => match kind.as_str() {
                        Some("cards") => SearchKind::Cards,
                        Some("notes") => SearchKind::Notes,
                        _ => return Err(bad(format!("request {k}: kind is cards or notes"))),
                    },
                };
                Ok(SearchRequest {
                    query: query.to_owned(),
                    kind,
                })
            })
            .collect()
    }
}

/// Reads a `canonical_search` list (search-ops.md S2): at least one search
/// text, every item a string.
pub fn canonical_search_texts(items: &[Value]) -> Result<Vec<String>, FormatError> {
    let bad = |why: &str| FormatError::new(format!("observation canonical_search: {why}"));
    if items.is_empty() {
        return Err(bad("needs at least one search text"));
    }
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| bad("every item is a string"))
        })
        .collect()
}

/// The functions of the `text` kind (observations.md O9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextFunction {
    StripMarkup,
    StripMarkupKeepMedia,
    TextLine,
    TextLineKeepMedia,
    MediaRefs,
    IriEncode,
    IriDecode,
    MediaFilename,
    SortField,
    Checksum,
    FieldEmpty,
    UnicaseCompare,
}

impl TextFunction {
    pub const ALL: [TextFunction; 12] = [
        TextFunction::StripMarkup,
        TextFunction::StripMarkupKeepMedia,
        TextFunction::TextLine,
        TextFunction::TextLineKeepMedia,
        TextFunction::MediaRefs,
        TextFunction::IriEncode,
        TextFunction::IriDecode,
        TextFunction::MediaFilename,
        TextFunction::SortField,
        TextFunction::Checksum,
        TextFunction::FieldEmpty,
        TextFunction::UnicaseCompare,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TextFunction::StripMarkup => "strip_markup",
            TextFunction::StripMarkupKeepMedia => "strip_markup_keep_media",
            TextFunction::TextLine => "text_line",
            TextFunction::TextLineKeepMedia => "text_line_keep_media",
            TextFunction::MediaRefs => "media_refs",
            TextFunction::IriEncode => "iri_encode",
            TextFunction::IriDecode => "iri_decode",
            TextFunction::MediaFilename => "media_filename",
            TextFunction::SortField => "sort_field",
            TextFunction::Checksum => "checksum",
            TextFunction::FieldEmpty => "field_empty",
            TextFunction::UnicaseCompare => "unicase_compare",
        }
    }

    pub fn from_name(name: &str) -> Option<TextFunction> {
        TextFunction::ALL.into_iter().find(|f| f.name() == name)
    }
}

/// The inputs of a `text` request: strings, or `[a, b]` pairs for
/// `unicase_compare`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputs {
    Strings(Vec<String>),
    Pairs(Vec<(String, String)>),
}

/// A `text` observation request: `{fns, inputs}` (observations.md O4, O9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRequest {
    pub fns: Vec<TextFunction>,
    pub inputs: TextInputs,
}

impl TextRequest {
    /// Reads the request with scenario.schema.json's rules: exactly `fns` and
    /// `inputs`; distinct known functions, at least one; `unicase_compare`
    /// alone, on pairs of strings; every other function on strings.
    pub fn from_mapping(map: &Map<String, Value>) -> Result<TextRequest, FormatError> {
        let bad = |why: &str| FormatError::new(format!("observation text: {why}"));
        if map.len() != 2 || !map.contains_key("fns") || !map.contains_key("inputs") {
            return Err(bad("takes exactly {fns, inputs}"));
        }
        let names = map["fns"]
            .as_array()
            .ok_or_else(|| bad("fns is a list of function names"))?;
        if names.is_empty() {
            return Err(bad("fns names at least one function"));
        }
        let mut fns = Vec::with_capacity(names.len());
        for name in names {
            let f = name
                .as_str()
                .and_then(TextFunction::from_name)
                .ok_or_else(|| bad(&format!("{name} is not a text function")))?;
            if fns.contains(&f) {
                return Err(bad(&format!("{} appears twice in fns", f.name())));
            }
            fns.push(f);
        }
        let items = map["inputs"].as_array().ok_or_else(|| bad("inputs is a list"))?;
        let inputs = if fns.contains(&TextFunction::UnicaseCompare) {
            if fns.len() > 1 {
                return Err(bad("unicase_compare shares a request with no other function"));
            }
            let pair = |item: &Value| match item.as_array().map(Vec::as_slice) {
                Some([Value::String(a), Value::String(b)]) => Some((a.clone(), b.clone())),
                _ => None,
            };
            let pairs = items.iter().map(pair).collect::<Option<Vec<_>>>();
            TextInputs::Pairs(pairs.ok_or_else(|| bad("unicase_compare inputs are [a, b] pairs of strings"))?)
        } else {
            let strings = items
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>();
            TextInputs::Strings(strings.ok_or_else(|| bad("inputs are strings"))?)
        };
        Ok(TextRequest { fns, inputs })
    }
}

/// The functions of the `cloze` kind (cloze-ops.md Z2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClozeFunction {
    Numbers,
    Front,
    Back,
    OnlyFront,
    OnlyBack,
    Typing,
}

impl ClozeFunction {
    pub const ALL: [ClozeFunction; 6] = [
        ClozeFunction::Numbers,
        ClozeFunction::Front,
        ClozeFunction::Back,
        ClozeFunction::OnlyFront,
        ClozeFunction::OnlyBack,
        ClozeFunction::Typing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ClozeFunction::Numbers => "numbers",
            ClozeFunction::Front => "front",
            ClozeFunction::Back => "back",
            ClozeFunction::OnlyFront => "only_front",
            ClozeFunction::OnlyBack => "only_back",
            ClozeFunction::Typing => "typing",
        }
    }

    pub fn from_name(name: &str) -> Option<ClozeFunction> {
        ClozeFunction::ALL.into_iter().find(|f| f.name() == name)
    }
}

/// The highest card number a `cloze` input names (`render/cloze#C1`).
pub const CLOZE_MAX_CARD: u16 = u16::MAX;

/// The inputs of a `cloze` request: strings for `numbers`, `[text, card]`
/// pairs for every other function (cloze-ops.md Z1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClozeInputs {
    Texts(Vec<String>),
    /// Card numbers are 1 to [`CLOZE_MAX_CARD`].
    Cards(Vec<(String, u16)>),
}

/// A `cloze` observation request: `{fns, inputs}` (cloze-ops.md Z1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClozeRequest {
    pub fns: Vec<ClozeFunction>,
    pub inputs: ClozeInputs,
}

impl ClozeRequest {
    /// Reads the request with Z1's rules: exactly `fns` and `inputs`;
    /// distinct known functions, at least one; `numbers` alone, on strings;
    /// every other function on `[string, integer 1-65535]` pairs. A card
    /// written as a float, a string or a boolean is refused.
    pub fn from_mapping(map: &Map<String, Value>) -> Result<ClozeRequest, FormatError> {
        let bad = |why: &str| FormatError::new(format!("observation cloze: {why}"));
        if map.len() != 2 || !map.contains_key("fns") || !map.contains_key("inputs") {
            return Err(bad("takes exactly {fns, inputs}"));
        }
        let names = map["fns"]
            .as_array()
            .ok_or_else(|| bad("fns is a list of function names"))?;
        if names.is_empty() {
            return Err(bad("fns names at least one function"));
        }
        let mut fns = Vec::with_capacity(names.len());
        for name in names {
            let f = name
                .as_str()
                .and_then(ClozeFunction::from_name)
                .ok_or_else(|| bad(&format!("{name} is not a cloze function")))?;
            if fns.contains(&f) {
                return Err(bad(&format!("{} appears twice in fns", f.name())));
            }
            fns.push(f);
        }
        let items = map["inputs"].as_array().ok_or_else(|| bad("inputs is a list"))?;
        let inputs = if fns.contains(&ClozeFunction::Numbers) {
            if fns.len() > 1 {
                return Err(bad("numbers shares a request with no other function"));
            }
            let texts = items
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>();
            ClozeInputs::Texts(texts.ok_or_else(|| bad("numbers inputs are strings"))?)
        } else {
            let pair = |item: &Value| match item.as_array().map(Vec::as_slice) {
                Some([Value::String(text), card]) => card
                    .as_u64()
                    .and_then(|card| u16::try_from(card).ok())
                    .filter(|&card| card >= 1)
                    .map(|card| (text.clone(), card)),
                _ => None,
            };
            let pairs = items.iter().map(pair).collect::<Option<Vec<_>>>();
            ClozeInputs::Cards(pairs.ok_or_else(|| bad("inputs are [text, card] pairs with a card from 1 to 65535"))?)
        };
        Ok(ClozeRequest { fns, inputs })
    }
}

/// One probe input of an `fsrs_next` request: a stored review card
/// (float-determinism.md F2.1, F3.1), with its `f32` members read per
/// `model/json-numbers` N2.
#[derive(Debug, Clone, PartialEq)]
pub struct FsrsNextInput {
    pub id: u64,
    pub card_id: u64,
    pub reps: u64,
    pub s: f32,
    pub d: f32,
    pub elapsed_days: u32,
    pub scheduled_days: u32,
    pub dr: f32,
    /// A member of the request's `param_sets`.
    pub params: String,
    pub max_ivl: u32,
}

/// An `fsrs_next` observation request (float-determinism.md F5.1).
#[derive(Debug, Clone, PartialEq)]
pub struct FsrsNextRequest {
    /// Parameter sets by name: 0, 17, 19 or 21 `f32` values.
    pub param_sets: BTreeMap<String, Vec<f32>>,
    /// 1 to 1,000 inputs.
    pub inputs: Vec<FsrsNextInput>,
}

/// The most inputs one `fsrs_next` request holds (F5.1).
pub const FSRS_NEXT_MAX_INPUTS: usize = 1000;

/// The members of an `fsrs_next` input (F2.1).
const FSRS_NEXT_MEMBERS: [&str; 10] = [
    "card_id",
    "d",
    "dr",
    "elapsed_days",
    "id",
    "max_ivl",
    "params",
    "reps",
    "s",
    "scheduled_days",
];

/// `^[a-z0-9][a-z0-9-]*$`: a parameter set's name (F5.1).
fn param_set_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A JSON number read as `f32` (`model/json-numbers` N2): an integer literal
/// to the nearest `f32`, any other to the nearest 64-bit float, then to the
/// nearest `f32`.
pub fn json_f32(value: &Value) -> Option<f32> {
    let n = value.as_number()?;
    if let Some(i) = n.as_i64() {
        return Some(i as f32);
    }
    if let Some(u) = n.as_u64() {
        return Some(u as f32);
    }
    n.as_f64().map(|x| x as f32)
}

impl FsrsNextInput {
    /// Reads one input with scenario.schema.json's `fsrs_next_input` rules.
    pub fn from_value(value: &Value) -> Result<FsrsNextInput, String> {
        let map = value.as_object().ok_or("an input is an object")?;
        let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
        if keys != BTreeSet::from(FSRS_NEXT_MEMBERS) {
            return Err(format!(
                "an input has exactly the members {}",
                FSRS_NEXT_MEMBERS.join(", ")
            ));
        }
        let int = |key: &str, min: u64, max: u64| {
            map[key]
                .as_u64()
                .filter(|v| (min..=max).contains(v))
                .ok_or_else(|| format!("{key} is an integer from {min} to {max}"))
        };
        let small = |key: &str, min: u32, max: u32| int(key, min.into(), max.into()).map(|v| v as u32);
        let number = |key: &str, ok: &dyn Fn(f64) -> bool, range: &str| {
            map[key]
                .as_f64()
                .filter(|v| ok(*v))
                .and_then(|_| json_f32(&map[key]))
                .ok_or_else(|| format!("{key} is a number {range}"))
        };
        Ok(FsrsNextInput {
            id: int("id", 0, u64::MAX)?,
            card_id: int("card_id", 1, (1 << 53) - 1)?,
            reps: int("reps", 0, 4_294_967_294)?,
            s: number("s", &|v| (0.001..=36500.0).contains(&v), "from 0.001 to 36,500")?,
            d: number("d", &|v| (1.0..=10.0).contains(&v), "from 1 to 10")?,
            elapsed_days: small("elapsed_days", 0, 20_000)?,
            scheduled_days: small("scheduled_days", 1, 36_500)?,
            dr: number("dr", &|v| v > 0.0 && v < 1.0, "above 0 and below 1")?,
            params: map["params"]
                .as_str()
                .filter(|p| param_set_name(p))
                .ok_or("params names a parameter set")?
                .to_owned(),
            max_ivl: small("max_ivl", 1, 36_500)?,
        })
    }
}

impl FsrsNextRequest {
    /// Reads parameter sets (`param_sets`, or the probe's `param-sets.json`):
    /// at least one, each named `^[a-z0-9][a-z0-9-]*$` with 0, 17, 19 or 21
    /// numbers read as `f32`.
    pub fn param_sets(value: &Value) -> Result<BTreeMap<String, Vec<f32>>, String> {
        let sets = value
            .as_object()
            .filter(|sets| !sets.is_empty())
            .ok_or("param_sets is an object with at least one parameter set")?;
        let mut param_sets = BTreeMap::new();
        for (name, values) in sets {
            if !param_set_name(name) {
                return Err(format!("parameter set name {name:?} must match ^[a-z0-9][a-z0-9-]*$"));
            }
            let values = values
                .as_array()
                .filter(|v| matches!(v.len(), 0 | 17 | 19 | 21))
                .and_then(|v| v.iter().map(json_f32).collect::<Option<Vec<f32>>>())
                .ok_or_else(|| format!("parameter set {name} is a list of 0, 17, 19 or 21 numbers"))?;
            param_sets.insert(name.clone(), values);
        }
        Ok(param_sets)
    }

    /// Reads the request with scenario.schema.json's rules and F5.1's: exactly
    /// `param_sets` and `inputs`; at least one parameter set, each named
    /// `^[a-z0-9][a-z0-9-]*$` with 0, 17, 19 or 21 numbers; 1 to 1,000 inputs,
    /// each naming a set of `param_sets`.
    pub fn from_mapping(map: &Map<String, Value>) -> Result<FsrsNextRequest, FormatError> {
        let bad = |why: &str| FormatError::new(format!("observation fsrs_next: {why}"));
        if map.len() != 2 || !map.contains_key("param_sets") || !map.contains_key("inputs") {
            return Err(bad("takes exactly {param_sets, inputs}"));
        }
        let param_sets = FsrsNextRequest::param_sets(&map["param_sets"]).map_err(|e| bad(&e))?;
        let items = map["inputs"]
            .as_array()
            .filter(|i| (1..=FSRS_NEXT_MAX_INPUTS).contains(&i.len()))
            .ok_or_else(|| bad("inputs is a list of 1 to 1,000 inputs"))?;
        let mut inputs = Vec::with_capacity(items.len());
        for (k, item) in items.iter().enumerate() {
            let input = FsrsNextInput::from_value(item).map_err(|e| bad(&format!("input {k}: {e}")))?;
            if !param_sets.contains_key(&input.params) {
                return Err(bad(&format!(
                    "input {k}: params {:?} is not in param_sets",
                    input.params
                )));
            }
            inputs.push(input);
        }
        Ok(FsrsNextRequest { param_sets, inputs })
    }
}

/// A `template` observation request (render-ops.md Q1.1-Q1.3), read with
/// the load-time checks of Q1.6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRequest {
    /// Present exactly when some case is a new-note case.
    pub notetype: Option<TemplateNotetype>,
    pub cases: Vec<TemplateCase>,
}

/// The note type of a request's new-note cases (Q1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateNotetype {
    pub name: String,
    pub kind: NotetypeKind,
    pub fields: Vec<String>,
}

/// One case of a `template` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateCase {
    /// Q1.2: a note and card not yet added.
    NewNote(NewNoteCase),
    /// Q1.3: a stored card with other template texts.
    Card { card: CardRef, front: String, back: String },
}

/// Q1.2, with the field values resolved to the note type's field order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNoteCase {
    pub front: String,
    pub back: String,
    pub ord: u32,
    /// The template name; `None` means `Card <ord + 1>`.
    pub name: Option<String>,
    /// One value per note type field, in field order; missing ones empty.
    pub fields: Vec<String>,
    pub tags: Vec<String>,
}

/// Q1.2: the largest card ordinal a new-note case takes.
pub const TEMPLATE_MAX_ORD: u64 = 499;

impl TemplateRequest {
    /// Reads the request with scenario.schema.json's rules and Q1.1's
    /// name rules, ord range and field checks.
    pub fn from_mapping(map: &Map<String, Value>) -> Result<TemplateRequest, FormatError> {
        let bad = |why: String| FormatError::new(format!("observation template: {why}"));
        if let Some(key) = map.keys().find(|k| !matches!(k.as_str(), "notetype" | "cases")) {
            return Err(bad(format!("takes {{notetype?, cases}}, not {key:?}")));
        }
        let cases = map
            .get("cases")
            .and_then(Value::as_array)
            .filter(|cases| !cases.is_empty())
            .ok_or_else(|| bad("cases is a non-empty list".to_owned()))?;
        let notetype = map
            .get("notetype")
            .map(TemplateNotetype::from_value)
            .transpose()
            .map_err(|e| bad(format!("notetype: {e}")))?;
        let has_new_note = cases.iter().any(|case| case.get("card").is_none());
        match (&notetype, has_new_note) {
            (None, true) => return Err(bad("a new-note case needs notetype".to_owned())),
            (Some(_), false) => {
                return Err(bad(
                    "notetype is only for new-note cases; every case names a card".to_owned()
                ));
            }
            _ => {}
        }
        let cases = cases
            .iter()
            .enumerate()
            .map(|(i, case)| {
                TemplateCase::from_value(case, notetype.as_ref()).map_err(|e| bad(format!("case {i}: {e}")))
            })
            .collect::<Result<_, _>>()?;
        Ok(TemplateRequest { notetype, cases })
    }
}

impl TemplateNotetype {
    fn from_value(value: &Value) -> Result<TemplateNotetype, String> {
        let map = value.as_object().ok_or("must be a mapping")?;
        if let Some(key) = map.keys().find(|k| !matches!(k.as_str(), "name" | "kind" | "fields")) {
            return Err(format!("takes {{name, kind?, fields}}, not {key:?}"));
        }
        let name = map.get("name").and_then(Value::as_str).ok_or("name is a string")?;
        if name.is_empty() || meguri_text::normalize::nfc(name) != name || name.contains('"') {
            return Err(format!("name {name:?} must be non-empty, NFC and without \""));
        }
        let kind = match map.get("kind").map(|k| k.as_str()) {
            None | Some(Some("standard")) => NotetypeKind::Standard,
            Some(Some("cloze")) => NotetypeKind::Cloze,
            Some(_) => return Err("kind is standard or cloze".to_owned()),
        };
        let fields = map
            .get("fields")
            .and_then(Value::as_array)
            .filter(|fields| !fields.is_empty())
            .ok_or("fields is a non-empty list of field names")?;
        let mut names: Vec<String> = Vec::with_capacity(fields.len());
        for field in fields {
            let field = field.as_str().ok_or("fields are strings")?;
            if let Some(problem) = field_name_problem(field) {
                return Err(format!("field {field:?} {problem}"));
            }
            if let Some(twin) = names
                .iter()
                .find(|n| meguri_text::fold::compare_folded(n, field) == std::cmp::Ordering::Equal)
            {
                return Err(format!(
                    "fields {twin:?} and {field:?} are the same name to Anki (unicase)"
                ));
            }
            names.push(field.to_owned());
        }
        Ok(TemplateNotetype {
            name: name.to_owned(),
            kind,
            fields: names,
        })
    }
}

/// Q1.1: why a note type save would change or refuse this field name.
fn field_name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("is empty")
    } else if meguri_text::normalize::nfc(name) != name {
        Some("is not NFC")
    } else if name.contains([':', '{', '}', '"']) {
        Some("holds one of : { } \"")
    } else if name.starts_with(['#', '/', '^']) || name.starts_with(char::is_whitespace) {
        Some("starts with #, /, ^ or white space")
    } else if name.ends_with(char::is_whitespace) {
        Some("ends with white space")
    } else {
        None
    }
}

impl TemplateCase {
    fn from_value(value: &Value, notetype: Option<&TemplateNotetype>) -> Result<TemplateCase, String> {
        let map = value.as_object().ok_or("must be a mapping")?;
        let text = |key: &str, required: bool| match map.get(key) {
            Some(Value::String(s)) => Ok(s.clone()),
            None if !required => Ok(String::new()),
            None => Err(format!("{key} is required")),
            Some(_) => Err(format!("{key} is a string")),
        };
        if let Some(card) = map.get("card") {
            if let Some(key) = map.keys().find(|k| !matches!(k.as_str(), "card" | "front" | "back")) {
                return Err(format!("a card case takes {{card, front, back?}}, not {key:?}"));
            }
            let card: LogicalRef = card
                .as_str()
                .ok_or("card is a #cN reference")?
                .parse()
                .map_err(|e: FormatError| e.to_string())?;
            if card.kind != RefKind::Card {
                return Err(format!("{card} is not a card reference"));
            }
            return Ok(TemplateCase::Card {
                card: CardRef(card),
                front: text("front", true)?,
                back: text("back", false)?,
            });
        }
        let notetype = notetype.ok_or("a new-note case needs notetype")?;
        if let Some(key) = map
            .keys()
            .find(|k| !matches!(k.as_str(), "front" | "back" | "ord" | "name" | "fields" | "tags"))
        {
            return Err(format!(
                "a new-note case takes {{front, back?, ord?, name?, fields?, tags?}}, not {key:?}"
            ));
        }
        let ord = match map.get("ord") {
            None => 0,
            Some(ord) => ord
                .as_u64()
                .filter(|&ord| ord <= TEMPLATE_MAX_ORD)
                .and_then(|ord| u32::try_from(ord).ok())
                .ok_or_else(|| format!("ord is an integer from 0 to {TEMPLATE_MAX_ORD}"))?,
        };
        let name = match map.get("name") {
            None => None,
            Some(Value::String(name)) if !name.is_empty() => Some(name.clone()),
            Some(_) => return Err("name is a non-empty string".to_owned()),
        };
        let mut fields = vec![String::new(); notetype.fields.len()];
        match map.get("fields") {
            None => {}
            Some(Value::Array(values)) => {
                if values.len() > fields.len() {
                    return Err(format!("{} field values for {} fields", values.len(), fields.len()));
                }
                for (slot, value) in fields.iter_mut().zip(values) {
                    *slot = value.as_str().ok_or("field values are strings")?.to_owned();
                }
            }
            Some(Value::Object(values)) => {
                for (name, value) in values {
                    let ord =
                        notetype.fields.iter().position(|f| f == name).ok_or_else(|| {
                            format!("fields names {name:?}, which is not a field of {:?}", notetype.name)
                        })?;
                    fields[ord] = value.as_str().ok_or("field values are strings")?.to_owned();
                }
            }
            Some(_) => return Err("fields is a list of values or a mapping of field name to value".to_owned()),
        }
        let tags = match map.get("tags") {
            None => Vec::new(),
            Some(tags) => tags
                .as_array()
                .and_then(|tags| {
                    tags.iter()
                        .map(|t| t.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or("tags is a list of strings")?,
        };
        Ok(TemplateCase::NewNote(NewNoteCase {
            front: text("front", true)?,
            back: text("back", false)?,
            ord,
            name,
            fields,
            tags,
        }))
    }
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

/// The new collection a scenario starts from as a capability (D8), listed
/// with the setup sections.
pub const NEW_COLLECTION: &str = "new_collection";

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

    /// The setup sections, ops and observation kinds the scenario uses, and
    /// the new collection it starts from (D8).
    pub fn needs(&self) -> Needs {
        let mut needs = Needs::default();
        if self.starts_from_new_collection() {
            needs.setup.insert(NEW_COLLECTION.to_owned());
        }
        needs.setup.extend(self.setup.sections().into_iter().map(str::to_owned));
        add_step_needs(&mut needs, &self.steps);
        needs
    }

    /// Whether a run starts from a new collection made at `clock.start`
    /// (D8): unless its first step is `open`, or it has no setup and no step
    /// touches a collection (only `advance`, `set_clock` and observations of
    /// kinds that never read one).
    pub fn starts_from_new_collection(&self) -> bool {
        if self.starts_with_open() {
            return false;
        }
        !self.setup.sections().is_empty() || self.steps.iter().any(touches_collection)
    }

    /// storage-ops.md P1: the scenario replaces its collection with a fixture
    /// before anything else.
    pub fn starts_with_open(&self) -> bool {
        matches!(self.steps.first(), Some(Step::Open(_)))
    }

    /// A live scenario has handoff steps (D9.2) or actors (D9.3): only T1
    /// runs it.
    pub fn is_live(&self) -> bool {
        self.has_actors() || self.has_handoffs()
    }

    /// An actor scenario (sync-ops.md S1).
    pub fn has_actors(&self) -> bool {
        self.actors.is_some()
    }

    /// The declared actors, in order; none outside actor scenarios.
    pub fn actors(&self) -> &[ActorName] {
        self.actors.as_deref().unwrap_or_default()
    }

    /// For each step, the actor that runs it (sync-ops.md S2.3): the current
    /// actor, which is the first actor until an `actor` step names another;
    /// `None` for global steps and in scenarios without actors.
    pub fn step_actors(&self) -> Vec<Option<&ActorName>> {
        let mut current = self.actors().first();
        self.steps
            .iter()
            .map(|step| {
                if let Step::Actor(name) = step {
                    current = self.actors().iter().find(|a| *a == name).or(current);
                }
                if step.is_global() { None } else { current }
            })
            .collect()
    }

    /// What Meguri needs to play the Meguri actors of an actor scenario
    /// (sync-ops.md S10.2): the op `actor`; the new collection each of them
    /// starts from (S2.2, D8); the setup sections when the first actor is a
    /// Meguri actor; the ops and observation kinds of the steps Meguri actors
    /// run; `advance` and `set_clock` when the scenario uses them; `db_dump`
    /// for `converge`. Nothing without a Meguri actor.
    pub fn needs_of_meguri_actors(&self) -> Needs {
        let mut needs = Needs::default();
        if !self.actors().iter().any(ActorName::is_meguri) {
            return needs;
        }
        needs.ops.insert("actor".to_owned());
        needs.setup.insert(NEW_COLLECTION.to_owned());
        if self.actors()[0].is_meguri() {
            needs.setup.extend(self.setup.sections().into_iter().map(str::to_owned));
        }
        for (step, by) in self.steps.iter().zip(self.step_actors()) {
            match step {
                Step::Advance(_) | Step::SetClock(_) => {
                    needs.ops.insert(step.name().to_owned());
                }
                Step::Converge(_) => {
                    needs.observations.insert(ObservationKind::DbDump.name().to_owned());
                }
                _ if by.is_some_and(ActorName::is_meguri) => add_step_needs(&mut needs, std::slice::from_ref(step)),
                _ => {}
            }
        }
        needs
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

    /// What one side needs to run its segments of a live scenario: the new
    /// collection (unless the first step is `open`) and the setup sections
    /// when it runs the first segment (D9.2), the ops and observations of its
    /// segments, and `handoff`.
    pub fn needs_on(&self, side: Side) -> Needs {
        let mut needs = Needs::default();
        for (i, segment) in self.segments().iter().enumerate().filter(|(_, s)| s.side == side) {
            if i == 0 {
                if !self.starts_with_open() {
                    needs.setup.insert(NEW_COLLECTION.to_owned());
                }
                needs.setup.extend(self.setup.sections().into_iter().map(str::to_owned));
            }
            add_step_needs(&mut needs, &self.steps[segment.steps.clone()]);
        }
        if self.has_handoffs() {
            needs.ops.insert("handoff".to_owned());
        }
        needs
    }

    /// Whether a step opens a fixture (storage-ops.md P1.3): the scenario
    /// then has no setup and starts with no collection.
    pub fn opens_fixtures(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Open(_)))
    }

    /// A live chain: handoff steps split it into segments (D9.2).
    pub fn has_handoffs(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Handoff(_)))
    }
}

/// D8: a step that reads or writes the scenario's collection. Only `advance`,
/// `set_clock` and observations of kinds that never read a collection leave
/// it alone.
fn touches_collection(step: &Step) -> bool {
    match step {
        Step::Advance(_) | Step::SetClock(_) => false,
        Step::Observe(requests) => requests.iter().any(|r| !r.reads_no_collection()),
        _ => true,
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
            // notetype-ops.md T10.1: the op and a capability per kind of edit.
            Step::UpdateNotetype(args) => {
                needs.ops.insert(step.name().to_owned());
                needs.ops.extend(args.edits.iter().map(NotetypeEdit::capability));
            }
            other => {
                needs.ops.insert(other.name().to_owned());
            }
        }
    }
}
