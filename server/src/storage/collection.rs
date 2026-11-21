// SPDX-License-Identifier: MIT
use super::{AccountStores, DataRoot, owned as at, root::unavailable};
use crate::{Result, Error, crypto, collection::schema};
use std::{collections::BTreeMap, fs::{self, File}, path::{Path,PathBuf}, sync::{Arc,Mutex}};

// Reserved before any bootstrap filesystem changes, including bounded SQLite
// pages and its transient WAL. This is not a physical disk-space guarantee.
pub(crate) const INITIAL_TEMP_BYTES: u64 = 2 * 1024 * 1024;

pub(super) fn manifest(bytes: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(bytes).map_err(|_| unavailable())?;
    let generation = text.strip_prefix("meguri-collection-v1\ngeneration=")
        .and_then(|s|s.strip_suffix('\n')).ok_or_else(unavailable)?;
    if generation != "initial" { crypto::bearer(generation).map_err(|_| unavailable())?; }
    Ok(generation.into())
}
pub(super) fn initial(s: &AccountStores, generation: &File, staging: &File) -> Result<()> {
    let path = s.generations.join("initial/collection.anki2");
    if path.try_exists()? {
        let c = schema::open(&path,false)?;
        return schema::validate(&c);
    }
    let pending = s.generations.join("initial/collection.pending");
    let existed=pending.try_exists()?;
    let file=at::open(generation,"collection.pending",false,!existed)?;
    let binding = format!("{:?}\n",at::identity(&file)?);
    file.sync_all()?;
    let binding_path=s.directory.join(".collection-staging/initial-binding");
    if binding_path.try_exists()? {
        if at::read(staging,"initial-binding")?!=binding.as_bytes() {return Err(unavailable());}
    } else {
        // A crash between exclusive creation and binding can leave only an
        // empty file: SQLite has not been called. Never adopt unknown bytes.
        if file.metadata()?.len()!=0 {return Err(unavailable());}
        at::write_new(staging,"initial-binding",binding.as_bytes())?;
    }
    let valid=existed && schema::open(&pending,false).and_then(|c|schema::validate(&c)).is_ok();
    if !valid {
        for name in ["collection.pending-wal","collection.pending-shm","collection.pending-journal"] {
            if s.generations.join("initial").join(name).try_exists()? { at::open(generation,name,false,false)?; at::unlink(generation,name)?; }
        }
        // The durable binding authorizes only this inode; keep that inode even
        // through a second interruption instead of guessing a new binding.
        file.set_len(0)?;file.sync_all()?;
        let now = time::OffsetDateTime::now_utc();
        schema::create_blank(&pending,now.unix_timestamp(),(now.unix_timestamp_nanos()/1_000_000) as i64)?;
    }
    drop(file);
    // Renaming requires all SQLite handles/sidecars closed first.
    at::publish(generation,"collection.pending","collection.anki2")?;
    schema::validate(&schema::open(&path,false)?)
}

/// A privately owned same-filesystem file. Unpublished drop removes only its
/// known children; unknown content is preserved. Activated generations survive.
pub(crate) struct Generation {
    pub path: PathBuf,
    name: String,
    directory: File,
    generations: File,
    account: File,
    keep: bool,
    file_identity: (u64,u64),
    _pin:Option<ReadPin>,
}
pub(crate) struct ReadPin {path:PathBuf,pins:Arc<Mutex<BTreeMap<PathBuf,usize>>>}
impl Drop for ReadPin {
    fn drop(&mut self) {
        if let Ok(mut pins)=self.pins.lock() {
            if let Some(count)=pins.get_mut(&self.path) {
                *count-=1;if *count==0 {pins.remove(&self.path);}
            }
        }
    }
}
impl Generation {
    pub fn writer(&self) -> Result<File> { at::open(&self.directory,"collection.anki2",false,false) }
    pub fn durable(&self) -> Result<()> {
        self.writer()?.sync_all()?;
        self.directory.sync_all()?;
        self.generations.sync_all()?;
        Ok(())
    }
    #[cfg(test)]
    pub fn activate(self) -> Result<()> { self.activate_notifying(||{}) }
    pub fn activate_notifying(mut self, boundary:impl FnOnce()) -> Result<()> {
        self.durable()?;
        crate::accounts::phase("collection_before_manifest")?;
        let pending = format!("pointer-{}",self.name);
        let bytes = format!("meguri-collection-v1\ngeneration={}\n",self.name);
        at::write_new(&self.account,&pending,bytes.as_bytes())?;
        // Any failure from rename through directory sync is uncertain. Keep
        // both generations, close collection admission, and require reopen.
        self.keep = true;
        boundary();
        at::replace(&self.account,&pending,"current-collection")?;
        crate::accounts::phase("collection_after_manifest_sync")?;
        Ok(())
    }
}
impl Drop for Generation {
    fn drop(&mut self) {
        if self.keep { return; }
        let Ok(file) = at::open(&self.directory,"collection.anki2",false,false) else { return; };
        if at::identity(&file).ok() != Some(self.file_identity) { return; }
        drop(file);
        for name in ["collection.anki2-wal","collection.anki2-shm","collection.anki2-journal","collection.anki2","owner"] {
            if self.path.parent().is_some_and(|p|p.join(name).exists()) {
                if at::open(&self.directory,name,false,false).is_err() || at::unlink(&self.directory,name).is_err() { return; }
            }
        }
        // Directory removal fails harmlessly if unrelated bytes are present.
        if let Some(path) = self.path.parent() { let _ = fs::remove_dir(path); }
        let _ = self.generations.sync_all();
    }
}
impl DataRoot {
    pub(crate) fn pin_collection(&self,path:&Path)->Result<ReadPin> {
        let mut pins=self.collection_pins.lock().map_err(|_|unavailable())?;
        *pins.entry(path.to_owned()).or_default()+=1;
        Ok(ReadPin {path:path.to_owned(),pins:self.collection_pins.clone()})
    }
    pub(crate) fn recover_collection_stages(&self,id:&str)->Result<()> {
        self.validate_stores(id)?;
        self.cleanup_collection_generations(id)
    }
    /// Called with exclusive startup ownership or the account barrier. Live
    /// streaming candidates and pinned snapshot sources are never reclaimed.
    pub(crate) fn cleanup_collection_generations(&self,id:&str)->Result<()> {
        self.validate()?;
        let relative=PathBuf::from("state/accounts").join(id);
        let account=self.bound_dir(&relative,false)?;
        let current=manifest(&at::read(&account,"current-collection")?)?;
        let generations=self.bound_dir(&relative.join("generations"),false)?;
        let stores=self.stores(id)?;
        for entry in fs::read_dir(&stores.generations)? {
            let entry=entry?;
            let Some(name)=entry.file_name().to_str().map(str::to_owned) else { continue; };
            if name==current || crypto::bearer(&name).is_err() { continue; }
            let path=entry.path().join("collection.anki2");
            // Keep the short pin lock through ownership inspection/removal, so
            // a streaming candidate cannot acquire a pin between these steps.
            let pins=self.collection_pins.lock().map_err(|_|unavailable())?;
            if pins.contains_key(&path) {continue;}
            let Ok(directory)=at::dir(&generations,&name,false) else { continue; };
            let Ok(file)=at::open(&directory,"collection.anki2",false,false) else { continue; };
            let file_identity=at::identity(&file)?;
            let expected=format!("meguri-collection-stage-v1\naccount={id}\nfile={file_identity:?}\n");
            if at::read(&directory,"owner").ok().as_deref()!=Some(expected.as_bytes()) { continue; }
            drop(file);
            // The same exact owned cleanup as a cancelled request. Unknown
            // names are retained, and no wildcard or remote filename is deleted.
            let retired=relative.join("generations").join(&name);
            let directory_identity=at::identity(&directory)?;
            let generation_parent=generations.try_clone()?;let generation_account=account.try_clone()?;
            self.retire_collection_directory(&retired,directory_identity,move || {
                drop(Generation {path,name,directory,generations:generation_parent,account:generation_account,keep:false,file_identity,_pin:None});
            })?;
            drop(pins);
        }
        Ok(())
    }
    pub(crate) fn collection_path(&self, id: &str) -> Result<PathBuf> {
        self.validate()?;
        let relative = PathBuf::from("state/accounts").join(id);
        let account = self.bound_dir(&relative,false)?;
        let name = manifest(&at::read(&account,"current-collection")?)?;
        self.bound_dir(&relative.join("generations").join(&name),false)?;
        Ok(self.stores(id)?.generations.join(name).join("collection.anki2"))
    }
    pub(crate) fn stage_collection(&self,id: &str) -> Result<Generation> {
        self.validate()?;
        let stores = self.stores(id)?;
        let relative = PathBuf::from("state/accounts").join(id);
        let account = self.bound_dir(&relative,false)?;
        let generations = self.bound_dir(&relative.join("generations"),false)?;
        let name = crypto::random_token()?;
        let directory = at::dir(&generations,&name,true)?;
        let file = at::open(&directory,"collection.anki2",false,true)?;
        let file_identity = at::identity(&file)?;
        let owner = format!("meguri-collection-stage-v1\naccount={id}\nfile={file_identity:?}\n");
        let path=stores.generations.join(&name).join("collection.anki2");
        let pin=self.pin_collection(&path)?;
        let generation=Generation {path, name,directory,generations,account,keep:false,file_identity,_pin:Some(pin)};
        at::write_new(&generation.directory,"owner",owner.as_bytes())?;
        Ok(generation)
    }
}

/// Exact SQLite logical size from a read snapshot; caller holds target barrier.
pub(crate) fn storage(root: &DataRoot, id: &str, media_bytes: u64) -> Result<crate::accounts::AccountStorage> {
    let path = root.collection_path(id)?;
    let mut c = crate::collection::schema::open(&path, false)?;
    let tx = c.transaction()?;
    // Establish one read snapshot before reading both PRAGMAs.
    tx.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get::<_, i64>(0))?;
    let pages: i64 = tx.pragma_query_value(None, "page_count", |r| r.get(0))?;
    let page_size: i64 = tx.pragma_query_value(None, "page_size", |r| r.get(0))?;
    let pages = u64::try_from(pages).map_err(|_| Error::new("storage_unavailable"))?;
    let page_size = u64::try_from(page_size).map_err(|_| Error::new("storage_unavailable"))?;
    let collection_bytes = pages.checked_mul(page_size)
        .filter(|n| *n > 0).ok_or(Error::new("storage_unavailable"))?;
    let total_bytes = collection_bytes.checked_add(media_bytes)
        .ok_or(Error::new("storage_unavailable"))?;
    Ok(crate::accounts::AccountStorage { collection_bytes, media_bytes, total_bytes })
}
