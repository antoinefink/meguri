// SPDX-License-Identifier: MIT
use crate::{Error, Result, Service, NativeAuthority, collection::{schema,session::{self,Round,Phase},graves,changes,chunks,sanity}};
use axum::{body::Body,response::Response};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{sync::Arc,time::Instant};

#[derive(Deserialize)]
struct Start { #[serde(rename="minUsn")] min:i32, lnewer:bool, graves:Box<RawValue> }
#[derive(Deserialize)] struct Graves {chunk:graves::Graves}
#[derive(Deserialize)] struct Changes {changes:changes::Changes}
#[derive(Deserialize)] struct Chunk {chunk:chunks::Chunk}
#[derive(Deserialize)] struct Sanity {client:sanity::Counts}
#[derive(Deserialize)] struct Empty {}
fn encode(value:&impl serde::Serialize)->Result<Vec<u8>> {serde_json::to_vec(value).map_err(|_|Error::new("internal_error"))}

pub(crate) async fn handle(service:Arc<Service>,authority:NativeAuthority,key:String,method:String,body:&mut Body)->Result<Response> {
    if key.is_empty() {return Err(Error::new("invalid_request"));}
    // Decompress before acquiring the account barrier. Decode/merge errors are
    // then retired only if the request owns the currently live round.
    let bytes=super::framing::receive(service.clone(),body,move |reader,limit| {
        let mut bytes=Vec::new();super::framing::decode(reader,&mut bytes,limit)?;Ok(bytes)
    }).await;
    let gate=service.publication_barrier(&authority).await?;
    let slot=service.normal_sessions.slot(authority.account_id(),gate.clone())?;
    let owner=service.metadata.owner();
    let capacity=service.collections.clone();let budget=service.temp.clone();
    let identity=authority.clone();
    let output=service.collection_operation(authority,false,move |root| {
        let mut state=slot.0.lock().map_err(|_|Error::new("storage_unavailable"))?;
        if state.as_ref().is_some_and(|r|r.completed.elapsed()>=session::IDLE) {state.take();gate.set_round(None)?;}
        let owns=state.as_ref().is_some_and(|r|r.key==key && r.credential_generation==identity.credential_generation());
        if method!="start" && !owns {return Err(session::conflict());}
        let result=(|| {
            let bytes=bytes?;
            if method=="start" {
                let start:Start=super::framing::json(&bytes)?;
                if start.graves.get()!="null" {return Err(Error::new("invalid_request"));}
                // A valid admitted start replaces any unfinished owner, even
                // when it reuses the same session key.
                state.take();gate.set_round(None)?;
                let permit=capacity.acquire()?;
                let path=root.collection_path(identity.account_id())?;
                crate::storage::private_collection_file(&path)?;
                let mut base=std::fs::metadata(&path)?.len();
                let mut wal=path.as_os_str().to_os_string();wal.push("-wal");
                match std::fs::metadata(&wal) {
                    Ok(metadata)=>base=base.checked_add(metadata.len()).ok_or(Error::new("storage_unavailable"))?,
                    Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(),
                    Err(e)=>return Err(e.into()),
                }
                // Limit per-round growth, reserve database/WAL/snapshot space
                // first, and prohibit repeated dirty-page spills to the WAL.
                let maximum=base.checked_add(16*1024*1024).ok_or(Error::new("storage_unavailable"))?;
                let reservation=budget.reserve(maximum.checked_mul(4).ok_or(Error::new("storage_unavailable"))?)?;
                let db=schema::open(&path,true)?;
                let page_size:i64=db.pragma_query_value(None,"page_size",|r|r.get(0))?;
                let page_size=u64::try_from(page_size).map_err(|_|Error::new("storage_unavailable"))?;
                let max_pages=i64::try_from(maximum/page_size).map_err(|_|Error::new("storage_unavailable"))?;
                db.pragma_update(None,"max_page_count",max_pages)?;
                db.pragma_update(None,"temp_store",2)?;
                db.pragma_update(None,"cache_spill",false)?;
                db.execute_batch("BEGIN IMMEDIATE")?;
                let usn:i32=db.query_row("SELECT usn FROM col WHERE id=1",[],|r|r.get(0))?;
                usn.checked_add(1).ok_or(Error::new("storage_unavailable"))?;
                let round=Round {db,path,key:key.clone(),credential_generation:identity.credential_generation(),usn,min_usn:start.min,newer:start.lnewer,phase:Phase::Graves,completed:Instant::now(),cursor:[None;3],exhausted:[false;3],turn:0,_capacity:permit,_reservation:reservation,_root:owner};
                let response=encode(&graves::outgoing(&round.db,round.min_usn)?)?;
                let mut round=round;round.completed=Instant::now();
                *state=Some(round);gate.set_round(Some(slot.clone()))?;
                return Ok(response);
            }
            let r=state.as_mut().ok_or(session::conflict())?;
            if r.path!=root.collection_path(identity.account_id())? {return Err(session::conflict());}
            let output=match method.as_str() {
                "abort"=> {let _:Empty=super::framing::json(&bytes)?;encode(&())?},
                "applyGraves"=> {
                    r.require(Phase::Graves)?;
                    let g:Graves=super::framing::json(&bytes)?;graves::apply(&r.db,g.chunk,r.usn)?;encode(&())?
                },
                "applyChanges"=> {
                    r.require(Phase::Graves)?;
                    let incoming:Changes=super::framing::json(&bytes)?;
                    r.snapshot_rows()?;
                    let output=encode(&changes::outgoing(&r.db,r.min_usn,r.newer)?)?;
                    changes::apply(r,incoming.changes)?;
                    r.phase=Phase::Sending;output
                },
                "chunk"=> {
                    r.require(Phase::Sending)?;
                    let _:Empty=super::framing::json(&bytes)?;
                    let chunk=chunks::outgoing(r)?;
                    if chunk["done"]==true {r.phase=Phase::Receiving;}
                    encode(&chunk)?
                },
                "applyChunk"=> {
                    r.require(Phase::Receiving)?;
                    let request:Chunk=super::framing::json(&bytes)?;
                    let done=request.chunk.done;
                    chunks::apply(r,request.chunk)?;
                    if done {r.phase=Phase::Sanity;}
                    encode(&())?
                },
                "sanityCheck2"=> {
                    r.require(Phase::Sanity)?;
                    let request:Sanity=super::framing::json(&bytes)?;
                    let client=request.client.compared();let server=sanity::counts(&r.db)?;
                    let ok=client==server;
                    let response=encode(&serde_json::json!({"status":if ok {"ok"} else {"bad"},"c":client,"s":server}))?;
                    if ok {r.phase=Phase::Finish;r.completed=Instant::now();} else {state.take();gate.set_round(None)?;}
                    return Ok(response);
                },
                "finish"=> {
                    r.require(Phase::Finish)?;
                    let _:Empty=super::framing::json(&bytes)?;
                    let next=r.usn.checked_add(1).ok_or(Error::new("storage_unavailable"))?;
                    let stamp=(time::OffsetDateTime::now_utc().unix_timestamp_nanos()/1_000_000) as i64;
                    let response=encode(&stamp)?;
                    r.db.execute("UPDATE col SET mod=?1,ls=?1,usn=?2 WHERE id=1",rusqlite::params![stamp,next])?;
                    crate::accounts::phase("normal_before_commit")?;
                    r.db.execute_batch("COMMIT")?;
                    // The round is retired before any acknowledgement work.
                    state.take();gate.set_round(None)?;
                    crate::accounts::phase("normal_after_commit")?;
                    return Ok(response);
                },
                _=>return Err(Error::new("route_not_found")),
            };
            if method=="abort" {state.take();gate.set_round(None)?;}
            else if let Some(r)=state.as_mut() {r.completed=Instant::now();}
            Ok(output)
        })();
        if result.is_err() && (owns || method=="start" && state.as_ref().is_some_and(|r|r.key==key)) {
            state.take();gate.set_round(None)?;
        }
        result
    }).await?;
    super::framing::response(&output)
}
