// SPDX-License-Identifier: MIT
//! Durable media publication, independent of collection-round state.
pub(crate) mod archive;
pub(crate) mod blobs;
pub(crate) mod index;
pub(crate) mod recovery;
use crate::{Service, NativeAuthority, Result, Error};
use blobs::Content;
use std::{collections::BTreeMap,sync::Arc};
use unicode_normalization::UnicodeNormalization;

pub(crate) fn name(value:&str)->Result<()> {
    if value.is_empty() || value.len()>255 || matches!(value,"."|"..") || value.contains(['/', '\\', '\0']) || !value.nfc().eq(value.chars()) {
        return Err(Error::new("invalid_request"));
    }
    Ok(())
}
/// Retry only if the brief snapshot was superseded while bytes were compared.
/// No hash or file read occurs in the metadata writer or account barrier.
pub(crate) async fn upload(service:Arc<Service>,authority:NativeAuthority,changes:Vec<(String,Option<Content>)>)->Result<(u32,i32)> {
    let processed=changes.len() as u32;
    let owner=service.metadata.owner();
    for _ in 0..8 {
        let id=authority.account_id().to_owned(); let names=changes.iter().map(|v|v.0.clone()).collect::<Vec<_>>();
        let initial=service.publish(authority.clone(),move |c| {
            let mut states=BTreeMap::new();
            for n in names { if !states.contains_key(&n) { states.insert(n.clone(),index::state(c,&id,&n)?); } }
            Ok(states)
        }).await?;
        let root=owner.clone(); let id=authority.account_id().to_owned(); let batch=changes.clone(); let expected=initial.clone();
        let effective=tokio::task::spawn_blocking(move || {
            let mut current=expected.into_iter().map(|(n,s)|(n,s.and_then(|s|s.content))).collect::<BTreeMap<_,_>>();
            let mut effective=Vec::with_capacity(batch.len());
            for (n,next) in batch {
                let prior=current.get(&n).and_then(|s|s.as_ref());
                let changed=match (prior,next.as_ref()) {
                    (None,None)=>false,
                    (Some(a),Some(b))=>!blobs::equal(&root,&id,a,b)?,
                    _=>true,
                };
                effective.push(changed);
                if changed { current.insert(n,next); }
            }
            Ok::<_,Error>(effective)
        }).await.map_err(|_|Error::new("storage_unavailable"))??;
        service.pause("media_before_publication").await;
        let id=authority.account_id().to_owned(); let batch=changes.clone();
        let result=service.publish_at(authority.clone(),"media_with_barrier",move |c| {
            for (n,expected) in initial { if index::state(c,&id,&n)?!=expected { return Err(Error::new("media_retry")); } }
            let usn=index::apply(c,&id,&batch,&effective)?;
            crate::accounts::phase("media_before_metadata_commit")?;
            Ok(usn)
        }).await;
        match result {
            Err(e) if e.code=="media_retry"=>continue,
            result=>{
                let usn=result?;
                // Test-only lost-response/crash boundary is after durable commit.
                #[cfg(feature="test-hooks")]
                tokio::task::spawn_blocking(||crate::accounts::phase("media_after_metadata_commit")).await.map_err(|_|Error::new("storage_unavailable"))??;
                return Ok((processed,usn));
            }
        }
    }
    Err(Error::new("storage_unavailable"))
}
