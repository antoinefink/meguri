//! The stamp (`storage/derived-store` D3) and the verdict each component
//! gets when the store binds to an open collection (D5.2).

use meguri_model::col::Col;

/// Which collection, and how far: `crt`, `scm` and `col.mod` as the `col`
/// row stores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CollectionMark {
    /// `crt`, epoch seconds.
    pub crt: i64,
    /// `scm`, epoch milliseconds.
    pub scm: i64,
    /// `col.mod`, epoch milliseconds.
    pub col_mod: i64,
}

impl CollectionMark {
    pub fn of(col: &Col) -> CollectionMark {
        CollectionMark {
            crt: col.created.0,
            scm: col.schema_modified.0,
            col_mod: col.modified.0,
        }
    }

    /// Whether `other` names the same collection (`crt` and `scm`).
    pub fn same_collection(&self, other: &CollectionMark) -> bool {
        self.crt == other.crt && self.scm == other.scm
    }
}

/// The `meguri_stamp` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// The mark of the last commit every ready component holds.
    pub mark: CollectionMark,
    /// Closed or suspended after applying every commit it was told about.
    pub clean: bool,
}

/// A `meguri_components` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentRecord {
    pub name: String,
    pub version: u32,
    /// The tables hold the content for the stamp's mark.
    pub ready: bool,
    /// Names the live tables' indexes.
    pub build: i64,
}

/// Why a component rebuilds (D5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebuildReason {
    /// The component has no row yet (a new component or a new file).
    NewComponent,
    /// Its version changed from `from`.
    VersionChanged { from: u32 },
    /// One of its tables is missing.
    MissingTable,
    /// The file holds no stamp.
    NoStamp,
    /// A rebuild or catch-up did not finish, or an apply failed.
    NotReady,
    /// The stamp names another collection (`crt` or `scm`).
    OtherCollection,
    /// The collection's `col.mod` is before the stamp's: a restored file.
    CollectionOlder,
    /// The collection changed while the store was closed cleanly (R1453).
    ExternalChange,
}

/// What a component needs at bind (D5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Its content is current.
    Fresh,
    /// Catch up from the commit whose `col.mod` is `since`.
    CatchUp {
        since: i64,
    },
    Rebuild(RebuildReason),
}

/// The verdict of one component (D5.2): `stamp` as read, `record` its row
/// after the components were synced, `reset` why the sync emptied it,
/// `current` the collection's mark at bind.
pub fn assess(
    stamp: Option<&Stamp>,
    record: &ComponentRecord,
    reset: Option<RebuildReason>,
    current: &CollectionMark,
) -> Verdict {
    if let Some(reason) = reset {
        return Verdict::Rebuild(reason);
    }
    let Some(stamp) = stamp else {
        return Verdict::Rebuild(RebuildReason::NoStamp);
    };
    if !record.ready {
        return Verdict::Rebuild(RebuildReason::NotReady);
    }
    if !stamp.mark.same_collection(current) {
        return Verdict::Rebuild(RebuildReason::OtherCollection);
    }
    if stamp.mark.col_mod > current.col_mod {
        return Verdict::Rebuild(RebuildReason::CollectionOlder);
    }
    if stamp.clean {
        if stamp.mark.col_mod != current.col_mod {
            return Verdict::Rebuild(RebuildReason::ExternalChange);
        }
        return Verdict::Fresh;
    }
    Verdict::CatchUp {
        since: stamp.mark.col_mod,
    }
}
