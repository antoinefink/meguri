// SPDX-License-Identifier: MIT
//! Generated, account-private immutable objects. Logical media names never enter
//! this module's filesystem API. Only offline recovery removes durable objects.
use crate::{DataRoot, Error, Result, crypto, limits::{TempBudget, TempReservation}, storage::owned as at};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::{fs::File, io::{Read, Seek, SeekFrom, Write}, path::PathBuf};

pub(crate) fn invalid() -> Error { Error::new("invalid_request") }
pub(crate) fn integrity() -> Error { Error::new("media_integrity") }
pub(crate) fn generated(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|id| crypto::bearer(id).is_ok())
}
impl DataRoot {
    pub(crate) fn media_directory(&self, id: &str) -> Result<File> {
        self.validate()?;
        self.stores(id)?;
        self.bound_dir(&PathBuf::from("state/accounts").join(id).join("media"), false)
    }
    pub(crate) fn media_blob(&self, id: &str, blob: &str) -> Result<File> {
        if !generated(blob, "blob-") { return Err(integrity()); }
        at::open(&self.media_directory(id)?, blob, false, false).map_err(|_|integrity())
    }
}

pub(crate) struct Temporary {
    pub file: File,
    directory: File,
    name: Option<String>,
    budget: TempBudget,
    holds: Vec<TempReservation>,
}
impl Temporary {
    pub fn create(root: &DataRoot, id: &str, budget: TempBudget) -> Result<Self> {
        Self::create_in(root.media_directory(id)?, budget)
    }
    pub(crate) fn create_in(directory: File, budget: TempBudget) -> Result<Self> {
        let metadata = budget.reserve(4096)?;
        let name = format!("stage-{}", crypto::random_token()?);
        let file = at::open(&directory, &name, false, true)?;
        Ok(Self { file, directory, name:Some(name), budget, holds:vec![metadata] })
    }
    pub fn rewind(&mut self) -> Result<()> { self.file.seek(SeekFrom::Start(0))?; Ok(()) }
    pub fn publish(mut self) -> Result<String> {
        crate::accounts::phase("media_before_blob_durability")?;
        self.file.sync_all()?;
        self.publish_name()
    }
    /// Attachment staging finishes file durability outside publication scopes.
    /// The wrapper prevents subsequent writes before the guarded rename.
    pub(crate) fn into_synced(self) -> Result<SyncedTemporary> {
        self.file.sync_all()?;
        Ok(SyncedTemporary(self))
    }
    fn publish_name(&mut self) -> Result<String> {
        let blob = format!("blob-{}", crypto::random_token()?);
        // Disarm before the rename: a directory-sync error can mean the rename
        // already happened. Preserve either name for exclusive startup recovery.
        let stage = self.name.take().ok_or_else(integrity)?;
        at::publish(&self.directory, &stage, &blob)?;
        Ok(blob)
    }
}
pub(crate) struct SyncedTemporary(Temporary);
impl SyncedTemporary {
    pub(crate) fn publish(mut self) -> Result<String> {
        crate::accounts::phase("media_before_blob_durability")?;
        self.0.publish_name()
    }
}
impl Write for Temporary {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let hold = self.budget.reserve(bytes.len() as u64).map_err(|_|std::io::Error::other("temporary storage unavailable"))?;
        self.holds.push(hold);
        crate::accounts::phase("media_stage_write").map_err(|_|std::io::Error::from_raw_os_error(libc::ENOSPC))?;
        self.file.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { self.file.flush() }
}
impl Seek for Temporary {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> { self.file.seek(pos) }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if let Some(name) = self.name.take() { let _ = at::unlink(&self.directory, &name); }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Content { pub blob:String, pub size:u64, pub sha1:String, pub sha256:Vec<u8> }
pub(crate) fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b|format!("{b:02x}")).collect() }
pub(crate) fn verify(file: &mut File, content: &Content) -> Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut sha1=Sha1::new(); let mut sha256=Sha256::new(); let mut size=0u64;
    let mut block=[0;65536];
    loop {
        let maximum=(content.size.saturating_sub(size).min(65535)+1) as usize;
        let n=file.read(&mut block[..maximum])?;
        if n==0 { break; }
        size+=n as u64;
        if size>content.size { return Err(integrity()); }
        sha1.update(&block[..n]); sha256.update(&block[..n]);
    }
    let strong:[u8;32]=sha256.finalize().into();
    if size!=content.size || hex(&sha1.finalize())!=content.sha1 || strong.as_slice()!=content.sha256.as_slice() {
        return Err(integrity());
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}
/// Digest equality is a filter, never proof that existing bytes may be reused.
/// Called outside publication coordination, including repeated-name comparisons.
pub(crate) fn equal(root:&DataRoot, id:&str, a:&Content, b:&Content) -> Result<bool> {
    let mut left=root.media_blob(id,&a.blob)?; verify(&mut left,a)?;
    if a.size!=b.size || a.sha256!=b.sha256 { return Ok(false); }
    let mut right=root.media_blob(id,&b.blob)?; verify(&mut right,b)?;
    let mut x=[0;65536]; let mut y=[0;65536];
    loop {
        let n=left.read(&mut x)?;
        right.read_exact(&mut y[..n])?;
        if x[..n]!=y[..n] { return Ok(false); }
        if n==0 { return Ok(true); }
    }
}

#[cfg(test)]
#[test]
fn pinned_staging_never_resolves_a_recreated_media_directory() {
    use std::{fs, num::NonZeroU64, os::unix::fs::PermissionsExt};
    for purged in [false,true] {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("media");
        let retired = temp.path().join("retired");
        fs::create_dir(&current).unwrap();
        fs::set_permissions(&current,fs::Permissions::from_mode(0o700)).unwrap();
        let pinned = File::open(&current).unwrap();
        // Replace the path before even creating the stage, as a reset during
        // acquisition can do. A purged inode must fail closed; a retained inode
        // may receive old work, but the recreated directory must stay empty.
        if purged { fs::remove_dir(&current).unwrap(); }
        else { fs::rename(&current,&retired).unwrap(); }
        fs::create_dir(&current).unwrap();
        fs::set_permissions(&current,fs::Permissions::from_mode(0o700)).unwrap();
        let staged = Temporary::create_in(pinned,TempBudget::new(NonZeroU64::new(8192).unwrap()));
        if purged { assert!(staged.is_err()); }
        else {
            let mut staged = staged.unwrap();
            staged.write_all(b"generated old epoch bytes").unwrap();
            let blob = staged.into_synced().unwrap().publish().unwrap();
            assert_eq!(fs::read(retired.join(blob)).unwrap(),b"generated old epoch bytes");
        }
        assert_eq!(fs::read_dir(&current).unwrap().count(),0);
    }
}
