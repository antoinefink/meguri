// SPDX-License-Identifier: MIT
use super::owned as at;
use crate::{Error, Result, crypto, limits::TempBudget};
use rusqlite::params;
use std::{collections::BTreeMap, sync::{Arc,Mutex}};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

/// Stable owner directory and lock are outside replaceable `state/` contents.
/// Also flock the directory inode so replacing only the lock file cannot admit
/// a second owner. Parent/root renames by the operator's UID are out of scope;
/// all operations detect binding changes. Never unlink either lock inode.
pub struct DataRoot {
    path: PathBuf,
    dir: File,
    lock: File,
    state_inode: Option<(u64, u64)>,
    directories: Mutex<BTreeMap<PathBuf, File>>,
    pub(super) collection_pins: Arc<Mutex<BTreeMap<PathBuf,usize>>>,
}
#[derive(Clone, Debug)]
pub struct AccountStores {
    pub account_id: String,
    pub directory: PathBuf,
    pub generations: PathBuf,
    pub media: PathBuf,
    pub current_manifest: PathBuf,
}
pub(super) fn unavailable() -> Error {
    Error::new("storage_unavailable")
}
pub(super) fn owned(m: &fs::Metadata) -> Result<()> {
    // SAFETY: geteuid has no preconditions.
    if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err(unavailable());
    }
    Ok(())
}
pub(crate) fn regular(path: &Path, required: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && m.nlink() == 1 && m.mode() & 0o777 == 0o600 => owned(&m),
        Ok(_) => Err(unavailable()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => Ok(()),
        Err(_) => Err(unavailable()),
    }
}
pub(crate) fn private_file(path: &Path) -> Result<()> {
    regular(path, true)
}
fn directory(path: &Path, create: bool) -> Result<()> {
    if create && !path.try_exists()? {
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path)?;
    }
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.mode() & 0o777 != 0o700 {
        return Err(unavailable());
    }
    owned(&m)
}
fn no_links(path: &Path) -> Result<()> {
    for p in path.ancestors() {
        if fs::symlink_metadata(p)?.file_type().is_symlink() {
            return Err(unavailable());
        }
    }
    Ok(())
}
fn file(path: &Path, exclusive: bool) -> Result<File> {
    let mut o = OpenOptions::new();
    o.read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    if exclusive {
        o.create_new(true);
    }
    Ok(o.open(path)?)
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
const INITIAL_INTENT: &[u8] = b"meguri-initialization-v1\n";
pub(super) const INITIAL_MANIFEST: &[u8] = b"meguri-collection-v1\ngeneration=initial\n";
fn lock(file: &File) -> Result<()> {
    // SAFETY: valid borrowed file descriptor, flock does not retain pointers.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(unavailable());
    }
    Ok(())
}
fn binding(path: &Path, f: &File, is_dir: bool) -> Result<()> {
    let a = fs::symlink_metadata(path)?;
    let b = f.metadata()?;
    owned(&a)?;
    if a.dev() != b.dev()
        || a.ino() != b.ino()
        || a.is_dir() != is_dir
        || a.file_type().is_symlink()
        || a.mode() & 0o777 != (if is_dir { 0o700 } else { 0o600 })
        || (!is_dir && a.nlink() != 1)
    {
        return Err(unavailable());
    }
    Ok(())
}
impl DataRoot {
    pub fn acquire(path: &Path, create: bool) -> Result<Self> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        if absolute
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(unavailable());
        }
        // Require a preexisting trusted parent: never create missing ancestors.
        // Retain it until the root entry is synced, including a retry after a
        // previous failed acquisition left a directory behind.
        let parent_path = absolute.parent().ok_or_else(unavailable)?;
        no_links(parent_path)?;
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent_path)?;
        directory(&absolute, create)?;
        no_links(&absolute)?;
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&absolute)?;
        lock(&dir)?;
        binding(&absolute, &dir, true)?;
        crate::accounts::phase("root_parent_sync")?;
        parent.sync_all()?;
        crate::accounts::phase("root_after_parent_sync")?;
        let lock_path = absolute.join("owner.lock");
        let lock_file = match file(&lock_path, true) {
            Ok(f) => {
                f.sync_all()?;
                sync_dir(&absolute)?;
                f
            }
            Err(_) => {
                regular(&lock_path, true)?;
                file(&lock_path, false)?
            }
        };
        lock(&lock_file)?;
        binding(&lock_path, &lock_file, false)?;
        let state = absolute.join("state");
        let state_inode = if state.try_exists()? {
            directory(&state, false)?;
            let m = fs::symlink_metadata(state)?;
            Some((m.dev(), m.ino()))
        } else {
            None
        };
        let root = Self {
            path: absolute,
            dir,
            lock: lock_file,
            state_inode,
            directories: Mutex::new(BTreeMap::new()),
            collection_pins: Arc::new(Mutex::new(BTreeMap::new())),
        };
        if root.state().try_exists()? {
            root.bound_dir(Path::new("state"), false)?;
            if root.state().join("accounts").try_exists()? {
                root.bound_dir(Path::new("state/accounts"), false)?;
            }
        }
        Ok(root)
    }
    /// Open each component relative to the retained root descriptor. Every
    /// observed directory stays pinned for this ownership session.
    pub(crate) fn bound_dir(&self, relative: &Path, create: bool) -> Result<File> {
        let mut pinned = self.directories.lock().map_err(|_| unavailable())?;
        self.bound_dir_locked(relative,create,&mut pinned)
    }
    fn bound_dir_locked(&self,relative:&Path,create:bool,pinned:&mut BTreeMap<PathBuf,File>)->Result<File> {
        let mut current = self.dir.try_clone()?;
        let mut prefix = PathBuf::new();
        for component in relative.components() {
            let std::path::Component::Normal(name) = component else {
                return Err(unavailable());
            };
            let name = name.to_str().ok_or_else(unavailable)?;
            prefix.push(name);
            let next = at::dir(&current, name, create && !pinned.contains_key(&prefix))?;
            if let Some(expected) = pinned.get(&prefix) {
                if at::identity(expected)? != at::identity(&next)? {
                    return Err(unavailable());
                }
            } else {
                pinned.insert(prefix.clone(), next.try_clone()?);
            }
            current = next;
        }
        Ok(current)
    }
    pub(super) fn retire_collection_directory(&self,relative:&Path,identity:(u64,u64),remove:impl FnOnce())->Result<()> {
        // Binding ownership spans the exact authorized removal and forgetting.
        // Validation holds this same lock across its entire known-path scan.
        let mut pinned=self.directories.lock().map_err(|_|unavailable())?;
        let current=self.bound_dir_locked(relative,false,&mut pinned)?;
        if at::identity(&current)?!=identity { return Err(unavailable()); }
        remove();
        if !self.path.join(relative).try_exists()? { pinned.remove(relative); }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        no_links(&self.path)?;
        binding(&self.path, &self.dir, true)?;
        binding(&self.path.join("owner.lock"), &self.lock, false)?;
        if let Some((dev, ino)) = self.state_inode {
            let m = fs::symlink_metadata(self.state())?;
            directory(&self.state(), false)?;
            if m.dev() != dev || m.ino() != ino {
                return Err(unavailable());
            }
        }
        let mut pinned=self.directories.lock().map_err(|_|unavailable())?;
        let expected:Vec<_>=pinned.keys().cloned().collect();
        for path in expected {
            self.bound_dir_locked(&path,false,&mut pinned)?;
        }
        Ok(())
    }
    pub fn state(&self) -> PathBuf {
        self.path.join("state")
    }
    pub fn database(&self) -> PathBuf {
        self.state().join("accounts.sqlite")
    }
    pub fn setup_file(&self) -> PathBuf {
        self.state().join("setup-token")
    }
    /// Protected local access for successful first-init console delivery only.
    /// HTTP and routine startup must never call this accessor.
    pub fn setup_code(&self) -> Result<String> {
        self.check_initialized()?;
        let db = super::inspect(self.database())?;
        let (phase, hash): (String, Vec<u8>) = db.query_row(
            "SELECT phase,capability FROM setup WHERE singleton=1", [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if phase != "open" { return Err(unavailable()); }
        let state = self.bound_dir(Path::new("state"), false)?;
        let code = String::from_utf8(at::read(&state, "setup-token")?).map_err(|_| unavailable())?;
        let code = code.trim_end_matches('\n');
        let digest = crypto::bearer(code).map_err(|_| unavailable())?;
        if !crypto::same_digest(&hash, &digest) { return Err(unavailable()); }
        Ok(code.to_owned())
    }
    pub fn init(&mut self) -> Result<()> {
        self.validate()?;
        if self.state().try_exists()? || self.path.join(".initialization").try_exists()? {
            return Err(unavailable());
        }
        // This durable private reservation precedes every state filesystem phase.
        // Never remove the reservation: it also binds repair to the original DB.
        let intent = at::dir(&self.dir, ".initialization", true)?;
        crate::accounts::phase("init_after_intent_directory")?;
        self.complete_initial_intent(&intent)?;
        self.prepare_initial_state()?;
        self.initialize_schema()
    }
    // Only explicit local repair may complete early intent publication. A
    // missing/partial intent proves no state ownership: require that no state
    // exists and the private intent directory contains nothing else.
    fn repair_initial_intent(&self) -> Result<()> {
        let path = self.path.join(".initialization");
        let intent = self.bound_dir(Path::new(".initialization"), false)?;
        if path.join("intent").try_exists()? && at::read(&intent, "intent")? == INITIAL_INTENT {
            return Ok(());
        }
        if self.state().try_exists()? {
            return Err(unavailable());
        }
        for entry in fs::read_dir(&path)? {
            if entry?.file_name() != "intent" {
                return Err(unavailable());
            }
        }
        self.complete_initial_intent(&intent)
    }
    fn complete_initial_intent(&self, intent: &File) -> Result<()> {
        let path = self.path.join(".initialization/intent");
        if !path.try_exists()? {
            at::open(intent, "intent", false, true)?.sync_all()?;
            intent.sync_all()?;
        }
        crate::accounts::phase("init_after_intent_create")?;
        let prefix = at::read(intent, "intent")?;
        if !INITIAL_INTENT.starts_with(&prefix) {
            return Err(unavailable());
        }
        use std::io::{Seek, SeekFrom};
        let mut f = at::open(intent, "intent", false, false)?;
        f.seek(SeekFrom::End(0))?;
        let remaining = &INITIAL_INTENT[prefix.len()..];
        let split = remaining.len() / 2;
        f.write_all(&remaining[..split])?;
        crate::accounts::phase("init_during_intent_write")?;
        f.write_all(&remaining[split..])?;
        f.sync_all()?;
        intent.sync_all()?;
        Ok(())
    }
    fn record(&self, parent: &File, path: &Path, name: &str, bytes: &[u8]) -> Result<()> {
        if path.join(name).try_exists()? {
            if at::read(parent, name)? != bytes {
                return Err(unavailable());
            }
            return Ok(());
        }
        let pending = format!("{name}.pending");
        if !path.join(&pending).try_exists()? {
            at::write_new(parent, &pending, b"")?;
        }
        let prefix = at::read(parent, &pending)?;
        if !bytes.starts_with(&prefix) {
            return Err(unavailable());
        }
        use std::io::{Seek, SeekFrom};
        let mut f = at::open(parent, &pending, false, false)?;
        f.seek(SeekFrom::End(0))?;
        f.write_all(&bytes[prefix.len()..])?;
        f.sync_all()?;
        at::publish(parent, &pending, name)
    }
    fn prepare_initial_state(&self) -> Result<()> {
        self.validate()?;
        let intent_path = self.path.join(".initialization");
        let intent = self.bound_dir(Path::new(".initialization"), false)?;
        if at::read(&intent, "intent")? != INITIAL_INTENT {
            return Err(unavailable());
        }
        if !self.state().try_exists()? {
            let staged = at::dir(&intent, "state", true)?;
            // No state exists yet. The only allowed staged content is an empty
            // accounts directory. Unknown bytes are refused and preserved.
            let accounts = at::dir(&staged, "accounts", true)?;
            if fs::read_dir(intent_path.join("state/accounts"))?
                .next()
                .is_some()
            {
                return Err(unavailable());
            }
            for entry in fs::read_dir(intent_path.join("state"))? {
                if entry?.file_name() != "accounts" {
                    return Err(unavailable());
                }
            }
            accounts.sync_all()?;
            staged.sync_all()?;
            let binding = format!("{:?}\n", at::identity(&staged)?);
            self.record(&intent, &intent_path, "state-binding", binding.as_bytes())?;
            at::publish_between(&intent, "state", &self.dir, "state")?;
        }
        let state = self.bound_dir(Path::new("state"), false)?;
        let binding = format!("{:?}\n", at::identity(&state)?);
        if at::read(&intent, "state-binding")? != binding.as_bytes() {
            return Err(unavailable());
        }
        self.bound_dir(Path::new("state/accounts"), false)?;
        crate::accounts::phase("init_after_state")?;
        if !self.database().try_exists()? {
            let pending_path = intent_path.join("database.pending");
            if !pending_path.try_exists()? {
                at::open(&intent, "database.pending", false, true)?.sync_all()?;
                intent.sync_all()?;
            }
            let database = at::open(&intent, "database.pending", false, false)?;
            if database.metadata()?.len() != 0 {
                return Err(unavailable());
            }
            let binding = format!("{:?}\n", at::identity(&database)?);
            self.record(
                &intent,
                &intent_path,
                "database-binding",
                binding.as_bytes(),
            )?;
            at::publish_between(&intent, "database.pending", &state, "accounts.sqlite")?;
        }
        let database = at::open(&state, "accounts.sqlite", false, false)?;
        let binding = format!("{:?}\n", at::identity(&database)?);
        if at::read(&intent, "database-binding")? != binding.as_bytes() {
            return Err(unavailable());
        }
        crate::accounts::phase("init_after_empty_database")?;
        self.validate()
    }
    fn initialize_schema(&self) -> Result<()> {
        self.prepare_initial_state()?;
        let mut db = super::open(self.database(), false)?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let objects: i64 = db.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))?;
        if version != 0 || objects != 0 {
            return Err(unavailable());
        }
        let token = crypto::random_token()?;
        let hash = crypto::bearer(&token)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.execute(
            "INSERT INTO capabilities(hash,kind,state) VALUES(?1,'setup','available')",
            params![hash.as_slice()],
        )?;
        tx.execute(
            "INSERT INTO setup VALUES(1,'incomplete',?1)",
            params![hash.as_slice()],
        )?;
        crate::accounts::phase("init_before_schema_commit")?;
        tx.commit()?;
        sync_dir(&self.state())?;
        crate::accounts::phase("init_after_database")?;
        self.publish_setup(&token)?;
        crate::accounts::phase("init_after_token")?;
        db.execute("UPDATE setup SET phase='open' WHERE singleton=1", [])?;
        sync_dir(&self.state())?;
        Ok(())
    }
    fn publish_setup(&self, token: &str) -> Result<()> {
        self.validate()?;
        let staging = self.bound_dir(Path::new(".initialization"), false)?;
        let state = self.bound_dir(Path::new("state"), false)?;
        let name = format!("setup-{}", crypto::random_token()?);
        let bytes = format!("{token}\n");
        // An interrupted private write never exposes a partial final token.
        // Unpublished leftovers are preserved, never guessed at or swept.
        at::write_new(&staging, &name, bytes.as_bytes())?;
        at::publish_between(&staging, &name, &state, "setup-token")
    }
    pub fn rotate_setup(&self) -> Result<()> {
        self.validate()?;
        self.repair_initial_intent()?;
        self.prepare_initial_state()?;
        let mut db = super::open(self.database(), false)?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 {
            drop(db);
            return self.initialize_schema();
        }
        if !matches!(version,1|2) {
            return Err(unavailable());
        }
        super::semantic::validate(self, &db, true)?;
        // A stale file from an interrupted rotation is removable only if it
        // proves one of our retained setup capabilities. Preserve unknown bytes.
        if self.setup_file().try_exists()? {
            let state = self.bound_dir(Path::new("state"), false)?;
            let bytes = at::read(&state, "setup-token")?;
            let text = std::str::from_utf8(&bytes).map_err(|_| unavailable())?;
            let old = crypto::bearer(text.trim_end_matches('\n')).map_err(|_| unavailable())?;
            let known: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM capabilities WHERE kind='setup' AND hash=?1)",
                [old.as_slice()],
                |r| r.get(0),
            )?;
            if !known {
                return Err(unavailable());
            }
        }
        let token = crypto::random_token()?;
        let hash = crypto::bearer(&token)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (phase, reserved): (String, i64) = tx.query_row(
            "SELECT phase,(SELECT count(*) FROM accounts) FROM setup WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if phase == "closed" || reserved != 0 {
            return Err(Error::new("claim_in_progress"));
        }
        tx.execute(
            "UPDATE capabilities SET state='revoked' WHERE kind='setup'",
            [],
        )?;
        tx.execute(
            "INSERT INTO capabilities(hash,kind,state) VALUES(?1,'setup','available')",
            params![hash.as_slice()],
        )?;
        tx.execute(
            "UPDATE setup SET phase='incomplete',capability=?1 WHERE singleton=1",
            params![hash.as_slice()],
        )?;
        tx.commit()?;
        if self.setup_file().try_exists()? {
            let state = self.bound_dir(Path::new("state"), false)?;
            at::open(&state, "setup-token", false, false)?;
            at::unlink(&state, "setup-token")?;
        }
        self.publish_setup(&token)?;
        db.execute("UPDATE setup SET phase='open' WHERE singleton=1", [])?;
        Ok(())
    }
    pub fn check_initialized(&self) -> Result<()> {
        self.validate()?;
        directory(&self.state().join("accounts"), false)?;
        let db = super::inspect(self.database())?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        let foreign: bool = db.prepare("PRAGMA foreign_key_check")?.exists([])?;
        if !matches!(version,1|2) || integrity != "ok" || foreign {
            return Err(unavailable());
        }
        let (phase, hash): (String, Vec<u8>) = db.query_row(
            "SELECT phase,capability FROM setup WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if phase == "incomplete" {
            return Err(unavailable());
        }
        if phase == "open" {
            regular(&self.setup_file(), true)?;
            let mut token = String::new();
            file(&self.setup_file(), false)?
                .take(128)
                .read_to_string(&mut token)?;
            let actual = crypto::bearer(token.trim_end_matches('\n')).map_err(|_| unavailable())?;
            if !crypto::same_digest(&hash, &actual) {
                return Err(unavailable());
            }
        }
        super::semantic::validate(self, &db, false)?;
        Ok(())
    }
    pub fn stores(&self, id: &str) -> Result<AccountStores> {
        // IDs have the same 32-byte base64url alphabet as tokens. Callers must
        // additionally resolve this ID from metadata before creating anything.
        crypto::bearer(id).map_err(|_| unavailable())?;
        let directory = self.state().join("accounts").join(id);
        Ok(AccountStores {
            account_id: id.into(),
            generations: directory.join("generations"),
            media: directory.join("media"),
            current_manifest: directory.join("current-collection"),
            directory,
        })
    }
    pub(crate) fn provision(&self, id: &str, budget: &TempBudget) -> Result<()> {
        self.validate()?;
        let s = self.stores(id)?;
        let ownership = format!("meguri-collection-staging-v1\naccount={id}\n");
        // Cover all service staging payload, including an existing prefix on
        // resume. Reserve before creating even an empty staging file. The guard
        // survives every write/sync/publication and releases on success or error;
        // errors retain the durable named intent for the next budgeted attempt.
        let _temporary = budget.reserve((ownership.len() + INITIAL_MANIFEST.len()) as u64 + super::collection::INITIAL_TEMP_BYTES)?;
        let relative = PathBuf::from("state/accounts").join(id);
        let account = self.bound_dir(&relative, true)?;
        let generations = self.bound_dir(&relative.join("generations"), true)?;
        let media = self.bound_dir(&relative.join("media"), true)?;
        let generation = self.bound_dir(&relative.join("generations/initial"), true)?;
        // The reservation owns this private intent. Only its named pending file
        // may be continued; unrelated children are never cleaned up.
        let staging = self.bound_dir(&relative.join(".collection-staging"), true)?;
        self.record(
            &staging,
            &s.directory.join(".collection-staging"),
            "owner",
            ownership.as_bytes(),
        )?;
        if at::read(&staging, "owner")? != ownership.as_bytes() {
            return Err(unavailable());
        }
        if !s.current_manifest.try_exists()? {
            let pending = s.directory.join(".collection-staging/manifest.pending");
            if !pending.try_exists()? {
                at::open(&staging, "manifest.pending", false, true)?.sync_all()?;
                staging.sync_all()?;
            }
            crate::accounts::phase("stores_after_stage_create")?;
            let mut f = at::open(&staging, "manifest.pending", false, false)?;
            let existing = at::read(&staging, "manifest.pending")?;
            // Only a prefix of our exact immutable marker can be unfinished work.
            // Never truncate or replace unrelated bytes at the pending name.
            if !INITIAL_MANIFEST.starts_with(&existing) {
                return Err(unavailable());
            }
            use std::io::{Seek, SeekFrom};
            f.seek(SeekFrom::End(0))?;
            let remaining = &INITIAL_MANIFEST[existing.len()..];
            let split = remaining.len() / 2;
            f.write_all(&remaining[..split])?;
            crate::accounts::phase("stores_during_stage_write")?;
            f.write_all(&remaining[split..])?;
            super::collection::initial(&s, &generation, &staging)?;
            crate::accounts::phase("stores_before_stage_sync")?;
            f.sync_all()?;
            staging.sync_all()?;
            // All stores and their ancestry precede the current marker.
            generation.sync_all()?;
            generations.sync_all()?;
            media.sync_all()?;
            account.sync_all()?;
            self.validate()?;
            at::publish_between(&staging, "manifest.pending", &account, "current-collection")?;
        }
        self.validate_stores(id)?;
        at::open(&account, "current-collection", false, false)?.sync_all()?;
        generation.sync_all()?;
        generations.sync_all()?;
        media.sync_all()?;
        account.sync_all()?;
        staging.sync_all()?;
        self.validate()?;
        self.bound_dir(Path::new("state/accounts"), false)?
            .sync_all()?;
        Ok(())
    }
    pub fn validate_stores(&self, id: &str) -> Result<()> {
        self.validate()?;
        self.stores(id)?;
        let relative = PathBuf::from("state/accounts").join(id);
        let account = self.bound_dir(&relative, false)?;
        for path in ["generations", "media"] {
            self.bound_dir(&relative.join(path), false)?;
        }
        let generation = super::collection::manifest(&at::read(&account, "current-collection")?)?;
        self.bound_dir(&relative.join("generations").join(&generation), false)?;
        let path = self.stores(id)?.generations.join(generation).join("collection.anki2");
        let c = crate::collection::schema::open(&path, false)?;
        crate::collection::schema::validate(&c)?;
        self.validate()
    }
    pub(crate) fn remove_setup_file(&self) -> Result<()> {
        crate::accounts::phase("before_setup_unlink")?;
        if self.setup_file().try_exists()? {
            let state = self.bound_dir(Path::new("state"), false)?;
            at::open(&state, "setup-token", false, false)?;
            at::unlink(&state, "setup-token")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod retirement_tests {
    use super::*;
    #[test]
    fn authorized_retirement_owns_binding_until_removal_is_forgotten() {
        let temp=tempfile::tempdir().unwrap();let mut root=DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
        let relative=PathBuf::from("state/accounts/owned-retired");
        let directory=root.bound_dir(&relative,true).unwrap();let identity=at::identity(&directory).unwrap();
        let root=Arc::new(root);let owner=root.clone();let retired=relative.clone();
        let (removed,removal)=std::sync::mpsc::channel();let (resume,release)=std::sync::mpsc::channel();
        let cleanup=std::thread::spawn(move ||owner.retire_collection_directory(&retired,identity,|| {
            fs::remove_dir(owner.path.join(&retired)).unwrap();
            assert!(matches!(owner.directories.try_lock(),Err(std::sync::TryLockError::WouldBlock)));
            removed.send(()).unwrap();release.recv().unwrap();
        }));
        removal.recv().unwrap();
        let validator=root.clone();let (attempted,attempt)=std::sync::mpsc::channel();
        let validation=std::thread::spawn(move || {attempted.send(()).unwrap();validator.validate()});
        attempt.recv().unwrap();resume.send(()).unwrap();
        cleanup.join().unwrap().unwrap();validation.join().unwrap().unwrap();root.validate().unwrap();
        assert!(!root.directories.lock().unwrap().contains_key(&relative));
    }
    #[test]
    fn unauthorized_missing_or_replaced_directory_cannot_be_forgotten() {
        for replace in [false,true] {
            let temp=tempfile::tempdir().unwrap();let mut root=DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
            let relative=PathBuf::from("state/accounts/owned-retired");let path=root.path.join(&relative);
            let original=root.bound_dir(&relative,true).unwrap();let identity=at::identity(&original).unwrap();
            fs::remove_dir(&path).unwrap();if replace {directory(&path,true).unwrap();}
            let called=std::cell::Cell::new(false);
            assert!(root.retire_collection_directory(&relative,identity,||called.set(true)).is_err());
            assert!(!called.get());assert!(root.validate().is_err());
            assert!(root.directories.lock().unwrap().contains_key(&relative));
        }
    }
}
