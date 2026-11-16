// SPDX-License-Identifier: MIT
mod support;

use meguri_sync_server::DataRoot;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeMap,
    fs::{self, File, Metadata, OpenOptions},
    io::{BufRead, Read, Write},
    os::{fd::AsRawFd, unix::{ffi::OsStringExt, fs::{MetadataExt, OpenOptionsExt, PermissionsExt}}},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use support::*;

const MOUNT: &str = "/mg-enospc";
const CAPACITY: u64 = 16 * 1024 * 1024;
const MAX_FILL: usize = 32 * 1024 * 1024;
const BLOCK: usize = 4096;
const OLD: &[u8] = b"synthetic original iris bytes\0\xff";
const RETAIN: &[u8] = b"synthetic retained cedar bytes\x80";
const REPLACEMENT: &[u8] = b"synthetic replacement violet bytes\0\xfe";
const ADDITION: &[u8] = b"synthetic addition fern bytes\x81";

fn identity(metadata: &Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

fn mount_path(encoded: &str) -> PathBuf {
    let input = encoded.as_bytes();
    let mut bytes = Vec::new();
    let mut at = 0;
    while at < input.len() {
        if input[at] == b'\\' {
            assert!(at + 3 < input.len(), "malformed mountinfo escape");
            let octal = &input[at + 1..at + 4];
            assert!(octal.iter().all(|b| (b'0'..=b'7').contains(b)));
            let value = (octal[0] - b'0') as u16 * 64
                + (octal[1] - b'0') as u16 * 8 + (octal[2] - b'0') as u16;
            bytes.push(u8::try_from(value).expect("mountinfo byte"));
            at += 4;
        } else {
            bytes.push(input[at]);
            at += 1;
        }
    }
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

// No writes, including temporary-directory creation, precede these checks.
// Never fall back to TMPDIR or accept an operator-selected filling path.
fn checked_mount() -> File {
    assert!(std::env::var_os("MEGURI_TEST_ENOSPC_ROOT").as_deref()
        == Some(std::ffi::OsStr::new(MOUNT)), "requires dedicated /mg-enospc fixture");
    let path = Path::new(MOUNT);
    assert_eq!(fs::canonicalize(path).expect("missing /mg-enospc fixture"), path);
    for ancestor in path.ancestors() {
        assert!(fs::symlink_metadata(ancestor).unwrap().is_dir(), "fixture ancestry must be directories without symlinks");
    }
    let directory = OpenOptions::new().read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path).expect("open dedicated tmpfs without following links");
    let metadata = directory.metadata().unwrap();
    assert_eq!(identity(&metadata), identity(&fs::symlink_metadata(path).unwrap()));
    assert_eq!(metadata.mode() & 0o7777, 0o1777);
    let scratch = fs::canonicalize("/tmp").unwrap();
    assert_ne!(metadata.dev(), fs::metadata(&scratch).unwrap().dev(), "fixture must be a separate device from /tmp");

    let mut text = String::new();
    File::open("/proc/self/mountinfo").expect("mountinfo required for fixture safety")
        .take(1024 * 1024 + 1).read_to_string(&mut text).unwrap();
    assert!(text.len() <= 1024 * 1024, "mountinfo exceeds safety bound");
    let mounts = text.lines().map(|line| {
        let (left, right) = line.split_once(" - ").expect("mountinfo separator");
        let fields = left.split_whitespace().collect::<Vec<_>>();
        let filesystem = right.split_whitespace().collect::<Vec<_>>();
        assert!(fields.len() >= 6 && filesystem.len() == 3, "mountinfo fields");
        (fields, filesystem)
    }).collect::<Vec<_>>();
    let dedicated = mounts.iter().filter(|(f, _)| mount_path(f[4]) == path).collect::<Vec<_>>();
    assert_eq!(dedicated.len(), 1, "requires one exact dedicated mount");
    let (fields, filesystem) = dedicated[0];
    assert_eq!(fields[3], "/", "refuse a bind-mounted subtree");
    assert_eq!(filesystem[0], "tmpfs");
    assert_eq!(filesystem[1], "tmpfs");
    for option in ["rw", "noexec", "nosuid", "nodev"] {
        assert!(fields[5].split(',').any(|v| v == option), "missing required tmpfs option {option}");
    }
    assert!(filesystem[2].split(',').any(|v| v == "rw"));
    assert_eq!(fields[2], format!("{}:{}", libc::major(metadata.dev() as _), libc::minor(metadata.dev() as _)));
    assert_eq!(mounts.iter().filter(|(f, _)| f[2] == fields[2]).count(), 1, "refuse device aliases");
    assert!(!mounts.iter().any(|(f, _)| {
        let mounted = mount_path(f[4]);
        mounted != path && mounted.starts_with(path)
    }), "refuse nested mounts under filling root");
    let scratch_mount = mounts.iter().filter(|(f, _)| scratch.starts_with(mount_path(f[4])))
        .max_by_key(|(f, _)| mount_path(f[4]).components().count()).expect("/tmp mount");
    assert_ne!(fields[0], scratch_mount.0[0], "fixture must be a separate mount from /tmp");
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: live directory fd and valid output storage; success initializes it.
    assert_eq!(unsafe { libc::fstatvfs(directory.as_raw_fd(), stats.as_mut_ptr()) }, 0);
    let stats = unsafe { stats.assume_init() };
    assert_eq!((stats.f_blocks as u64).checked_mul(stats.f_frsize as u64), Some(CAPACITY), "requires exactly 16 MiB");
    directory
}

struct Filler(Option<PathBuf>);
impl Filler {
    fn exhaust(temp: &tempfile::TempDir, mount: &File, remaining: &mut usize) -> Self {
        let checked = checked_mount();
        assert_eq!(identity(&checked.metadata().unwrap()), identity(&mount.metadata().unwrap()));
        let private = fs::symlink_metadata(temp.path()).unwrap();
        assert!(private.is_dir());
        assert_eq!(private.dev(), mount.metadata().unwrap().dev());
        assert_eq!(private.mode() & 0o777, 0o700);
        assert_eq!(private.uid(), unsafe { libc::geteuid() });
        assert_eq!(fs::canonicalize(temp.path()).unwrap().parent(), Some(Path::new(MOUNT)));
        let owned = Self(Some(temp.path().join("filler")));
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(owned.0.as_ref().unwrap()).unwrap();
        assert_eq!(file.metadata().unwrap().dev(), private.dev());
        assert_eq!(file.metadata().unwrap().nlink(), 1);
        let block = [0xa7; BLOCK];
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "bounded tmpfs fill exceeded 10 seconds");
            assert!(*remaining >= BLOCK, "no actual ENOSPC within total 32 MiB filler budget");
            *remaining -= BLOCK; // Count attempted bytes, including a partial final write.
            match file.write_all(&block) {
                Ok(()) => (),
                Err(error) => {
                    assert_eq!(error.raw_os_error(), Some(28), "requires actual OS ENOSPC");
                    assert_eq!(libc::ENOSPC, 28);
                    break;
                }
            }
        }
        owned
    }
    fn remove(mut self) {
        fs::remove_file(self.0.as_ref().unwrap()).unwrap();
        self.0 = None;
    }
}
impl Drop for Filler {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

fn archive(changes: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut mapping = Vec::new();
    for (index, (name, bytes)) in changes.iter().enumerate() {
        let member = bytes.map(|_| index.to_string());
        if let Some(bytes) = bytes {
            zip.start_file(member.as_ref().unwrap(), options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        mapping.push((*name, member));
    }
    zip.start_file("_meta", options).unwrap();
    zip.write_all(&serde_json::to_vec(&mapping).unwrap()).unwrap();
    zip.finish().unwrap().into_inner()
}

fn request(client: &reqwest::Client, base: &str, family: &str, method: &str, key: &str, bytes: &[u8]) -> reqwest::RequestBuilder {
    client.post(format!("{base}/{family}/{method}"))
        .header("anki-sync", json!({"v":11,"k":key,"c":"synthetic-disk-full","s":"disk-proof"}).to_string())
        .body(zstd::stream::encode_all(bytes, 0).unwrap())
}
async fn decoded(response: reqwest::Response) -> Vec<u8> {
    assert_eq!(response.status(), 200);
    let size: usize = response.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
    let result = zstd::stream::decode_all(within(response.bytes()).await.unwrap().as_ref()).unwrap();
    assert_eq!(result.len(), size);
    result
}
async fn media(client: &reqwest::Client, base: &str, key: &str, method: &str, bytes: &[u8]) -> Value {
    let response = within(request(client, base, "msync", method, key, bytes).send()).await.unwrap();
    let envelope: Value = serde_json::from_slice(&decoded(response).await).unwrap();
    assert_eq!(envelope["err"], "");
    envelope["data"].clone()
}
async fn storage_error(response: reqwest::Response) {
    assert_eq!(response.status(), 503, "disk-full upload must never acknowledge success");
    assert_eq!(within(response.text()).await.unwrap(), "storage_unavailable");
}

#[derive(Debug, PartialEq, Eq)]
struct MetadataState {
    cursor: i64,
    names: Vec<(String, i64, Option<String>)>,
    blobs: Vec<(String, i64, String, Vec<u8>)>,
}
fn persistent(db: &Connection, id: &str) -> MetadataState {
    let cursor = db.query_row("SELECT media_usn FROM accounts WHERE id=?1", [id], |r| r.get(0)).unwrap();
    let mut names = db.prepare("SELECT name,usn,blob FROM media_names WHERE account_id=?1 ORDER BY usn").unwrap();
    let names = names.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap();
    let mut blobs = db.prepare("SELECT blob,size,sha1,sha256 FROM media_blobs WHERE account_id=?1 ORDER BY blob").unwrap();
    let blobs = blobs.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap();
    MetadataState { cursor, names, blobs }
}
fn checkpoint(db: &Connection) {
    // An empty WAL prevents a preallocated WAL from concealing metadata ENOSPC.
    let result = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    }).unwrap();
    assert_eq!(result, (0, 0, 0));
}
fn same_bytes(directory: &Path, expected: &BTreeMap<PathBuf, Vec<u8>>) {
    for (relative, bytes) in expected {
        assert_eq!(&fs::read(directory.join(relative)).unwrap(), bytes);
    }
}
fn digest(bytes: &[u8]) -> String {
    Sha1::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
fn listing(committed: bool) -> Value {
    if committed {
        json!([["gone.bin",4,""],["replace.bin",5,digest(REPLACEMENT)],
            ["added.bin",6,digest(ADDITION)],["delete.bin",7,""]])
    } else {
        json!([["replace.bin",1,digest(OLD)],["delete.bin",2,digest(RETAIN)],["gone.bin",4,""]])
    }
}
async fn http_state(client: &reqwest::Client, base: &str, key: &str, committed: bool) {
    let cursor = if committed { 7 } else { 4 };
    assert_eq!(media(client, base, key, "begin", br#"{"v":"synthetic-disk-full"}"#).await["usn"], cursor);
    assert_eq!(media(client, base, key, "mediaChanges", br#"{"lastUsn":0}"#).await, listing(committed));
    assert_eq!(media(client, base, key, "mediaChanges", &serde_json::to_vec(&json!({"lastUsn":cursor})).unwrap()).await, json!([]));
    assert_eq!(media(client, base, key, "mediaSanity", br#"{"local":2}"#).await, "OK");
    let live = if committed { [("replace.bin", REPLACEMENT), ("added.bin", ADDITION)] }
        else { [("replace.bin", OLD), ("delete.bin", RETAIN)] };
    for (name, expected) in live {
        let response = within(request(client, base, "msync", "downloadFiles", key,
            &serde_json::to_vec(&json!({"files":[name]})).unwrap()).send()).await.unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(decoded(response).await)).unwrap();
        assert_eq!(zip.len(), 2);
        let mut bytes = Vec::new();
        zip.by_name("0").unwrap().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, expected);
        let mapping: BTreeMap<String, String> = serde_json::from_reader(zip.by_name("_meta").unwrap()).unwrap();
        assert_eq!(mapping, BTreeMap::from([("0".into(), name.into())]));
    }
    for missing in ["gone.bin", if committed { "delete.bin" } else { "added.bin" }] {
        let response = within(request(client, base, "msync", "downloadFiles", key,
            &serde_json::to_vec(&json!({"files":[missing]})).unwrap()).send()).await.unwrap();
        assert_eq!(response.status(), 409);
        assert_eq!(within(response.text()).await.unwrap(), "missing_media");
    }
}

async fn child_service(path: &Path, signal: Option<&Path>) -> (Child, String) {
    let mut command = Command::new(binary());
    command.args(["serve", path.to_str().unwrap(), ORIGIN, "127.0.0.1:0"])
        .env_remove("MEGURI_TEST_FAIL_PHASE").env_remove("MEGURI_TEST_KILL_PHASE")
        .env_remove("MEGURI_TEST_SIGNAL_FILE").stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(signal) = signal {
        command.env("MEGURI_TEST_KILL_PHASE", "media_after_metadata_commit")
            .env("MEGURI_TEST_SIGNAL_FILE", signal);
    }
    let mut child = Child(command.spawn().unwrap());
    let stdout = child.0.stdout.take().unwrap();
    let line = within(tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        std::io::BufReader::new(stdout.take(256)).read_line(&mut line).unwrap();
        line
    })).await.unwrap();
    let address = line.trim().strip_prefix("listening ").expect("bounded child listening announcement");
    let address: std::net::SocketAddr = address.parse().unwrap();
    assert_eq!(address.ip(), IP);
    (child, format!("http://{address}"))
}
fn kill(child: &mut Child) {
    child.0.kill().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
}

// One test owns the mount; phases run sequentially. Missing/unsafe fixtures fail
// even without test-hooks. Enable test-hooks for the late publication and real
// lost-acknowledgement boundaries, in addition to the unconditional ENOSPC phase.
#[tokio::test]
async fn actual_tmpfs_enospc_preserves_media_batch_and_collection_across_restart() {
    tokio::time::timeout(Duration::from_secs(180), async {
        let mount = checked_mount();
        for variable in ["MEGURI_TEST_FAIL_PHASE", "MEGURI_TEST_KILL_PHASE", "MEGURI_TEST_SIGNAL_FILE"] {
            assert!(std::env::var_os(variable).is_none(), "disk-full proof requires no ambient fault injection");
        }
        let temp = tempfile::Builder::new().prefix("meguri-media-disk-full-")
            .permissions(fs::Permissions::from_mode(0o700)).tempdir_in(MOUNT).unwrap();
        // Control files never compete with service data or the filler.
        let control = tempfile::tempdir().unwrap();
        assert_ne!(fs::metadata(control.path()).unwrap().dev(), mount.metadata().unwrap().dev());
        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        root.init().unwrap();
        drop(root);
        let h = within(Harness::reopen(temp)).await;
        let identity = within(h.setup()).await;
        let id = identity["id"].as_str().unwrap();
        let response = within(request(&h.client, &h.base, "sync", "hostKey", "",
            &serde_json::to_vec(&json!({"u":"root.admin","p":PASSWORD})).unwrap()).send()).await.unwrap();
        let login: Value = serde_json::from_slice(&decoded(response).await).unwrap();
        let key = login["key"].as_str().unwrap();
        let initial = archive(&[("replace.bin", Some(OLD)), ("delete.bin", Some(RETAIN)), ("gone.bin", Some(b"synthetic past tombstone bytes"))]);
        assert_eq!(media(&h.client, &h.base, key, "uploadChanges", &initial).await, json!([3,3]));
        assert_eq!(media(&h.client, &h.base, key, "uploadChanges", &archive(&[("gone.bin", None)])).await, json!([1,4]));
        http_state(&h.client, &h.base, key, false).await;
        let account = path.join("state/accounts").join(id);
        let marker = fs::read(account.join("current-collection")).unwrap();
        let collections = store_bytes(&account.join("generations"));
        let originals = store_bytes(&account.join("media"));
        assert_eq!(originals.len(), 3);
        // Keep the SQLite handle alive (without a read transaction) so checking
        // committed rows while full cannot itself require a new WAL/SHM file.
        let db = Connection::open_with_flags(h.database(), OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW).unwrap();
        let before = persistent(&db, id);
        assert_eq!(before.cursor, 4);
        assert_eq!(before.names.iter().filter(|(_, _, blob)| blob.is_none()).count(), 1);
        let batch = archive(&[("replace.bin", Some(REPLACEMENT)), ("added.bin", Some(ADDITION)), ("delete.bin", None)]);
        let assert_old = || {
            assert_eq!(persistent(&db, id), before);
            same_bytes(&account.join("media"), &originals);
            assert_eq!(fs::read(account.join("current-collection")).unwrap(), marker);
            assert_eq!(store_bytes(&account.join("generations")), collections);
        };
        let mut remaining = MAX_FILL;

        // Actual filesystem failure during upload staging, with a valid mixed
        // addition/replacement/deletion ZIP already prepared in process memory.
        checkpoint(&db);
        let filler = Filler::exhaust(&h.temp, &mount, &mut remaining);
        let response = within(request(&h.client, &h.base, "msync", "uploadChanges", key, &batch).send()).await.unwrap();
        storage_error(response).await;
        assert_old();
        filler.remove();
        http_state(&h.client, &h.base, key, false).await;

        #[cfg(feature = "test-hooks")]
        {
            // This barrier is reached only after receive validated the entire
            // ZIP and published/synced its immutable blobs. It injects no error.
            let pause = h.service.hooks.arm("media_with_barrier");
            let upload = request(&h.client, &h.base, "msync", "uploadChanges", key, &batch);
            let upload = tokio::spawn(async move { upload.send().await });
            within(pause.arrived()).await;
            let durable = store_bytes(&account.join("media"));
            let mut candidates = durable.iter().filter(|(name, _)| !originals.contains_key(*name)).map(|(name, bytes)| {
                assert!(name.to_str().unwrap().starts_with("blob-"));
                bytes.clone()
            }).collect::<Vec<_>>();
            candidates.sort();
            let mut expected = vec![REPLACEMENT.to_vec(), ADDITION.to_vec()];
            expected.sort();
            assert_eq!(candidates, expected, "both new blobs are durable before metadata publication");
            assert_old();
            checkpoint(&db);
            assert_eq!(fs::metadata(path.join("state/accounts.sqlite-wal")).unwrap().len(), 0);
            let filler = Filler::exhaust(&h.temp, &mount, &mut remaining);
            pause.resume();
            storage_error(within(upload).await.unwrap().unwrap()).await;
            assert_old();
            filler.remove();
            http_state(&h.client, &h.base, key, false).await;
        }
        assert!(remaining < MAX_FILL);
        assert_old();
        drop(db);
        let temp = within(h.stop()).await;

        // Restart the actual service entrypoint on the same persistent bytes.
        // Startup must reconcile unreferenced durable candidates, preserving
        // all historical committed blobs (including the old tombstone's blob).
        let signal = control.path().join("retry-committed-before-ack");
        let hook_signal = if cfg!(feature = "test-hooks") { Some(signal.as_path()) } else { None };
        let (mut child, base) = child_service(&path, hook_signal).await;
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(30)).build().unwrap();
        http_state(&client, &base, key, false).await;
        assert_eq!(store_bytes(&account.join("media")), originals);
        let retry = request(&client, &base, "msync", "uploadChanges", key, &batch);
        #[cfg(feature = "test-hooks")]
        {
            let retry = tokio::spawn(async move { retry.send().await });
            wait_file(&signal, &mut child).await;
            assert_eq!(fs::read(&signal).unwrap(), b"media_after_metadata_commit");
            kill(&mut child);
            assert!(within(retry).await.unwrap().is_err(), "committed retry acknowledgement was lost");
        }
        #[cfg(not(feature = "test-hooks"))]
        {
            let envelope: Value = serde_json::from_slice(&decoded(within(retry.send()).await.unwrap()).await).unwrap();
            assert_eq!(envelope, json!({"data":[3,7],"err":""}));
            kill(&mut child);
        }
        drop(child);
        let (mut child, base) = child_service(&path, None).await;
        http_state(&client, &base, key, true).await;
        let db = Connection::open_with_flags(path.join("state/accounts.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW).unwrap();
        let committed = persistent(&db, id);
        assert_eq!(committed.cursor, 7);
        assert_eq!(committed.names.len(), 4);
        assert_eq!(committed.names.iter().filter(|(_, _, blob)| blob.is_none()).count(), 2);
        assert_eq!(committed.blobs.len(), before.blobs.len() + 2);
        same_bytes(&account.join("media"), &originals);
        // An identical replay after a lost acknowledgement is a successful
        // no-op: no new cursor, names, tombstones or committed blob references.
        assert_eq!(media(&client, &base, key, "uploadChanges", &batch).await, json!([3,7]));
        assert_eq!(persistent(&db, id), committed);
        drop(db);
        kill(&mut child);
        drop(child);
        let (mut child, base) = child_service(&path, None).await;
        http_state(&client, &base, key, true).await;
        let db = Connection::open_with_flags(path.join("state/accounts.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW).unwrap();
        assert_eq!(persistent(&db, id), committed);
        same_bytes(&account.join("media"), &originals);
        assert_eq!(fs::read(account.join("current-collection")).unwrap(), marker);
        assert_eq!(store_bytes(&account.join("generations")), collections);
        drop(db);
        kill(&mut child);
        drop(child);
        temp.close().unwrap(); // Only this test's private directory is removed.
        control.close().unwrap();
    }).await.expect("disk-full proof exceeded total 180-second deadline");
}
