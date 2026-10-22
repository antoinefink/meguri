//! Prepared package contracts: parsing and load-time rules, no execution.
//! conformance/package-ops E1-E3, backup-ops F2-F7, backup-trigger-ops H1.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{Duration, FormatError, Scenario, Step};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AddBackupFilesArgs {
    Names(Vec<String>),
    #[serde(deserialize_with = "super::model::op_mapping")]
    Series(BackupFileSeries),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupFileSeries {
    pub from: String,
    pub until: String,
    #[serde(serialize_with = "duration_text")]
    pub every: Duration,
}

fn duration_text<S: serde::Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{}s", duration.seconds()))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForceBackupArgs {
    #[serde(
        default,
        deserialize_with = "super::model::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub copy_as: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SaveBackupSettingsArgs {
    pub daily: u32,
    pub weekly: u32,
    pub monthly: u32,
    pub minimum_interval_mins: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupArgs {
    pub force: bool,
}

/// Exactly one session file or fixture, shared by listing and colpkg import.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageListingRequest {
    Name(String),
    Fixture(String),
}

impl PackageListingRequest {
    pub fn from_mapping(map: &Map<String, Value>) -> Result<Self, FormatError> {
        let request = Self::deserialize(Value::Object(map.clone()))
            .map_err(|e| FormatError::new(format!("package_listing: {e}")))?;
        let valid = match &request {
            Self::Name(name) => package_name(name, &[".colpkg", ".apkg"]),
            Self::Fixture(path) => package_fixture(path),
        };
        if !valid {
            return Err(FormatError::new("invalid package name or fixture path"));
        }
        Ok(request)
    }
}

pub(super) fn package_name(text: &str, extensions: &[&str]) -> bool {
    extensions.iter().any(|extension| {
        text.strip_suffix(extension).is_some_and(|stem| {
            !stem.is_empty()
                && stem.as_bytes()[0].is_ascii_alphanumeric()
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
    })
}

fn package_fixture(text: &str) -> bool {
    (text.ends_with(".colpkg") || text.ends_with(".apkg"))
        && text.split('/').all(|segment| {
            segment
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionPackageImportArgs {
    pub source: PackageListingRequest,
}

impl CollectionPackageImportArgs {
    pub fn from_mapping(args: &Map<String, Value>) -> Result<Self, FormatError> {
        if args.get("format").and_then(Value::as_str) != Some("colpkg") {
            return Err(FormatError::new("collection package import needs format: colpkg"));
        }
        let mut source = args.clone();
        source.remove("format");
        let request = PackageListingRequest::from_mapping(&source)?;
        let path = match &request {
            PackageListingRequest::Name(name) | PackageListingRequest::Fixture(name) => name,
        };
        if !path.ends_with(".colpkg") {
            return Err(FormatError::new("collection package import needs a .colpkg file"));
        }
        Ok(Self { source: request })
    }
}

impl Serialize for CollectionPackageImportArgs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("format", "colpkg")?;
        match &self.source {
            PackageListingRequest::Name(name) => map.serialize_entry("name", name)?,
            PackageListingRequest::Fixture(path) => map.serialize_entry("fixture", path)?,
        }
        map.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionPackageExportArgs {
    pub name: String,
}

impl CollectionPackageExportArgs {
    pub fn from_mapping(args: &Map<String, Value>) -> Result<Self, FormatError> {
        if args.len() != 2
            || args.get("format").and_then(Value::as_str) != Some("colpkg")
            || !args
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| package_name(n, &[".colpkg"]))
        {
            return Err(FormatError::new(
                "collection package export is exactly {format: colpkg, name: <colpkg file name>}",
            ));
        }
        Ok(Self {
            name: args["name"].as_str().expect("checked package name").to_owned(),
        })
    }
}

impl Serialize for CollectionPackageExportArgs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("format", "colpkg")?;
        map.serialize_entry("name", &self.name)?;
        map.end()
    }
}

/// Interpret series times as plain calendar seconds, with no zone or DST.
fn wall_seconds(text: &str) -> Result<i64, FormatError> {
    let checked: super::DateTimeText = text.parse()?;
    if checked.as_str().len() != 19 {
        return Err(FormatError::new("backup series wall times have no offset"));
    }
    text.parse::<jiff::civil::DateTime>()
        .map_err(|_| FormatError::new("invalid backup series wall time"))?;
    format!("{text}Z")
        .parse::<jiff::Timestamp>()
        .map(|t| t.as_second())
        .map_err(|_| FormatError::new("invalid backup series wall time"))
}

fn backup_files(args: &AddBackupFilesArgs) -> Result<(), FormatError> {
    let valid = match args {
        AddBackupFilesArgs::Names(names) => {
            let mut seen = BTreeSet::new();
            (1..=20_000).contains(&names.len())
                && names.iter().all(|name| {
                    super::validate::media_component(name) && !name.starts_with(".meguri-") && seen.insert(name)
                })
        }
        AddBackupFilesArgs::Series(series) => {
            let from = wall_seconds(&series.from)?;
            let until = wall_seconds(&series.until)?;
            let every = series.every.seconds();
            every >= 1 && until >= from && (until - from) / every < 20_000
        }
    };
    if valid {
        Ok(())
    } else {
        Err(FormatError::new("invalid backup file names or series (F2)"))
    }
}

pub(super) fn scenario(s: &Scenario, raw: &Value) -> Result<(), FormatError> {
    let mut forced = false;
    let mut imports = false;
    for step in &s.steps {
        match step {
            Step::Open(_) | Step::Reopen(_) | Step::Import(_) | Step::Handoff(_) => forced = false,
            Step::ForceBackup(args) => {
                if forced {
                    return Err(FormatError::new("at most one force_backup per collection opening (F7)"));
                }
                forced = true;
                if args
                    .copy_as
                    .as_ref()
                    .is_some_and(|name| !package_name(name, &[".colpkg"]))
                {
                    return Err(FormatError::new("force_backup.copy_as needs a .colpkg file name"));
                }
            }
            Step::AddBackupFiles(args) => backup_files(args)?,
            _ => {}
        }
        if let Step::Import(args) = step {
            imports |= args.0.get("format").and_then(Value::as_str) == Some("colpkg");
        }
    }
    if imports {
        let logical_ref = regex::Regex::new(r"#[nc][1-9][0-9]*").expect("logical reference pattern");
        fn names_ref(value: &Value, pattern: &regex::Regex) -> bool {
            match value {
                Value::String(text) => pattern.is_match(text),
                Value::Array(items) => items.iter().any(|v| names_ref(v, pattern)),
                Value::Object(map) => map.iter().any(|(k, v)| pattern.is_match(k) || names_ref(v, pattern)),
                _ => false,
            }
        }
        if raw.get("setup").is_some()
            || !matches!(s.steps.first(), Some(Step::Open(_)))
            || s.steps.iter().any(|step| matches!(step, Step::AddNote(_)))
            || names_ref(raw, &logical_ref)
        {
            return Err(FormatError::new(
                "a collection package import starts with open, has no setup, add_note or logical ids (E2.4)",
            ));
        }
    }
    Ok(())
}
