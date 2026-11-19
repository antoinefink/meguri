// SPDX-License-Identifier: MIT
use super::*;
use meguri_model::{deck::Deck, notetype::Notetype, proto::deck::DeckKind};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use unicase::UniCase;

pub(super) struct StoredNote {
    pub id: i64, pub mid: i64, pub fields: String, pub tags: String,
    pub state: Sha256,
}
fn record_size(c: &Connection, table: &str, key: i64, columns: &str) -> Result<Option<usize>> {
    // octet_length uses SQLite's stored byte length without materializing a large
    // TEXT/BLOB overflow value, including values with embedded NUL characters.
    let expression = columns.split(',').map(|s|format!("coalesce(octet_length({s}),0)")).collect::<Vec<_>>().join("+");
    let n: Option<i64> = c.query_row(&format!("SELECT {expression} FROM {table} WHERE id=?1"), [key], |r|r.get(0))
        .optional().map_err(sql_error)?;
    n.map(|n|usize::try_from(n).map_err(|_| too_large())).transpose()
}
pub(super) fn stored_note(c: &Connection, id: i64) -> Result<Option<StoredNote>> {
    let Some(n) = record_size(c,"notes",id,"guid,tags,flds,sfld,data")? else { return Ok(None); };
    let content: i64 = c.query_row("SELECT octet_length(flds)+octet_length(tags) FROM notes WHERE id=?1", [id], |r|r.get(0)).map_err(sql_error)?;
    let content = usize::try_from(content).map_err(|_| too_large())?;
    if n > MAX_OBJECT_BYTES || content > MAX_RECORD_BYTES { return Err(too_large()); }
    let mut q = c.prepare("SELECT id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data FROM notes WHERE id=?1").map_err(sql_error)?;
    let mut rows = q.query([id]).map_err(sql_error)?;
    let row = rows.next().map_err(sql_error)?.ok_or_else(unsupported)?;
    let mut state = Sha256::new();
    for i in 0..11 { tokens::hash_value(&mut state,row.get_ref(i).map_err(sql_error)?); }
    Ok(Some(StoredNote { id, mid:row.get(2).map_err(sql_error)?, fields:row.get(6).map_err(sql_error)?,
        tags:row.get(5).map_err(sql_error)?, state }))
}
fn child_size(c: &Connection, table: &str, id: i64, limit: usize) -> Result<usize> {
    let mut q = c.prepare(&format!("SELECT coalesce(octet_length(name),0)+coalesce(octet_length(config),0) FROM {table} WHERE ntid=?1 ORDER BY ord LIMIT ?2")).map_err(sql_error)?;
    let sql_limit = i64::try_from(limit).ok().and_then(|n|n.checked_add(1)).ok_or_else(too_large)?;
    let mut rows = q.query(params![id,sql_limit]).map_err(sql_error)?;
    let mut bytes = 0usize; let mut count = 0;
    while let Some(r) = rows.next().map_err(sql_error)? {
        count += 1;
        let child_bytes = usize::try_from(r.get::<_,i64>(0).map_err(sql_error)?).map_err(|_| too_large())?;
        bytes = bytes.checked_add(child_bytes).ok_or_else(too_large)?;
        if count > limit || bytes > MAX_OBJECT_BYTES { return Err(too_large()); }
    }
    Ok(bytes)
}
pub(super) fn notetype(c: &Connection, id: i64) -> Result<Notetype> {
    let head = record_size(c,"notetypes",id,"name,config")?.ok_or_else(unsupported)?;
    if head > MAX_OBJECT_BYTES { return Err(too_large()); }
    let bytes = head + child_size(c,"fields",id,MAX_FIELDS)? + child_size(c,"templates",id,MAX_TEMPLATES)?;
    if bytes > MAX_OBJECT_BYTES { return Err(too_large()); }
    let mut q = c.prepare("SELECT id,name,mtime_secs,usn,config FROM notetypes WHERE id=?1").map_err(sql_error)?;
    let mut rows = q.query([id]).map_err(sql_error)?;
    let nt = super::super::objects::notetype(c,rows.next().map_err(sql_error)?.ok_or_else(unsupported)?)
        .map_err(|_| unsupported())?;
    let mut names = std::collections::BTreeSet::new();
    if nt.fields.is_empty() || nt.fields.iter().any(|f|f.name.is_empty() || !names.insert(f.name.as_str())) { return Err(unsupported()); }
    Ok(nt)
}
pub(super) fn deck(c: &Connection, id: i64) -> Result<Option<Deck>> {
    let Some(bytes) = record_size(c,"decks",id,"name,common,kind")? else { return Ok(None); };
    if bytes > MAX_OBJECT_BYTES { return Err(too_large()); }
    let mut q = c.prepare("SELECT id,name,mtime_secs,usn,common,kind FROM decks WHERE id=?1").map_err(sql_error)?;
    let mut rows = q.query([id]).map_err(sql_error)?;
    super::super::objects::deck(rows.next().map_err(sql_error)?.ok_or_else(unsupported)?)
        .map(Some).map_err(|_| unsupported())
}
pub(super) fn revision_state(mut state: Sha256, nt: &Notetype) -> [u8;32] {
    // Stamps detect changes back to old content; raw field configs bind stable
    // field identity and opaque layout attributes, in exact ordinal order.
    state.update(nt.mtime.0.to_le_bytes()); state.update(nt.usn.0.to_le_bytes());
    tokens::hash_value(&mut state,rusqlite::types::ValueRef::Blob(&nt.config.to_bytes()));
    for f in &nt.fields {
        state.update(f.ord.to_le_bytes());
        tokens::hash_value(&mut state,rusqlite::types::ValueRef::Text(f.name.as_bytes()));
        tokens::hash_value(&mut state,rusqlite::types::ValueRef::Blob(&f.config.to_bytes()));
    }
    state.finalize().into()
}
pub(super) fn tag_list(stored: &str) -> Vec<String> {
    stored.split([' ','\u{3000}']).filter(|s|!s.is_empty()).map(str::to_owned).collect()
}
impl Backend {
    pub(super) fn note_result(&self, context: &Context, note: &StoredNote, nt: &Notetype) -> Result<NoteResult> {
        if note.id<=0 || note.mid<=0 { return Err(unsupported()); }
        let values: Vec<_> = super::super::notes::fields(&note.fields).collect();
        let tags = tag_list(&note.tags);
        if values.len() != nt.fields.len() { return Err(unsupported()); }
        if tags.len() > MAX_TAGS { return Err(too_large()); }
        Ok(NoteResult { note_id:note.id.to_string(), note_type_id:note.mid.to_string(),
            fields:nt.fields.iter().zip(values).map(|(f,v)|(f.name.clone(),v.to_owned())).collect(), tags,
            revision:self.revision(context,&revision_state(note.state.clone(),nt))? })
    }
    pub fn search_notes<A: Authority>(&self, path: &Path, a: &A, input: SearchInput) -> Result<SearchResult> {
        input::bounded_input(&input)?;
        if let Some(ref id) = input.note_id {
            let id = input::id(id)?;
            if input.query.is_some() || input.deck_id.is_some() || input.tag.is_some() || input.limit.is_some() || input.cursor.is_some() { return Err(invalid()); }
            return self.transaction(path,a,false,|c| {
                let notes = match stored_note(c,id)? {
                    None => vec![], Some(note) => vec![self.note_result(a.context(),&note,&notetype(c,note.mid)?)?],
                };
                let result = SearchResult { notes, next_cursor:None };
                Self::result_json(&result)?;
                Ok(result)
            });
        }
        if input.query.is_none() && input.deck_id.is_none() && input.tag.is_none() { return Err(invalid()); }
        let limit = input::limit(input.limit)?;
        if input.query.as_ref().is_some_and(|q|q.is_empty() || q.len()>1024) || input.tag.as_ref().is_some_and(|t|t.is_empty() || t.len()>1024 || t.contains([' ','\u{3000}'])) { return Err(invalid()); }
        let deck_id = input.deck_id.as_deref().map(input::id).transpose()?;
        let tag=input.tag.as_deref().map(write::normalize_tag);
        let query=input.query.as_deref().map(|q|regex::RegexBuilder::new(&regex::escape(q)).case_insensitive(true).build().map_err(|_|invalid())).transpose()?;
        let shape = tokens::shape(&(&input.query,deck_id,&tag,limit))?;
        let page = self.page(input.cursor.as_deref(),a.context(),"search_notes",&shape)?;
        if page.as_ref().is_some_and(|p|p.kind != 0) { return Err(invalid()); }
        let after = page.map_or(0,|p|p.after);
        self.transaction(path,a,false,|c| {
            let mut result = SearchResult { notes:vec![], next_cursor:None };
            let mut position = after; let mut work = 0;
            for _ in 0..MAX_CANDIDATES {
                let id: Option<i64> = c.query_row("SELECT id FROM notes WHERE id>?1 ORDER BY id LIMIT 1",[position],|r|r.get(0)).optional().map_err(sql_error)?;
                let Some(id) = id else { break; };
                // All scanned candidates are keyed. A deck test examines at
                // most MAX_SIBLINGS cards through the existing nid index.
                let matches_deck = match deck_id { None => true, Some(d) => generation::siblings(c,id)?.iter().any(|s|s.home == d) };
                if !matches_deck { position=id; continue; }
                let note = stored_note(c,id)?.ok_or_else(unsupported)?;
                let bytes = note.fields.len()+note.tags.len();
                if work + bytes > MAX_PAGE_WORK_BYTES { break; }
                work += bytes;
                let matches_tag = tag.as_ref().is_none_or(|tag|tag_list(&note.tags).iter().any(|t|UniCase::new(t.as_str()) == UniCase::new(tag.as_str())));
                let matches_text = query.as_ref().is_none_or(|q|super::super::notes::fields(&note.fields).any(|f|q.is_match(f)));
                if matches_tag && matches_text {
                    let record = self.note_result(a.context(),&note,&notetype(c,note.mid)?)?;
                    result.notes.push(record);
                    if size(&result)? + TOKEN_RESERVE > MAX_RESULT_BYTES {
                        result.notes.pop();
                        if result.notes.is_empty() { return Err(too_large()); }
                        break; // Do not advance past the match that did not fit.
                    }
                }
                position=id;
                if result.notes.len() == limit { break; }
            }
            let more = c.query_row("SELECT id FROM notes WHERE id>?1 ORDER BY id LIMIT 1",[position],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?.is_some();
            if more { result.next_cursor=Some(self.page_token(a.context(),"search_notes",&shape,0,position)?); }
            Self::result_json(&result)?;
            Ok(result)
        })
    }
    pub fn list_decks_and_note_types<A: Authority>(&self, path: &Path, a: &A, input: CatalogInput) -> Result<CatalogResult> {
        input::bounded_input(&input)?;
        let limit = input::limit(input.limit)?; let kind = input.kind.unwrap_or_default();
        let shape = tokens::shape(&(kind,limit))?;
        let page = self.page(input.cursor.as_deref(),a.context(),"list_decks_and_note_types",&shape)?;
        let (mut phase,mut position) = page.map_or((if kind==CatalogKind::NoteType {1} else {0},0),|p|(p.kind,p.after));
        if (kind==CatalogKind::Deck && phase!=0) || (kind==CatalogKind::NoteType && phase!=1) { return Err(invalid()); }
        self.transaction(path,a,false,|c| {
            let mut result = CatalogResult { decks:vec![], note_types:vec![], next_cursor:None };
            let mut count = 0;
            loop {
                let table = if phase==0 {"decks"} else {"notetypes"};
                let id = c.query_row(&format!("SELECT id FROM {table} WHERE id>?1 ORDER BY id LIMIT 1"),[position],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?;
                let Some(id) = id else {
                    if phase==0 && kind==CatalogKind::All { phase=1; position=0; continue; }
                    break;
                };
                if count == limit {
                    result.next_cursor=Some(self.page_token(a.context(),"list_decks_and_note_types",&shape,phase,position)?); break;
                }
                if phase==0 {
                    let d=deck(c,id)?.ok_or_else(unsupported)?;
                    result.decks.push(DeckResult { deck_id:id.to_string(),name:d.human_name(),filtered:matches!(&d.kind.kind,DeckKind::Filtered(_)) });
                } else {
                    let nt=notetype(c,id)?;
                    result.note_types.push(NoteTypeResult { note_type_id:id.to_string(),name:nt.name,field_names:nt.fields.into_iter().map(|f|f.name).collect() });
                }
                if size(&result)? + TOKEN_RESERVE > MAX_RESULT_BYTES {
                    if phase==0 { result.decks.pop(); } else { result.note_types.pop(); }
                    if count==0 { return Err(too_large()); }
                    result.next_cursor=Some(self.page_token(a.context(),"list_decks_and_note_types",&shape,phase,position)?); break;
                }
                position=id; count+=1;
            }
            Self::result_json(&result)?;
            Ok(result)
        })
    }
}
