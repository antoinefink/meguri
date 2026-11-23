// SPDX-License-Identifier: MIT
use super::*;
use meguri_model::{deck::Deck, notetype::Notetype, proto::deck::DeckKind};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU32, Ordering}};
use unicase::UniCase;
use std::time::{Duration, Instant};

const MAX_SCAN_CANDIDATES: usize = 32_768;
const MAX_SCAN_BYTES: usize = 32 * 1024 * 1024;
const SCAN_FOR: Duration = Duration::from_millis(500);

// Private inputs let unit tests force each stop without timing assertions.
#[derive(Clone, Copy)]
pub(super) struct ScanLimits { pub candidates: usize, pub bytes: usize, pub duration: Duration }
impl Default for ScanLimits {
    fn default() -> Self {
        Self { candidates: MAX_SCAN_CANDIDATES, bytes: MAX_SCAN_BYTES, duration: SCAN_FOR }
    }
}

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

#[derive(Default)]
pub(super) struct DeckBudget { ticks: AtomicU32, interrupted: AtomicBool }
impl DeckBudget {
    pub(super) fn install(c: &mut Connection) -> rusqlite::Result<Arc<Self>> {
        let budget = Arc::new(Self::default());
        let handler = budget.clone();
        // Install before borrowing the connection for its sole transaction.
        // Zero ticks keep the callback dormant outside candidate selection.
        c.progress_handler(1000, Some(move || {
            let ticks = handler.ticks.load(Ordering::Relaxed);
            if ticks == 0 { return false; }
            handler.ticks.store(ticks-1, Ordering::Relaxed);
            if ticks == 1 { handler.interrupted.store(true, Ordering::Relaxed); true } else { false }
        }))?;
        Ok(budget)
    }
}

/// Candidate IDs only; callers retain authoritative note/sibling reads and fit
/// accounting. None means use the existing global keyset path from the same ID.
pub(super) fn deck_candidates(c: &Connection, deck: i64, after: i64, budget: &DeckBudget) -> Result<Option<Vec<i64>>> {
    // Be conservative about imported indexes, especially partial predicates.
    // These checks choose a plan; submitted indexes never prove bounded work.
    for (name, expected) in [
        ("ix_cards_sched", "CREATEINDEXIX_CARDS_SCHEDONCARDS(DID,QUEUE,DUE)"),
        ("idx_cards_odid", "CREATEINDEXIDX_CARDS_ODIDONCARDS(ODID)WHEREODID!=0"),
    ] {
        let sql: Option<String> = c.query_row(
            "SELECT sql FROM sqlite_schema WHERE type='index' AND tbl_name='cards' AND name=?1 AND octet_length(sql)<=256",
            [name], |r|r.get(0)).optional().map_err(sql_error)?;
        if sql.is_none_or(|s|s.chars().filter(|c|!c.is_ascii_whitespace()).collect::<String>().to_ascii_uppercase()!=expected) {
            return Ok(None);
        }
    }
    // LIMIT bounds retained IDs, not the UNION scan/dedup/sort. Interrupt at a
    // 100,000 VM-instruction budget, including prepare-time work. Disarm before
    // fallback or ordinary reads; the request-owned callback can stay dormant.
    struct WorkGuard<'a>(&'a DeckBudget);
    impl Drop for WorkGuard<'_> {
        fn drop(&mut self) { self.0.ticks.store(0, Ordering::Relaxed); }
    }
    let guard = WorkGuard(budget);
    budget.interrupted.store(false, Ordering::Relaxed);
    budget.ticks.store(100, Ordering::Relaxed);
    let selected = (|| -> rusqlite::Result<Vec<i64>> {
        // CROSS JOIN fixes cards first, with keyed surviving-note lookups. A
        // filtered card belongs only to odid; orphans never consume the cap.
        let mut q = c.prepare(
            "SELECT n.id FROM cards AS c INDEXED BY ix_cards_sched
             CROSS JOIN notes AS n ON n.id=c.nid
             WHERE c.did=?1 AND c.odid=0 AND n.id>?2
             UNION
             SELECT n.id FROM cards AS c INDEXED BY idx_cards_odid
             CROSS JOIN notes AS n ON n.id=c.nid
             WHERE c.odid=?1 AND c.odid!=0 AND n.id>?2
             ORDER BY 1 LIMIT ?3")?;
        let rows = q.query_map(params![deck,after,(MAX_CANDIDATES+1) as i64], |r|r.get(0))?;
        rows.collect()
    })();
    drop(guard);
    match selected {
        Ok(ids) => Ok(Some(ids)),
        Err(rusqlite::Error::SqliteFailure(ref e, _)) if e.code==rusqlite::ErrorCode::OperationInterrupted
            && budget.interrupted.load(Ordering::Relaxed) => Ok(None),
        Err(e) => Err(sql_error(e)),
    }
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
        self.search_notes_with_index(path, a, input, |_| None)
    }
    // The service supplies cache ownership separately from the authority guard.
    // This callback runs only for paged search, inside its committed snapshot.
    pub(crate) fn search_notes_with_index<A: Authority>(&self, path: &Path, a: &A, input: SearchInput,
        index: impl FnOnce(&Connection) -> Option<Arc<SearchIndex>>) -> Result<SearchResult> {
        self.search_notes_with_scan_limits(path,a,input,index,ScanLimits::default())
    }
    #[cfg(test)]
    pub(super) fn test_search_notes_with_scan_limits<A: Authority>(&self, path: &Path, a: &A,
        input: SearchInput, scan: ScanLimits) -> Result<SearchResult> {
        self.search_notes_with_scan_limits(path,a,input,|_|None,scan)
    }
    fn search_notes_with_scan_limits<A: Authority>(&self, path: &Path, a: &A, input: SearchInput,
        index: impl FnOnce(&Connection) -> Option<Arc<SearchIndex>>, scan: ScanLimits) -> Result<SearchResult> {
        input::bounded_input(&input)?;
        if let Some(ref id) = input.note_id {
            let id = input::id(id)?;
            if input.query.is_some() || input.deck_id.is_some() || input.tag.is_some() || input.limit.is_some() || input.cursor.is_some() { return Err(invalid()); }
            return self.transaction(path,a,false,|c| {
                let notes = match stored_note(c,id)? {
                    None => vec![], Some(note) => vec![self.note_result(a.context(),&note,&notetype(c,note.mid)?)?],
                };
                let result = SearchResult { notes, next_cursor:None, scan_incomplete:false };
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
        self.transaction_with_deck_budget(path,a,false,deck_id.is_some(),|c,budget| {
            let index = index(c);
            // Even an empty scoped posting must not bypass legacy sibling
            // validation. On failure let the original global cursor reach its
            // next note and return the authoritative sibling error (or exhaust).
            let eligible = deck_id.is_none() || generation::prepare_siblings(c).is_ok();
            let indexed = index.as_ref().filter(|_|eligible)
                .and_then(|i| i.candidates(deck_id,tag.as_deref(),input.query.as_deref()));
            // Every driver uses the same authenticated note-ID position. Keep
            // the full posting for an exact lookahead even at a byte-fit stop.
            let indexed = indexed.map(|ids| &ids[ids.partition_point(|id| *id <= after)..]);
            let cold = if eligible && indexed.is_none() {
                match (deck_id,budget) { (Some(d),Some(b)) => deck_candidates(c,d,after,b)?, _ => None }
            } else { None };
            let scoped = indexed.or(cold.as_deref());
            let (mut result,position,more) = if deck_id.is_none() && scoped.is_none() {
                self.streamed_search(c,a.context(),after,tag.as_deref(),query.as_ref(),limit,scan)?
            } else {
                let mut result = SearchResult { notes:vec![], next_cursor:None, scan_incomplete:false };
                let mut position = after; let mut work = 0;
                let mut fit_stop = false;
                for candidate in 0..MAX_CANDIDATES {
                    let id = match &scoped {
                        Some(ids) => ids.get(candidate).copied(),
                        None => c.query_row("SELECT id FROM notes WHERE id>?1 ORDER BY id LIMIT 1",[position],|r|r.get(0)).optional().map_err(sql_error)?,
                    };
                    let Some(id) = id else { break; };
                    // Global fallback must establish membership before decoding
                    // unrelated scheduling data. Probe only with the qualified nid
                    // index and the existing sibling bound; an invalid index still
                    // reaches the authoritative siblings error below.
                    // Selected matching notes retain the complete sibling read and
                    // validation, including cards whose home is another deck.
                    let matches_deck = match deck_id {
                        None => true,
                        Some(d) if scoped.is_none() && eligible && !generation::has_home_deck(c,id,d)? => false,
                        Some(d) => generation::siblings(c,id)?.iter().any(|s|s.home == d),
                    };
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
                            fit_stop = true;
                            break; // Do not advance past the match that did not fit.
                        }
                    }
                    position=id;
                    if result.notes.len() == limit { break; }
                }
                let more = match &scoped {
                    Some(ids) => ids.iter().any(|id|*id>position),
                    None => c.query_row("SELECT id FROM notes WHERE id>?1 ORDER BY id LIMIT 1",[position],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?.is_some(),
                };
                result.scan_incomplete = more && result.notes.len()<limit && !fit_stop;
                (result,position,more)
            };
            if more { result.next_cursor=Some(self.page_token(a.context(),"search_notes",&shape,0,position)?); }
            Self::result_json(&result)?;
            Ok(result)
        },None)
    }
    // One authoritative keyset cursor. CASE guards reject oversized records
    // before SQLite returns their text, even when they contain embedded NULs.
    // Only actual matches pay the existing full record/hash/layout reads.
    fn streamed_search(&self, c: &Connection, context: &Context, after: i64,
        tag: Option<&str>, query: Option<&regex::Regex>, limit: usize, scan: ScanLimits)
        -> Result<(SearchResult,i64,bool)> {
        let deadline = Instant::now() + scan.duration;
        let mut q = c.prepare(
            "SELECT id,
             coalesce(octet_length(guid),0)+coalesce(octet_length(tags),0)+coalesce(octet_length(flds),0)+coalesce(octet_length(sfld),0)+coalesce(octet_length(data),0),
             octet_length(flds)+octet_length(tags),
             CASE WHEN octet_length(flds)+octet_length(tags)<=?2
               AND coalesce(octet_length(guid),0)+coalesce(octet_length(tags),0)+coalesce(octet_length(flds),0)+coalesce(octet_length(sfld),0)+coalesce(octet_length(data),0)<=?3
               THEN flds END,
             CASE WHEN octet_length(flds)+octet_length(tags)<=?2
               AND coalesce(octet_length(guid),0)+coalesce(octet_length(tags),0)+coalesce(octet_length(flds),0)+coalesce(octet_length(sfld),0)+coalesce(octet_length(data),0)<=?3
               THEN tags END,
             typeof(mid)='integer'
             FROM notes WHERE id>?1 ORDER BY id LIMIT ?4").map_err(sql_error)?;
        let mut rows = q.query(params![after,MAX_RECORD_BYTES as i64,MAX_OBJECT_BYTES as i64,
            scan.candidates.max(1).saturating_add(1) as i64]).map_err(sql_error)?;
        let mut result = SearchResult { notes:vec![], next_cursor:None, scan_incomplete:false };
        let mut position = after;
        let mut count = 0;
        let mut work = 0usize;
        while let Some(row) = rows.next().map_err(sql_error)? {
            // Always examine at least one row, even if the deadline has passed.
            // The clock is checked per bounded record, separate from admission.
            if count>0 && (count>=scan.candidates || Instant::now()>=deadline) {
                result.scan_incomplete = true;
                return Ok((result,position,true));
            }
            let raw = usize::try_from(row.get::<_,i64>(1).map_err(sql_error)?).map_err(|_|too_large())?;
            let bytes = usize::try_from(row.get::<_,i64>(2).map_err(sql_error)?).map_err(|_|too_large())?;
            if raw>MAX_OBJECT_BYTES || bytes>MAX_RECORD_BYTES { return Err(too_large()); }
            if count>0 && work.saturating_add(bytes)>scan.bytes {
                result.scan_incomplete = true;
                return Ok((result,position,true));
            }
            count += 1;
            work += bytes;
            let id = row.get(0).map_err(sql_error)?;
            if !row.get::<_,bool>(5).map_err(sql_error)? {
                return Err(unsupported().with_mcp_detail(Detail::SqliteAccess));
            }
            let fields = row.get_ref(3).map_err(sql_error)?.as_str().map_err(|_|unsupported())?;
            let tags = row.get_ref(4).map_err(sql_error)?.as_str().map_err(|_|unsupported())?;
            let matches_tag = tag.is_none_or(|tag|tags.split([' ','\u{3000}']).filter(|t|!t.is_empty())
                .any(|t|UniCase::new(t)==UniCase::new(tag)));
            let matches_text = query.is_none_or(|q|super::super::notes::fields(fields).any(|f|q.is_match(f)));
            if matches_tag && matches_text {
                let note = stored_note(c,id)?.ok_or_else(unsupported)?;
                result.notes.push(self.note_result(context,&note,&notetype(c,note.mid)?)?);
                if size(&result)? + TOKEN_RESERVE > MAX_RESULT_BYTES {
                    result.notes.pop();
                    if result.notes.is_empty() { return Err(too_large()); }
                    return Ok((result,position,true)); // Retry the confirmed match.
                }
            }
            position = id;
            if result.notes.len()==limit {
                let more = rows.next().map_err(sql_error)?.is_some();
                return Ok((result,position,more));
            }
        }
        Ok((result,position,false))
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
