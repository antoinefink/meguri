//! Explicit E7/P15 flow; safety backups and server copies never enter here.
use crate::{
    CollectionPackageWriteContext, Error, LiveCollectionPackageWritten, Result, cancelled,
    export_capture::{self, ExportSourceValidation},
    writer,
};
use meguri_storage::{Reader, collection_package, leftovers::staging_directory};
use std::path::{Path, PathBuf};

/// Preflight authority bound to one live source and destination. Created before
/// even host-owned parent/recovery mutations; consumed by exactly one attempt.
pub struct MediaCollectionExportPlan {
    source: PathBuf,
    destination: PathBuf,
    validation: ExportSourceValidation,
}
impl MediaCollectionExportPlan {
    /// Host-owned retained output paths must be protected before recovery can
    /// unlink them. This accepts no foreign/FFI authority and performs no writes.
    pub fn protect_retained_outputs(&mut self, paths: &[PathBuf], check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
        self.validation.protect(paths, check)
    }
}
pub struct MediaCollectionExportWritten {
    pub written: LiveCollectionPackageWritten,
    pub source_validation: ExportSourceValidation,
}

pub fn preflight_media_collection_export_from_live(
    reader: &Reader,
    destination: &Path,
    context: &CollectionPackageWriteContext<'_>,
) -> Result<MediaCollectionExportPlan> {
    cancelled(context.is_cancelled)?;
    let (source, size) = reader.live_snapshot_source()?;
    // The staging input must describe the admitted database's actual media
    // sibling, never a caller-selected alternate media root.
    use std::os::unix::fs::MetadataExt;
    let source_parent = std::fs::metadata(
        source
            .parent()
            .ok_or_else(|| crate::corrupt("export source has no parent"))?,
    )?;
    let directory = std::fs::metadata(context.collection_directory)?;
    if (source_parent.dev(), source_parent.ino()) != (directory.dev(), directory.ino()) {
        return Err(Error::Io {
            detail: "export media root does not belong to the admitted collection".into(),
        });
    }
    let output_parent = writer::existing_directory(destination.parent().unwrap_or(Path::new(".")))?.canonicalize()?;
    if output_parent.starts_with(context.collection_directory.canonicalize()?.join("collection.media")) {
        return Err(Error::Io {
            detail: "export output overlaps the source media folder".into(),
        });
    }
    writer::check_source_alias(&source, destination)?;
    let mut protected = vec![source.clone(), destination.to_owned()];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut auxiliary = source.as_os_str().to_owned();
        auxiliary.push(suffix);
        writer::check_source_alias(Path::new(&auxiliary), destination)?;
        protected.push(auxiliary.into());
    }
    let validation = ExportSourceValidation::inventory(context.collection_directory, protected, context.is_cancelled)?;
    export_capture::preflight(size, &validation.files, destination, context)?;
    Ok(MediaCollectionExportPlan {
        source,
        destination: destination.to_owned(),
        validation,
    })
}

pub fn write_media_collection_export_from_live(
    reader: &Reader,
    destination: &Path,
    context: &CollectionPackageWriteContext<'_>,
    mut plan: MediaCollectionExportPlan,
) -> Result<MediaCollectionExportWritten> {
    cancelled(context.is_cancelled)?;
    let (source, size) = reader.live_snapshot_source()?;
    if source != plan.source || destination != plan.destination {
        return Err(Error::Io {
            detail: "export preflight source/destination mismatch".into(),
        });
    }
    plan.validation.validate(context.is_cancelled)?;
    writer::check_exclusive_destination(destination, context.finalisation)?;
    // Repeat the complete concurrent cost, never the independent P10 amounts.
    export_capture::preflight(size, &plan.validation.files, destination, context)?;
    cancelled(context.is_cancelled)?;
    let staging = staging_directory(context.collection_directory)?;
    let snapshot = reader.snapshot_live(staging.path())?;
    let snapshot_modified = collection_package::snapshot_modified(&snapshot).map_err(writer::source_error)?;
    let snapshot_size = std::fs::metadata(&snapshot)?.len();
    export_capture::preflight(snapshot_size, &plan.validation.files, destination, context)?;
    plan.validation.capture(staging.path(), context.is_cancelled)?;
    writer::publish_snapshot_with_media(&snapshot, staging.path(), destination, context, Some(&plan.validation))?;
    // No new fallible work after durable publication here. The owned proof is
    // checked at delivery, where the host can discard its private artifact.

    Ok(MediaCollectionExportWritten {
        written: LiveCollectionPackageWritten { snapshot_modified },
        source_validation: plan.validation,
    })
}
