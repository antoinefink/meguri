// SPDX-License-Identifier: MIT
use crate::{Error, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Graves { pub cards:Vec<i64>, pub decks:Vec<i64>, pub notes:Vec<i64> }
pub(crate) fn outgoing(c:&Connection, min:i32) -> Result<Graves> {
    let mut g=Graves::default();
    let mut q=c.prepare("SELECT oid,type FROM graves WHERE usn>=?1 ORDER BY type,oid")?;
    for r in q.query_map([min],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))? {
        let (id,kind)=r?;
        match kind {0=>g.cards.push(id),1=>g.notes.push(id),2=>g.decks.push(id),_=>return Err(Error::new("invalid_collection"))}
    }
    Ok(g)
}
pub(crate) fn apply(c:&Connection, g:Graves, usn:i32) -> Result<()> {
    if g.cards.len()+g.notes.len()+g.decks.len()>250 { return Err(Error::new("invalid_request")); }
    for (table,kind,ids) in [("cards",0,g.cards),("notes",1,g.notes),("decks",2,g.decks)] {
        for id in ids {
            if kind==2 {
                // Approved graves.raw_deck_grave_only: relocate only did;
                // neither card stamp changes. Default-deck deletion remains
                // unobserved and is explicitly outside this policy.
                if id==1 {return Err(super::session::unsupported());}
                c.execute("UPDATE cards SET did=1 WHERE did=?1",[id])?;
            }
            c.execute(&format!("DELETE FROM {table} WHERE id=?1"),[id])?;
            c.execute("INSERT INTO graves(oid,type,usn) VALUES(?1,?2,?3) ON CONFLICT(oid,type) DO NOTHING",rusqlite::params![id,kind,usn])?;
        }
    }
    Ok(())
}
