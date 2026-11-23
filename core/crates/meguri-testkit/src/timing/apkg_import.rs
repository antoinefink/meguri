//! T6.11 `package.apkg_import`: the T5.4 package imported into a new, empty
//! collection (T5.3) through the entry point the bridge will call
//! (`meguri_package::apkg`), with learning progress and presets included,
//! note types not merged, and notes and note types updated only when the
//! package's are newer.

use std::path::PathBuf;
use std::sync::Arc;

use meguri_engine::{CancelToken, ProgressSink};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::rng::SeededRandom;
use meguri_package::apkg::{self, ApkgContext, ApkgImportError, ImportOptions, UpdateCondition};
use meguri_storage::space::StatvfsSpace;
use meguri_storage::{ReadRows, ReaderRole, Table};
use serde_json::json;

use super::protocol::{Check, RunContext, RunFailure, TimedOperation, TimedRun};

const SEED: u64 = 6011;

/// The operation, for `meguri-cli timing run`'s table.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApkgImport;

impl TimedOperation for ApkgImport {
    fn start<'a>(&'a self, run: &'a RunContext<'a>) -> Box<dyn TimedRun + 'a> {
        Box::new(Run {
            context: run,
            package: PathBuf::new(),
            host: None,
            notes_added: 0,
        })
    }
}

struct Run<'a> {
    context: &'a RunContext<'a>,
    package: PathBuf,
    host: Option<CoreHost>,
    notes_added: u64,
}

/// The variant's name, never a message (R1940).
fn named(error: &ApkgImportError) -> RunFailure {
    let text = format!("{error:?}");
    let variant = text.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or_default();
    RunFailure::failed(format!("ApkgImportError::{variant}"))
}

fn engine(_: meguri_engine::EngineError) -> RunFailure {
    RunFailure::failed("EngineError")
}

impl Run<'_> {
    fn host(&self) -> Result<&CoreHost, RunFailure> {
        self.host.as_ref().ok_or_else(|| RunFailure::failed("setup"))
    }
}

impl TimedRun for Run<'_> {
    fn setup(&mut self) -> Result<(), RunFailure> {
        let session = self.context.session;
        self.package = session.package().map_err(|_| RunFailure::failed("io"))?.to_owned();
        // T5.3: Meguri's own new collection, made at `now` in the run
        // directory, and open.
        let root = self.context.dir.join("import-target");
        let directory = root.join("collection");
        std::fs::create_dir_all(&directory).map_err(|_| RunFailure::failed("io"))?;
        let clock = session.clock();
        let random = Arc::new(SeededRandom::new(SEED));
        meguri_engine::bootstrap::create(&directory.join("collection.anki2"), clock.as_ref(), random.as_ref())
            .map_err(engine)?;
        let host = CoreHost::open(HostConfig::new(
            directory,
            root.join("backups"),
            root.join("snapshot"),
            clock,
            random,
            Arc::new(StatvfsSpace),
        ))
        .map_err(engine)?;
        self.host = Some(host);
        Ok(())
    }

    fn timed(&mut self) -> Result<(), RunFailure> {
        let host = self.host()?;
        let collection = host.collection().map_err(engine)?;
        let context = ApkgContext {
            collection_directory: collection.path().parent().ok_or_else(|| RunFailure::failed("io"))?.to_owned(),
            clock: self.context.session.clock(),
            random: Arc::new(SeededRandom::new(SEED)),
            space: Arc::new(StatvfsSpace),
        };
        let options = ImportOptions {
            merge_notetypes: false,
            update_notes: UpdateCondition::IfNewer,
            update_notetypes: UpdateCondition::IfNewer,
            with_scheduling: true,
            with_deck_configs: true,
        };
        let (cancel, progress) = (CancelToken::new(), ProgressSink::new());
        let session = apkg::inspect(&self.package, "import.apkg", &context, &cancel, &progress).map_err(|e| named(&e))?;
        let result = apkg::apply(host.engine(), &session, options, None, &cancel, &progress).map_err(|e| named(&e))?;
        session.close();
        self.notes_added = result.counts.notes_added;
        Ok(())
    }

    fn check(&mut self) -> Result<Check, RunFailure> {
        let collection = self.host()?.collection().map_err(engine)?;
        let reader = collection.reader(ReaderRole::Query).map_err(engine)?;
        let count = |table| reader.row_count(table).map_err(|_| RunFailure::failed("StorageError"));
        let mut check = Check::new();
        check.insert("notes_added".into(), json!(self.notes_added));
        check.insert("cards".into(), json!(count(Table::Cards)?));
        check.insert("revlog".into(), json!(count(Table::Revlog)?));
        Ok(check)
    }

    fn close(self: Box<Self>) -> Result<(), RunFailure> {
        match self.host {
            Some(host) => host.close().map_err(engine),
            None => Ok(()),
        }
    }
}
