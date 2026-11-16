// SPDX-License-Identifier: MIT
use crate::{Error, Result};
use super::{session::{Round, unsupported}, model_adapter::blob};
use meguri_model::{card_data::CardData, proto::notetype::NotetypeConfig, revlog::RevlogKind};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, value::RawValue};

fn invalid()->Error { Error::new("invalid_request") }
#[derive(Clone, Copy, Serialize)]
pub(crate) struct Loose(pub i64);
impl<'de> Deserialize<'de> for Loose {
    fn deserialize<D:Deserializer<'de>>(d:D)->std::result::Result<Self,D::Error> {
        let raw=Box::<RawValue>::deserialize(d)?;
        let text=raw.get();
        if !text.starts_with(|c:char|c=='-' || c.is_ascii_digit()) {return Err(serde::de::Error::custom("number required"));}
        let value=if text.contains(['.','e','E']) {
            text.parse::<f64>().map_err(serde::de::Error::custom)? as i64
        } else {
            match text.parse::<i64>() {Ok(v)=>v,Err(_)=>if text.starts_with('-') {i64::MIN} else {i64::MAX}}
        };
        Ok(Self(value))
    }
}
pub(crate) type Note=(i64,String,i64,i64,i32,String,String,String,String,u32,String);
#[derive(Deserialize, Serialize)]
pub(crate) struct Card(pub i64,pub i64,pub i64,pub u16,pub Loose,pub i32,pub u8,pub i8,pub Loose,pub Loose,pub u16,pub u32,pub u32,pub u32,pub Loose,pub i64,pub u8,pub String);

// Review's final element has the W10.1 missing/invalid default. Deserialize
// positions directly so strict IDs/stamps never pass through a floating value.
#[derive(Deserialize, Serialize)]
pub(crate) struct Review(pub i64,pub i64,pub i32,pub u8,pub Loose,pub Loose,pub Loose,pub Loose,#[serde(default)] pub Value);
#[derive(Default,Deserialize)]
pub(crate) struct Chunk {
    #[serde(default)] pub done:bool,
    #[serde(default)] pub revlog:Vec<Review>,
    #[serde(default)] pub cards:Vec<Card>,
    #[serde(default)] pub notes:Vec<Note>,
}
pub(crate) fn tags(text:&str)->String {
    let parts:Vec<_>=text.split([' ','\u{3000}']).filter(|s|!s.is_empty()).collect();
    if parts.is_empty() {String::new()} else {format!(" {} ",parts.join(" "))}
}
fn card_data(text:&str)->Result<String> {
    // The public parser/writer owns leniency and rounding. The normal server
    // channel then drops unknown top-level keys (approved card_samples[4]);
    // custom scheduling data is a known string and remains opaque.
    let normalized=CardData::parse(text).to_text();
    let values:std::collections::BTreeMap<String,Box<RawValue>>=serde_json::from_str(&normalized).map_err(|_|invalid())?;
    let mut out=String::from("{");
    for key in ["pos","s","d","dr","decay","lrt","cd"] {
        if let Some(value)=values.get(key) {
            if out.len()>1 {out.push(',');}
            meguri_model::json_text::push_string(&mut out,key);out.push(':');out.push_str(value.get());
        }
    }
    out.push('}');Ok(out)
}
pub(crate) fn apply(round:&Round, chunk:Chunk)->Result<()> {
    if chunk.revlog.len()+chunk.cards.len()+chunk.notes.len()>250 {return Err(invalid());}
    let c=&round.db;
    for r in chunk.revlog {
        let kind=r.8.as_i64().filter(|n|(0..=5).contains(n)).unwrap_or(0);
        c.execute("INSERT INTO revlog VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO NOTHING",
            params![r.0,r.1,r.2,r.3,r.4.0 as i32,r.5.0 as i32,r.6.0.max(0) as u32,r.7.0.max(0) as u32,kind])?;
    }
    for r in chunk.cards {
        if r.6>3 || !(-3..=4).contains(&r.7) {return Err(invalid());}
        let local:Option<(i64,i32)>=c.query_row("SELECT mod,usn FROM cards WHERE id=?1",[r.0],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if local.is_some_and(|(mtime,usn)|usn>=round.min_usn && mtime>=r.4.0) {continue;}
        c.execute("INSERT INTO cards VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) ON CONFLICT(id) DO UPDATE SET nid=excluded.nid,did=excluded.did,ord=excluded.ord,mod=excluded.mod,usn=excluded.usn,type=excluded.type,queue=excluded.queue,due=excluded.due,ivl=excluded.ivl,factor=excluded.factor,reps=excluded.reps,lapses=excluded.lapses,left=excluded.left,odue=excluded.odue,odid=excluded.odid,flags=excluded.flags,data=excluded.data",
            params![r.0,r.1,r.2,r.3,r.4.0,r.5,r.6,r.7,r.8.0 as i32,r.9.0.max(0) as u32,r.10,r.11,r.12,r.13,r.14.0 as i32,r.15,r.16,card_data(&r.17)?])?;
    }
    for r in chunk.notes {
        let local:Option<(i64,i32)>=c.query_row("SELECT mod,usn FROM notes WHERE id=?1",[r.0],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if local.is_some_and(|(mtime,usn)|usn>=round.min_usn && mtime>=r.3) {continue;}
        let nt:Option<Vec<u8>>=c.query_row("SELECT config FROM notetypes WHERE id=?1",[r.2],|r|r.get(0)).optional()?;
        let config=blob::<NotetypeConfig>(&nt.ok_or(invalid())?)?;
        let count:i64=c.query_row("SELECT count(*) FROM fields WHERE ntid=?1",[r.2],|r|r.get(0))?;
        if count<1 || usize::try_from(count).ok()!=Some(super::notes::fields(&r.6).count()) {return Err(invalid());}
        let derived=super::notes::derive_values(&r.6,config.sort_field_idx as usize);
        c.execute("INSERT INTO notes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,'') ON CONFLICT(id) DO UPDATE SET guid=excluded.guid,mid=excluded.mid,mod=excluded.mod,usn=excluded.usn,tags=excluded.tags,flds=excluded.flds,sfld=excluded.sfld,csum=excluded.csum,flags=0,data=''",
            params![r.0,r.1,r.2,r.3,r.4,tags(&r.5),derived.fields,derived.sort_field,derived.checksum])?;
    }
    Ok(())
}

pub(crate) fn outgoing(round:&mut Round)->Result<Value> {
    let mut arrays=[Vec::new(),Vec::new(),Vec::new()];
    let mut count=0;
    while count<250 && round.exhausted.iter().any(|b|!*b) {
        let n=round.turn;round.turn=(n+1)%3;
        if round.exhausted[n] {continue;}
        let sql=match n {
            0=>"SELECT id,cid,usn,ease,CAST(ivl AS INTEGER),CAST(lastIvl AS INTEGER),factor,time,type FROM outgoing_revlog WHERE (?1 IS NULL OR id>?1) ORDER BY id LIMIT 1",
            1=>"SELECT * FROM outgoing_notes WHERE (?1 IS NULL OR id>?1) ORDER BY id LIMIT 1",
            _=>"SELECT * FROM outgoing_cards WHERE (?1 IS NULL OR id>?1) ORDER BY id LIMIT 1",
        };
        let mut q=round.db.prepare(sql)?;
        let mut rows=q.query([round.cursor[n]])?;
        let Some(r)=rows.next()? else {round.exhausted[n]=true;continue;};
        let id:i64=r.get(0)?;
        let row=match n {
            0=> {
                let kind:i64=r.get(8)?;
                let kind=RevlogKind::from_stored(kind).stored();
                let time=r.get::<_,u32>(7).unwrap_or(0);
                let review=Review(
                    id,r.get(1)?,r.get(2)?,r.get(3)?,
                    Loose(i64::from(r.get::<_,i32>(4)?)),
                    Loose(i64::from(r.get::<_,i32>(5)?)),
                    Loose(i64::from(r.get::<_,u32>(6)?)),
                    Loose(i64::from(time)),kind.into(),
                );
                serde_json::json!(review)
            },
            1=> {
                // W9's derived columns are wire placeholders, not stored values.
                let note:Note=(
                    id,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,
                    tags(&r.get::<_,String>(5)?),r.get(6)?,
                    String::new(),String::new(),0,String::new(),
                );
                serde_json::json!(note)
            },
            _=> {
                let data=CardData::parse(&r.get::<_,String>(17).unwrap_or_default());
                if data.unknown_keys().next().is_some() {return Err(unsupported());}
                let kind=r.get::<_,i64>(6)? as u8;
                let queue=r.get::<_,i64>(7)? as i8;
                if kind>3 || !(-3..=4).contains(&queue) {return Err(Error::new("invalid_collection"));}
                // Reuse W8's typed tuple; narrow SQLite reads before wrapping
                // loose wire integers so their stored range checks still apply.
                let card=Card(
                    id,r.get(1)?,r.get(2)?,r.get(3)?,Loose(r.get(4)?),r.get(5)?,kind,queue,
                    Loose(i64::from(r.get::<_,i32>(8).unwrap_or(0))),
                    Loose(i64::from(r.get::<_,u32>(9)?)),
                    r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?,
                    Loose(i64::from(r.get::<_,i32>(14).unwrap_or(0))),
                    r.get(15)?,r.get(16)?,data.to_text(),
                );
                serde_json::json!(card)
            },
        };
        arrays[n].push(row);round.cursor[n]=Some(id);count+=1;
    }
    // Exactly 250 requires a later empty terminal response (D7).
    Ok(serde_json::json!({"done":count<250,"revlog":arrays[0],"notes":arrays[1],"cards":arrays[2]}))
}
