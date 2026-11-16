// SPDX-License-Identifier: MIT
//! SQL adapters use only the audited public schema-11 records/codecs. No card
//! generation, scheduler defaults, or note-type service runs during a merge.
use crate::{Error, Result};
use super::{objects, session::{Round, unsupported}};
use meguri_model::{schema11, proto::BlobMessage};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

#[derive(Default)]
pub(crate) enum Optional { #[default] Missing, Present(Box<RawValue>) }
impl<'de> Deserialize<'de> for Optional {
    fn deserialize<D:Deserializer<'de>>(d:D)->std::result::Result<Self,D::Error> {Box::<RawValue>::deserialize(d).map(Self::Present)}
}
#[derive(Deserialize)]
pub(crate) struct Changes {
    pub models:Vec<Box<RawValue>>,
    pub decks:(Vec<Box<RawValue>>,Vec<Box<RawValue>>),
    pub tags:Vec<String>,
    #[serde(default)] pub conf:Optional,
    #[serde(default)] pub crt:Optional,
}
#[derive(Serialize)]
pub(crate) struct Outgoing {
    models:Vec<Box<RawValue>>,
    decks:(Vec<Box<RawValue>>,Vec<Box<RawValue>>),
    tags:Vec<String>,
    #[serde(skip_serializing_if="Option::is_none")] conf:Option<Box<RawValue>>,
    #[serde(skip_serializing_if="Option::is_none")] crt:Option<i64>,
}
fn invalid()->Error { Error::new("invalid_request") }
fn raw(text:String)->Result<Box<RawValue>> {RawValue::from_string(text).map_err(|_|Error::new("invalid_collection"))}
fn compact(text:&str)->Vec<u8> {
    // Validated JSON only: remove insignificant whitespace without changing
    // string contents or routing exact integer tokens through floating point.
    let mut out=Vec::with_capacity(text.len());let mut quoted=false;let mut escaped=false;
    for b in text.bytes() {
        if quoted {
            out.push(b);
            if escaped {escaped=false;} else if b==b'\\' {escaped=true;} else if b==b'"' {quoted=false;}
        } else if b==b'"' {quoted=true;out.push(b);}
        else if !matches!(b,b' '|b'\t'|b'\r'|b'\n') {out.push(b);}
    }
    out
}
pub(crate) fn outgoing(c:&Connection,min:i32,newer:bool)->Result<Outgoing> {
    let mut out=Outgoing {models:vec![],decks:(vec![],vec![]),tags:vec![],conf:None,crt:None};
    let mut q=c.prepare("SELECT id,name,mtime_secs,usn,config FROM notetypes WHERE usn>=?1 ORDER BY id")?;
    let mut rows=q.query([min])?;
    while let Some(r)=rows.next()? {out.models.push(raw(schema11::notetype_to_json(&objects::notetype(c,r)?))?);}
    let mut q=c.prepare("SELECT id,name,mtime_secs,usn,common,kind FROM decks WHERE usn>=?1 ORDER BY id")?;
    let mut rows=q.query([min])?;
    while let Some(r)=rows.next()? {out.decks.0.push(raw(schema11::deck_to_json(&objects::deck(r)?))?);}
    let mut q=c.prepare("SELECT id,name,mtime_secs,usn,config FROM deck_config WHERE usn>=?1 ORDER BY id")?;
    let mut rows=q.query([min])?;
    while let Some(r)=rows.next()? {out.decks.1.push(raw(schema11::deck_config_to_json(&objects::preset(r)?))?);}
    let mut q=c.prepare("SELECT tag FROM tags WHERE usn>=?1 ORDER BY tag")?;
    for r in q.query_map([min],|r|r.get(0))? {out.tags.push(r?);}
    if !newer {
        let mut text=String::from("{");
        let mut q=c.prepare("SELECT key,val FROM config ORDER BY key")?;
        let mut rows=q.query([])?;
        while let Some(r)=rows.next()? {
            let key:String=r.get(0)?;
            let bytes:Vec<u8>=r.get(1)?;
            let value=std::str::from_utf8(&bytes).ok().and_then(meguri_model::json_text::parse).ok_or(Error::new("invalid_collection"))?;
            if text.len()>1 {text.push(',');}
            meguri_model::json_text::push_string(&mut text,&key);text.push(':');text.push_str(value.get());
        }
        text.push('}');out.conf=Some(raw(text)?);
        out.crt=Some(c.query_row("SELECT crt FROM col WHERE id=1",[],|r|r.get(0))?);
        // Approved matrix configuration.config-replacement-outgoing: this is
        // literal zero, independent of the collection or round USN.
        drop(rows);drop(q);
        c.execute("UPDATE config SET usn=0 WHERE usn!=0",[])?;
    }
    Ok(out)
}
fn unique(c:&Connection,table:&str,id:i64,name:&mut String)->Result<bool> {
    let mut renamed=false;
    while c.query_row(&format!("SELECT 1 FROM {table} WHERE name=?1 COLLATE unicase AND id!=?2 LIMIT 1"),params![name.as_str(),id],|_|Ok(())).optional()?.is_some() {
        name.push('+');renamed=true;
    }
    Ok(renamed)
}
pub(crate) fn apply(round:&Round, incoming:Changes)->Result<()> {
    let c=&round.db;
    for text in incoming.models {
        let mut nt=schema11::notetype_from_json(text.get()).map_err(|_|invalid())?;
        let mut q=c.prepare("SELECT id,name,mtime_secs,usn,config FROM notetypes WHERE id=?1")?;
        let mut rows=q.query([nt.id.0])?;
        if let Some(row)=rows.next()? {
            let old=objects::notetype(c,row)?;
            if old.mtime.0>nt.mtime.0 {continue;}
            // Independent schema-preservation policy; no vendor timing claim.
            if nt.fields.len()!=old.fields.len() || nt.templates.len()!=old.templates.len() {return Err(unsupported());}
            objects::preservation::notetype(&old,&mut nt)?;
        }
        if unique(c,"notetypes",nt.id.0,&mut nt.name)? {
            nt.mtime.0=time::OffsetDateTime::now_utc().unix_timestamp();nt.usn.0=round.usn;
        }
        c.execute("INSERT INTO notetypes VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,mtime_secs=excluded.mtime_secs,usn=excluded.usn,config=excluded.config",params![nt.id.0,nt.name,nt.mtime.0,nt.usn.0,nt.config.to_bytes().as_ref()])?;
        c.execute("DELETE FROM fields WHERE ntid=?1",[nt.id.0])?;
        c.execute("DELETE FROM templates WHERE ntid=?1",[nt.id.0])?;
        for field in nt.fields {
            c.execute("INSERT INTO fields VALUES(?1,?2,?3,?4)",params![nt.id.0,field.ord,field.name,field.config.to_bytes().as_ref()])?;
        }
        for template in nt.templates {
            c.execute("INSERT INTO templates VALUES(?1,?2,?3,?4,?5,?6)",params![nt.id.0,template.ord,template.name,template.mtime.0,template.usn.0,template.config.to_bytes().as_ref()])?;
        }
    }
    for text in incoming.decks.0 {
        let mut deck=schema11::deck_from_json(text.get()).map_err(|_|invalid())?;
        let mut q=c.prepare("SELECT id,name,mtime_secs,usn,common,kind FROM decks WHERE id=?1")?;
        let mut rows=q.query([deck.id.0])?;
        if let Some(row)=rows.next()? {
            let old=objects::deck(row)?;
            if old.mtime.0>deck.mtime.0 {continue;}
            objects::preservation::deck(&old,&mut deck)?;
        }
        if unique(c,"decks",deck.id.0,&mut deck.name)? {
            deck.mtime.0=time::OffsetDateTime::now_utc().unix_timestamp();deck.usn.0=round.usn;
        }
        c.execute("INSERT INTO decks VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET name=excluded.name,mtime_secs=excluded.mtime_secs,usn=excluded.usn,common=excluded.common,kind=excluded.kind",params![deck.id.0,deck.name,deck.mtime.0,deck.usn.0,deck.common.to_bytes().as_ref(),deck.kind.to_bytes().as_ref()])?;
    }
    for text in incoming.decks.1 {
        let mut p=schema11::deck_config_from_json(text.get()).map_err(|_|invalid())?;
        let mut q=c.prepare("SELECT id,name,mtime_secs,usn,config FROM deck_config WHERE id=?1")?;
        let mut rows=q.query([p.id.0])?;
        if let Some(row)=rows.next()? {
            let old=objects::preset(row)?;
            if old.mtime.0>p.mtime.0 {continue;}
            objects::preservation::preset(&old,&mut p)?;
        }
        c.execute("INSERT INTO deck_config VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,mtime_secs=excluded.mtime_secs,usn=excluded.usn,config=excluded.config",params![p.id.0,p.name,p.mtime.0,p.usn.0,p.config.to_bytes().as_ref()])?;
    }
    for tag in incoming.tags {
        let spelling:Option<String>=c.query_row("SELECT tag FROM tags WHERE tag=?1 COLLATE unicase",[&tag],|r|r.get(0)).optional()?;
        if spelling.is_some() {continue;}
        c.execute("INSERT INTO tags(tag,usn,collapsed,config) VALUES(?1,?2,1,NULL) ON CONFLICT(tag) DO NOTHING",params![tag,round.usn])?;
    }
    match (incoming.conf,incoming.crt) {
        (Optional::Missing,Optional::Missing)=>(),
        (Optional::Present(conf),Optional::Present(crt)) if conf.get()=="null" && crt.get()=="null"=>(),
        (Optional::Present(conf),Optional::Present(crt)) if round.newer && conf.get()!="null" && crt.get()!="null"=> {
            let values:std::collections::BTreeMap<String,Box<RawValue>>=serde_json::from_str(conf.get()).map_err(|_|invalid())?;
            let crt:i64=serde_json::from_str(crt.get()).map_err(|_|invalid())?;
            let now=time::OffsetDateTime::now_utc().unix_timestamp();
            c.execute("DELETE FROM config",[])?;
            for (key,value) in values {
                if meguri_model::json_text::parse(value.get()).is_none() {return Err(invalid());}
                let value=compact(value.get());
                c.execute("INSERT INTO config(key,usn,mtime_secs,val) VALUES(?1,?2,?3,?4)",params![key,round.usn,now,value])?;
            }
            c.execute("UPDATE col SET crt=?1 WHERE id=1",[crt])?;
        },
        // Mixed forms remain unobserved; omission is never an empty set.
        _=>return Err(unsupported()),
    }
    Ok(())
}
