//! Typed engine refusal details owned by U37. No display-string decoding.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CoreTable {
    Col,
    Notes,
    Cards,
    Revlog,
    Graves,
    DeckConfig,
    Config,
    Tags,
    Fields,
    Templates,
    Notetypes,
    Decks,
}
impl From<meguri_storage::Table> for CoreTable {
    fn from(v: meguri_storage::Table) -> Self {
        match v {
            meguri_storage::Table::Col => Self::Col,
            meguri_storage::Table::Notes => Self::Notes,
            meguri_storage::Table::Cards => Self::Cards,
            meguri_storage::Table::Revlog => Self::Revlog,
            meguri_storage::Table::Graves => Self::Graves,
            meguri_storage::Table::DeckConfig => Self::DeckConfig,
            meguri_storage::Table::Config => Self::Config,
            meguri_storage::Table::Tags => Self::Tags,
            meguri_storage::Table::Fields => Self::Fields,
            meguri_storage::Table::Templates => Self::Templates,
            meguri_storage::Table::Notetypes => Self::Notetypes,
            meguri_storage::Table::Decks => Self::Decks,
        }
    }
}
impl From<CoreTable> for meguri_storage::Table {
    fn from(v: CoreTable) -> Self {
        match v {
            CoreTable::Col => Self::Col,
            CoreTable::Notes => Self::Notes,
            CoreTable::Cards => Self::Cards,
            CoreTable::Revlog => Self::Revlog,
            CoreTable::Graves => Self::Graves,
            CoreTable::DeckConfig => Self::DeckConfig,
            CoreTable::Config => Self::Config,
            CoreTable::Tags => Self::Tags,
            CoreTable::Fields => Self::Fields,
            CoreTable::Templates => Self::Templates,
            CoreTable::Notetypes => Self::Notetypes,
            CoreTable::Decks => Self::Decks,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SchemaChangeReason {
    FieldsChanged,
    SortFieldChanged,
    TemplatesChanged,
    NotetypeRemoved,
    NotetypeRestored,
    NotesNotetypeChanged,
    PresetRemoved,
    NotetypesMerged,
    CollectionReplaced,
    CheckDatabaseRepair,
    SyncSanityFailed,
    FullUpload,
    ForceOneWay,
}
impl From<meguri_engine::schema_gate::SchemaReason> for SchemaChangeReason {
    fn from(v: meguri_engine::schema_gate::SchemaReason) -> Self {
        match v {
            meguri_engine::schema_gate::SchemaReason::FieldsChanged => Self::FieldsChanged,
            meguri_engine::schema_gate::SchemaReason::SortFieldChanged => Self::SortFieldChanged,
            meguri_engine::schema_gate::SchemaReason::TemplatesChanged => Self::TemplatesChanged,
            meguri_engine::schema_gate::SchemaReason::NotetypeRemoved => Self::NotetypeRemoved,
            meguri_engine::schema_gate::SchemaReason::NotetypeRestored => Self::NotetypeRestored,
            meguri_engine::schema_gate::SchemaReason::NotesNotetypeChanged => Self::NotesNotetypeChanged,
            meguri_engine::schema_gate::SchemaReason::PresetRemoved => Self::PresetRemoved,
            meguri_engine::schema_gate::SchemaReason::NotetypesMerged => Self::NotetypesMerged,
            meguri_engine::schema_gate::SchemaReason::CollectionReplaced => Self::CollectionReplaced,
            meguri_engine::schema_gate::SchemaReason::CheckDatabaseRepair => Self::CheckDatabaseRepair,
            meguri_engine::schema_gate::SchemaReason::SyncSanityFailed => Self::SyncSanityFailed,
            meguri_engine::schema_gate::SchemaReason::FullUpload => Self::FullUpload,
            meguri_engine::schema_gate::SchemaReason::ForceOneWay => Self::ForceOneWay,
        }
    }
}
impl From<SchemaChangeReason> for meguri_engine::schema_gate::SchemaReason {
    fn from(v: SchemaChangeReason) -> Self {
        match v {
            SchemaChangeReason::FieldsChanged => Self::FieldsChanged,
            SchemaChangeReason::SortFieldChanged => Self::SortFieldChanged,
            SchemaChangeReason::TemplatesChanged => Self::TemplatesChanged,
            SchemaChangeReason::NotetypeRemoved => Self::NotetypeRemoved,
            SchemaChangeReason::NotetypeRestored => Self::NotetypeRestored,
            SchemaChangeReason::NotesNotetypeChanged => Self::NotesNotetypeChanged,
            SchemaChangeReason::PresetRemoved => Self::PresetRemoved,
            SchemaChangeReason::NotetypesMerged => Self::NotetypesMerged,
            SchemaChangeReason::CollectionReplaced => Self::CollectionReplaced,
            SchemaChangeReason::CheckDatabaseRepair => Self::CheckDatabaseRepair,
            SchemaChangeReason::SyncSanityFailed => Self::SyncSanityFailed,
            SchemaChangeReason::FullUpload => Self::FullUpload,
            SchemaChangeReason::ForceOneWay => Self::ForceOneWay,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckRefusal {
    FilteredParent,
    TooDeep,
    UnderDescendant,
    FilteredDeck,
    DefaultPreset,
}
impl From<meguri_engine::decks::Refusal> for DeckRefusal {
    fn from(v: meguri_engine::decks::Refusal) -> Self {
        match v {
            meguri_engine::decks::Refusal::FilteredParent => Self::FilteredParent,
            meguri_engine::decks::Refusal::TooDeep => Self::TooDeep,
            meguri_engine::decks::Refusal::UnderDescendant => Self::UnderDescendant,
            meguri_engine::decks::Refusal::FilteredDeck => Self::FilteredDeck,
            meguri_engine::decks::Refusal::DefaultPreset => Self::DefaultPreset,
        }
    }
}
impl From<DeckRefusal> for meguri_engine::decks::Refusal {
    fn from(v: DeckRefusal) -> Self {
        match v {
            DeckRefusal::FilteredParent => Self::FilteredParent,
            DeckRefusal::TooDeep => Self::TooDeep,
            DeckRefusal::UnderDescendant => Self::UnderDescendant,
            DeckRefusal::FilteredDeck => Self::FilteredDeck,
            DeckRefusal::DefaultPreset => Self::DefaultPreset,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TemplateCheckKind {
    DuplicateFront,
    TemplateParseError,
    NoFrontField,
    UnknownField,
    MissingCloze,
}
impl From<meguri_engine::notetypes::CheckKind> for TemplateCheckKind {
    fn from(v: meguri_engine::notetypes::CheckKind) -> Self {
        match v {
            meguri_engine::notetypes::CheckKind::DuplicateFront => Self::DuplicateFront,
            meguri_engine::notetypes::CheckKind::TemplateParseError => Self::TemplateParseError,
            meguri_engine::notetypes::CheckKind::NoFrontField => Self::NoFrontField,
            meguri_engine::notetypes::CheckKind::UnknownField => Self::UnknownField,
            meguri_engine::notetypes::CheckKind::MissingCloze => Self::MissingCloze,
        }
    }
}
impl From<TemplateCheckKind> for meguri_engine::notetypes::CheckKind {
    fn from(v: TemplateCheckKind) -> Self {
        match v {
            TemplateCheckKind::DuplicateFront => Self::DuplicateFront,
            TemplateCheckKind::TemplateParseError => Self::TemplateParseError,
            TemplateCheckKind::NoFrontField => Self::NoFrontField,
            TemplateCheckKind::UnknownField => Self::UnknownField,
            TemplateCheckKind::MissingCloze => Self::MissingCloze,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeTemplateSide {
    Front,
    Back,
}
impl From<meguri_engine::notetypes::TemplateSide> for NotetypeTemplateSide {
    fn from(v: meguri_engine::notetypes::TemplateSide) -> Self {
        match v {
            meguri_engine::notetypes::TemplateSide::Front => Self::Front,
            meguri_engine::notetypes::TemplateSide::Back => Self::Back,
        }
    }
}
impl From<NotetypeTemplateSide> for meguri_engine::notetypes::TemplateSide {
    fn from(v: NotetypeTemplateSide) -> Self {
        match v {
            NotetypeTemplateSide::Front => Self::Front,
            NotetypeTemplateSide::Back => Self::Back,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum InvalidNotetypeReason {
    NoField,
    NoTemplate,
    EmptyName,
    EmptyFieldName { ord: u64 },
    EmptyTemplateName { ord: u64 },
}
impl From<meguri_engine::notetypes::InvalidNotetype> for InvalidNotetypeReason {
    fn from(v: meguri_engine::notetypes::InvalidNotetype) -> Self {
        use meguri_engine::notetypes::InvalidNotetype as I;
        match v {
            I::NoField => Self::NoField,
            I::NoTemplate => Self::NoTemplate,
            I::EmptyName => Self::EmptyName,
            I::EmptyFieldName { ord } => Self::EmptyFieldName { ord: ord as u64 },
            I::EmptyTemplateName { ord } => Self::EmptyTemplateName { ord: ord as u64 },
        }
    }
}
impl From<InvalidNotetypeReason> for meguri_engine::notetypes::InvalidNotetype {
    fn from(v: InvalidNotetypeReason) -> Self {
        match v {
            InvalidNotetypeReason::NoField => Self::NoField,
            InvalidNotetypeReason::NoTemplate => Self::NoTemplate,
            InvalidNotetypeReason::EmptyName => Self::EmptyName,
            InvalidNotetypeReason::EmptyFieldName { ord } => Self::EmptyFieldName { ord: ord as usize },
            InvalidNotetypeReason::EmptyTemplateName { ord } => Self::EmptyTemplateName { ord: ord as usize },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeRefusal {
    Invalid {
        reason: InvalidNotetypeReason,
    },
    Template {
        kind: TemplateCheckKind,
        template: u64,
        side: Option<NotetypeTemplateSide>,
        name: Option<String>,
        earlier: Option<u64>,
    },
}
impl From<meguri_engine::notetypes::NotetypeRefusal> for NotetypeRefusal {
    fn from(v: meguri_engine::notetypes::NotetypeRefusal) -> Self {
        match v {
            meguri_engine::notetypes::NotetypeRefusal::Invalid(reason) => Self::Invalid { reason: reason.into() },
            meguri_engine::notetypes::NotetypeRefusal::Template(t) => Self::Template {
                kind: t.kind.into(),
                template: t.template as u64,
                side: t.side.map(Into::into),
                name: t.name,
                earlier: t.earlier.map(|n| n as u64),
            },
        }
    }
}
impl From<NotetypeRefusal> for meguri_engine::notetypes::NotetypeRefusal {
    fn from(v: NotetypeRefusal) -> Self {
        match v {
            NotetypeRefusal::Invalid { reason } => Self::Invalid(reason.into()),
            NotetypeRefusal::Template {
                kind,
                template,
                side,
                name,
                earlier,
            } => Self::Template(meguri_engine::notetypes::TemplateFailure {
                kind: kind.into(),
                template: template as usize,
                side: side.map(Into::into),
                name,
                earlier: earlier.map(|n| n as usize),
            }),
        }
    }
}
impl From<crate::lifecycle::CollectionOpenFailure> for meguri_storage::OpenError {
    fn from(v: crate::lifecycle::CollectionOpenFailure) -> Self {
        use crate::lifecycle::CollectionOpenFailure as F;
        match v {
            F::OpenInAnotherProgram => Self::OpenInAnotherProgram,
            F::TooOld { version } => Self::TooOld { version },
            F::TooNew { version } => Self::TooNew { version },
            F::V1Scheduler => Self::V1Scheduler,
            F::NotACollection { missing_tables } => Self::NotACollection { missing_tables },
            F::UpgradeFailed { version, stage, detail } => Self::UpgradeFailed {
                version,
                detail,
                stage: match stage {
                    crate::lifecycle::CollectionUpgradeStage::Copy => meguri_storage::UpgradeStage::Copy,
                    crate::lifecycle::CollectionUpgradeStage::Step { step } => meguri_storage::UpgradeStage::Step(step),
                },
            },
            F::Corrupt { detail } => Self::Corrupt { detail },
            F::ConnectionInit { detail } => Self::ConnectionInit { detail },
            F::NotFound => Self::NotFound,
            F::Io { detail } => Self::Io { detail },
        }
    }
}
impl From<crate::review_session::ReviewDisabledReason> for meguri_engine::DisabledReason {
    fn from(v: crate::review_session::ReviewDisabledReason) -> Self {
        use crate::review_session::ReviewDisabledReason as R;
        match v {
            R::None => Self::None,
            R::ClearedByRollover => Self::ClearedByRollover,
            R::ClearedBySync => Self::ClearedBySync,
            R::ClearedByOneWay => Self::ClearedByOneWay,
            R::ClearedByNonUndoableOp => Self::ClearedByNonUndoableOp,
            R::ChangedExternally => Self::ChangedExternally,
            R::AlreadyUploaded => Self::AlreadyUploaded,
            R::UploadPending => Self::UploadPending,
        }
    }
}
