//! U15: the private collection vault, the output guard's candidate strings
//! and the private suite's summary, on synthetic profile folders only
//! (R1938–R1940).
//!
//! What the vault opens is watched from outside it with inotify (Linux, where
//! `tools/mg core-test` runs), so the "never opened" checks do not rest on
//! the vault's own access log. Expected hashes are computed here with sha2.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::deck::{Deck, native_name};
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{DeckConfigId, DeckId, NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck};
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_model::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};
use meguri_model::rng::SeededRandom;
use meguri_model::tag::Tag;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::raw::{PlainDatabase, RawValue};
use meguri_storage::{CollectionBuilder, OpenOptions, StampMode, open_collection};
use meguri_testkit::private::redact::Candidates;
use meguri_testkit::private::suite::{self, Check, CheckCounts, Outcome};
use meguri_testkit::private::vault::{self, CopyStep, Vault, VaultError, VaultOptions};
use sha2::{Digest, Sha256};

// ------------------------------------------------------------------ helpers

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Options for copy tests on a temporary (writable) folder: no CI marker,
/// no read-only mount required.
fn test_options() -> VaultOptions {
    VaultOptions {
        env: vec![("PATH".to_owned(), "/usr/bin".to_owned())],
        require_read_only: false,
    }
}

/// A profile folder as Anki leaves it: the collection, its log and media,
/// plus files the vault must never open (preferences, an add-on's config with
/// an API key, a media database, a backup).
fn profile_folder(root: &Path) {
    fs::write(
        root.join("collection.anki2"),
        b"SQLite format 3\0 stand-in collection bytes",
    )
    .unwrap();
    fs::write(root.join("collection.anki2-wal"), b"stand-in log").unwrap();
    let media = root.join("collection.media");
    fs::create_dir(&media).unwrap();
    fs::write(media.join("a.jpg"), vec![7u8; 3000]).unwrap();
    fs::write(media.join("b c.mp3"), vec![9u8; 120]).unwrap();
    // Not copied: a subfolder (Anki ignores them) and a symlink that leads
    // to the preferences.
    fs::create_dir(media.join("sub")).unwrap();
    fs::write(media.join("sub").join("deep.png"), b"deep").unwrap();
    std::os::unix::fs::symlink("../prefs21.db", media.join("link.db")).unwrap();
    fs::write(root.join("prefs21.db"), b"preferences with a sync key").unwrap();
    fs::write(root.join("collection.media.db2"), b"media database").unwrap();
    let addon = root.join("addons21").join("1234567");
    fs::create_dir_all(&addon).unwrap();
    fs::write(addon.join("meta.json"), br#"{"config": {"api_key": "secret"}}"#).unwrap();
    fs::create_dir(root.join("backups")).unwrap();
    fs::write(root.join("backups").join("backup-1.colpkg"), b"backup").unwrap();
}

/// Every regular file under `dir`, as relative paths.
fn files_under(dir: &Path) -> BTreeSet<String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                walk(base, &entry.path(), out);
            } else {
                out.insert(entry.path().strip_prefix(base).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

/// Size, modification time and SHA-256 of every file under `dir`.
fn snapshot(dir: &Path) -> Vec<(String, u64, std::time::SystemTime, String)> {
    files_under(dir)
        .into_iter()
        .filter(|rel| !fs::symlink_metadata(dir.join(rel)).unwrap().file_type().is_symlink())
        .map(|rel| {
            let path = dir.join(&rel);
            let meta = fs::metadata(&path).unwrap();
            let hash = sha256_hex(&fs::read(&path).unwrap());
            (rel, meta.len(), meta.modified().unwrap(), hash)
        })
        .collect()
}

// --------------------------------------------------------------- the vault

#[test]
fn a_copy_holds_only_the_collection_its_log_and_media_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    profile_folder(&root);
    let before = snapshot(&root);
    let dest = dir.path().join("build").join("private").join("run-1");

    let report = Vault::open(&root, &test_options()).unwrap().copy_to(&dest).unwrap();

    let expected: BTreeSet<String> = [
        "collection.anki2",
        "collection.anki2-wal",
        "collection.media/a.jpg",
        "collection.media/b c.mp3",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(files_under(&dest), expected);
    for rel in &expected {
        assert_eq!(
            fs::read(dest.join(rel)).unwrap(),
            fs::read(root.join(rel)).unwrap(),
            "{rel}"
        );
    }
    let collection = fs::read(root.join("collection.anki2")).unwrap();
    assert_eq!(report.collection.sha256, sha256_hex(&collection));
    assert_eq!(report.collection.bytes, collection.len() as u64);
    let wal = report.wal.as_ref().unwrap();
    assert_eq!(
        (wal.bytes, wal.sha256.as_str()),
        (12, sha256_hex(b"stand-in log").as_str())
    );
    assert_eq!(
        (report.media_files, report.media_bytes, report.media_skipped),
        (2, 3120, 2)
    );
    // The source is untouched: same files, sizes, modification times and
    // hashes.
    assert_eq!(snapshot(&root), before);
}

#[test]
fn a_copy_without_a_log_or_media_folder_holds_the_collection_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("collection.anki2"), b"collection").unwrap();
    fs::write(root.join("prefs21.db"), b"preferences").unwrap();
    let dest = dir.path().join("copy");

    let report = Vault::open(&root, &test_options()).unwrap().copy_to(&dest).unwrap();

    assert_eq!(files_under(&dest), BTreeSet::from(["collection.anki2".to_owned()]));
    assert!(report.wal.is_none());
    assert_eq!((report.media_files, report.media_skipped), (0, 0));
    assert_eq!(report.access, vec!["collection.anki2"]);
}

#[test]
fn the_access_log_names_only_the_collection_and_media() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    profile_folder(&root);

    let report = Vault::open(&root, &test_options())
        .unwrap()
        .copy_to(&dir.path().join("copy"))
        .unwrap();

    assert_eq!(
        report.access,
        vec![
            "collection.anki2",
            "collection.anki2-wal",
            "collection.media/",
            "collection.media/a.jpg",
            "collection.media/b c.mp3",
        ]
    );
}

/// inotify watches on the profile folder and the folders the vault must not
/// enter: every open, read or listing there shows up as an event.
#[cfg(target_os = "linux")]
mod watch {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub struct Watch {
        fd: i32,
        dirs: Vec<(i32, String)>,
    }

    /// One event: the watched folder (as given) and the name inside it, empty
    /// when the folder itself was opened or read.
    pub type Event = (String, String);

    impl Watch {
        pub fn new() -> Watch {
            // SAFETY: plain syscall without pointers.
            let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            assert!(fd >= 0, "inotify_init1: {}", std::io::Error::last_os_error());
            Watch { fd, dirs: Vec::new() }
        }

        pub fn add(&mut self, dir: &Path, label: &str) {
            let path = CString::new(dir.as_os_str().as_bytes()).unwrap();
            // SAFETY: `path` is NUL-terminated and outlives the call.
            let wd = unsafe { libc::inotify_add_watch(self.fd, path.as_ptr(), libc::IN_OPEN | libc::IN_ACCESS) };
            assert!(wd >= 0, "inotify_add_watch: {}", std::io::Error::last_os_error());
            self.dirs.push((wd, label.to_owned()));
        }

        pub fn events(&self) -> Vec<Event> {
            let mut out = Vec::new();
            let mut buffer = vec![0u8; 1 << 16];
            loop {
                // SAFETY: the buffer is valid for its length.
                let n = unsafe { libc::read(self.fd, buffer.as_mut_ptr().cast(), buffer.len()) };
                if n <= 0 {
                    break;
                }
                let mut at = 0usize;
                while at < n as usize {
                    let wd = i32::from_ne_bytes(buffer[at..at + 4].try_into().unwrap());
                    let len = u32::from_ne_bytes(buffer[at + 12..at + 16].try_into().unwrap()) as usize;
                    let raw = &buffer[at + 16..at + 16 + len];
                    let name = String::from_utf8_lossy(raw).trim_end_matches('\0').to_owned();
                    let dir = self
                        .dirs
                        .iter()
                        .find(|(w, _)| *w == wd)
                        .map(|(_, label)| label.clone())
                        .unwrap_or_default();
                    out.push((dir, name));
                    at += 16 + len;
                }
            }
            out
        }
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            // SAFETY: the descriptor is ours.
            unsafe { libc::close(self.fd) };
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_vault_never_opens_other_files_of_the_profile() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    profile_folder(&root);
    let mut watch = watch::Watch::new();
    watch.add(&root, "profile");
    watch.add(&root.join("addons21"), "addons21");
    watch.add(&root.join("addons21").join("1234567"), "addon");
    watch.add(&root.join("backups"), "backups");
    watch.add(&root.join("collection.media").join("sub"), "media-sub");
    // Sanity: the watch sees an open of a profile file.
    drop(fs::File::open(root.join("collection.anki2")).unwrap());
    assert!(
        watch
            .events()
            .contains(&("profile".to_owned(), "collection.anki2".to_owned()))
    );

    Vault::open(&root, &test_options())
        .unwrap()
        .copy_to(&dir.path().join("copy"))
        .unwrap();

    let events = watch.events();
    assert!(!events.is_empty(), "the watch saw the copy");
    let allowed = ["collection.anki2", "collection.anki2-wal", "collection.media"];
    for (folder, name) in &events {
        assert!(
            folder == "profile" && allowed.contains(&name.as_str()),
            "the vault opened {folder}/{name}"
        );
    }
}

#[test]
fn a_writable_source_is_refused_when_a_read_only_mount_is_required() {
    let dir = tempfile::tempdir().unwrap();
    profile_folder(dir.path());
    let options = VaultOptions {
        require_read_only: true,
        ..test_options()
    };

    let err = Vault::open(dir.path(), &options).unwrap_err();

    assert!(matches!(err, VaultError::NotReadOnly), "{err}");
}

/// The read-only test agrees with the kernel's own list of mounts: a mount
/// `/proc/self/mountinfo` lists with `ro` counts as read-only, the temporary
/// folder does not.
#[cfg(target_os = "linux")]
#[test]
fn the_read_only_test_agrees_with_mountinfo() {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").unwrap();
    let read_only: Vec<std::path::PathBuf> = mountinfo
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let (point, options) = (fields.get(4)?, fields.get(5)?);
            options
                .split(',')
                .any(|o| o == "ro")
                .then(|| std::path::PathBuf::from(point.replace("\\040", " ")))
        })
        .filter(|point| point.is_dir())
        .collect();
    assert!(!read_only.is_empty(), "no read-only mount to compare with");
    for point in &read_only {
        assert!(vault::is_read_only_mount(point).unwrap(), "{}", point.display());
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(!vault::is_read_only_mount(dir.path()).unwrap());
}

#[test]
fn a_ci_marker_refuses_the_vault_before_anything_is_copied() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    profile_folder(&root);
    for (name, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("CI", "false"),
        ("JENKINS_URL", "http://ci"),
    ] {
        let options = VaultOptions {
            env: vec![
                ("PATH".to_owned(), "/bin".to_owned()),
                (name.to_owned(), value.to_owned()),
            ],
            require_read_only: false,
        };
        let err = Vault::open(&root, &options).unwrap_err();
        assert!(
            matches!(&err, VaultError::CiMarker(marker) if marker == name),
            "{name}={value}: {err}"
        );
    }
    assert!(!dir.path().join("copy").exists());
}

#[test]
fn ci_markers_are_names_from_the_list_with_a_value() {
    assert_eq!(vault::ci_marker([("CI", "1")]), Some("CI".to_owned()));
    assert_eq!(vault::ci_marker([("TF_BUILD", "True")]), Some("TF_BUILD".to_owned()));
    assert_eq!(vault::ci_marker([("CI", "")]), None);
    assert_eq!(vault::ci_marker([("CIRCLE", "1"), ("PATH", "/bin")]), None);
    assert_eq!(vault::ci_marker([("ci", "1")]), None);
}

#[test]
fn folders_that_are_not_a_plain_profile_are_refused() {
    let dir = tempfile::tempdir().unwrap();

    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    fs::write(empty.join("prefs21.db"), b"preferences").unwrap();
    assert!(matches!(
        Vault::open(&empty, &test_options()).unwrap_err(),
        VaultError::NoCollection
    ));

    // A collection that is a symlink could lead anywhere.
    let linked = dir.path().join("linked");
    fs::create_dir(&linked).unwrap();
    fs::write(linked.join("prefs21.db"), b"preferences").unwrap();
    std::os::unix::fs::symlink("prefs21.db", linked.join("collection.anki2")).unwrap();
    assert!(matches!(
        Vault::open(&linked, &test_options()).unwrap_err(),
        VaultError::NotAllowed(_)
    ));

    // So could a media folder that is a symlink.
    let media_link = dir.path().join("media-link");
    fs::create_dir(&media_link).unwrap();
    fs::write(media_link.join("collection.anki2"), b"collection").unwrap();
    fs::create_dir(dir.path().join("elsewhere")).unwrap();
    std::os::unix::fs::symlink("../elsewhere", media_link.join("collection.media")).unwrap();
    let vault = Vault::open(&media_link, &test_options()).unwrap();
    assert!(matches!(
        vault.copy_to(&dir.path().join("copy-a")).unwrap_err(),
        VaultError::NotAllowed(_)
    ));

    // An existing destination is never written into.
    let plain = dir.path().join("plain");
    fs::create_dir(&plain).unwrap();
    fs::write(plain.join("collection.anki2"), b"collection").unwrap();
    let taken = dir.path().join("taken");
    fs::create_dir(&taken).unwrap();
    assert!(matches!(
        Vault::open(&plain, &test_options())
            .unwrap()
            .copy_to(&taken)
            .unwrap_err(),
        VaultError::DestinationExists
    ));
    assert_eq!(fs::read_dir(&taken).unwrap().count(), 0);
}

// ------------------------------------------- the collection and its log

/// A collection in WAL mode as a running Anki leaves it: rows 1 to 10
/// checkpointed into the database file, rows 11 to 20 only in its log. The
/// connection returned is Anki's: while it is open, the log is not folded
/// into the database.
fn wal_source(root: &Path) -> PlainDatabase {
    let path = root.join("collection.anki2");
    fs::write(&path, b"").unwrap();
    let db = PlainDatabase::open(&path).unwrap();
    db.rows("PRAGMA journal_mode = WAL", &[]).unwrap();
    db.rows("PRAGMA wal_autocheckpoint = 0", &[]).unwrap();
    db.execute_batch("CREATE TABLE t (n INTEGER)").unwrap();
    for n in 1..=20 {
        if n == 11 {
            checkpoint(&db);
        }
        db.execute("INSERT INTO t VALUES (?)", &[RawValue::Integer(n)]).unwrap();
    }
    assert!(
        fs::metadata(root.join("collection.anki2-wal")).unwrap().len() > 0,
        "rows 11 to 20 are in the log"
    );
    db
}

/// A checkpoint as Anki's: the log folded into the database and truncated.
fn checkpoint(db: &PlainDatabase) {
    db.rows("PRAGMA wal_checkpoint(TRUNCATE)", &[]).unwrap();
}

/// The rows a copied collection holds, read as SQLite reads it (its log
/// included).
fn rows_in(copy: &Path) -> Vec<i64> {
    let db = PlainDatabase::open(&copy.join("collection.anki2")).unwrap();
    db.rows("SELECT n FROM t ORDER BY n", &[])
        .unwrap()
        .into_iter()
        .map(|row| match row[0] {
            RawValue::Integer(n) => n,
            ref other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_checkpoint_between_the_collection_and_its_log_starts_the_copy_over() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    let anki = wal_source(&root);
    let dest = dir.path().join("copy");
    let mut steps = Vec::new();

    let report = Vault::open(&root, &test_options())
        .unwrap()
        .copy_to_observed(&dest, &mut |step| {
            // Once, after the database file is copied and before its log is
            // read: Anki commits row 21 and checkpoints, truncating the log.
            if steps.is_empty() {
                anki.execute("INSERT INTO t VALUES (21)", &[]).unwrap();
                checkpoint(&anki);
            }
            steps.push(step);
        })
        .unwrap();

    // Every committed row: the old database file with the new, empty log
    // would hold rows 1 to 10 only.
    assert_eq!(rows_in(&dest), (1..=21).collect::<Vec<_>>());
    assert_eq!(
        steps,
        vec![
            CopyStep::CollectionCopied { attempt: 1 },
            CopyStep::CollectionCopied { attempt: 2 }
        ]
    );
    assert_eq!(
        report.collection.sha256,
        sha256_hex(&fs::read(root.join("collection.anki2")).unwrap())
    );
    assert_eq!(report.wal.as_ref().map(|wal| wal.bytes), Some(0));
    drop(anki);
}

#[test]
fn a_log_folded_in_and_removed_between_the_two_files_starts_the_copy_over() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    let mut anki = Some(wal_source(&root));
    let dest = dir.path().join("copy");
    let mut attempts = 0;

    let report = Vault::open(&root, &test_options())
        .unwrap()
        .copy_to_observed(&dest, &mut |_| {
            attempts += 1;
            // Anki quits: its last connection checkpoints and deletes the log.
            if let Some(db) = anki.take() {
                db.close().unwrap();
                assert!(!root.join("collection.anki2-wal").exists());
            }
        })
        .unwrap();

    assert_eq!(rows_in(&dest), (1..=20).collect::<Vec<_>>());
    assert_eq!(attempts, 2);
    assert!(report.wal.is_none());
    assert_eq!(
        report.access,
        vec!["collection.anki2", "collection.anki2-wal", "collection.anki2"]
    );
}

#[test]
fn a_collection_that_changes_at_every_attempt_is_refused_and_no_copy_is_left() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("profile");
    fs::create_dir(&root).unwrap();
    let anki = wal_source(&root);
    let dest = dir.path().join("copy");
    let mut n = 20;

    let err = Vault::open(&root, &test_options())
        .unwrap()
        .copy_to_observed(&dest, &mut |_| {
            n += 1;
            anki.execute("INSERT INTO t VALUES (?)", &[RawValue::Integer(n)])
                .unwrap();
        })
        .unwrap_err();

    assert!(
        matches!(err, VaultError::CollectionBusy { attempts } if attempts == vault::COLLECTION_ATTEMPTS),
        "{err}"
    );
    assert_eq!(n, 20 + i64::from(vault::COLLECTION_ATTEMPTS));
    assert!(!dest.exists(), "a refused copy leaves nothing");
    // The message names no file of the collection beyond the vault's own.
    assert!(err.to_string().contains("collection.anki2"));
}

// ------------------------------------------------- the guard's candidates

const CRT: i64 = 1_893_470_400;

fn hand_deck(id: i64, human: &str) -> Deck {
    let mut normal = NormalDeck::default();
    normal.config_id = 1;
    Deck {
        id: DeckId(id),
        name: native_name(human),
        mtime: EpochSecs(CRT),
        usn: Usn(0),
        common: ProtoBlob::new(DeckCommon::default()),
        kind: ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))),
    }
}

fn hand_notetype() -> Notetype {
    let mut template = TemplateConfig::default();
    template.q_format = "{{Front}}".to_owned();
    template.a_format = "{{Back}}".to_owned();
    let field = |ord: u32, name: &str| NotetypeField {
        ord,
        name: name.to_owned(),
        config: ProtoBlob::new(FieldConfig::default()),
        prior_ord: None,
    };
    Notetype {
        id: NotetypeId(100),
        name: "Basic".to_owned(),
        mtime: EpochSecs(CRT),
        usn: Usn(0),
        config: ProtoBlob::new(NotetypeConfig::default()),
        fields: vec![field(0, "Front"), field(1, "Back")],
        templates: vec![NotetypeTemplate {
            ord: 0,
            name: "Card 1".to_owned(),
            mtime: EpochSecs(0),
            usn: Usn(0),
            config: ProtoBlob::new(template),
            prior_ord: None,
        }],
    }
}

fn hand_note(id: i64, fields: &[&str], tags: &str) -> Note {
    Note {
        id: NoteId(id),
        guid: format!("g{id}"),
        notetype_id: NotetypeId(100),
        mtime: EpochSecs(id / 1000),
        usn: Usn(0),
        tags: tags.to_owned(),
        fields: fields.join("\u{1f}"),
        sort_field: fields[0].to_owned(),
        checksum: 0,
        flags: 0,
        data: String::new(),
    }
}

fn tag(name: &str) -> Tag {
    Tag {
        name: name.to_owned(),
        usn: Usn(0),
        collapsed: false,
        config: None,
    }
}

/// A collection with known decks, tags, notes and media in `dir`.
fn hand_collection(dir: &Path) {
    let clock = FixedClock::new(CRT * 1000, UtcOffset::UTC);
    let random = SeededRandom::new(5);
    let col = Col {
        version: 18,
        created: EpochSecs(CRT),
        modified: EpochMillis(CRT * 1000),
        schema_modified: EpochMillis(CRT * 1000),
        ..Col::default()
    };
    let config = vec![ConfigEntry {
        key: "schedVer".to_owned(),
        value: b"2".to_vec(),
        ..ConfigEntry::default()
    }];
    let builder = CollectionBuilder::create(
        &dir.join("collection.anki2"),
        &col,
        &config,
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    {
        let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
        scope
            .put(&DeckConfig {
                id: DeckConfigId(1),
                name: "Default".to_owned(),
                mtime: EpochSecs(CRT),
                usn: Usn(0),
                config: ProtoBlob::new(DeckConfigMessage::default()),
            })
            .unwrap();
        scope.put(&hand_deck(1, "Default")).unwrap();
        scope.put(&hand_deck(10, "Japanisch")).unwrap();
        scope.put(&hand_deck(11, "Japanisch::Hörverstehen")).unwrap();
        scope.put_notetype(&hand_notetype()).unwrap();
        scope.put(&tag("wortart::nomen")).unwrap();
        scope.put(&tag("leftover")).unwrap();
        scope
            .put(&hand_note(
                CRT * 1000 + 1,
                &[
                    "<b>Schmetterling</b> fliegt&nbsp;hoch",
                    "<img src=\"falter.jpg\"> [sound:Schmetterling-Aussprache.mp3]",
                ],
                " wortart::nomen tiere ",
            ))
            .unwrap();
        scope
            .put(&hand_note(CRT * 1000 + 2, &["蝶々が飛ぶ", "ab, cd. Wiese"], ""))
            .unwrap();
        scope.commit().unwrap();
    }
    builder.finish().unwrap();
    let media = dir.join("collection.media");
    fs::create_dir(&media).unwrap();
    fs::write(media.join("falter.jpg"), b"jpeg").unwrap();
    fs::write(media.join("unbenutzt.png"), b"png").unwrap();
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn candidates_hold_deck_names_tags_media_names_and_field_words() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    fs::create_dir(&live).unwrap();
    hand_collection(&live);
    // Copied while the collection is open, as a profile of a running Anki
    // is: the last write is still in the write-ahead log.
    let copy = dir.path().join("copy");
    fs::create_dir(&copy).unwrap();
    {
        let clock = FixedClock::new(CRT * 1000, UtcOffset::UTC);
        let random = SeededRandom::new(5);
        let open = open_collection(&live.join("collection.anki2"), &OpenOptions::new(&clock, &random)).unwrap();
        open.writer()
            .execute_raw(
                "INSERT INTO tags (tag, usn, collapsed) VALUES ('aus-dem-log', 0, 0)",
                &[],
            )
            .unwrap();
        for name in ["collection.anki2", "collection.anki2-wal"] {
            fs::copy(live.join(name), copy.join(name)).unwrap();
        }
        open.close().unwrap();
    }
    copy_dir(&live.join("collection.media"), &copy.join("collection.media"));
    assert!(fs::metadata(copy.join("collection.anki2-wal")).unwrap().len() > 0);
    let before = snapshot(&copy);

    let found = Candidates::extract(&copy).unwrap();

    // The copy it read is left as the vault made it, log included.
    assert_eq!(snapshot(&copy), before);

    assert_eq!(
        found.decks,
        set(&["Default", "Japanisch", "Japanisch::Hörverstehen", "Hörverstehen"])
    );
    assert_eq!(
        found.tags,
        set(&["wortart::nomen", "wortart", "nomen", "leftover", "tiere", "aus-dem-log"])
    );
    assert_eq!(
        found.media,
        set(&["falter.jpg", "unbenutzt.png", "Schmetterling-Aussprache.mp3"])
    );
    // Markup is removed and entities decoded; text splits at white space and
    // ASCII punctuation; chunks shorter than two characters are dropped.
    for word in [
        "Schmetterling",
        "fliegt",
        "hoch",
        "蝶々が飛ぶ",
        "ab",
        "cd",
        "Wiese",
        "sound",
        "Aussprache",
        "mp3",
    ] {
        assert!(found.words.contains(word), "{word}");
    }
    assert!(!found.words.iter().any(|w| w.contains('<') || w.contains("nbsp")));
}

#[test]
fn the_candidates_file_is_one_json_object_per_string() {
    let found = Candidates {
        decks: set(&["A::B"]),
        tags: set(&["t"]),
        media: set(&["x \"y\".jpg"]),
        words: set(&["Wort"]),
    };
    let mut out = Vec::new();
    found.write_jsonl(&mut out).unwrap();
    let lines: Vec<serde_json::Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines,
        vec![
            serde_json::json!({"kind": "deck", "text": "A::B"}),
            serde_json::json!({"kind": "tag", "text": "t"}),
            serde_json::json!({"kind": "media", "text": "x \"y\".jpg"}),
            serde_json::json!({"kind": "word", "text": "Wort"}),
        ]
    );
}

// --------------------------------------------------------- suite summary

struct Fixed(&'static str, Result<CheckCounts, String>);

impl Check for Fixed {
    fn name(&self) -> &'static str {
        self.0
    }
    fn run(&self, _copy: &Path) -> Result<CheckCounts, String> {
        self.1.clone()
    }
}

#[test]
fn the_summary_holds_check_names_and_counts_only() {
    let next_states = Fixed(
        "next-states",
        Ok(CheckCounts {
            passed: 41,
            failed: 0,
            ..CheckCounts::default()
        }),
    );
    let renders = Fixed(
        "render",
        Ok(CheckCounts {
            passed: 9,
            failed: 2,
            ..CheckCounts::default()
        }),
    );
    let broken = Fixed(
        "queues",
        Err("deck Japanisch::Hörverstehen could not be built".to_owned()),
    );
    let mut log = Vec::new();

    let summary = suite::run(
        &[&next_states, &renders, &broken],
        Path::new("/nowhere"),
        &mut |name, text| log.push(format!("{name}: {text}")),
    );

    assert_eq!(
        summary.to_json(),
        serde_json::json!({
            "checks": {
                "next-states": {"failed": 0, "passed": 41},
                "queues": {"error": true},
                "render": {"failed": 2, "passed": 9},
            },
            "report_version": 1,
        })
    );
    assert!(!summary.passed());
    // The error text, which may quote the collection, reaches only the log.
    assert_eq!(log, vec!["queues: deck Japanisch::Hörverstehen could not be built"]);
    assert!(!summary.to_json().to_string().contains("Japanisch"));
    assert_eq!(summary.checks["queues"], Outcome::Error);
}

#[test]
fn a_suite_passes_when_no_check_failed() {
    let a = Fixed(
        "a",
        Ok(CheckCounts {
            passed: 3,
            failed: 0,
            ..CheckCounts::default()
        }),
    );
    let summary = suite::run(&[&a], Path::new("/nowhere"), &mut |_, _| {});
    assert!(summary.passed());
    assert!(suite::run(&[], Path::new("/nowhere"), &mut |_, _| {}).passed());
}

#[test]
#[should_panic(expected = "is not [a-z0-9-]+")]
fn a_check_name_with_other_characters_is_refused() {
    let bad = Fixed("Deck Names", Ok(CheckCounts::default()));
    suite::run(&[&bad], Path::new("/nowhere"), &mut |_, _| {});
}

#[test]
#[should_panic(expected = "two checks are named")]
fn two_checks_with_one_name_are_refused() {
    let a = Fixed("a", Ok(CheckCounts::default()));
    let b = Fixed("a", Ok(CheckCounts::default()));
    suite::run(&[&a, &b], Path::new("/nowhere"), &mut |_, _| {});
}

#[test]
fn check_breakdowns_hold_only_fixed_categories_and_numeric_counts() {
    let check = Fixed(
        "io-roundtrip",
        Ok(CheckCounts {
            passed: 4,
            failed: 2,
            breakdown: [("round-tripped", 4), ("malformed_shape", 2)].into(),
        }),
    );
    let summary = suite::run(&[&check], Path::new("/nowhere"), &mut |_, _| {});
    assert_eq!(
        summary.to_json(),
        serde_json::json!({
            "report_version": 1, "checks": {"io-roundtrip": {
                "passed": 4, "failed": 2, "breakdown": {"round-tripped": 4, "malformed_shape": 2}
            }}
        })
    );
    assert!(!summary.passed());
}

#[test]
#[should_panic(expected = "is not [a-z0-9_-]+")]
fn a_breakdown_key_with_other_characters_is_refused() {
    let check = Fixed(
        "io-roundtrip",
        Ok(CheckCounts {
            breakdown: [("Private Field Text", 1)].into(),
            ..CheckCounts::default()
        }),
    );
    suite::run(&[&check], Path::new("/nowhere"), &mut |_, _| {});
}
