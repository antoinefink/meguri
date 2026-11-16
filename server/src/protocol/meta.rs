// SPDX-License-Identifier: MIT
use crate::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
pub struct Request { pub v:i32,pub cv:String }
#[derive(Serialize)]
pub struct Meta {
    #[serde(rename="mod")] pub modified:i64,
    pub scm:i64,pub usn:i32,pub ts:i64,pub msg:&'static str,pub cont:bool,
    pub empty:bool,pub media_usn:i32,
    #[serde(rename="hostNum")] pub host_num:i32,
}
pub fn read(c:&Connection)->Result<Meta> {
    let (modified,scm,usn):(i64,i64,i32)=c.query_row("SELECT mod,scm,usn FROM col WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let empty=!c.prepare("SELECT 1 FROM cards LIMIT 1")?.exists([])?;
    Ok(Meta {modified,scm,usn,ts:time::OffsetDateTime::now_utc().unix_timestamp(),msg:"",cont:true,empty,media_usn:0,host_num:0})
}
