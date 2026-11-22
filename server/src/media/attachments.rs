// SPDX-License-Identifier: MIT
//! Insert-or-reuse native media boundary for M8. Preparation owns all byte work;
//! installation is synchronous under the caller's sole publication barrier and
//! collection transaction. File publication never owns the metadata writer (L4).
use super::*;
use crate::{accounts::oauth, limits::TempBudget, mcp_service::attachments::Validated, storage};
use std::io::{Read, Write};
pub(crate) struct Insert {
    name: String,
    observed: Option<index::State>,
    temporary: Option<blobs::SyncedTemporary>,
    content: Option<Content>,
}
#[cfg(test)]
pub(crate) fn prepare(root: &crate::DataRoot, id: &str, values: Vec<Arc<Validated>>, budget: TempBudget) -> Result<Vec<Insert>> {
    prepare_in(root,id,root.media_directory(id)?,values,budget)
}
pub(crate) fn prepare_in(root: &crate::DataRoot, id: &str, directory: std::fs::File, values: Vec<Arc<Validated>>, budget: TempBudget) -> Result<Vec<Insert>> {
    // Snapshot ends before reading or hashing any blob bytes.
    let observed = {
        let mut c = storage::inspect(root.database())?;
        let tx = c.transaction()?;
        let rows = values.iter().map(|v|Ok((v.name.clone(),index::state(&tx,id,&v.name)?))).collect::<Result<BTreeMap<_,_>>>()?;
        tx.commit()?; rows
    };
    let mut prepared = Vec::new();
    for v in values {
        let prior = observed.get(&v.name).cloned().flatten();
        if let Some(old) = prior.as_ref().and_then(|s|s.content.as_ref()) {
            if !blobs::generated(&old.blob,"blob-") { return Err(blobs::integrity()); }
            let mut file = storage::owned::open(&directory,&old.blob,false,false).map_err(|_|blobs::integrity())?;
            blobs::verify(&mut file,old)?;
            if old.size != v.bytes.len() as u64 { return Err(Error::new("attachment_conflict")); }
            let mut offset = 0; let mut block = [0;65536];
            loop {
                let n = file.read(&mut block)?;
                if n == 0 { break; }
                if v.bytes.get(offset..offset+n) != Some(&block[..n]) { return Err(Error::new("attachment_conflict")); }
                offset += n;
            }
            if offset != v.bytes.len() { return Err(Error::new("attachment_conflict")); }
            prepared.push(Insert { name:v.name.clone(),observed:prior,temporary:None,content:None });
        } else {
            let mut temporary = blobs::Temporary::create_in(directory.try_clone()?,budget.clone())?;
            for block in v.bytes.chunks(65536) { temporary.write_all(block)?; }
            prepared.push(Insert { name:v.name.clone(),observed:prior,temporary:Some(temporary.into_synced()?),
                content:Some(Content {blob:String::new(),size:v.bytes.len() as u64,sha1:v.sha1.clone(),sha256:v.sha256.clone()}) });
        }
    }
    Ok(prepared)
}
pub(crate) fn install(root: &crate::DataRoot, original: &oauth::Authority, prepared: &mut [Insert]) -> Result<()> {
    let id = original.account_id();
    // The caller holds the target barrier. Refuse ordinary stale/invalid work
    // before creating durable blobs, without retaining a metadata transaction
    // through rename or directory sync.
    {
        let mut c = storage::inspect(root.database())?;
        c.busy_timeout(std::time::Duration::ZERO)?;
        let tx = c.transaction()?;
        check(&tx,original,prepared)?;
        tx.commit()?;
    }
    let mut changes = Vec::new();
    for item in prepared.iter_mut() {
        if let Some(temporary) = item.temporary.take() {
            let mut content = item.content.take().ok_or(Error::new("internal_error"))?;
            content.blob = temporary.publish()?;
            changes.push((item.name.clone(),Some(content)));
        }
    }
    let mut c = storage::open(root.database(),false)?;
    c.busy_timeout(std::time::Duration::ZERO)?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    check(&tx,original,prepared)?;
    if !changes.is_empty() {
        index::apply(&tx,id,&changes,&vec![true;changes.len()])?;
        crate::accounts::phase("media_before_metadata_commit")?;
    }
    tx.commit()?;
    Ok(())
}
fn check(c: &rusqlite::Connection, original: &oauth::Authority, prepared: &[Insert]) -> Result<()> {
    let id = original.account_id();
    oauth::check_authority(c,original,true)?;
    for item in prepared {
        if index::state(c,id,&item.name)? != item.observed { return Err(Error::new("attachment_conflict")); }
    }
    // content is consumed only for rows that need a new media USN. Count those
    // same rows both before and after publishing the immutable blobs.
    let count = prepared.iter().filter(|v|v.observed.as_ref().is_none_or(|s|s.content.is_none())).count() as i32;
    index::cursor(c,id)?.checked_add(count).ok_or(Error::new("media_usn_exhausted"))?;
    Ok(())
}
