// SPDX-License-Identifier: MIT
//! Startup-only reconciliation under DataRoot's exclusive lifecycle owner.
use super::{blobs::{generated, integrity}, index};
use crate::{DataRoot, Result, storage::owned as at};
use rusqlite::Connection;
use std::collections::BTreeSet;

pub(crate) fn validate(root:&DataRoot,c:&Connection)->Result<()> {
    if c.prepare("SELECT 1 FROM accounts WHERE typeof(media_usn)<>'integer' OR media_usn<0 OR media_usn>2147483647")?.exists([])? {return Err(integrity());}
    if c.prepare("SELECT 1 FROM media_names n JOIN accounts a ON a.id=n.account_id WHERE typeof(n.usn)<>'integer' OR n.usn<=0 OR n.usn>a.media_usn")?.exists([])? {return Err(integrity());}
    let mut names=c.prepare("SELECT name FROM media_names")?;
    for name in names.query_map([],|r|r.get::<_,String>(0))? {super::name(&name?).map_err(|_|integrity())?;}
    let mut blobs=c.prepare("SELECT account_id,blob,size,sha1,sha256 FROM media_blobs")?;
    let mut rows=blobs.query([])?;
    while let Some(r)=rows.next()? {
        let id:String=r.get(0)?; let blob:String=r.get(1)?; let size:i64=r.get(2)?;
        let hash:String=r.get(3)?; let strong:Vec<u8>=r.get(4)?;
        if !(1..=104_857_600).contains(&size) || hash.len()!=40 || !hash.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || strong.len()!=32 {
            return Err(integrity());
        }
        let size=u64::try_from(size).map_err(|_|integrity())?;
        if root.media_blob(&id,&blob)?.metadata()?.len()!=size {return Err(integrity());}
    }
    Ok(())
}
pub(crate) fn startup(root:&DataRoot)->Result<()> {
    let c=crate::storage::open(root.database(),false)?;
    let mut accounts=c.prepare("SELECT id FROM accounts WHERE provisioning_state='ready'")?;
    for id in accounts.query_map([],|r|r.get::<_,String>(0))? {
        let id=id?;
        let directory=root.media_directory(&id)?;
        let mut q=c.prepare("SELECT blob FROM media_blobs WHERE account_id=?1")?;
        let keep=q.query_map([&id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<BTreeSet<_>>>()?;
        for entry in std::fs::read_dir(root.stores(&id)?.media)? {
            let entry=entry?;
            let Some(name)=entry.file_name().to_str().map(str::to_owned) else {continue;};
            if generated(&name,"stage-") || (generated(&name,"blob-") && !keep.contains(&name)) {
                // Descriptor-relative type/mode/link checks precede removal;
                // unknown files and all historically referenced blobs survive.
                at::open(&directory,&name,false,false)?;
                at::unlink(&directory,&name)?;
            }
        }
    }
    Ok(())
}
impl DataRoot {
    pub(crate) fn upgrade_media(&self)->Result<()> {
        self.check_initialized()?;
        let mut c=crate::storage::open(self.database(),false)?;
        let version:i64=c.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if version==1 {
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(index::MIGRATION)?;
            crate::accounts::phase("media_migration_before_commit")?;
            tx.commit()?;
        }
        self.check_initialized()?;
        Ok(())
    }
}
