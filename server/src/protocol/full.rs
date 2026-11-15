// SPDX-License-Identifier: MIT
use crate::{Error, Result, Service, NativeAuthority, collection::schema, limits::TempReservation, storage::collection::Generation};
use std::{fs::File, io::{Read,Write}, sync::Arc};

#[cfg(test)]
std::thread_local! { static STAGE_DISK_FULL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(all(test,feature="test-hooks"))]
std::thread_local! {
    static PUBLICATION_BOUNDARY: std::cell::RefCell<Option<(tokio::sync::oneshot::Sender<()>,std::sync::mpsc::Receiver<()>)>> = const { std::cell::RefCell::new(None) };
}

pub(crate) struct Upload { pub generation:Generation,pub reservations:Vec<TempReservation> }
pub(crate) fn receive(reader:Box<dyn Read+Send>,limit:u64,budget:crate::limits::TempBudget,generation:Generation,metadata_reservation:TempReservation)->Result<Upload> {
    let mut upload=Upload {generation,reservations:vec![metadata_reservation]};
    struct Reserved<'a> {file:File,holds:&'a mut Vec<TempReservation>,budget:crate::limits::TempBudget}
    impl Write for Reserved<'_> {
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize> {
            let hold=self.budget.reserve(bytes.len() as u64).map_err(|_|std::io::Error::other("temporary storage unavailable"))?;
            self.holds.push(hold);
            #[cfg(test)]
            if STAGE_DISK_FULL.with(|v|v.get()) {return Err(std::io::Error::from_raw_os_error(libc::ENOSPC));}
            self.file.write_all(bytes)?;
            Ok(bytes.len())
        }
        fn flush(&mut self)->std::io::Result<()> {self.file.flush()}
    }
    let mut writer=Reserved {file:upload.generation.writer()?,holds:&mut upload.reservations,budget};
    super::framing::decode(reader,&mut writer,limit)?;
    crate::accounts::phase("collection_before_stage_sync")?;
    writer.file.sync_all()?;
    drop(writer);
    let c=schema::open_upload(&upload.generation.path).map_err(|_|Error::new("invalid_collection"))?;
    schema::validate(&c).map_err(|_|Error::new("invalid_collection"))?;
    drop(c);
    // Validation neither mutates the main file nor creates SQLite sidecars.
    // The streamed main bytes and bounded owner/pointer metadata are the only
    // upload disk work; their reservations remain with this candidate.
    upload.generation.durable()?;
    Ok(upload)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn upload_main_file_forms_need_no_sqlite_sidecar_budget() {
        use std::os::unix::fs::OpenOptionsExt;
        let temp=tempfile::tempdir().unwrap();let mut root=crate::DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
        let setup=std::fs::read_to_string(root.setup_file()).unwrap();
        let service=Service::open(root,crate::Config::new("https://example.test","127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let identity=service.claim("127.0.0.1".parse().unwrap(),"setup",setup.trim().into(),"synthetic","synthetic password".into()).await.unwrap();
        let account=temp.path().join("owner/state/accounts").join(&identity.id);
        let before=std::fs::read(account.join("current-collection")).unwrap();
        for page_size in [4096,65536] { for mode in ["DELETE","WAL"] {
            // Owned, modest fixtures, including URI metacharacters in the path.
            let input=temp.path().join(format!("owned #?% {page_size}-{mode}.anki2"));
            std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&input).unwrap();
            let c=rusqlite::Connection::open(&input).unwrap();crate::collection::unicase::register(&c).unwrap();
            c.pragma_update(None,"page_size",page_size).unwrap();
            c.execute_batch(include_str!("../collection/schema.sql")).unwrap();
            c.execute("INSERT INTO col VALUES(1,123,0,456,18,0,0,0,'','','','','')",[]).unwrap();
            c.execute("INSERT INTO config VALUES('owned-payload',0,0,?1)",[vec![7u8;131075]]).unwrap();
            c.pragma_update(None,"journal_mode",mode).unwrap();drop(c);
            let bytes=std::fs::read(&input).unwrap();
            assert_eq!(bytes[18],if mode=="WAL" {2} else {1});
            let check=schema::open_upload(&input).unwrap();schema::validate(&check).unwrap();
            assert_eq!(check.pragma_query_value(None,"foreign_keys",|r|r.get::<_,i64>(0)).unwrap(),0);
            assert_eq!(check.pragma_query_value(None,"temp_store",|r|r.get::<_,i64>(0)).unwrap(),2);
            for suffix in ["-wal","-shm","-journal"] {assert!(!std::path::PathBuf::from(format!("{}{suffix}",input.display())).exists());}
            drop(check);assert_eq!(std::fs::read(&input).unwrap(),bytes);
            for short in [true,false] {
                let budget=crate::limits::TempBudget::new(std::num::NonZeroU64::new(bytes.len() as u64+4096-u64::from(short)).unwrap());
                let metadata=budget.reserve(4096).unwrap();let id=identity.id.clone();
                let generation=service.metadata.files(move |r|r.stage_collection(&id)).await.unwrap();let path=generation.path.clone();
                let compressed=zstd::stream::encode_all(bytes.as_slice(),0).unwrap();
                let result=receive(Box::new(std::io::Cursor::new(compressed)),314_572_800,budget.clone(),generation,metadata);
                if short {assert!(matches!(result,Err(ref e) if e.code=="storage_unavailable"));}
                else {
                    let upload=result.unwrap();assert_eq!(budget.used(),bytes.len() as u64+4096);
                    assert_eq!(std::fs::read(&path).unwrap(),bytes);
                    let mut names=std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e|e.unwrap().file_name()).collect::<Vec<_>>();names.sort();
                    assert_eq!(names,vec![std::ffi::OsString::from("collection.anki2"),std::ffi::OsString::from("owner")]);
                    drop(upload);
                }
                assert!(!path.exists());assert_eq!(budget.used(),0);
                assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),before);
            }
        }}
    }
    #[tokio::test]
    async fn prepublication_metadata_refusal_preserves_manifest_and_native_admission() {
        let temp=tempfile::tempdir().unwrap();let mut root=crate::DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
        let setup=std::fs::read_to_string(root.setup_file()).unwrap();
        let service=Service::open(root,crate::Config::new("https://example.test","127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let ip="127.0.0.1".parse().unwrap();
        let identity=service.claim(ip,"setup",setup.trim().into(),"synthetic","synthetic password".into()).await.unwrap();
        let (_,key)=service.native_login(ip,"synthetic","synthetic password".into()).await.unwrap();
        let authority=service.validate_hostkey(&key).await.unwrap();
        let account=temp.path().join("owner/state/accounts").join(&identity.id);
        let before=std::fs::read(account.join("current-collection")).unwrap();
        let id=identity.id.clone();let generation=service.metadata.files(move |r|r.stage_collection(&id)).await.unwrap();
        schema::create_blank(&generation.path,123,456).unwrap();let staged=generation.path.clone();
        let upload=Upload {generation,reservations:vec![service.temp.reserve(4096).unwrap()]};
        let held=service.metadata.test_hold_capacity();
        assert!(matches!(publish(service.clone(),authority,upload).await,Err(e) if e.code=="storage_unavailable"));
        drop(held);
        assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),before);
        assert!(!staged.exists());assert_eq!(service.temp.used(),0);
        let _permit=service.native_ingress.enter().expect("transient metadata admission must recover");
        let authority=service.validate_hostkey(&key).await.unwrap();
        let generation=service.metadata.files(move |r|r.stage_collection(&identity.id)).await.unwrap();
        schema::create_blank(&generation.path,123,789).unwrap();
        let upload=Upload {generation,reservations:vec![service.temp.reserve(4096).unwrap()]};
        publish(service.clone(),authority,upload).await.unwrap();
        assert_ne!(std::fs::read(account.join("current-collection")).unwrap(),before);
        assert_eq!(service.temp.used(),0);
    }
    #[cfg(feature="test-hooks")]
    async fn uncertain_publication_lifetime(cancel:bool) {
        let temp=tempfile::tempdir().unwrap();let mut root=crate::DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
        let setup=std::fs::read_to_string(root.setup_file()).unwrap();
        let service=Service::open(root,crate::Config::new("https://example.test","127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let ip="127.0.0.1".parse().unwrap();
        let identity=service.claim(ip,"setup",setup.trim().into(),"synthetic","synthetic password".into()).await.unwrap();
        let (_,key)=service.native_login(ip,"synthetic","synthetic password".into()).await.unwrap();
        let authority=service.validate_hostkey(&key).await.unwrap();
        let account=temp.path().join("owner/state/accounts").join(&identity.id);
        let before=std::fs::read(account.join("current-collection")).unwrap();
        let id=identity.id.clone();let generation=service.metadata.files(move |r|r.stage_collection(&id)).await.unwrap();
        schema::create_blank(&generation.path,123,456).unwrap();let staged=generation.path.clone();
        let upload=Upload {generation,reservations:vec![service.temp.reserve(4096).unwrap()]};
        let ingress=service.native_ingress.clone();let budget=service.temp.clone();let owner=Arc::downgrade(&service);
        let callback=publication_callback(service.clone(),identity.id,upload);
        let (arrived,reached)=tokio::sync::oneshot::channel();
        let (resume,release)=std::sync::mpsc::sync_channel(1);
        let (finished,done)=tokio::sync::oneshot::channel();
        // Invoke the production callback under the real blocking operation.
        // Only this worker thread gets a one-shot boundary pause and failure.
        let work=tokio::spawn(async move {
            service.collection_operation(authority,true,move |root| {
                struct Reset;
                impl Drop for Reset {fn drop(&mut self){PUBLICATION_BOUNDARY.with(|slot|{slot.borrow_mut().take();});}}
                let _reset=Reset;
                PUBLICATION_BOUNDARY.with(|slot|{assert!(slot.borrow().is_none());*slot.borrow_mut()=Some((arrived,release));});
                let result=crate::test_hooks::storage_failure("collection_after_manifest_sync",||callback(root));
                let _=finished.send(());
                result
            }).await
        });
        let deadline=std::time::Duration::from_secs(10);
        tokio::time::timeout(deadline,reached).await.unwrap().unwrap();
        assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),before);
        assert!(owner.upgrade().is_some());drop(ingress.enter().unwrap());
        let work=if cancel {
            work.abort();
            assert!(tokio::time::timeout(deadline,work).await.unwrap().unwrap_err().is_cancelled());
            // The caller is gone; the armed blocking callback alone retains
            // the Service. Cancellation must not drop/close it prematurely.
            assert!(owner.upgrade().is_some());drop(ingress.enter().unwrap());
            None
        } else {Some(work)};
        resume.send(()).unwrap();
        tokio::time::timeout(deadline,done).await.unwrap().unwrap();
        // A real pointer replacement occurred, followed by injected failure.
        // The callback's guard has dropped even without a waiting caller.
        assert_ne!(std::fs::read(account.join("current-collection")).unwrap(),before);
        assert!(matches!(ingress.enter(),Err(e) if e.code=="admission_closed"));
        assert!(staged.exists());assert!(account.join("generations/initial/collection.anki2").exists());
        assert_eq!(budget.used(),0);
        if let Some(work)=work {
            assert!(matches!(tokio::time::timeout(deadline,work).await.unwrap().unwrap(),Err(e) if e.code=="provision_unavailable"));
        }
        assert!(owner.upgrade().is_none());
    }
    #[cfg(feature="test-hooks")]
    #[tokio::test]
    async fn uncertain_activation_drops_callback_guard_and_closes_native_ingress() {
        uncertain_publication_lifetime(false).await;
    }
    #[cfg(feature="test-hooks")]
    #[tokio::test]
    async fn cancelled_caller_leaves_service_and_uncertainty_guard_with_blocking_callback() {
        uncertain_publication_lifetime(true).await;
    }
    #[tokio::test]
    async fn injected_disk_full_drops_candidate_before_releasing_reserved_bytes() {
        use std::os::unix::fs::OpenOptionsExt;
        let temp=tempfile::tempdir().unwrap();let mut root=crate::DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
        let setup=std::fs::read_to_string(root.setup_file()).unwrap();
        let service=Service::open(root,crate::Config::new("https://example.test","127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let identity=service.claim("127.0.0.1".parse().unwrap(),"setup",setup.trim().into(),"synthetic","synthetic password".into()).await.unwrap();
        let account=temp.path().join("owner/state/accounts").join(&identity.id);let before=std::fs::read(account.join("current-collection")).unwrap();
        let input=temp.path().join("input.anki2");std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&input).unwrap();schema::create_blank(&input,123,456).unwrap();
        let compressed=zstd::stream::encode_all(std::fs::read(input).unwrap().as_slice(),0).unwrap();
        let metadata=service.temp.reserve(4096).unwrap();let stage=service.metadata.files(move |r|r.stage_collection(&identity.id)).await.unwrap();let path=stage.path.clone();
        struct Reset;
        impl Drop for Reset {fn drop(&mut self){STAGE_DISK_FULL.with(|v|v.set(false));}}
        let _reset=Reset;STAGE_DISK_FULL.with(|v|v.set(true));
        let result=receive(Box::new(std::io::Cursor::new(compressed)),314_572_800,service.temp.clone(),stage,metadata);
        assert!(matches!(result,Err(ref e) if e.code=="storage_unavailable"));
        assert!(!path.exists());assert_eq!(service.temp.used(),0);
        assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),before);
        // Controlled ENOSPC at the actual staging writer; no physical disk or
        // power-loss claim, and no activation callback has run.
    }
}
pub(crate) struct Snapshot { pub file:File,_generation:Generation,_reservations:Vec<TempReservation>,_capacity:tokio::sync::OwnedSemaphorePermit }
struct Source {connection:rusqlite::Connection,_pin:crate::storage::collection::ReadPin}
pub(crate) async fn publish(service:Arc<Service>,authority:NativeAuthority,upload:Upload)->Result<()> {
    service.pause("collection_after_validation").await;
    let id=authority.account_id().to_owned();
    let callback=publication_callback(service.clone(),id,upload);
    service.collection_operation(authority,true,callback).await
}
fn publication_callback(service:Arc<Service>,id:String,upload:Upload)->impl FnOnce(&crate::storage::DataRoot)->Result<()>+Send+'static {
    // The guard lives inside the blocking publication callback, so cancellation
    // cannot lose a rename-uncertainty notification. No callback means no switch.
    struct Publication { service:Arc<Service>, may_have_switched:bool }
    impl Drop for Publication {
        fn drop(&mut self) { if self.may_have_switched { self.service.native_ingress.close(); } }
    }
    let publication=Publication {service,may_have_switched:false};
    move |root| {
        // Move the entire Drop-bearing value, not just its Copy bool field.
        let mut publication=publication;
        root.collection_path(&id)?;
        let Upload {generation,reservations}=upload;
        let result=generation.activate_notifying(|| {
            publication.may_have_switched=true;
            #[cfg(all(test,feature="test-hooks"))]
            if let Some((arrived,resume))=PUBLICATION_BOUNDARY.with(|slot|slot.borrow_mut().take()) {
                let _=arrived.send(());
                resume.recv_timeout(std::time::Duration::from_secs(10)).expect("publication fixture release");
            }
        });
        drop(reservations);
        result?;
        root.cleanup_collection_generations(&id)?;
        publication.may_have_switched=false;
        Ok(())
    }
}
pub(crate) async fn snapshot(service:Arc<Service>,authority:NativeAuthority,capacity:tokio::sync::OwnedSemaphorePermit)->Result<Snapshot> {
    let id=authority.account_id().to_owned();
    // Reserve metadata before creating a private destination. Snapshot pages
    // are reserved from the pinned committed view, not a remote length hint.
    let metadata_reservation=service.temp.reserve(4096)?;
    let staged_id=id.clone();
    let stage=service.metadata.files(move |r|r.stage_collection(&staged_id)).await?;
    let (connection,pin)=service.collection_operation(authority,true,move |root| {
        let path=root.collection_path(&id)?;
        let mut c=schema::open(&path,true)?;
        let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let usn:i32=tx.query_row("SELECT usn FROM col WHERE id=1",[],|r|r.get(0))?;
        let next=usn.checked_add(1).ok_or(Error::new("storage_unavailable"))?;
        let stamp=(time::OffsetDateTime::now_utc().unix_timestamp_nanos()/1_000_000) as i64;
        tx.execute("UPDATE col SET mod=?1,usn=?2 WHERE id=1",rusqlite::params![stamp,next])?;
        tx.commit()?;
        // Pin a committed read transaction before releasing the account barrier.
        c.execute_batch("BEGIN DEFERRED")?;
        c.query_row("SELECT usn FROM col WHERE id=1",[],|r|r.get::<_,i32>(0))?;
        let pin=root.pin_collection(&path)?;
        Ok((c,pin))
    }).await?;
    let source=Source {connection,_pin:pin};
    let pages:i64=source.connection.pragma_query_value(None,"page_count",|r|r.get(0))?;
    let page_size:i64=source.connection.pragma_query_value(None,"page_size",|r|r.get(0))?;
    let bytes=u64::try_from(pages).ok().and_then(|n|n.checked_mul(u64::try_from(page_size).ok()?)).ok_or(Error::new("storage_unavailable"))?;
    let reservation=service.temp.reserve(bytes.checked_mul(2).ok_or(Error::new("storage_unavailable"))?)?;
    let snapshot=Snapshot {file:stage.writer()?,_generation:stage,_reservations:vec![metadata_reservation,reservation],_capacity:capacity};
    service.pause("collection_snapshot_pinned").await;
    let owner=service.clone();
    tokio::task::spawn_blocking(move || {
        let _owner=owner;
        schema::snapshot(&source.connection,&snapshot._generation.path)?;
        source.connection.execute_batch("ROLLBACK")?;
        drop(source);
        Ok(snapshot)
    }).await.map_err(|_|Error::new("storage_unavailable"))?
}
