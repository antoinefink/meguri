//! `edges.toml`: the KTD2 allowed-edge table as data.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use crate::Error;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgesFile {
    row: Vec<RowEntry>,
    any: AnyEntry,
    isolated: IsolatedEntry,
    #[serde(default)]
    restricted: BTreeMap<String, Restricted>,
    raw_open: RawOpen,
    #[serde(default)]
    storage_builder: Option<StorageBuilder>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RowEntry {
    row: u32,
    crates: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)] // documentation for readers of edges.toml
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnyEntry {
    crates: Vec<String>,
    #[serde(default)]
    except: Vec<String>,
    #[serde(default)]
    row_dev_dependencies: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IsolatedEntry {
    crates: Vec<String>,
}

/// A third-party crate that only the `allowed` workspace crates may use.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restricted {
    pub allowed: Vec<String>,
}

/// The engine's raw open entry point and the crates that may name it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawOpen {
    pub ident: String,
    pub allowed: Vec<String>,
}

/// The storage-only builder and the crates that must not name it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageBuilder {
    pub ident: String,
    pub denied: Vec<String>,
}

/// How a crate depends on another. An edge declared as several kinds counts
/// as the first of normal, build and dev.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Normal,
    Build,
    Dev,
}

impl EdgeKind {
    fn suffix(self) -> &'static str {
        match self {
            EdgeKind::Normal => "",
            EdgeKind::Build => " as a build-dependency",
            EdgeKind::Dev => " as a dev-dependency",
        }
    }
}

/// Where edges.toml places a workspace crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Row(u32),
    Any,
    Isolated,
}

/// The parsed and validated table.
#[derive(Debug)]
pub struct Edges {
    places: BTreeMap<String, Place>,
    any_except: BTreeSet<String>,
    /// `[any]` crates that row crates may use as dev-dependencies.
    row_dev: BTreeSet<String>,
    pub restricted: BTreeMap<String, Restricted>,
    pub raw_open: RawOpen,
    pub storage_builder: Option<StorageBuilder>,
}

impl Edges {
    pub fn load(path: &Path) -> Result<Edges, Error> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::new(format!("reading {}: {e}", path.display())))?;
        Edges::parse(&text).map_err(|e| Error::new(format!("{}: {e}", path.display())))
    }

    pub fn parse(text: &str) -> Result<Edges, Error> {
        let file: EdgesFile = toml::from_str(text).map_err(|e| Error::new(e.to_string()))?;
        let mut places = BTreeMap::new();
        let mut place = |name: &str, at: Place| {
            if places.insert(name.to_owned(), at).is_some() {
                return Err(Error::new(format!("{name} is placed twice")));
            }
            Ok(())
        };
        let mut rows = BTreeSet::new();
        for entry in &file.row {
            if !rows.insert(entry.row) {
                return Err(Error::new(format!("row {} appears twice", entry.row)));
            }
            for name in &entry.crates {
                place(name, Place::Row(entry.row))?;
            }
        }
        for name in &file.any.crates {
            place(name, Place::Any)?;
        }
        for name in &file.isolated.crates {
            place(name, Place::Isolated)?;
        }
        for name in &file.any.row_dev_dependencies {
            if !file.any.crates.contains(name) {
                return Err(Error::new(format!(
                    "[any] row_dev_dependencies names {name}, which is not an [any] crate"
                )));
            }
        }
        Ok(Edges {
            places,
            any_except: file.any.except.into_iter().collect(),
            row_dev: file.any.row_dev_dependencies.into_iter().collect(),
            restricted: file.restricted,
            raw_open: file.raw_open,
            storage_builder: file.storage_builder,
        })
    }

    pub fn place(&self, name: &str) -> Option<Place> {
        self.places.get(name).copied()
    }

    /// Every crate name the table mentions anywhere.
    pub fn names(&self) -> BTreeSet<&str> {
        let mut names: BTreeSet<&str> = self.places.keys().map(String::as_str).collect();
        names.extend(self.any_except.iter().map(String::as_str));
        names.extend(
            self.restricted
                .values()
                .flat_map(|r| r.allowed.iter().map(String::as_str)),
        );
        names.extend(self.raw_open.allowed.iter().map(String::as_str));
        names.extend(
            self.storage_builder
                .iter()
                .flat_map(|b| b.denied.iter().map(String::as_str)),
        );
        names
    }

    /// Why `from` must not depend on `to` as `kind`, or `None` when the edge is
    /// allowed or either crate is unplaced (reported separately).
    pub fn deny(&self, from: &str, to: &str, kind: EdgeKind) -> Option<String> {
        let (Some(from_place), Some(to_place)) = (self.place(from), self.place(to)) else {
            return None;
        };
        let row_dev = self.row_dev.contains(to);
        let suffix = kind.suffix();
        match (from_place, to_place) {
            (Place::Row(a), Place::Row(b)) if b < a => None,
            (Place::Row(a), Place::Row(b)) if b == a => Some(format!(
                "{from} (edges.toml row {a}) must not depend on {to} (row {b}){suffix}: \
                 crates in the same row are siblings with no edge between them"
            )),
            (Place::Row(a), Place::Row(b)) => Some(format!(
                "{from} (edges.toml row {a}) must not depend on {to} (row {b}){suffix}: \
                 a crate may depend only on crates in lower rows"
            )),
            (Place::Row(_), Place::Any) if row_dev && kind == EdgeKind::Dev => None,
            (Place::Row(a), Place::Any) if row_dev => Some(format!(
                "{from} (edges.toml row {a}) must not depend on {to} ([any] group){suffix}: \
                 row crates may use {to} only as a dev-dependency"
            )),
            (Place::Row(a), Place::Any) => Some(format!(
                "{from} (edges.toml row {a}) must not depend on {to} ([any] group){suffix}: \
                 a crate may depend only on crates in lower rows"
            )),
            (Place::Row(_) | Place::Any, Place::Isolated) => Some(format!(
                "{from} must not depend on {to}{suffix}: no workspace crate depends on edges.toml [isolated] crates"
            )),
            (Place::Any, _) if self.any_except.contains(to) => Some(format!(
                "{from} must not depend on {to}{suffix}: edges.toml [any] crates must not depend on {to}"
            )),
            (Place::Any, _) => None,
            (Place::Isolated, _) => Some(format!(
                "{from} must not depend on {to}{suffix}: edges.toml [isolated] crates depend on no workspace crate"
            )),
        }
    }
}
