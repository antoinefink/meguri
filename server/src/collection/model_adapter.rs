// SPDX-License-Identifier: MIT
//! Read validation only. Full transfers never round-trip rows through records.
use crate::{Result, collection::schema::invalid};
use rusqlite::{Connection, types::ValueRef};
use meguri_model::proto::{BlobMessage, ProtoBlob};

pub fn blob<M: BlobMessage>(bytes: &[u8]) -> Result<ProtoBlob<M>> {
    ProtoBlob::decode(bytes).map_err(|_| invalid())
}

#[derive(Clone, Copy)]
enum Scalar { Integer(i64,i64), Text, Any, Sort, NullableBlob }
const I: Scalar = Scalar::Integer(i64::MIN, i64::MAX);
const U: Scalar = Scalar::Integer(i32::MIN as i64, i32::MAX as i64);
const UINT32: Scalar = Scalar::Integer(0, u32::MAX as i64);
const UINT16: Scalar = Scalar::Integer(0, u16::MAX as i64);
const UINT8: Scalar = Scalar::Integer(0, u8::MAX as i64);
const T: Scalar = Scalar::Text;
const A: Scalar = Scalar::Any;

fn scalars(c: &Connection, table: &str, columns: &str, types: &[Scalar]) -> Result<()> {
    let mut q = c.prepare(&format!("SELECT {columns} FROM {table}"))?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        for (n, expected) in types.iter().enumerate() {
            let value = row.get_ref(n)?;
            let valid = match (expected, value) {
                (Scalar::Any, _) => true,
                (Scalar::Integer(min,max), ValueRef::Integer(v)) => (*min..=*max).contains(&v),
                (Scalar::Text, ValueRef::Text(v)) => std::str::from_utf8(v).is_ok(),
                (Scalar::Sort, ValueRef::Text(v)) => std::str::from_utf8(v).is_ok(),
                (Scalar::Sort, ValueRef::Integer(_) | ValueRef::Real(_)) => true,
                (Scalar::NullableBlob, ValueRef::Null | ValueRef::Blob(_)) => true,
                _ => false,
            };
            if !valid { return Err(invalid()); }
        }
    }
    Ok(())
}

pub(crate) fn validate(c: &Connection) -> Result<()> {
    scalars(c,"col","id,crt,mod,scm,ver,dty,usn,ls,conf,models,decks,dconf,tags", &[I,I,I,I,I,I,U,I,T,T,T,T,T])?;
    scalars(c,"notes","id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data", &[I,T,I,I,U,T,T,Scalar::Sort,I,I,T])?;
    scalars(c,"cards","id,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data", &[I,I,I,UINT16,I,U,I,I,A,UINT32,UINT16,UINT32,UINT32,UINT32,A,I,UINT8,A])?;
    let mut q = c.prepare("SELECT type,queue FROM cards")?;
    for row in q.query_map([], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))? {
        let (kind,queue) = row?;
        if !matches!(kind as u8,0..=3) || !matches!(queue as i8,-3..=4) { return Err(invalid()); }
    }
    scalars(c,"revlog","id,cid,usn,ease,ivl,lastIvl,factor,time,type", &[I,I,U,UINT8,A,A,UINT32,A,I])?;
    scalars(c,"graves","oid,type,usn", &[I,I,U])?;
    scalars(c,"tags","tag,usn,collapsed,config", &[T,U,Scalar::Integer(0,1),Scalar::NullableBlob])?;
    // Config values intentionally retain even unreadable storage classes/JSON:
    // typed client reads default them, and full transfer must preserve bytes.
    scalars(c,"config","KEY,usn,mtime_secs,val", &[T,U,I,A])?;
    scalars(c,"notetypes","id,name,mtime_secs,usn", &[I,T,I,U])?;
    scalars(c,"fields","ntid,ord,name", &[I,UINT32,T])?;
    scalars(c,"templates","ntid,ord,name,mtime_secs,usn", &[I,UINT32,T,I,U])?;
    scalars(c,"decks","id,name,mtime_secs,usn", &[I,T,I,U])?;
    scalars(c,"deck_config","id,name,mtime_secs,usn", &[I,T,I,U])?;
    super::objects::validate(c)
}
