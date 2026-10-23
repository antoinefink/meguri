//! Select this binary alone on an idle runner; measures E6's actual file save.
#![cfg(feature = "test-hooks")]
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::space::StatvfsSpace;
use meguri_testkit::bench::memory::{DEFAULT_INTERVAL, MemorySampler, process_footprint_bytes};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
};

fn digest(path: &Path) -> [u8; 32] {
    let mut file = File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        let n = file.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
    }
    hash.finalize().into()
}
#[test]
fn sparse_gigabyte_file_save_has_measured_bounded_peak_increase() {
    let root = tempfile::tempdir().unwrap();
    let collection = root.path().join("collection");
    fs::create_dir(&collection).unwrap();
    let config = HostConfig::new(
        collection.clone(),
        root.path().join("backups"),
        root.path().join("widget"),
        Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(47)),
        Arc::new(StatvfsSpace),
    );
    meguri_engine::bootstrap::create(
        &collection.join("collection.anki2"),
        config.clock.as_ref(),
        config.random.as_ref(),
    )
    .unwrap();
    let host = CoreHost::open(config).unwrap();
    let owned = root.path().join("fixture");
    fs::create_dir(&owned).unwrap();
    let source = owned.join("large.colpkg");
    let size = 1024_u64 * 1024 * 1024;
    let mut file = File::create(&source).unwrap();
    file.set_len(size).unwrap();
    file.write_all(b"sparse file export begins").unwrap();
    file.seek(SeekFrom::End(-32)).unwrap();
    file.write_all(&[0xa5; 32]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let expected = digest(&source);
    let destination = root.path().join("saved.colpkg");
    let baseline = process_footprint_bytes().expect("RSS/phys_footprint measurement must be available");
    assert!(baseline > 0);
    let sampler = MemorySampler::start(DEFAULT_INTERVAL);
    meguri_ffi::collection_export::save_file_for_memory_regression(&host, &source, &destination).unwrap();
    let peak = sampler.stop();
    assert!(process_footprint_bytes().expect("memory measurement must remain available") > 0);
    assert!(peak > 0);
    let increase = peak.saturating_sub(baseline);
    eprintln!("file_save bytes={size} baseline={baseline} peak={peak} increase={increase}");
    assert!(
        increase < 128 * 1024 * 1024,
        "save RSS increase {increase} must stay well below the 1 GiB file"
    );
    assert_eq!(fs::metadata(&destination).unwrap().len(), size);
    assert_eq!(digest(&destination), expected);
    assert_eq!(digest(&source), expected);
    host.close().unwrap();
}
