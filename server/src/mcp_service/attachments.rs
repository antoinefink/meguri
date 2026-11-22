// SPDX-License-Identifier: MIT
//! M8 acquisition and ephemeral browser staging. Never diagnose input metadata.
mod download;
mod format;
use super::*;
use crate::{accounts::{self, BrowserAuthority}, mcp_transport::inputs::{FileInput, UploadInput}, media};
use std::{collections::BTreeMap, sync::{Mutex, Weak}};
use tokio::sync::{Semaphore, OwnedSemaphorePermit};

pub(crate) const FILE_BYTES: usize = 8 * 1024 * 1024;
const TOTAL_BYTES: usize = 16 * 1024 * 1024;
const STORED_BYTES: usize = 64 * 1024 * 1024;
fn unavailable() -> Error { Error::new("attachment_unavailable") }
fn invalid() -> Error { Error::new("attachment_invalid") }

pub(crate) struct Validated {
    pub bytes: Vec<u8>,
    pub name: String,
    pub sha1: String,
    pub sha256: Vec<u8>,
    extension: &'static str,
}
impl Validated {
    pub(super) fn new(bytes: Vec<u8>) -> Result<Self> {
        use sha2::Digest;
        if bytes.is_empty() || bytes.len() > FILE_BYTES { return Err(invalid()); }
        let extension = format::detect(&bytes).ok_or_else(invalid)?;
        let digest = sha2::Sha256::digest(&bytes);
        Ok(Self { name:format!("meguri-{}.{}", media::blobs::hex(&digest), extension),
            sha1:media::blobs::hex(&sha1::Sha1::digest(&bytes)), sha256:digest.to_vec(),
            extension, bytes })
    }
    fn reference(&self) -> String {
        match self.extension {
            "pdf" => format!("<a href=\"{0}\">Open PDF</a><object data=\"{0}\" hidden></object>", self.name),
            "ogg" => format!("<audio controls src=\"{}\"></audio>", self.name),
            "mp3" | "wav" | "flac" | "aac" | "m4a" | "aiff" | "aifc" | "caf" => format!("[sound:{}]", self.name),
            _ => format!("<img src=\"{}\">", self.name),
        }
    }
}
#[derive(Clone)]
struct Record {
    authority: oauth::Authority,
    path: PathBuf,
    expires: i64,
    content: Option<Arc<Validated>>,
}
pub(crate) struct Uploads {
    records: Mutex<BTreeMap<[u8;32], Record>>,
    capacity: Arc<Semaphore>,
}
impl Default for Uploads {
    fn default() -> Self { Self { records:Mutex::new(BTreeMap::new()), capacity:Arc::new(Semaphore::new(4)) } }
}
impl Uploads {
    fn slot(&self) -> Result<OwnedSemaphorePermit> {
        self.capacity.clone().try_acquire_owned().map_err(|_| mcp::busy())
    }
    #[cfg(test)]
    pub(super) fn expire_fixture(&self, id: &str) {
        self.records.lock().unwrap().get_mut(&upload_key(id).unwrap()).unwrap().expires = accounts::now() - 1;
    }
    fn read(&self, key: &[u8;32]) -> Result<Record> {
        let mut records = self.records.lock().map_err(|_| unavailable())?;
        records.retain(|_, r| r.expires > accounts::now());
        records.get(key).cloned().ok_or_else(unavailable)
    }
    fn check(&self, key: &[u8;32], expected: &Record) -> Result<()> {
        let current = self.read(key)?;
        if current.expires != expected.expires || current.path != expected.path
            || !current.authority.same_upload_binding(&expected.authority)
            || !match (&current.content, &expected.content) {
                (Some(a), Some(b)) => Arc::ptr_eq(a,b), (None,None) => true, _ => false,
            } { return Err(unavailable()); }
        Ok(())
    }
}
fn upload_key(id: &str) -> Result<[u8;32]> {
    crypto::bearer(id).map_err(|_| unavailable())?;
    Ok(crypto::digest(id.as_bytes()))
}
pub(super) struct Pinned {
    pub(super) path: PathBuf,
    // Captured with the original authority's data epoch and publication under
    // the target barrier. Never resolve an account media path after acquisition.
    media: std::fs::File,
    _pin: ReadPin,
}
impl McpService {
    async fn fetch_direct(&self, url: &str, maximum: usize) -> Result<Vec<u8>> {
        if maximum == 0 { return Err(invalid()); }
        #[cfg(test)]
        if let Some(fixture) = &self.direct_fixture {
            download::target(url)?;
            return fixture(url);
        }
        download::fetch(url,maximum).await
    }
    async fn pin(&self, original: oauth::Authority) -> Result<Pinned> {
        let barrier = self.service.mcp_publication_barrier(&original).await?;
        let guard = barrier.try_enter().ok_or_else(mcp::busy)?;
        self.service.metadata.files(move |root| {
            let _held = (barrier, guard);
            check_oauth(root, &original, true)?;
            let path = root.collection_path(original.account_id())?;
            let pin = root.pin_collection(&path)?;
            let media = root.media_directory(original.account_id())?;
            Ok(Pinned { path, media, _pin:pin })
        }).await
    }
    pub(crate) async fn prepare_media_upload(&self, original: oauth::Authority) -> Result<(String,String,i64)> {
        let pinned = self.pin(original.clone()).await?;
        let id = crypto::random_token()?;
        let key = upload_key(&id)?;
        let expires = accounts::now() + 600;
        // No await between final authority/publication validation and insertion.
        let store = self.service.mcp_uploads.clone();
        let record = Record { authority:original.clone(), path:pinned.path, expires, content:None };
        let barrier = self.service.mcp_publication_barrier(&original).await?;
        let guard = barrier.try_enter().ok_or_else(mcp::busy)?;
        self.service.metadata.files(move |root| {
            let _held = (pinned._pin, barrier, guard);
            check_oauth(root, &original, true)?;
            if root.collection_path(original.account_id())? != record.path { return Err(Error::new("conflict")); }
            let mut records = store.records.lock().map_err(|_| unavailable())?;
            records.retain(|_, r| r.expires > accounts::now());
            if records.len() >= 64 || records.values().filter(|r|r.authority.account_id() == original.account_id()).count() >= 8 {
                return Err(mcp::busy());
            }
            if records.contains_key(&key) { return Err(Error::new("storage_unavailable")); }
            records.insert(key, record);
            Ok(())
        }).await?;
        let weak: Weak<Uploads> = Arc::downgrade(&self.service.mcp_uploads);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(600)).await;
            if let Some(store) = weak.upgrade() {
                if let Ok(mut records) = store.records.lock() { records.retain(|_, r|r.expires > accounts::now()); }
            }
        });
        Ok((id.clone(), format!("{}/mcp/upload#upload={id}",self.service.config.origin()), expires))
    }
    pub(super) async fn write_with_media(&self, original: oauth::Authority,
        files: Option<Vec<FileInput>>, fields: Option<Vec<String>>, uploads: Option<Vec<UploadInput>>,
        operation: impl FnOnce(&Backend, &Path, &CollectionAuthority, &[(String,String)], &mut dyn FnMut() -> Result<()>) -> Result<WriteResult> + Send + 'static,
    ) -> Result<WriteResult> {
        // Bounded before DNS, allocations or filesystem work, even if called outside transport.
        crate::mcp_transport::inputs::validate_attachments(&files,&fields,&uploads).map_err(|_| invalid())?;
        if files.is_some() && !oauth::direct_file_client(&original) { return Err(unavailable()); }
        let mut permit = self.service.mcp_uploads.slot()?;
        let pinned = self.pin(original.clone()).await?;
        let mut acquired = Vec::new();
        // One deadline for all DNS and downloads in this call, including partial failures.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut total = 0usize;
        for (file,field) in files.unwrap_or_default().into_iter().zip(fields.unwrap_or_default()) {
            let bytes = tokio::time::timeout_at(deadline,self.fetch_direct(&file.download_url,FILE_BYTES.min(TOTAL_BYTES-total))).await.map_err(|_| unavailable())??;
            total = total.checked_add(bytes.len()).filter(|n|*n <= TOTAL_BYTES).ok_or_else(invalid)?;
            let validated = self.service.metadata.files(move |_| Ok((Arc::new(Validated::new(bytes)?),permit))).await?;
            permit = validated.1;
            acquired.push((field,validated.0));
        }
        let mut redeemed = Vec::new();
        for upload in uploads.unwrap_or_default() {
            let key = upload_key(&upload.upload_id)?;
            let record = self.service.mcp_uploads.read(&key)?;
            if !record.authority.same_upload_binding(&original) || record.path != pinned.path { return Err(unavailable()); }
            let content = record.content.clone().ok_or_else(unavailable)?;
            total = total.checked_add(content.bytes.len()).filter(|n|*n <= TOTAL_BYTES).ok_or_else(invalid)?;
            acquired.push((upload.field,content)); redeemed.push((key,record));
        }
        let root = self.service.metadata.owner();
        let id = original.account_id().to_owned();
        let budget = self.service.temp.clone();
        let directory = pinned.media.try_clone()?;
        let (references, mut prepared, permit) = self.service.metadata.files(move |_| {
            let references = acquired.iter().map(|(field,v)|(field.clone(),v.reference())).collect::<Vec<_>>();
            let mut unique = BTreeMap::<String,Arc<Validated>>::new();
            for (_,content) in acquired {
                match unique.entry(content.name.clone()) {
                    std::collections::btree_map::Entry::Occupied(prior) => {
                        if prior.get().bytes != content.bytes { return Err(Error::new("attachment_conflict")); }
                    }
                    std::collections::btree_map::Entry::Vacant(slot) => { slot.insert(content); }
                }
            }
            let prepared = media::prepare_insert_in(&root,&id,directory,unique.into_values().collect(),budget)?;
            Ok((references,prepared,permit))
        }).await?;
        let store = self.service.mcp_uploads.clone();
        self.execute_pinned(original, true, Some(pinned), move |backend,path,a| {
            let _permit = permit;
            let mut install = || {
                // Already holding the sole account barrier and collection transaction.
                a.check()?;
                for (key,record) in &redeemed { store.check(key,record)?; }
                media::install_insert(&a.held.root,&a.original,&mut prepared)
            };
            operation(backend,path,a,&references,&mut install)
        }).await
    }
}
impl Service {
    pub(crate) fn media_upload_slot(&self) -> Result<OwnedSemaphorePermit> { self.mcp_uploads.slot() }
    pub(crate) async fn check_media_upload(&self, browser: BrowserAuthority, id: &str) -> Result<()> {
        let key = upload_key(id).map_err(|_| Error::new("capability_invalid"))?;
        let record = self.mcp_uploads.read(&key).map_err(|_| Error::new("capability_invalid"))?;
        self.metadata.files(move |root| check_browser_upload(root,&browser,&record)).await
    }
    pub(crate) async fn accept_media_upload(&self, browser: BrowserAuthority, id: String, bytes: Vec<u8>, permit: OwnedSemaphorePermit) -> Result<()> {
        let key = upload_key(&id).map_err(|_| Error::new("capability_invalid"))?;
        let record = self.mcp_uploads.read(&key).map_err(|_| Error::new("capability_invalid"))?;
        let (content,permit) = self.metadata.files(move |_| Ok((Arc::new(Validated::new(bytes)?),permit))).await?;
        let barrier = self.barriers.get(record.authority.account_id())?;
        let guard = barrier.try_enter().ok_or_else(mcp::busy)?;
        let store = self.mcp_uploads.clone();
        self.metadata.files(move |root| {
            let _held = (permit,barrier,guard);
            check_browser_upload(root,&browser,&record)?;
            store.check(&key,&record).map_err(|_| Error::new("capability_invalid"))?;
            let mut records = store.records.lock().map_err(|_| unavailable())?;
            if records.values().filter_map(|r|r.content.as_ref()).map(|v|v.bytes.len()).sum::<usize>() + content.bytes.len() > STORED_BYTES { return Err(invalid()); }
            let r = records.get_mut(&key).ok_or(Error::new("capability_invalid"))?;
            if r.content.is_some() || r.expires <= accounts::now() { return Err(Error::new("capability_invalid")); }
            r.content = Some(content);
            Ok(())
        }).await
    }
}
fn check_browser_upload(root: &DataRoot, browser: &BrowserAuthority, record: &Record) -> Result<()> {
    let mut c = storage::inspect(root.database())?;
    let tx = c.transaction()?;
    let account = accounts::check_browser(&tx,browser,"authentication_failed")?;
    oauth::check_upload_binding(&tx,&record.authority)?;
    if account.identity.id != record.authority.account_id() || record.expires <= accounts::now() || record.content.is_some()
        || root.collection_path(record.authority.account_id())? != record.path { return Err(Error::new("capability_invalid")); }
    Ok(())
}
