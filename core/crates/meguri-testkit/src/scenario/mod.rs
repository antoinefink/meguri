//! Scenario files: parsing (YAML 1.2 core schema subset or JSON), the scenario
//! hash and the typed model (specs/conformance/dsl.md D1-D7).

mod card_extractors;
mod image_occlusion;
mod intake;
mod model;
mod package;
mod type_answer;
mod validate;
mod yaml;

use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

pub use card_extractors::{CardExtractorInput, CardExtractorsRequest, ExtractorValue};
pub use image_occlusion::ImageOcclusionRequest;
pub use model::*;
pub use package::*;
pub use type_answer::{ReviewHtmlCase, TypeAnswerFunction, TypeAnswerRequest};
pub use validate::draft_requirements;

pub use crate::observe::jcs;

/// A file that is not a scenario in the supported format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    message: String,
}

impl FormatError {
    pub fn new(message: impl Into<String>) -> FormatError {
        FormatError {
            message: message.into(),
        }
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FormatError {}

/// Suffixes of scenario files.
pub const SUFFIXES: [&str; 3] = [".yaml", ".yml", ".json"];

/// Parses authored YAML into its JSON value (D2).
pub fn parse_yaml(text: &str) -> Result<Value, FormatError> {
    yaml::parse(text)
}

/// Parses a generated JSON scenario; duplicate keys are rejected.
pub fn parse_json(text: &str) -> Result<Value, FormatError> {
    yaml::parse_json(text)
}

/// Parses golden or overlay JSON for conversion (observations.md O6), retaining
/// signed/unsigned 64-bit integers and float kind, and rejecting duplicate keys.
/// Authored scenarios must use `parse_json` or `load` to enforce D2's range.
pub fn parse_observation_json(text: &str) -> Result<Value, FormatError> {
    yaml::parse_observation_json(text)
}

/// The lower-case hex SHA-256 of the scenario value's JCS text (D3).
pub fn scenario_sha256(value: &Value) -> Result<String, FormatError> {
    let text = jcs(value)?;
    Ok(hex(&Sha256::digest(text.as_bytes())))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A parsed scenario file.
#[derive(Debug, Clone)]
pub struct LoadedScenario {
    pub path: PathBuf,
    /// The parsed value: what is hashed and what the oracle receives.
    pub value: Value,
    pub sha256: String,
    pub scenario: Scenario,
}

/// Reads, parses and hashes a scenario file and reads its typed model.
pub fn load(path: &Path) -> Result<LoadedScenario, FormatError> {
    let text =
        std::fs::read_to_string(path).map_err(|e| FormatError::new(format!("reading {}: {e}", path.display())))?;
    let value = if path.extension().is_some_and(|e| e == "json") {
        parse_json(&text)?
    } else {
        parse_yaml(&text)?
    };
    let sha256 = scenario_sha256(&value)?;
    let scenario = Scenario::from_value(&value)?;
    Ok(LoadedScenario {
        path: path.to_path_buf(),
        value,
        sha256,
        scenario,
    })
}

/// Checks that the scenario's id is its path without suffix (D1).
pub fn check_id(loaded: &LoadedScenario, rel: &str) -> Result<(), FormatError> {
    let id = &loaded.scenario.id;
    let well_formed = id.split('/').count() >= 2
        && id.split('/').all(|seg| {
            seg.bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        });
    if !well_formed {
        return Err(FormatError::new(format!(
            "id {id:?} must be lower-case segments of a-z, 0-9, '.', '_' and '-' joined by '/'"
        )));
    }
    if id != rel {
        return Err(FormatError::new(format!(
            "id {id:?} does not match the file path {rel:?}"
        )));
    }
    Ok(())
}
