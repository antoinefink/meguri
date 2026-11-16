// SPDX-License-Identifier: MIT
use super::blobs::{Content, integrity};
use crate::{Result, Error};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const MIGRATION: &str = "
ALTER TABLE accounts ADD COLUMN media_usn INTEGER NOT NULL DEFAULT 0 CHECK(media_usn BETWEEN 0 AND 2147483647);
CREATE TABLE media_blobs (
 account_id TEXT NOT NULL REFERENCES accounts(id),
 blob TEXT NOT NULL,
 size INTEGER NOT NULL CHECK(size BETWEEN 1 AND 104857600),
 sha1 TEXT NOT NULL CHECK(length(sha1)=40),
 sha256 BLOB NOT NULL CHECK(length(sha256)=32),
 PRIMARY KEY(account_id,blob)
) WITHOUT ROWID;
CREATE TABLE media_names (
 account_id TEXT NOT NULL REFERENCES accounts(id),
 name TEXT COLLATE BINARY NOT NULL,
 usn INTEGER NOT NULL CHECK(usn BETWEEN 1 AND 2147483647),
 blob TEXT,
 PRIMARY KEY(account_id,name),
 UNIQUE(account_id,usn),
 FOREIGN KEY(account_id,blob) REFERENCES media_blobs(account_id,blob)
) WITHOUT ROWID;
PRAGMA user_version=2;
";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct State { pub usn:i32, pub content:Option<Content> }
pub(crate) fn cursor(c:&Connection,id:&str)->Result<i32> {
    Ok(c.query_row("SELECT media_usn FROM accounts WHERE id=?1",[id],|r|r.get(0))?)
}
pub(crate) fn state(c:&Connection,id:&str,name:&str)->Result<Option<State>> {
    Ok(c.query_row("SELECT n.usn,n.blob,b.size,b.sha1,b.sha256 FROM media_names n LEFT JOIN media_blobs b ON b.account_id=n.account_id AND b.blob=n.blob WHERE n.account_id=?1 AND n.name=?2",params![id,name],|r| {
        let blob:Option<String>=r.get(1)?;
        Ok(State {usn:r.get(0)?,content:match blob {Some(blob)=>{
            let size:i64=r.get(2)?;
            if !(1..=104_857_600).contains(&size) {return Err(rusqlite::Error::IntegralValueOutOfRange(2,size));}
            let size=u64::try_from(size).map_err(|_|rusqlite::Error::IntegralValueOutOfRange(2,size))?;
            Some(Content {blob,size,sha1:r.get(3)?,sha256:r.get(4)?})
        },None=>None}})
    }).optional()?)
}
pub(crate) fn changes(c:&Connection,id:&str,last:i32)->Result<Vec<(String,i32,String)>> {
    if last<0 { return Err(Error::new("invalid_request")); }
    let mut q=c.prepare("SELECT n.name,n.usn,coalesce(b.sha1,'') FROM media_names n LEFT JOIN media_blobs b ON b.account_id=n.account_id AND b.blob=n.blob WHERE n.account_id=?1 AND n.usn>?2 ORDER BY n.usn LIMIT 1000")?;
    Ok(q.query_map(params![id,last],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
}
pub(crate) fn count(c:&Connection,id:&str)->Result<u32> {
    let n:i64=c.query_row("SELECT count(*) FROM media_names WHERE account_id=?1 AND blob IS NOT NULL",[id],|r|r.get(0))?;
    n.try_into().map_err(|_|integrity())
}
pub(crate) fn apply(c:&Connection,id:&str,changes:&[(String,Option<Content>)],effective:&[bool])->Result<i32> {
    let mut usn=cursor(c,id)?;
    let increment:i32=effective.iter().filter(|v|**v).count().try_into().map_err(|_|integrity())?;
    usn.checked_add(increment).ok_or(Error::new("media_usn_exhausted"))?;
    for ((name,content),changed) in changes.iter().zip(effective) {
        if !changed { continue; }
        usn+=1;
        if let Some(value)=content {
            let size=i64::try_from(value.size).map_err(|_|integrity())?;
            if !(1..=104_857_600).contains(&size) {return Err(integrity());}
            c.execute("INSERT OR IGNORE INTO media_blobs(account_id,blob,size,sha1,sha256) VALUES(?1,?2,?3,?4,?5)",params![id,value.blob,size,value.sha1,value.sha256])?;
        }
        c.execute("INSERT INTO media_names(account_id,name,usn,blob) VALUES(?1,?2,?3,?4) ON CONFLICT(account_id,name) DO UPDATE SET usn=excluded.usn,blob=excluded.blob",params![id,name,usn,content.as_ref().map(|v|&v.blob)])?;
    }
    c.execute("UPDATE accounts SET media_usn=?2 WHERE id=?1",params![id,usn])?;
    Ok(usn)
}
