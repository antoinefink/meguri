//! The collections benches run on (R1803, R1864): U11's generated
//! collections at 10k, 100k, 150k, 500k and 1m cards, shaped like the user's
//! (`profiles/user-shape.json`), seed 1, media referenced, at the default
//! pinned instant.
//!
//! They live where `tools/mg fixtures <size>` puts them,
//! `<root>/<user-shape sha256, 16 hex>-1-<size>/`, so a bench and the job
//! share one copy: a directory whose manifest matches is reused, under the
//! directory's lock, and is generated otherwise. The root is
//! `$MEGURI_BENCH_FIXTURES`, or the checkout's `build/fixtures`.
//!
//! Benches never open the shared copy: they open a [`WorkingCopy`] in the
//! temporary directory (the container's tmpfs in Docker), so a bench cannot
//! change the fixture and SQLite never runs on the bind mount.

use std::path::{Path, PathBuf};

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::{OpenError, OpenOptions, OpenedCollection, open_collection};

use crate::generate::{
    self, DEFAULT_NOW_MS, GENERATOR_VERSION, GenError, Manifest, MediaMode, OutputLock, Request, parse_size,
};

/// The seed every bench collection uses (the `tools/mg fixtures` default).
pub const SEED: u64 = 1;

/// A generated collection a bench may copy.
#[derive(Debug)]
pub struct BenchCollection {
    /// The output directory (`collection.anki2`, `manifest.json`).
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// The size as asked for, lowercase (`150k`).
    pub size: String,
    /// Whether an existing directory was reused.
    pub reused: bool,
}

/// `$MEGURI_BENCH_FIXTURES`, or `build/fixtures` in the checkout.
pub fn fixtures_root() -> PathBuf {
    match std::env::var_os("MEGURI_BENCH_FIXTURES") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../build/fixtures"),
    }
}

/// The collection of `size` cards (`10k`, `150k`, a number) under
/// [`fixtures_root`].
pub fn profile_collection(size: &str) -> Result<BenchCollection, GenError> {
    profile_collection_in(&fixtures_root(), size)
}

/// The collection of `size` cards under `root`, reused when its manifest
/// matches and generated otherwise.
pub fn profile_collection_in(root: &Path, size: &str) -> Result<BenchCollection, GenError> {
    let size = size.trim().to_ascii_lowercase();
    let cards = parse_size(&size).ok_or_else(|| GenError::Request(format!("not a size: {size}")))?;
    let shape_path = generate::profiles_dir().join("user-shape.json");
    let shape_bytes = std::fs::read(&shape_path).map_err(|e| GenError::io(&shape_path, e))?;
    let profile = generate::user_shape()?;
    let dir = root.join(format!("{}-{SEED}-{size}", &sha256_hex(&shape_bytes)[..16]));

    let _lock = OutputLock::acquire(&dir)?;
    if let Ok(have) = Manifest::load(&dir) {
        let matches = have.generator_version == GENERATOR_VERSION
            && have.kind == "profile"
            && have.seed == SEED
            && have.cards_requested == cards
            && have.now_ms == DEFAULT_NOW_MS
            && have.profile_sha256.as_deref() == Some(profile.sha256().as_str())
            && have.worst_case_sha256.is_none()
            && have.media == MediaMode::References
            && dir.join("collection.anki2").is_file();
        if matches {
            return Ok(BenchCollection {
                dir,
                manifest: have,
                size,
                reused: true,
            });
        }
    }
    let manifest = generate::generate(&Request::new(profile, cards, SEED), &dir)?;
    Ok(BenchCollection {
        dir,
        manifest,
        size,
        reused: false,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A private copy of a bench collection in the temporary directory,
/// deleted when dropped.
#[derive(Debug)]
pub struct WorkingCopy {
    _dir: tempfile::TempDir,
    /// The copied `collection.anki2`.
    pub path: PathBuf,
}

impl BenchCollection {
    /// Copies `collection.anki2` into a new temporary directory.
    pub fn working_copy(&self) -> Result<WorkingCopy, GenError> {
        let dir = tempfile::Builder::new()
            .prefix("meguri-bench-")
            .tempdir()
            .map_err(|e| GenError::io(&std::env::temp_dir(), e))?;
        let source = self.dir.join("collection.anki2");
        let path = dir.path().join("collection.anki2");
        std::fs::copy(&source, &path).map_err(|e| GenError::io(&source, e))?;
        Ok(WorkingCopy { _dir: dir, path })
    }
}

/// Opens the collection at `path` through the open gate, with the pinned
/// instant and a fixed seed (nothing here may read the system clock).
pub fn open(path: &Path) -> Result<OpenedCollection, OpenError> {
    let clock = FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC);
    let random = SeededRandom::new(SEED);
    open_collection(path, &OpenOptions::new(&clock, &random))
}
