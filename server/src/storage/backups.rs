// SPDX-License-Identifier: MIT
//! Account-local recovery points. Collection payloads and native media objects
//! are immutable; SQLite references publish a point only after file durability.
use super::{DataRoot, owned as at, root::unavailable};
use crate::{Result, crypto, media::{blobs::Content, index}};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{collections::{BTreeMap, BTreeSet}, fs::File, io::{Read, Seek, SeekFrom, Write}, path::PathBuf};
use sha2::{Digest, Sha256};

const MIGRATION: &str = "
CREATE TABLE backup_points (
 account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 id TEXT NOT NULL, created_at INTEGER NOT NULL CHECK(created_at>=0),
 kind TEXT NOT NULL CHECK(kind IN ('daily','weekly','monthly','before_restore')),
 payload TEXT NOT NULL, size INTEGER NOT NULL CHECK(size>0),
 sha256 BLOB NOT NULL CHECK(length(sha256)=32), epoch INTEGER NOT NULL CHECK(epoch>=0),
 PRIMARY KEY(account_id,id)
) WITHOUT ROWID;
CREATE TABLE backup_media (
 account_id TEXT NOT NULL, backup_id TEXT NOT NULL, name TEXT NOT NULL, blob TEXT NOT NULL,
 PRIMARY KEY(account_id,backup_id,name),
 FOREIGN KEY(account_id,backup_id) REFERENCES backup_points(account_id,id) ON DELETE CASCADE,
 FOREIGN KEY(account_id,blob) REFERENCES media_blobs(account_id,blob)
) WITHOUT ROWID;
CREATE TABLE backup_restores (
 account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id),
 backup_id TEXT NOT NULL, generation TEXT NOT NULL, epoch INTEGER NOT NULL CHECK(epoch>0),
 FOREIGN KEY(account_id,backup_id) REFERENCES backup_points(account_id,id)
) WITHOUT ROWID;
PRAGMA user_version=6;
";
#[derive(Clone, Debug, Serialize)]
pub struct Backup {
    pub id: String,
    pub created_at: i64,
    pub kind: String,
    pub size_bytes: u64,
}
pub(crate) struct Point {
    pub summary: Backup,
    pub payload: String,
    pub bytes: u64,
    pub digest: Vec<u8>,
}
#[derive(Clone, Debug)]
pub(crate) struct Intent {
    pub account: String,
    pub backup: String,
    pub generation: String,
    pub epoch: i64,
}
fn supported(c: &Connection) -> Result<bool> {
    Ok(c.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0))? >= 6)
}
pub(crate) fn pending(c: &Connection, id: &str) -> Result<bool> {
    Ok(supported(c)? && c.prepare("SELECT 1 FROM backup_restores WHERE account_id=?1")?.exists([id])?)
}
pub(crate) fn intents(c: &Connection) -> Result<Vec<Intent>> {
    if !supported(c)? { return Ok(Vec::new()); }
    let mut q=c.prepare("SELECT account_id,backup_id,generation,epoch FROM backup_restores ORDER BY account_id")?;
    let jobs=q.query_map([],|r|Ok(Intent {account:r.get(0)?,backup:r.get(1)?,generation:r.get(2)?,epoch:r.get(3)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for job in &jobs {
        for token in [&job.account,&job.backup,&job.generation] { crypto::bearer(token).map_err(|_|unavailable())?; }
        if job.epoch<=0 || super::reset::epoch(c,&job.account)?!=job.epoch
            || c.prepare("SELECT 1 FROM account_resets WHERE id=?1")?.exists([&job.account])?
            || !c.prepare("SELECT 1 FROM accounts WHERE id=?1 AND provisioning_state='ready'")?.exists([&job.account])? { return Err(unavailable()); }
    }
    Ok(jobs)
}
pub(crate) fn validate(root: &DataRoot,c: &Connection) -> Result<()> {
    if !supported(c)? {return Ok(());}
    intents(c)?;
    let mut q=c.prepare("SELECT account_id,id,created_at,kind,payload,size,sha256,epoch FROM backup_points")?;
    let mut rows=q.query([])?;
    while let Some(r)=rows.next()? {
        let account:String=r.get(0)?;let id:String=r.get(1)?;let at:i64=r.get(2)?;
        let kind:String=r.get(3)?;let payload:String=r.get(4)?;let bytes:i64=r.get(5)?;
        let digest:Vec<u8>=r.get(6)?;let epoch:i64=r.get(7)?;
        if crypto::bearer(&account).is_err() || crypto::bearer(&id).is_err() || at<0
            || !matches!(kind.as_str(),"daily"|"weekly"|"monthly"|"before_restore")
            || !payload_name(&payload) || bytes<=0 || digest.len()!=32
            || payload!=digest.iter().map(|v|format!("{v:02x}")).collect::<String>()
            || epoch<0 || epoch>super::reset::epoch(c,&account)? {
            return Err(unavailable());
        }
        if root.backup_file(&account,&payload)?.metadata()?.len()!=u64::try_from(bytes).map_err(|_|unavailable())? {return Err(unavailable());}
    }
    let mut q=c.prepare("SELECT name FROM backup_media")?;
    for name in q.query_map([],|r|r.get::<_,String>(0))? {crate::media::name(&name?).map_err(|_|unavailable())?;}
    Ok(())
}
/// SQLite stores sizes as signed integers; Rust exposes only checked,
/// positive byte counts. Keep the same decoder convention as the media index.
fn positive_size(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64=row.get(column)?;
    let bytes=u64::try_from(value).map_err(|_|rusqlite::Error::IntegralValueOutOfRange(column,value))?;
    if bytes==0 {return Err(rusqlite::Error::IntegralValueOutOfRange(column,value));}
    Ok(bytes)
}
fn sqlite_size(bytes: u64) -> Result<i64> {
    i64::try_from(bytes).ok().filter(|n|*n>0).ok_or_else(unavailable)
}
pub(crate) fn collection_bytes(pages: i64,page_size: i64) -> Result<u64> {
    let pages=u64::try_from(pages).map_err(|_|unavailable())?;
    let page_size=u64::try_from(page_size).map_err(|_|unavailable())?;
    let bytes=pages.checked_mul(page_size).ok_or_else(unavailable)?;
    sqlite_size(bytes)?;
    Ok(bytes)
}
pub(crate) fn media(c: &Connection,id: &str,backup: Option<&str>) -> Result<BTreeMap<String,Content>> {
    let sql=if backup.is_some() {
        "SELECT n.name,b.blob,b.size,b.sha1,b.sha256 FROM backup_media n JOIN media_blobs b ON b.account_id=n.account_id AND b.blob=n.blob WHERE n.account_id=?1 AND n.backup_id=?2 ORDER BY n.name"
    } else {
        "SELECT n.name,b.blob,b.size,b.sha1,b.sha256 FROM media_names n JOIN media_blobs b ON b.account_id=n.account_id AND b.blob=n.blob WHERE n.account_id=?1 AND ?2 IS NULL ORDER BY n.name"
    };
    let mut q=c.prepare(sql)?;
    let values=q.query_map(params![id,backup],|r| {
        let size:i64=r.get(2)?;
        if !(1..=104_857_600).contains(&size) {return Err(rusqlite::Error::IntegralValueOutOfRange(2,size));}
        let size=u64::try_from(size).map_err(|_|rusqlite::Error::IntegralValueOutOfRange(2,size))?;
        Ok((r.get::<_,String>(0)?,Content {blob:r.get(1)?,size,sha1:r.get(3)?,sha256:r.get(4)?}))
    })?.collect::<rusqlite::Result<BTreeMap<_,_>>>()?;
    Ok(values)
}
pub(crate) fn point(c: &Connection,id: &str,backup: &str) -> Result<Point> {
    let p=c.query_row("SELECT id,created_at,kind,payload,size,sha256 FROM backup_points WHERE account_id=?1 AND id=?2",params![id,backup],|r| {
        let bytes=positive_size(r,4)?;
        Ok(Point {summary:Backup {id:r.get(0)?,created_at:r.get(1)?,kind:r.get(2)?,size_bytes:bytes},payload:r.get(3)?,bytes,digest:r.get(5)?})
    }).optional()?.ok_or(crate::Error::new("route_not_found"))?;
    if !payload_name(&p.payload) || p.digest.len()!=32 {return Err(unavailable());}
    Ok(p)
}
pub(crate) fn list(c: &Connection,id: &str) -> Result<Vec<Backup>> {
    let mut q=c.prepare("SELECT id,created_at,kind,size+(SELECT coalesce(sum(size),0) FROM media_blobs b WHERE b.account_id=p.account_id AND b.blob IN (SELECT blob FROM backup_media WHERE account_id=p.account_id AND backup_id=p.id)) FROM backup_points p WHERE account_id=?1 ORDER BY created_at DESC,id DESC")?;
    Ok(q.query_map([id],|r|Ok(Backup {id:r.get(0)?,created_at:r.get(1)?,kind:r.get(2)?,size_bytes:positive_size(r,3)?}))?.collect::<rusqlite::Result<Vec<_>>>()?)
}
pub(crate) fn insert(c: &Connection,id: &str,point: &Point,media: &BTreeMap<String,Content>,epoch: i64) -> Result<()> {
    let p=&point.summary;
    let bytes=sqlite_size(point.bytes)?;
    c.execute("INSERT INTO backup_points VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![id,p.id,p.created_at,p.kind,point.payload,bytes,point.digest,epoch])?;
    let mut q=c.prepare("INSERT INTO backup_media VALUES(?1,?2,?3,?4)")?;
    for (name,content) in media {q.execute(params![id,p.id,name,content.blob])?;}
    Ok(())
}
pub(crate) fn clear(c: &Connection,id: &str) -> Result<()> {
    if supported(c)? {
        if pending(c,id)? {return Err(unavailable());}
        c.execute("DELETE FROM backup_points WHERE account_id=?1",[id])?;
    }
    Ok(())
}
/// UTC day, Monday-based UTC week and calendar month. Keep the newest point
/// in each bucket. Safety points have a separate, finite three-point allowance.
pub(crate) fn prune(c: &Connection,id: &str,now: i64,protected: &[&str]) -> Result<()> {
    let today=now.div_euclid(86400);
    let month=|stamp| -> Result<i32> {
        let d=time::OffsetDateTime::from_unix_timestamp(stamp).map_err(|_|unavailable())?;
        Ok(d.year()*12+i32::from(u8::from(d.month())))
    };
    let current_month=month(now)?;
    // Retention needs only metadata, never the public list's media-size sums.
    struct Retained {id: String,created_at: i64,kind: String}
    let mut q=c.prepare("SELECT id,created_at,kind FROM backup_points WHERE account_id=?1 ORDER BY created_at DESC,id DESC")?;
    let points=q.query_map([id],|r|Ok(Retained {id:r.get(0)?,created_at:r.get(1)?,kind:r.get(2)?}))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let newest=points.iter().find(|p|p.kind!="before_restore").map(|p|p.id.clone());
    // A selected point and captures ahead of a backwards-moving sweep clock
    // stay intact. Reserve their monthly slots before considering other points.
    let preserve=|p: &Retained| protected.contains(&p.id.as_str())
        || (p.kind!="before_restore" && p.created_at>now);
    let reserved=points.iter().filter(|p|p.kind=="before_restore" && preserve(p)).count();
    let mut buckets=BTreeSet::new(); let mut safety=reserved; let mut monthly=0;
    for p in points.iter().filter(|p|preserve(p) && p.kind=="monthly") {
        buckets.insert(("monthly",i64::from(month(p.created_at)?)));
        monthly+=1;
    }
    for p in points {
        if preserve(&p) {continue;}
        let day=p.created_at.div_euclid(86400);
        let age=today-day;
        let week=(today+3).div_euclid(7)-(day+3).div_euclid(7);
        let months=current_month-month(p.created_at)?;
        let tier=if p.kind=="before_restore" {
            safety+=1;
            if safety<=3 {Some(("before_restore",p.created_at))} else {None}
        } else if (0..7).contains(&age) {Some(("daily",day))}
        else if (1..=4).contains(&week) {Some(("weekly",(day+3).div_euclid(7)))}
        else if (1..=12).contains(&months) {Some(("monthly",i64::from(month(p.created_at)?)))}
        else {None};
        let keep=match tier {Some(("before_restore",_))=>true,Some(key)=>{
            let fresh=buckets.insert(key);
            if fresh && key.0=="monthly" {monthly+=1;}
            fresh && (key.0!="monthly" || monthly<=12)
        },None=>false};
        if keep {
            let kind=tier.unwrap().0;
            if p.kind!=kind {
                c.execute("UPDATE backup_points SET kind=?3 WHERE account_id=?1 AND id=?2",params![id,p.id,kind])?;
            }
        } else if newest.as_deref()!=Some(p.id.as_str()) { c.execute("DELETE FROM backup_points WHERE account_id=?1 AND id=?2",params![id,p.id])?; }
    }
    Ok(())
}
pub(crate) fn payload_name(name: &str) -> bool {
    name.len()==64 && name.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn digest(file: &mut File) -> Result<(u64,Vec<u8>)> {
    file.seek(SeekFrom::Start(0))?;
    let mut hasher=Sha256::new(); let mut bytes=0u64; let mut buffer=[0;65536];
    loop {let n=file.read(&mut buffer)?;if n==0 {break;}bytes=bytes.checked_add(n as u64).ok_or_else(unavailable)?;hasher.update(&buffer[..n]);}
    file.seek(SeekFrom::Start(0))?;
    Ok((bytes,hasher.finalize().to_vec()))
}
pub(crate) struct Staged {
    pub path: PathBuf,
    directory: File,
    name: String,
}
impl Drop for Staged {
    fn drop(&mut self) {
        for suffix in ["-wal","-shm","-journal",""] {
            let name=format!("{}{suffix}",self.name);
            if at::open(&self.directory,&name,false,false).is_ok() {let _=at::unlink(&self.directory,&name);}
        }
    }
}
impl Staged {
    pub fn file(&self) -> Result<File> {at::open(&self.directory,&self.name,false,false)}
    pub fn publish(&self,created_at: i64,kind: &str) -> Result<Point> {
        let mut file=self.file()?;
        let (bytes,digest)=digest(&mut file)?;
        sqlite_size(bytes)?;
        let payload=digest.iter().map(|v|format!("{v:02x}")).collect::<String>();
        if self.path.parent().ok_or_else(unavailable)?.join(&payload).try_exists()? {
            // Compare bytes as well as hashes before reusing a payload.
            let mut other=at::open(&self.directory,&payload,false,false)?;
            let mut left=[0;65536];let mut right=[0;65536];
            loop {let n=file.read(&mut left)?;other.read_exact(&mut right[..n])?;if left[..n]!=right[..n] {return Err(unavailable());}if n==0 {break;}}
            if other.read(&mut right[..1])?!=0 {return Err(unavailable());}
        } else {
            file.sync_all()?;
            at::publish(&self.directory,&self.name,&payload)?;
        }
        Ok(Point {summary:Backup {id:crypto::random_token()?,created_at,kind:kind.into(),size_bytes:bytes},payload,bytes,digest})
    }
}
impl DataRoot {
    pub(crate) fn backup_directory(&self,id: &str) -> Result<File> {
        self.validate()?;
        self.stores(id)?;
        let relative=PathBuf::from("state/accounts").join(id);
        let account=self.bound_dir(&relative,false)?;
        at::dir(&account,"backups",true)?;
        self.bound_dir(&relative.join("backups"),false)
    }
    pub(crate) fn backup_file(&self,id: &str,payload: &str) -> Result<File> {
        self.validate()?;
        self.stores(id)?;
        if !payload_name(payload) {return Err(unavailable());}
        at::open(&self.bound_dir(&PathBuf::from("state/accounts").join(id).join("backups"),false)?,payload,false,false)
    }
    pub(crate) fn stage_backup(&self,id: &str) -> Result<Staged> {
        let directory=self.backup_directory(id)?;
        let name=format!("stage-{}",crypto::random_token()?);
        at::open(&directory,&name,false,true)?;
        Ok(Staged {path:self.stores(id)?.directory.join("backups").join(&name),directory,name})
    }
    /// Caller owns exclusive startup access or both the account publication
    /// barrier and backup-work lease through the entire reclamation.
    pub(crate) fn cleanup_backups(&self,c: &Connection,id: &str) -> Result<()> {
        let directory=self.bound_dir(&PathBuf::from("state/accounts").join(id).join("backups"),false)?;
        let mut q=c.prepare("SELECT DISTINCT payload FROM backup_points WHERE account_id=?1")?;
        let keep=q.query_map([id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<BTreeSet<_>>>()?;
        for entry in std::fs::read_dir(self.stores(id)?.directory.join("backups"))? {
            let name=entry?.file_name();let Some(name)=name.to_str() else {continue;};
            let stage=name.strip_suffix("-wal").or_else(||name.strip_suffix("-shm")).or_else(||name.strip_suffix("-journal")).unwrap_or(name);
            if (payload_name(name) && !keep.contains(name)) || crate::media::blobs::generated(stage,"stage-") {
                at::open(&directory,name,false,false)?;at::unlink(&directory,name)?;
            }
        }
        Ok(())
    }
    pub(crate) fn upgrade_backups(&self) -> Result<()> {
        self.check_initialized()?;
        let mut c=super::open(self.database(),false)?;
        let version:i64=c.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if version==6 {return Ok(());}
        if version!=5 {return Err(unavailable());}
        let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(MIGRATION)?;
        crate::accounts::phase("backup_migration_before_commit")?;
        tx.commit()?;
        self.check_initialized()
    }
    /// The intent pins an already durable owned generation. Replaying the same
    /// pointer is harmless; the media diff and journal clear commit together.
    pub(crate) fn finish_backup_restore(&self,job: &Intent) -> Result<()> {
        let relative=PathBuf::from("state/accounts").join(&job.account);
        let account=self.bound_dir(&relative,false)?;
        let generation=self.bound_dir(&relative.join("generations").join(&job.generation),false)?;
        let file=at::open(&generation,"collection.anki2",false,false)?;
        let expected=format!("meguri-collection-stage-v1\naccount={}\nfile={:?}\n",job.account,at::identity(&file)?);
        if at::read(&generation,"owner")?!=expected.as_bytes() {return Err(unavailable());}
        let bytes=format!("meguri-collection-v1\ngeneration={}\n",job.generation);
        // The durable intent owns one scratch name for this generation. A
        // crash can leave an empty/prefix write; only rebuild that owned file.
        // Rename consumes it before the intent clears, bounding orphan growth.
        let pointer=format!("backup-restore-pointer-{}",job.generation);
        let exists=self.stores(&job.account)?.directory.join(&pointer).try_exists()?;
        let mut stage=at::open(&account,&pointer,false,!exists)?;
        let mut previous=Vec::new();
        (&mut stage).take(4097).read_to_end(&mut previous)?;
        if !bytes.as_bytes().starts_with(&previous) {return Err(unavailable());}
        stage.seek(SeekFrom::Start(0))?;
        stage.write_all(bytes.as_bytes())?;
        stage.sync_all()?;
        account.sync_all()?;
        at::replace(&account,&pointer,"current-collection")?;
        crate::accounts::phase("backup_restore_after_pointer")?;
        let mut c=super::open(self.database(),false)?;
        let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::accounts::admission(&tx)?;
        let desired=media(&tx,&job.account,Some(&job.backup))?;
        let current=media(&tx,&job.account,None)?;
        let mut changes=Vec::new();
        for name in current.keys().chain(desired.keys()).collect::<BTreeSet<_>>() {
            let before=current.get(name);let after=desired.get(name);
            if before!=after {changes.push((name.clone(),after.cloned()));}
        }
        index::apply(&tx,&job.account,&changes,&vec![true;changes.len()])?;
        tx.execute("DELETE FROM backup_restores WHERE account_id=?1 AND epoch=?2",params![job.account,job.epoch])?;
        crate::accounts::phase("backup_restore_before_complete")?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let c=Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE accounts(id TEXT PRIMARY KEY);
            INSERT INTO accounts VALUES('account');
            CREATE TABLE media_blobs(account_id TEXT,blob TEXT,size INTEGER,sha1 TEXT,sha256 BLOB,PRIMARY KEY(account_id,blob));
            CREATE TABLE media_names(account_id TEXT,name TEXT,blob TEXT);").unwrap();
        c.execute_batch(MIGRATION).unwrap();
        c
    }
    fn add(c: &Connection,id: &str,at: i64,kind: &str,bytes: u64) -> Result<()> {
        insert(c,"account",&Point {
            summary:Backup {id:id.into(),created_at:at,kind:kind.into(),size_bytes:bytes},
            payload:"00".repeat(32),bytes,digest:vec![0;32],
        },&BTreeMap::new(),0)
    }
    fn stamp(year: i32,month: u8,day: u8) -> i64 {
        time::Date::from_calendar_date(year,time::Month::try_from(month).unwrap(),day).unwrap()
            .midnight().assume_utc().unix_timestamp()
    }
    #[test]
    fn sizes_use_checked_positive_sqlite_integers_without_an_upload_cap() {
        let c=database();
        for bytes in [314_572_801,4_294_967_296,i64::MAX as u64] {
            add(&c,"point",0,"daily",bytes).unwrap();
            assert_eq!(point(&c,"account","point").unwrap().bytes,bytes);
            assert_eq!(list(&c,"account").unwrap()[0].size_bytes,bytes);
            c.execute("DELETE FROM backup_points",[]).unwrap();
        }
        for bytes in [0,(i64::MAX as u64)+1,u64::MAX] {assert!(add(&c,"bad",0,"daily",bytes).is_err());}
        c.pragma_update(None,"ignore_check_constraints",true).unwrap();
        add(&c,"point",0,"daily",1).unwrap();
        for bytes in [-1i64,0] {
            c.execute("UPDATE backup_points SET size=?1",[bytes]).unwrap();
            assert!(point(&c,"account","point").is_err());
            assert!(list(&c,"account").is_err());
        }
        assert_eq!(collection_bytes(81920,4096).unwrap(),335_544_320);
        assert_eq!(collection_bytes(i64::MAX,1).unwrap(),i64::MAX as u64);
        for (pages,size) in [(0,4096),(1,0),(-1,4096),(1,-4096),(-1,-4096),(i64::MAX,2),(i64::MAX,i64::MAX)] {
            assert!(collection_bytes(pages,size).is_err(),"{pages} * {size}");
        }
    }
    #[test]
    fn media_sizes_decode_as_signed_and_reject_invalid_values_on_both_manifests() {
        let c=database();
        add(&c,"point",0,"daily",1).unwrap();
        c.execute("INSERT INTO media_names VALUES('account','name','blob')",[]).unwrap();
        c.execute("INSERT INTO media_blobs VALUES('account','blob',7,'sha1',zeroblob(32))",[]).unwrap();
        c.execute("INSERT INTO backup_media VALUES('account','point','name','blob')",[]).unwrap();
        for selected in [None,Some("point")] {assert_eq!(media(&c,"account",selected).unwrap()["name"].size,7);}
        for size in [-1i64,0,104_857_601] {
            c.execute("UPDATE media_blobs SET size=?1",[size]).unwrap();
            for selected in [None,Some("point")] {assert!(media(&c,"account",selected).is_err());}
        }
    }
    #[test]
    fn monthly_limit_includes_a_protected_old_point_at_calendar_boundaries() {
        // Exercise leap February, year rollover and the protected oldest month.
        for now in [stamp(2028,1,31)+86399,stamp(2028,2,1),stamp(2028,2,29)+86399,stamp(2029,1,1)] {
            let c=database();
            let date=time::OffsetDateTime::from_unix_timestamp(now).unwrap();
            let month_number=date.year()*12+i32::from(u8::from(date.month()))-1;
            for age in 0..=12 {
                let m=month_number-age;
                let at=stamp(m.div_euclid(12),u8::try_from(m.rem_euclid(12)+1).unwrap(),1);
                add(&c,&format!("month-{age:02}"),at,"monthly",1).unwrap();
            }
            add(&c,"fresh",now+40*86400,"daily",1).unwrap();
            prune(&c,"account",now,&["month-12"]).unwrap();
            let points=list(&c,"account").unwrap();
            let monthly=points.iter().filter(|p|p.kind=="monthly").count();
            assert!(monthly<=12);
            if now==stamp(2028,1,31)+86399 {assert_eq!(monthly,12);assert!(!points.iter().any(|p|p.id=="month-00"));}
            assert!(points.iter().any(|p|p.id=="month-12"));
            assert!(points.iter().any(|p|p.id=="fresh" && p.kind=="daily"));
        }
    }
    #[test]
    fn sparse_months_exclude_current_month_and_unchanged_tiers_do_not_write() {
        let c=database();
        let now=stamp(2028,1,31)+86399;
        add(&c,"today",now,"daily",1).unwrap();
        add(&c,"current-month",stamp(2028,1,1),"monthly",1).unwrap();
        add(&c,"preceding-month",stamp(2027,12,1),"monthly",1).unwrap();
        add(&c,"twelfth-month",stamp(2027,1,1),"monthly",1).unwrap();
        add(&c,"expired",stamp(2026,12,31),"monthly",1).unwrap();
        prune(&c,"account",now,&[]).unwrap();
        let ids=list(&c,"account").unwrap().into_iter().map(|p|p.id).collect::<BTreeSet<_>>();
        assert_eq!(ids,BTreeSet::from(["today".into(),"preceding-month".into(),"twelfth-month".into()]));
        let changes=c.total_changes();
        prune(&c,"account",now,&[]).unwrap();
        assert_eq!(c.total_changes(),changes,"unchanged tiers produce no writes");
    }
    #[test]
    fn expired_history_keeps_the_last_ordinary_recovery_point() {
        let c=database();
        add(&c,"last-good",stamp(2025,1,1),"daily",1).unwrap();
        add(&c,"older",stamp(2024,12,1),"monthly",1).unwrap();
        prune(&c,"account",stamp(2028,1,31),&[]).unwrap();
        assert_eq!(list(&c,"account").unwrap().iter().map(|p|p.id.as_str()).collect::<Vec<_>>(),vec!["last-good"]);
        add(&c,"new-good",stamp(2028,1,31),"daily",1).unwrap();
        prune(&c,"account",stamp(2028,1,31),&[]).unwrap();
        assert_eq!(list(&c,"account").unwrap().iter().map(|p|p.id.as_str()).collect::<Vec<_>>(),vec!["new-good"]);
    }

}
