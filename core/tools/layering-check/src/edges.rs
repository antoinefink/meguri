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
    pub restricted: BTreeMap<String, Restricted>,
    pub raw_open: RawOpen,
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
        Ok(Edges {
            places,
            any_except: file.any.except.into_iter().collect(),
            restricted: file.restricted,
            raw_open: file.raw_open,
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
        names
    }

    /// Why `from` must not depend on `to`, or `None` when the edge is allowed
    /// or either crate is unplaced (reported separately). `kind` is empty for
    /// a normal dependency, else " as a dev-dependency" or " as a build-dependency".
    pub fn deny(&self, from: &str, to: &str, kind: &str) -> Option<String> {
        let (Some(from_place), Some(to_place)) = (self.place(from), self.place(to)) else {
            return None;
        };
        match (from_place, to_place) {
            (Place::Row(a), Place::Row(b)) if b < a => None,
            (Place::Row(a), Place::Row(b)) if b == a => Some(format!(
                "{from} (edges.toml row {a}) must not depend on {to} (row {b}){kind}: \
                 crates in the same row are siblings with no edge between them"
            )),
            (Place::Row(a), Place::Row(b)) => Some(format!(
                "{from} (edges.toml row {a}) must not depend on {to} (row {b}){kind}: \
                 a crate may depend only on crates in lower rows"
            )),
            (Place::Row(a), Place::Any) => Some(format!(
                "{from} (edges.toml row {a}) must not depend on {to} ([any] group){kind}: \
                 a crate may depend only on crates in lower rows"
            )),
            (Place::Row(_) | Place::Any, Place::Isolated) => Some(format!(
                "{from} must not depend on {to}{kind}: no workspace crate depends on edges.toml [isolated] crates"
            )),
            (Place::Any, _) if self.any_except.contains(to) => Some(format!(
                "{from} must not depend on {to}{kind}: edges.toml [any] crates must not depend on {to}"
            )),
            (Place::Any, _) => None,
            (Place::Isolated, _) => Some(format!(
                "{from} must not depend on {to}{kind}: edges.toml [isolated] crates depend on no workspace crate"
            )),
        }
    }
}
