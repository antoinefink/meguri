// SPDX-License-Identifier: MIT
//! One dedicated transaction per admitted account round. Locks cover individual
//! requests only; the publication barrier also serializes finish and revocation.
use crate::{Error, Result, accounts::{PublicationBarrier, RoundRetirer}, limits::TempReservation, storage::DataRoot};
use rusqlite::Connection;
use std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tokio::sync::OwnedSemaphorePermit;

pub(crate) const IDLE: Duration = Duration::from_secs(120);
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase { Graves, Sending, Receiving, Sanity, Finish }
pub(crate) struct Round {
    pub db: Connection,
    pub path: PathBuf,
    pub key: String,
    pub credential_generation: i64,
    pub usn: i32,
    pub min_usn: i32,
    pub newer: bool,
    pub phase: Phase,
    pub completed: Instant,
    pub cursor: [Option<i64>;3],
    pub exhausted: [bool;3],
    pub turn: usize,
    pub _capacity: OwnedSemaphorePermit,
    pub _reservation: TempReservation,
    // The root lock must outlive rollback even if the service is shutting down.
    pub _root: Arc<DataRoot>,
}
impl Drop for Round {
    fn drop(&mut self) { let _ = self.db.execute_batch("ROLLBACK"); }
}
#[derive(Default)]
pub(crate) struct Slot(pub Mutex<Option<Round>>);
impl RoundRetirer for Slot {
    fn retire(&self) -> Result<()> {
        self.0.lock().map_err(|_|Error::new("storage_unavailable"))?.take();
        Ok(())
    }
}
struct Entry { slot: Arc<Slot>, _gate: Arc<PublicationBarrier> }
#[derive(Default)]
pub(crate) struct Sessions(Mutex<HashMap<String,Entry>>);
impl Sessions {
    /// Inspect only: callers own the account publication barrier. Even an idle
    /// retained round owns its writer until native cleanup retires it. Registry
    /// or slot contention is conservatively active; never allocate or prune.
    pub(crate) fn has_retained_writer(&self, id: &str) -> Result<bool> {
        use std::sync::TryLockError;
        let slot = match self.0.try_lock() {
            Ok(map) => map.get(id).map(|entry| entry.slot.clone()),
            Err(TryLockError::WouldBlock) => return Ok(true),
            Err(TryLockError::Poisoned(_)) => return Err(Error::new("storage_unavailable")),
        };
        let Some(slot) = slot else { return Ok(false); };
        match slot.0.try_lock() {
            Ok(state) => Ok(state.is_some()),
            Err(TryLockError::WouldBlock) => Ok(true),
            Err(TryLockError::Poisoned(_)) => Err(Error::new("storage_unavailable")),
        }
    }
    pub fn new() -> Arc<Self> {
        let sessions=Arc::new(Self::default());
        let weak=Arc::downgrade(&sessions);
        // Exactly one bounded sweeper per service, not one sleeping task per
        // start/replacement. It holds no registry/round/root across the sleep.
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(sessions)=weak.upgrade() else {break;};
                let _=tokio::task::spawn_blocking(move ||sessions.expire_idle()).await;
            }
        });
        sessions
    }
    fn expire_idle(&self) {
        let slots=match self.0.lock() {
            Ok(map)=>map.values().map(|e|e.slot.clone()).collect::<Vec<_>>(),
            Err(_)=>return,
        };
        for slot in slots {
            if let Ok(mut state)=slot.0.try_lock() {
                if state.as_ref().is_some_and(|r|r.completed.elapsed()>=IDLE) {state.take();}
            }
        }
    }
    pub fn close(&self) {
        let entries=match self.0.lock() {Ok(mut map)=>std::mem::take(&mut *map),Err(_)=>return};
        for entry in entries.into_values() {
            let _=entry.slot.retire();
            let _=entry._gate.set_round(None);
        }
    }
    pub fn slot(&self, id: &str, gate: Arc<PublicationBarrier>) -> Result<Arc<Slot>> {
        let mut map=self.0.lock().map_err(|_|Error::new("storage_unavailable"))?;
        map.retain(|_,e| Arc::strong_count(&e._gate)>1 || e.slot.0.try_lock().map_or(true,|s|s.is_some()));
        if let Some(e)=map.get(id) { return Ok(e.slot.clone()); }
        if map.len()>=1024 { return Err(Error::new("storage_unavailable")); }
        let slot=Arc::new(Slot::default());
        map.insert(id.into(),Entry {slot:slot.clone(),_gate:gate});
        Ok(slot)
    }
}
pub(crate) fn conflict() -> Error { Error::new("invalid_session") }
pub(crate) fn unsupported() -> Error { Error::new("unsupported_normal_case") }

impl Round {
    pub fn require(&self, phase: Phase) -> Result<()> {
        if self.phase==phase { Ok(()) } else { Err(conflict()) }
    }
    pub fn snapshot_rows(&self) -> Result<()> {
        // These private connection-local tables never enter transferable files.
        for table in ["revlog","notes","cards"] {
            self.db.execute(&format!("CREATE TEMP TABLE outgoing_{table} AS SELECT * FROM {table} WHERE usn>=?1"),[self.min_usn])?;
            self.db.execute_batch(&format!("CREATE UNIQUE INDEX temp.outgoing_{table}_id ON outgoing_{table}(id)"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn expired_real_http_round_rolls_back_and_releases_connection_capacity() {
        let temp=tempfile::tempdir().unwrap();
        let path=temp.path().join("owner");let mut root=DataRoot::acquire(&path,true).unwrap();root.init().unwrap();
        let token=std::fs::read_to_string(root.setup_file()).unwrap();
        let service=crate::Service::open(root,crate::Config::new("https://example.test","127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let id=service.claim("127.0.0.1".parse().unwrap(),"setup",token.trim().into(),"synthetic","synthetic password".into()).await.unwrap().id;
        let (_,key)=service.native_login("127.0.0.1".parse().unwrap(),"synthetic","synthetic password".into()).await.unwrap();
        let authority=service.validate_hostkey(&key).await.unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
        let (stop,stopped)=tokio::sync::oneshot::channel();let serving=service.clone();
        let server=tokio::spawn(async move {crate::httpapi::serve(listener,serving,async {let _=stopped.await;}).await.unwrap();});
        let client=reqwest::Client::builder().no_proxy().build().unwrap();
        for (method,body) in [("start",json!({"minUsn":0,"lnewer":false,"graves":null})),("applyChanges",json!({"changes":{"models":[],"decks":[[],[]],"tags":["expires"]}}))] {
            let r=client.post(format!("http://{address}/sync/{method}")).header("anki-sync",json!({"v":11,"k":key,"s":"expiry","c":"synthetic"}).to_string())
                .body(zstd::stream::encode_all(serde_json::to_vec(&body).unwrap().as_slice(),0).unwrap()).send().await.unwrap();
            assert_eq!(r.status(),200);r.bytes().await.unwrap();
        }
        // Advance only the retained round's private monotonic completion stamp.
        // No production clock override, export, environment switch, or sleep.
        let gate=service.publication_barrier(&authority).await.unwrap();
        let slot=service.normal_sessions.slot(&id,gate).unwrap();
        slot.0.lock().unwrap().as_mut().unwrap().completed=Instant::now()-IDLE;
        let r=client.post(format!("http://{address}/sync/chunk")).header("anki-sync",json!({"v":11,"k":key,"s":"expiry","c":"synthetic"}).to_string())
            .body(zstd::stream::encode_all(b"{}".as_slice(),0).unwrap()).send().await.unwrap();
        assert_eq!(r.status(),409);r.bytes().await.unwrap();
        assert!(slot.0.lock().unwrap().is_none());
        let root=service.metadata.owner();let committed=crate::collection::schema::open(&root.collection_path(&id).unwrap(),false).unwrap();
        assert_eq!(committed.query_row("SELECT count(*) FROM tags",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        let held=(0..16).map(|_|service.collections.acquire().unwrap()).collect::<Vec<_>>();
        assert!(service.collections.acquire().is_err());drop(held);
        stop.send(()).unwrap();server.await.unwrap();
    }
}
