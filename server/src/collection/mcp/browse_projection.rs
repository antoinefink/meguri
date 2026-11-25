// SPDX-License-Identifier: MIT
//! Browse labels and ordinal metadata, independent of the MCP object readers.
//! Opaque templates, CSS and deck configuration never enter these projections.
use super::*;
use rusqlite::{OptionalExtension, params};

pub(super) fn unavailable() -> Error { Error::new("browse_content_unavailable") }
pub(super) fn is_content_error(e: &Error) -> bool {
    e.code == "browse_content_unavailable" || (e.mcp_detail.is_none() && matches!(e.code, "unsupported_collection" | "result_too_large"))
}
pub(super) fn short(text: &str) -> String {
    let mut chars = text.chars(); let mut label: String = chars.by_ref().take(120).collect();
    if chars.next().is_some() { label.push('…'); } label
}
fn text(value: rusqlite::types::ValueRef<'_>) -> Result<String> {
    let rusqlite::types::ValueRef::Text(bytes) = value else { return Err(unavailable()); };
    let value = std::str::from_utf8(bytes).map_err(|_| unavailable())?;
    if value.is_empty() { return Err(unavailable()); }
    Ok(value.to_owned())
}
pub(super) fn note_head(c: &Connection, id: i64) -> Result<(i64,usize)> {
    let mut q=c.prepare("SELECT mid,typeof(flds),octet_length(flds) FROM notes WHERE id=?1").map_err(sql_error)?;
    let mut rows=q.query([id]).map_err(sql_error)?;
    let row=rows.next().map_err(sql_error)?.ok_or_else(unavailable)?;
    let mid=match row.get_ref(0).map_err(sql_error)? {rusqlite::types::ValueRef::Integer(n)=>n,_=>return Err(unavailable())};
    if row.get::<_,String>(1).map_err(sql_error)?!="text" {return Err(unavailable());}
    let bytes=row.get::<_,i64>(2).map_err(sql_error)?;
    Ok((mid,usize::try_from(bytes).map_err(|_|unavailable())?))
}
pub(super) fn note_fields(c: &Connection, id: i64) -> Result<String> {
    let mut q=c.prepare("SELECT flds FROM notes WHERE id=?1").map_err(sql_error)?;
    let mut rows=q.query([id]).map_err(sql_error)?;
    let row=rows.next().map_err(sql_error)?.ok_or_else(unavailable)?;
    match row.get_ref(0).map_err(sql_error)? {
        rusqlite::types::ValueRef::Text(bytes)=>Ok(std::str::from_utf8(bytes).map_err(|_|unavailable())?.to_owned()),
        _=>Err(unavailable()),
    }
}
fn label(c: &Connection, table: &str, id: i64) -> Result<String> {
    // Bound even malformed imported names before bringing them into Rust.
    let mut q = c.prepare(&format!("SELECT CASE WHEN octet_length(name)<=4096 THEN name END FROM {table} WHERE id=?1")).map_err(sql_error)?;
    let mut rows = q.query([id]).map_err(sql_error)?;
    let row = rows.next().map_err(sql_error)?.ok_or_else(unavailable)?;
    Ok(short(&text(row.get_ref(0).map_err(sql_error)?)?))
}

// P2/P6 top-level wire projection. Read only key/length/scalar headers through
// bounded SQLite blob slices; skip payloads by offset, including >512KiB CSS.
// This is not validation of opaque payloads and never changes stored content.
fn headers(c: &Connection, table: &str, column: &str, id: i64, mut take: impl FnMut(u32, u8, u64) -> Result<()>) -> Result<()> {
    let length: Option<i64> = c.query_row(&format!("SELECT CASE WHEN typeof({column})='blob' THEN octet_length({column}) END FROM {table} WHERE id=?1"), [id], |r| r.get(0)).optional().map_err(sql_error)?.flatten();
    let length = length.ok_or_else(unavailable)?;
    let mut at = 0i64; let mut steps = 0; let mut groups = Vec::new();
    let mut q = c.prepare(&format!("SELECT substr({column},?2,20) FROM {table} WHERE id=?1")).map_err(sql_error)?;
    while at < length {
        steps += 1; if steps > 1024 { return Err(unavailable()); }
        let bytes: Vec<u8> = q.query_row(params![id, at+1], |r|r.get(0)).map_err(sql_error)?;
        let mut pos = 0;
        let key = varint(&bytes, &mut pos)?;
        if key > u32::MAX as u64 || key >> 3 == 0 { return Err(unavailable()); }
        let number = (key >> 3) as u32; let wire = (key & 7) as u8;
        let value = match wire { 0 | 2 => varint(&bytes, &mut pos)?, _ => 0 };
        if groups.is_empty() { take(number, wire, value)?; }
        let skip = match wire {
            0 => 0, 1 => 8, 2 => i64::try_from(value).map_err(|_|unavailable())?, 5 => 4,
            3 => { if groups.len() >= 100 { return Err(unavailable()); } groups.push(number); 0 },
            4 => { if groups.pop() != Some(number) { return Err(unavailable()); } 0 },
            _ => return Err(unavailable()),
        };
        at = at.checked_add(pos as i64).and_then(|n|n.checked_add(skip)).filter(|n|*n<=length).ok_or_else(unavailable)?;
    }
    if !groups.is_empty() { return Err(unavailable()); } Ok(())
}
fn varint(bytes: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*pos).ok_or_else(unavailable)?; *pos += 1;
        if shift == 63 && byte > 1 { return Err(unavailable()); }
        value |= ((byte & 127) as u64) << shift;
        if byte < 128 { return Ok(value); }
    }
    Err(unavailable())
}
pub(super) struct NoteType { pub name: String, pub fields: Vec<String>, pub templates: Vec<(u32,String)>, pub kind: u32, pub sort: usize }
pub(super) fn notetype(c: &Connection, id: i64) -> Result<NoteType> {
    let name = label(c, "notetypes", id)?;
    let mut kind = 0; let mut sort = 0;
    headers(c,"notetypes","config",id,|number,wire,value| {
        if matches!(number,1|2) {
            if wire != 0 { return Err(unavailable()); }
            if number == 1 { kind=value as u32; } else { sort=value as u32 as usize; }
        } Ok(())
    })?;
    let fields = children(c,"fields",id,MAX_FIELDS)?;
    if fields.is_empty() || fields.iter().enumerate().any(|(i,(ord,_))|*ord as usize!=i) || sort>=fields.len() || kind>1 { return Err(unavailable()); }
    let fields: Vec<_> = fields.into_iter().map(|(_,name)|name).collect();
    if fields.iter().collect::<std::collections::BTreeSet<_>>().len()!=fields.len() { return Err(unavailable()); }
    let templates = children(c,"templates",id,MAX_TEMPLATES)?;
    if templates.is_empty() { return Err(unavailable()); }
    Ok(NoteType { name, fields, templates, kind, sort })
}
fn children(c: &Connection, table: &str, id: i64, limit: usize) -> Result<Vec<(u32,String)>> {
    let mut q = c.prepare(&format!("SELECT ord,CASE WHEN octet_length(name)<=1024 THEN name END FROM {table} WHERE ntid=?1 ORDER BY ord LIMIT ?2")).map_err(sql_error)?;
    let mut rows = q.query(params![id,(limit+1) as i64]).map_err(sql_error)?;
    let mut labels = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        if labels.len() == limit { return Err(unavailable()); }
        let ord = match row.get_ref(0).map_err(sql_error)? { rusqlite::types::ValueRef::Integer(n) => u32::try_from(n).map_err(|_|unavailable())?, _ => return Err(unavailable()) };
        labels.push((ord,text(row.get_ref(1).map_err(sql_error)?)?));
    } Ok(labels)
}
pub(super) struct Deck { pub name: String, pub filtered: bool }
pub(super) fn deck(c: &Connection, id: i64) -> Result<Deck> {
    let name = meguri_model::deck::human_name(&label(c,"decks",id)?);
    let mut member = None;
    headers(c,"decks","kind",id,|number,wire,_| {
        if matches!(number,1|2) { if wire != 2 { return Err(unavailable()); } member=Some(number); } Ok(())
    })?;
    Ok(Deck {name:short(&name),filtered:member.ok_or_else(unavailable)?==2})
}
