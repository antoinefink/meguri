//! The parts of `cargo metadata` the layering checks read.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::Error;

#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub workspace_root: PathBuf,
    pub resolve: Option<Resolve>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct Resolve {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
pub struct Node {
    pub id: String,
    pub deps: Vec<NodeDep>,
}

#[derive(Debug, Deserialize)]
pub struct NodeDep {
    pub pkg: String,
    pub dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Deserialize)]
pub struct DepKind {
    /// `None` for a normal dependency, else `"dev"` or `"build"`.
    pub kind: Option<String>,
}

/// Runs `cargo metadata` with every feature on, so optional and
/// platform-specific dependencies are part of the graph.
pub fn load(manifest_path: &Path) -> Result<Metadata, Error> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(&cargo)
        .args(["metadata", "--format-version", "1", "--all-features", "--manifest-path"])
        .arg(manifest_path)
        .output()
        .map_err(|e| Error::new(format!("running {}: {e}", cargo.to_string_lossy())))?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "cargo metadata failed for {}:\n{}",
            manifest_path.display(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| Error::new(format!("reading cargo metadata: {e}")))
}
