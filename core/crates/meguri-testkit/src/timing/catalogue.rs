//! The operation catalogue (T6): the names, in catalogue order, and the fixed
//! text some operations take (T6.5's queries, T6.6's columns). The history
//! keys on the names, so they never change.

/// A T6 operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operation {
    CollectionOpen,
    DecksCounts,
    QueueBuild,
    ReviewAnswer,
    SearchDeck,
    SearchState,
    SearchTag,
    SearchText,
    SearchRegex,
    SearchProp,
    SearchRated,
    SearchCombined,
    BrowserSortField,
    BrowserSortDue,
    StatsGraphs,
    MaintenanceCheckDb,
    PackageApkgExport,
    PackageColpkgExport,
    PackageApkgImport,
    SyncFullUpload,
    SyncFullDownload,
    FsrsRecomputeMemoryStates,
}

impl Operation {
    /// Every operation, in catalogue order.
    pub const ALL: [Operation; 22] = [
        Operation::CollectionOpen,
        Operation::DecksCounts,
        Operation::QueueBuild,
        Operation::ReviewAnswer,
        Operation::SearchDeck,
        Operation::SearchState,
        Operation::SearchTag,
        Operation::SearchText,
        Operation::SearchRegex,
        Operation::SearchProp,
        Operation::SearchRated,
        Operation::SearchCombined,
        Operation::BrowserSortField,
        Operation::BrowserSortDue,
        Operation::StatsGraphs,
        Operation::MaintenanceCheckDb,
        Operation::PackageApkgExport,
        Operation::PackageColpkgExport,
        Operation::PackageApkgImport,
        Operation::SyncFullUpload,
        Operation::SyncFullDownload,
        Operation::FsrsRecomputeMemoryStates,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Operation::CollectionOpen => "collection.open",
            Operation::DecksCounts => "decks.counts",
            Operation::QueueBuild => "queue.build",
            Operation::ReviewAnswer => "review.answer",
            Operation::SearchDeck => "search.deck",
            Operation::SearchState => "search.state",
            Operation::SearchTag => "search.tag",
            Operation::SearchText => "search.text",
            Operation::SearchRegex => "search.regex",
            Operation::SearchProp => "search.prop",
            Operation::SearchRated => "search.rated",
            Operation::SearchCombined => "search.combined",
            Operation::BrowserSortField => "browser.sort_field",
            Operation::BrowserSortDue => "browser.sort_due",
            Operation::StatsGraphs => "stats.graphs",
            Operation::MaintenanceCheckDb => "maintenance.check_db",
            Operation::PackageApkgExport => "package.apkg_export",
            Operation::PackageColpkgExport => "package.colpkg_export",
            Operation::PackageApkgImport => "package.apkg_import",
            Operation::SyncFullUpload => "sync.full_upload",
            Operation::SyncFullDownload => "sync.full_download",
            Operation::FsrsRecomputeMemoryStates => "fsrs.recompute_memory_states",
        }
    }

    /// The operation of an exact T6 name.
    pub fn from_name(name: &str) -> Option<Operation> {
        Operation::ALL.into_iter().find(|op| op.name() == name)
    }

    /// T6.5: a search class's query, as the user would type it.
    pub fn query(self) -> Option<&'static str> {
        Some(match self {
            Operation::SearchDeck => "deck:current",
            Operation::SearchState => "is:due",
            Operation::SearchTag => "tag:*::*",
            Operation::SearchText => "kamo",
            Operation::SearchRegex => "re:k[aeiou]m",
            Operation::SearchProp => "prop:ivl>=21",
            Operation::SearchRated => "rated:7",
            Operation::SearchCombined => "deck:current (is:new OR is:due) -is:suspended",
            _ => return None,
        })
    }

    /// T6.6: a browser sort's column, as its `sortType` config value.
    pub fn sort_column(self) -> Option<&'static str> {
        match self {
            Operation::BrowserSortField => Some("noteFld"),
            Operation::BrowserSortDue => Some("cardDue"),
            _ => None,
        }
    }
}

/// T1 `--op`: the named operations in catalogue order, each once whatever
/// the order and repetition of the names; every operation when none is
/// named. An unknown name is the error.
pub fn select(names: &[String]) -> Result<Vec<Operation>, String> {
    if names.is_empty() {
        return Ok(Operation::ALL.to_vec());
    }
    let mut chosen = Vec::with_capacity(names.len());
    for name in names {
        chosen.push(Operation::from_name(name).ok_or_else(|| name.clone())?);
    }
    chosen.sort();
    chosen.dedup();
    Ok(chosen)
}
