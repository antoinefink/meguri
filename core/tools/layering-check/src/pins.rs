//! Parity pins (KTD2, R2027): `core/pins.toml` against the workspace
//! dependencies in `core/Cargo.toml` and the resolved graph in `core/Cargo.lock`.

use std::path::Path;

use serde::Deserialize;

use crate::{Error, Rule, Violation};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PinsFile {
    source: Source,
    pin: Vec<Pin>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    oracle: String,
    #[allow(dead_code)] // provenance for readers of pins.toml
    lockfile: String,
    rule: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    #[serde(rename = "crate")]
    name: String,
    version: Option<String>,
    via: Option<String>,
    #[serde(default)]
    pending: bool,
    #[allow(dead_code)] // provenance for readers of pins.toml
    why: String,
}

#[derive(Debug, Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<LockPackage>,
}

#[derive(Debug, Deserialize)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

/// A `[workspace.dependencies]` entry.
struct WorkspaceDep {
    key: String,
    package: String,
    requirement: Option<String>,
}

/// The outcome of a pin check.
#[derive(Debug)]
pub struct PinReport {
    pub violations: Vec<Violation>,
    /// Parity crates whose oracle version is not recorded yet, in file order.
    pub pending: Vec<String>,
}

impl PinReport {
    /// One line per violation, then the pending pins.
    pub fn render(&self) -> String {
        let mut text: String = self.violations.iter().map(|v| format!("{v}\n")).collect();
        if !self.pending.is_empty() {
            text.push_str(&format!(
                "pending pins (no oracle version recorded yet): {}\n",
                self.pending.join(", ")
            ));
        }
        text
    }
}

/// Checks `pins.toml`, `Cargo.toml` and `Cargo.lock` in the core directory.
pub fn check_workspace(core_dir: &Path) -> Result<PinReport, Error> {
    let read = |name: &str| {
        let path = core_dir.join(name);
        std::fs::read_to_string(&path).map_err(|e| Error::new(format!("reading {}: {e}", path.display())))
    };
    check(&read("pins.toml")?, &read("Cargo.toml")?, &read("Cargo.lock")?)
}

/// Checks the text of `pins.toml` against the workspace manifest and lockfile.
pub fn check(pins_toml: &str, manifest_toml: &str, lock_toml: &str) -> Result<PinReport, Error> {
    let pins: PinsFile = toml::from_str(pins_toml).map_err(|e| Error::new(format!("pins.toml: {e}")))?;
    for pin in &pins.pin {
        if pin.version.is_some() == pin.pending {
            return Err(Error::new(format!(
                "pins.toml: the pin for {} needs exactly one of `version` or `pending = true`",
                pin.name
            )));
        }
        if pin.via.is_some() && pin.version.is_none() {
            return Err(Error::new(format!(
                "pins.toml: the pin for {} has `via` but no `version`",
                pin.name
            )));
        }
    }
    let manifest: toml::Table = manifest_toml
        .parse()
        .map_err(|e| Error::new(format!("Cargo.toml: {e}")))?;
    let lock: Lock = toml::from_str(lock_toml).map_err(|e| Error::new(format!("Cargo.lock: {e}")))?;
    let deps = workspace_deps(&manifest);

    let from = format!("from the {} lockfile", pins.source.oracle);
    let rule = &pins.source.rule;
    let mut violations = Vec::new();
    let mut pending = Vec::new();

    for pin in &pins.pin {
        let name = &pin.name;
        let Some(version) = &pin.version else {
            pending.push(name.clone());
            pending_uses(name, &deps, &lock, &mut violations);
            continue;
        };
        let via = pin.via.as_ref().map(|v| format!(" (via {v})")).unwrap_or_default();

        if pin.via.is_none() && !deps.iter().any(|d| &d.package == name) {
            violations.push(Violation::new(
                Rule::Pin,
                name,
                format!(
                    "{name} is pinned to {version} but core/Cargo.toml [workspace.dependencies] has no entry \
                     for it; add `{name} = \"={version}\"`"
                ),
            ));
        }

        let locked: Vec<&str> = lock
            .package
            .iter()
            .filter(|p| &p.name == name)
            .map(|p| p.version.as_str())
            .collect();
        if !locked.contains(&version.as_str()) {
            let message = if locked.is_empty() {
                format!(
                    "Cargo.lock has no {name}, but pins.toml pins {version}{via} {from}; the crate that owns \
                     the pin must depend on it"
                )
            } else {
                format!(
                    "Cargo.lock has {name} {}, but pins.toml pins {version}{via} {from}. {rule}",
                    locked.join(", ")
                )
            };
            violations.push(Violation::new(Rule::Pin, name, message));
        }
    }

    // Every workspace requirement on a pinned crate is exactly one of its pins.
    for dep in &deps {
        let pinned: Vec<&str> = pins
            .pin
            .iter()
            .filter(|p| p.name == dep.package)
            .filter_map(|p| p.version.as_deref())
            .collect();
        if pinned.is_empty() {
            continue;
        }
        let exact: Vec<String> = pinned.iter().map(|v| format!("={v}")).collect();
        if dep.requirement.as_ref().is_some_and(|r| exact.contains(r)) {
            continue;
        }
        let requires = dep.requirement.as_deref().unwrap_or("no version");
        let key = if dep.key == dep.package {
            String::new()
        } else {
            format!(" (as {})", dep.key)
        };
        violations.push(Violation::new(
            Rule::Pin,
            &dep.package,
            format!(
                "{}{key}: core/Cargo.toml [workspace.dependencies] requires {requires}, but pins.toml pins {} \
                 {from}; the requirement must be exactly {}. {rule}",
                dep.package,
                pinned.join(" and "),
                exact
                    .iter()
                    .map(|e| format!("\"{e}\""))
                    .collect::<Vec<_>>()
                    .join(" or "),
            ),
        ));
    }

    Ok(PinReport { violations, pending })
}

fn pending_uses(name: &str, deps: &[WorkspaceDep], lock: &Lock, out: &mut Vec<Violation>) {
    let why = "the spec role records the oracle version before any crate depends on it";
    for dep in deps.iter().filter(|d| d.package == name) {
        out.push(Violation::new(
            Rule::PendingPin,
            name,
            format!(
                "{name} has no oracle version in pins.toml yet, but core/Cargo.toml [workspace.dependencies] \
                 declares it as `{}`; {why}",
                dep.key
            ),
        ));
    }
    // Workspace and other path crates have no `source` in Cargo.lock.
    for member in lock.package.iter().filter(|p| p.source.is_none()) {
        if member
            .dependencies
            .iter()
            .any(|d| d.split_whitespace().next() == Some(name))
        {
            out.push(Violation::new(
                Rule::PendingPin,
                name,
                format!(
                    "{} depends on {name}, which has no oracle version in pins.toml yet; {why}",
                    member.name
                ),
            ));
        }
    }
}

fn workspace_deps(manifest: &toml::Table) -> Vec<WorkspaceDep> {
    let Some(table) = manifest
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table)
    else {
        return Vec::new();
    };
    table
        .iter()
        .map(|(key, value)| match value {
            toml::Value::String(requirement) => WorkspaceDep {
                key: key.clone(),
                package: key.clone(),
                requirement: Some(requirement.clone()),
            },
            other => WorkspaceDep {
                key: key.clone(),
                package: other
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key)
                    .to_owned(),
                requirement: other.get("version").and_then(toml::Value::as_str).map(str::to_owned),
            },
        })
        .collect()
}
