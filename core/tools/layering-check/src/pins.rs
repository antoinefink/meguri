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

    // Every workspace requirement on a pinned crate is exactly one of its
    // direct pins: a `via` pin is for the crate that brings it, never for ours.
    for dep in &deps {
        let Some(versions) = Versions::of(&pins.pin, &dep.package) else {
            continue;
        };
        let name = &dep.package;
        let exact: Vec<String> = versions.direct.iter().map(|v| format!("={v}")).collect();
        if dep.requirement.as_ref().is_some_and(|r| exact.contains(r)) {
            continue;
        }
        let requires = dep.requirement.as_deref().unwrap_or("no version");
        let key = if &dep.key == name {
            String::new()
        } else {
            format!(" (as {})", dep.key)
        };
        let message = if exact.is_empty() {
            format!(
                "{name}{key}: core/Cargo.toml [workspace.dependencies] requires {requires}, but pins.toml pins \
                 {name} only as {}; remove it. {rule}",
                versions.via_text()
            )
        } else {
            format!(
                "{name}{key}: core/Cargo.toml [workspace.dependencies] requires {requires}, but pins.toml pins {} \
                 {from}{}; the requirement must be exactly {}. {rule}",
                versions.direct.join(" and "),
                versions.via_note(name, requires.trim_start_matches('=')),
                exact
                    .iter()
                    .map(|e| format!("\"{e}\""))
                    .collect::<Vec<_>>()
                    .join(" or "),
            )
        };
        violations.push(Violation::new(Rule::Pin, name, message));
    }

    // Every workspace crate resolves each pinned crate it uses to a direct pin,
    // however its own manifest declares the dependency. Workspace and other
    // path crates have no `source` in Cargo.lock.
    for member in lock.package.iter().filter(|p| p.source.is_none()) {
        for entry in &member.dependencies {
            let name = entry.split_whitespace().next().unwrap_or_default();
            let Some(versions) = Versions::of(&pins.pin, name) else {
                continue;
            };
            let Some(version) = resolve(&lock, entry)? else {
                continue; // no locked package of that name: reported above
            };
            if versions.direct.contains(&version) {
                continue;
            }
            let message = if versions.direct.is_empty() {
                format!(
                    "{} depends on {name} {version}, but pins.toml pins {name} only as {}; no workspace crate \
                     may depend on it directly. {rule}",
                    member.name,
                    versions.via_text()
                )
            } else {
                format!(
                    "{} depends on {name} {version}, but pins.toml pins {} {from}{}; depend on it with \
                     `{name}.workspace = true`. {rule}",
                    member.name,
                    versions.direct.join(" and "),
                    versions.via_note(name, version),
                )
            };
            violations.push(Violation::new(Rule::Pin, name, message));
        }
    }

    Ok(PinReport { violations, pending })
}

/// The recorded versions of one pinned crate.
struct Versions<'a> {
    /// Versions workspace crates use.
    direct: Vec<&'a str>,
    /// Versions only another crate brings: (version, that crate).
    via: Vec<(&'a str, &'a str)>,
}

impl<'a> Versions<'a> {
    /// `None` when `name` has no pin with a version.
    fn of(pins: &'a [Pin], name: &str) -> Option<Versions<'a>> {
        let mut versions = Versions {
            direct: Vec::new(),
            via: Vec::new(),
        };
        for pin in pins.iter().filter(|p| p.name == name) {
            match (pin.version.as_deref(), pin.via.as_deref()) {
                (Some(version), None) => versions.direct.push(version),
                (Some(version), Some(via)) => versions.via.push((version, via)),
                (None, _) => {}
            }
        }
        (!versions.direct.is_empty() || !versions.via.is_empty()).then_some(versions)
    }

    /// ", and <name> <version> only as the version <via> brings" when
    /// `version` is a `via` pin, else empty.
    fn via_note(&self, name: &str, version: &str) -> String {
        self.via
            .iter()
            .find(|(v, _)| *v == version)
            .map(|(v, via)| format!(", and {name} {v} only as the version {via} brings"))
            .unwrap_or_default()
    }

    /// "the version <via> brings (<version>)" for each `via` pin.
    fn via_text(&self) -> String {
        self.via
            .iter()
            .map(|(version, via)| format!("the version {via} brings ({version})"))
            .collect::<Vec<_>>()
            .join(" and ")
    }
}

/// The version a Cargo.lock dependency entry resolves to. Cargo writes
/// `name` when the lock holds one version of the crate, else `name version`
/// (plus the source when even that is ambiguous). `None` when no locked
/// package has the name.
fn resolve<'a>(lock: &'a Lock, entry: &'a str) -> Result<Option<&'a str>, Error> {
    let mut parts = entry.split_whitespace();
    let name = parts.next().unwrap_or_default();
    if let Some(version) = parts.next() {
        return Ok(Some(version));
    }
    let mut locked = lock.package.iter().filter(|p| p.name == name);
    match (locked.next(), locked.next()) {
        (None, _) => Ok(None),
        (Some(package), None) => Ok(Some(package.version.as_str())),
        (Some(_), Some(_)) => Err(Error::new(format!(
            "Cargo.lock: the dependency entry \"{entry}\" names no version, but the lock holds several versions of \
             {name}"
        ))),
    }
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
