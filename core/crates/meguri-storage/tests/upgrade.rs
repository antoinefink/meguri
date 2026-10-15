//! The upgrade's own guarantees (`storage/schema-upgrade` U2, U3, R1480):
//! the pre-upgrade copy is made first, a crash or failure inside the
//! transaction leaves the original as it was, and the next open upgrades
//! again. What the upgrade makes of each file is compared with the oracle by
//! the scenarios under `conformance/scenarios/storage/upgrade/`.

mod common;

use std::cell::RefCell;
use std::fs;
use std::process::Command;

use common::{copy_fixture, exec, int, listing, open, plain, sha256, text};
use meguri_model::faultpoint::{self, Fault};
use meguri_model::rng::SeededRandom;
use meguri_storage::{OpenError, OpenEvent, OpenOptions, UpgradeStage, open_collection};

const KILL_DIR: &str = "MEGURI_TEST_UPGRADE_KILL_DIR";

#[test]
fn every_supported_version_upgrades_to_18_with_a_copy_of_the_original() {
    for version in [11, 14, 15, 16, 17] {
        let fixture = format!("schema-{version}.anki2");
        let (dir, path) = copy_fixture(&fixture);
        let original = sha256(&path);
        let opened = open(&path).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        assert_eq!(opened.upgraded_from(), Some(version));
        assert_eq!(int(&exec(opened.writer(), "SELECT ver FROM col")), 18);
        assert_eq!(text(&exec(opened.writer(), "SELECT * FROM pragma_journal_mode")), "wal");
        assert_eq!(
            text(&exec(opened.writer(), "SELECT * FROM pragma_integrity_check")),
            "ok"
        );

        let copy = dir.path().join(format!("collection.anki2.pre-upgrade-v{version}"));
        assert_eq!(opened.pre_upgrade_copy(), Some(copy.as_path()));
        opened.close().unwrap();
        assert!(!listing(dir.path()).iter().any(|name| name.ends_with(".tmp")));

        // The copy holds the original collection: open a copy of the copy.
        let (_check_dir, check) = common::copy_fixture(&fixture);
        fs::copy(&copy, &check).unwrap();
        let reopened = open(&check).unwrap();
        assert_eq!(
            reopened.upgraded_from(),
            Some(version),
            "{fixture}: the copy is at {version}"
        );
        reopened.close().unwrap();
        assert_ne!(sha256(&path), original, "the upgrade rewrote the collection");
    }
}

#[test]
fn a_file_at_18_opens_without_an_upgrade_or_a_copy() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    assert_eq!(opened.upgraded_from(), None);
    assert_eq!(opened.pre_upgrade_copy(), None);
    opened.close().unwrap();
    assert_eq!(
        listing(dir.path()).into_iter().collect::<Vec<_>>(),
        ["collection.anki2"]
    );
}

#[test]
fn each_step_is_reported_as_it_starts() {
    for (version, steps) in [(11, vec![14, 15, 16, 17, 18]), (15, vec![16, 17, 18]), (17, vec![18])] {
        let (_dir, path) = copy_fixture(&format!("schema-{version}.anki2"));
        let events = RefCell::new(Vec::new());
        let record = |event: OpenEvent| events.borrow_mut().push(event);
        let clock = common::clock();
        let random = SeededRandom::new(1);
        open_collection(&path, &OpenOptions::new(&clock, &random).progress(&record)).unwrap();
        let mut expected = vec![OpenEvent::Opening];
        expected.extend(
            steps
                .into_iter()
                .map(|step| OpenEvent::Upgrading { from: version, step }),
        );
        assert_eq!(events.into_inner(), expected, "from {version}");
    }
}

#[test]
fn a_failure_inside_the_transaction_rolls_back_and_the_next_open_upgrades() {
    // A WAL-mode file, so the journal-mode switch before the transaction
    // changes no byte either.
    let (dir, path) = copy_fixture("schema-14.anki2");
    let original = sha256(&path);
    faultpoint::arm(
        "storage.upgrade.in_transaction",
        Fault::IoError(std::io::ErrorKind::Other),
    );
    let error = open(&path).unwrap_err();
    faultpoint::disarm_all();
    assert!(
        matches!(
            error,
            OpenError::UpgradeFailed {
                version: 14,
                stage: UpgradeStage::Step(15),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(sha256(&path), original);
    assert!(dir.path().join("collection.anki2.pre-upgrade-v14").exists());
    let reopened = open(&path).unwrap();
    assert_eq!(reopened.upgraded_from(), Some(14));
    reopened.close().unwrap();
    assert_ne!(sha256(&path), original);
}

#[test]
fn a_copy_that_cannot_be_made_fails_before_the_original_is_touched() {
    let (dir, path) = copy_fixture("schema-11.anki2");
    // A directory where the copy's temporary file must go.
    fs::create_dir(dir.path().join("collection.anki2.pre-upgrade-v11.tmp")).unwrap();
    let original = sha256(&path);
    let before = listing(dir.path());
    let error = open(&path).unwrap_err();
    assert!(
        matches!(
            error,
            OpenError::UpgradeFailed {
                version: 11,
                stage: UpgradeStage::Copy,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(sha256(&path), original);
    assert_eq!(listing(dir.path()), before);
}

/// Runs only as the child of `killed_mid_transaction_...`: arms the fault
/// point to abort the process, then opens the collection in `KILL_DIR`.
#[test]
fn child_upgrade_until_killed() {
    let Some(dir) = std::env::var_os(KILL_DIR) else {
        return;
    };
    faultpoint::arm("storage.upgrade.in_transaction", Fault::Abort);
    let path = std::path::Path::new(&dir).join("collection.anki2");
    let _ = open(&path);
    unreachable!("the fault point did not abort the process");
}

#[test]
fn killed_mid_transaction_leaves_the_original_intact_and_the_next_open_upgrades() {
    // From 14, so step 15 has run when the process dies; a WAL-mode file, so
    // the main file must keep every byte.
    let (dir, path) = copy_fixture("schema-14.anki2");
    let original = sha256(&path);
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_upgrade_until_killed", "--nocapture"])
        .env(KILL_DIR, dir.path())
        .status()
        .unwrap();
    assert!(!status.success(), "the child was not killed");

    assert_eq!(sha256(&path), original, "the original changed");
    assert!(dir.path().join("collection.anki2.pre-upgrade-v14").exists());
    let reopened = open(&path).unwrap();
    assert_eq!(reopened.upgraded_from(), Some(14));
    assert_eq!(int(&exec(reopened.writer(), "SELECT ver FROM col")), 18);
    assert_eq!(
        text(&exec(reopened.writer(), "SELECT * FROM pragma_integrity_check")),
        "ok"
    );
    reopened.close().unwrap();
}

#[test]
fn an_original_killed_before_the_writer_opened_is_byte_for_byte_intact() {
    let (dir, path) = copy_fixture("schema-11.anki2");
    let original = sha256(&path);
    faultpoint::arm("storage.upgrade.after_copy", Fault::IoError(std::io::ErrorKind::Other));
    let error = open(&path).unwrap_err();
    faultpoint::disarm_all();
    assert!(
        matches!(
            error,
            OpenError::UpgradeFailed {
                version: 11,
                stage: UpgradeStage::Copy,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(sha256(&path), original);
    assert!(dir.path().join("collection.anki2.pre-upgrade-v11").exists());
}

/// Foreign objects (`storage/schema-18` S6) on the tables that steps 17 and
/// 18 rebuild: indexes and triggers on them, views that read them, and the
/// add-on table the triggers write to.
const FOREIGN_ON_GRAVES: [&str; 4] = [
    "CREATE TABLE addon_log (what text)",
    "CREATE INDEX addon_graves_oid ON graves (oid)",
    "CREATE TRIGGER addon_grave_deleted AFTER DELETE ON graves BEGIN INSERT INTO addon_log VALUES ('grave'); END",
    "CREATE VIEW addon_graves AS SELECT oid, type FROM graves",
];
const FOREIGN_ON_TAGS: [&str; 3] = [
    "CREATE INDEX addon_tags_usn ON tags (usn)",
    "CREATE TRIGGER addon_tag_added AFTER INSERT ON tags BEGIN INSERT INTO addon_log VALUES (new.tag); END",
    "CREATE VIEW addon_tags AS SELECT tag FROM tags",
];

/// `(type, name, tbl_name, sql)` of the `addon_*` objects, as stored.
type Stored = Vec<(String, String, String, Option<String>)>;

fn addon_objects(conn: &meguri_storage::rusqlite::Connection) -> Stored {
    let mut statement = conn
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name LIKE 'addon%' ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn foreign_objects_on_the_rebuilt_tables_are_kept() {
    // U12: the upgrade keeps foreign objects, including those that depend on
    // `tags` (rebuilt by step 17) and `graves` (rebuilt by step 18).
    for version in [11, 14, 15, 16, 17] {
        let (_dir, path) = copy_fixture(&format!("schema-{version}.anki2"));
        let before = {
            let conn = plain(&path);
            // An index on `tags` holds its key, which `unicase` orders.
            conn.create_collation("unicase", meguri_text::fold::compare_folded)
                .unwrap();
            let tags = if version >= 14 { &FOREIGN_ON_TAGS[..] } else { &[] };
            for sql in FOREIGN_ON_GRAVES.iter().chain(tags) {
                conn.execute_batch(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
            }
            addon_objects(&conn)
        };
        let opened = open(&path).unwrap_or_else(|e| panic!("from {version}: {e}"));
        let w = opened.writer();
        let mut after: Stored = w
            .schema_objects()
            .unwrap()
            .into_iter()
            .filter(|o| o.name.starts_with("addon"))
            .map(|o| (o.kind, o.name, o.tbl_name, o.sql))
            .collect();
        after.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(after, before, "from {version}: foreign objects changed");
        assert_eq!(text(&exec(w, "SELECT * FROM pragma_integrity_check")), "ok");
        // The upgrade's own copies of the rows are no add-on's business.
        assert_eq!(int(&exec(w, "SELECT count(*) FROM addon_log")), 0, "from {version}");

        // The views read the rebuilt tables, and the triggers fire on them.
        assert_eq!(
            int(&exec(w, "SELECT count(*) FROM addon_graves")),
            int(&exec(w, "SELECT count(*) FROM graves")),
            "from {version}"
        );
        exec(
            w,
            "DELETE FROM graves WHERE (oid, type) = (SELECT oid, type FROM graves ORDER BY oid, type LIMIT 1)",
        );
        assert_eq!(
            int(&exec(w, "SELECT count(*) FROM addon_log WHERE what = 'grave'")),
            1,
            "from {version}"
        );
        if version >= 14 {
            assert_eq!(
                int(&exec(w, "SELECT count(*) FROM addon_tags")),
                int(&exec(w, "SELECT count(*) FROM tags")),
                "from {version}"
            );
            exec(
                w,
                "INSERT INTO tags (tag, usn, collapsed) VALUES ('addon-check', -1, 0)",
            );
            assert_eq!(
                int(&exec(w, "SELECT count(*) FROM addon_log WHERE what = 'addon-check'")),
                1,
                "from {version}"
            );
        }
        opened.close().unwrap();
    }
}
