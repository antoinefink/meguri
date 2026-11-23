// SPDX-License-Identifier: MIT
use super::*;
use super::read::{notetype, revision_state, stored_note, tag_list};
use meguri_model::{notetype::Notetype, proto::deck::DeckKind};
use rusqlite::{OptionalExtension, params};
use unicode_normalization::UnicodeNormalization;
use unicase::UniCase;

pub(super) fn config(c: &Connection, key: &str) -> Result<Option<serde_json::Value>> {
    // Config is typed only for the keys this operation needs. Preserve opaque
    // unrelated entries, and never allocate a large malformed config value.
    let value: Option<Option<Vec<u8>>> = c.query_row("SELECT CASE WHEN octet_length(val)<=1024 THEN CAST(val AS BLOB) END FROM config WHERE key=?1",[key],|r|r.get(0))
        .optional().map_err(sql_error)?;
    Ok(value.flatten().and_then(|b|serde_json::from_slice(&b).ok()))
}
fn normalized(c: &Connection, value: &str) -> Result<String> {
    let text: String = value.chars().filter(|c|!c.is_ascii_control() || matches!(*c,'\t'|'\n')).collect();
    Ok(if config(c,"normalize_note_text")?.and_then(|v|v.as_bool()).unwrap_or(true) { text.nfc().collect() } else { text })
}
fn patch_fields(c: &Connection, nt: &Notetype, old: Option<&str>, patch: &Fields, references: &[(String, String)]) -> Result<String> {
    if patch.0.len() > MAX_FIELDS || patch.0.keys().any(|name|!nt.fields.iter().any(|f|f.name == *name)) { return Err(invalid()); }
    let mut values = match old {
        Some(old) => old.split('\u{1f}').map(str::to_owned).collect::<Vec<_>>(),
        None => vec![String::new();nt.fields.len()],
    };
    if values.len() != nt.fields.len() { return Err(unsupported()); }
    for (i,f) in nt.fields.iter().enumerate() {
        if let Some(value) = patch.0.get(&f.name) { values[i] = normalized(c,value)?; }
    }
    for (field, reference) in references {
        let i = nt.fields.iter().position(|f| f.name == *field).ok_or_else(invalid)?;
        if !values[i].contains(reference) {
            if !values[i].is_empty() { values[i].push_str("<br>"); }
            values[i].push_str(reference);
        }
    }
    if !references.is_empty() && values.iter().any(|v| v.len() > 64 * 1024) { return Err(too_large()); }
    let fields = values.join("\u{1f}");
    if fields.len() > MAX_RECORD_BYTES { return Err(too_large()); }
    Ok(fields)
}
pub(super) fn normalize_tag(text: &str) -> String {
    text.split("::").map(|part| {
        let value: String = part.nfc().filter(|c|!c.is_ascii_control() && !matches!(*c,' '|'\u{3000}')).collect();
        let value=value.trim_matches(|c:char|c.is_whitespace());
        if value.is_empty() { "blank".into() } else { value.to_owned() }
    }).collect::<Vec<_>>().join("::")
}
fn same(a: &str, b: &str) -> bool { UniCase::new(a) == UniCase::new(b) }
fn prefix_spelling(text: &str, prefix: &str) -> Option<String> {
    let wanted:Vec<_>=prefix.split("::").collect(); let found:Vec<_>=text.split("::").collect();
    if found.len()<wanted.len() || !wanted.iter().zip(&found).all(|(a,b)|same(a,b)) { return None; }
    Some(found[..wanted.len()].join("::"))
}
fn registry_spelling(c: &Connection, name: &str, pending: &[String]) -> Result<Option<String>> {
    let stored: Option<Option<String>> = c.query_row("SELECT CASE WHEN octet_length(tag)<=1024 THEN tag END FROM tags WHERE tag=?1 COLLATE unicase",[name],|r|r.get(0))
        .optional().map_err(sql_error)?;
    if let Some(value)=stored { return value.map(Some).ok_or_else(too_large); }
    if let Some(tag)=pending.iter().find(|t|same(t,name)) { return Ok(Some(tag.clone())); }
    // Probe one indexed lower bound per ancestor. Tree spelling comes from
    // the first descendant in the registry's collation, including staged tags.
    let parts:Vec<_>=name.split("::").collect();
    for n in (1..=parts.len()).rev() {
        let prefix=parts[..n].join("::");
        let mut candidates=Vec::new();
        let row:Option<Option<String>>=c.query_row("SELECT CASE WHEN octet_length(tag)<=1024 THEN tag END FROM tags WHERE tag>=?1 COLLATE unicase ORDER BY tag COLLATE unicase LIMIT 1",[format!("{prefix}::")],|r|r.get(0)).optional().map_err(sql_error)?;
        if let Some(row)=row {
            let row=row.ok_or_else(too_large)?;
            if prefix_spelling(&row,&prefix).is_some() { candidates.push(row); }
        }
        // A registered ancestor wins over any descendant spelling.
        let exact:Option<Option<String>>=c.query_row("SELECT CASE WHEN octet_length(tag)<=1024 THEN tag END FROM tags WHERE tag=?1 COLLATE unicase",[&prefix],|r|r.get(0)).optional().map_err(sql_error)?;
        if let Some(row)=exact { candidates=vec![row.ok_or_else(too_large)?]; }
        for tag in pending { if prefix_spelling(tag,&prefix).is_some() { candidates.push(tag.clone()); } }
        candidates.sort_by(|a,b|UniCase::new(a).cmp(&UniCase::new(b)));
        if let Some(tag)=candidates.first() {
            let mut result=prefix_spelling(tag,&prefix).ok_or_else(unsupported)?;
            if n<parts.len() { result.push_str("::");result.push_str(&parts[n..].join("::")); }
            return Ok(Some(result));
        }
    }
    Ok(None)
}
fn canonical_tags(c: &Connection, input: &[String]) -> Result<(String,Vec<String>)> {
    input::tag_inputs(input)?;
    let mut pending=Vec::new(); let mut tags=Vec::<String>::new();
    for item in input {
        for text in item.split([' ','\u{3000}']).filter(|s|!s.is_empty()) {
            if tags.len()>=MAX_TAGS { return Err(invalid()); }
            let name=normalize_tag(text);
            if name.len()>1024 { return Err(invalid()); }
            let name=registry_spelling(c,&name,&pending)?.unwrap_or(name);
            if !tags.iter().any(|t|same(t,&name)) { tags.push(name.clone()); }
            if !pending.iter().any(|t|same(t,&name)) { pending.push(name); }
        }
    }
    tags.sort_by(|a,b|UniCase::new(a).cmp(&UniCase::new(b)));
    let text=if tags.is_empty() { String::new() } else { format!(" {} ",tags.join(" ")) };
    Ok((text,pending))
}
fn edit_tags(c: &Connection, old: &str, add: Option<&[String]>, remove: Option<&[String]>) -> Result<(String,Vec<String>)> {
    if add.is_none() && remove.is_none() { return Ok((old.to_owned(),Vec::new())); }
    let names=|items:&[String]| -> Result<Vec<String>> {
        input::tag_inputs(items)?;
        let names:Vec<_>=items.iter().flat_map(|t|t.split([' ','\u{3000}'])).filter(|s|!s.is_empty()).map(normalize_tag).collect();
        if names.len()>MAX_TAGS { return Err(invalid()); } Ok(names)
    };
    let add=names(add.unwrap_or(&[]))?; let remove=names(remove.unwrap_or(&[]))?;
    if add.iter().any(|a|remove.iter().any(|r|same(a,r))) { return Err(invalid()); }
    let mut tags=tag_list(old);
    if tags.len()>MAX_TAGS { return Err(too_large()); }
    // MCP names exact tags; it does not expose the browser's descendant-removal
    // command. In particular removing a parent does not remove its children.
    tags.retain(|t|!remove.iter().any(|r|same(t,r)));
    tags.extend(add);
    canonical_tags(c,&tags)
}
fn register_tags(c: &Connection, tags: &[String], usn: i32) -> Result<()> {
    for tag in tags {
        c.execute("INSERT INTO tags(tag,usn,collapsed,config) VALUES(?1,?2,1,NULL) ON CONFLICT(tag) DO NOTHING",params![tag,usn]).map_err(sql_error)?;
    }
    Ok(())
}
pub(super) fn allocate(c: &Connection, table: &str, now: i64) -> Result<i64> {
    if now<=0 { return Err(unsupported()); }
    let cap=now.checked_add(86_400_000).ok_or_else(unsupported)?;
    let exists=c.query_row(&format!("SELECT id FROM {table} WHERE id=?1"),[now],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?.is_some();
    if !exists { return Ok(now); }
    let maximum:i64=c.query_row(&format!("SELECT id FROM {table} ORDER BY id DESC LIMIT 1"),[],|r|r.get(0)).map_err(sql_error)?;
    if let Some(next)=maximum.checked_add(1).filter(|n|*n<=cap) { return Ok(next); }
    let mut candidate=now+1;
    let mut q=c.prepare(&format!("SELECT id FROM {table} WHERE id>?1 AND id<=?2 ORDER BY id LIMIT 4096")).map_err(sql_error)?;
    let mut rows=q.query(params![now,cap]).map_err(sql_error)?;
    let mut count=0;
    while let Some(row)=rows.next().map_err(sql_error)? {
        let id:i64=row.get(0).map_err(sql_error)?;
        if id>candidate { return Ok(candidate); }
        candidate=id+1; count+=1;
    }
    if candidate>cap { return Err(unsupported()); }
    if count==4096 { return Err(busy()); }
    Ok(candidate)
}
fn guid(mut number: u64) -> String {
    const DIGITS:&[u8]=b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_`{|}~";
    let mut out=Vec::new();
    while number>0 { out.push(DIGITS[(number%91) as usize]);number/=91; }
    out.reverse();String::from_utf8(out).expect("ASCII alphabet")
}
struct Stamps { usn: i32, next: i32, millis: i64, seconds: i64 }
impl Stamps {
    fn read(c: &Connection, now: i64) -> Result<Self> {
        let (usn,old):(i32,i64)=c.query_row("SELECT usn,mod FROM col WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sql_error)?;
        if usn<0 || now<=0 { return Err(unsupported()); }
        Ok(Self { usn,next:usn.checked_add(1).ok_or_else(unsupported)?,millis:now.max(old.checked_add(1).ok_or_else(unsupported)?),seconds:now.div_euclid(1000) })
    }
    fn save(&self, c: &Connection) -> Result<()> {
        c.execute("UPDATE col SET usn=?1,mod=?2 WHERE id=1",params![self.next,self.millis]).map_err(sql_error)?; Ok(())
    }
}
impl Backend {
    pub fn create_note<A: Authority>(&self, path: &Path, a: &A, input: CreateInput, runtime: &impl WriteRuntime) -> Result<WriteResult> {
        self.create_note_with_media(path, a, input, runtime, &[], None)
    }
    pub(crate) fn create_note_with_media<A: Authority>(&self, path: &Path, a: &A, input: CreateInput,
        runtime: &impl WriteRuntime, references: &[(String, String)], hook: Option<&mut dyn FnMut() -> Result<()>>) -> Result<WriteResult> {
        input::bounded_input(&input)?;
        let did=input::id(&input.deck_id)?; let mid=input::id(&input.note_type_id)?;
        input::tag_inputs(input.tags.as_deref().unwrap_or(&[]))?;
        self.transaction_with_hook(path,a,true,|c| {
            let d=read::deck(c,did)?.ok_or_else(invalid)?;
            if !matches!(&d.kind.kind,DeckKind::Normal(_)) { return Err(invalid()); }
            if c.query_row("SELECT id FROM notetypes WHERE id=?1",[mid],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?.is_none() { return Err(invalid()); }
            let nt=notetype(c,mid)?;
            generation::validate_type(&nt)?;
            let fields=patch_fields(c,&nt,None,&input.fields,references)?;
            let (tags,registered)=canonical_tags(c,input.tags.as_deref().unwrap_or(&[]))?;
            let now=runtime.now_millis()?; let stamps=Stamps::read(c,now)?;
            let id=allocate(c,"notes",now)?;
            let derived=super::super::notes::derive_saved_values(&fields,nt.config.sort_field_idx as usize);
            c.execute("INSERT INTO notes(id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,'')",
                params![id,guid(runtime.guid_random_u64()?),mid,stamps.seconds,stamps.usn,tags,fields,derived.sort_field,derived.checksum]).map_err(sql_error)?;
            register_tags(c,&registered,stamps.usn)?;
            generation::generate(c,id,&nt,&fields,&tags,Some(did),now,stamps.usn,runtime)?;
            stamps.save(c)?;
            self.write_result(c,a.context(),id,&nt)
        }, hook)
    }
    pub fn edit_note<A: Authority>(&self, path: &Path, a: &A, input: EditInput, runtime: &impl WriteRuntime) -> Result<WriteResult> {
        self.edit_note_with_media(path, a, input, runtime, &[], None)
    }
    pub(crate) fn edit_note_with_media<A: Authority>(&self, path: &Path, a: &A, input: EditInput,
        runtime: &impl WriteRuntime, references: &[(String, String)], hook: Option<&mut dyn FnMut() -> Result<()>>) -> Result<WriteResult> {
        input::bounded_input(&input)?;
        let id=input::id(&input.note_id)?;
        if input.revision.len()>128 || (input.fields.is_none() && input.add_tags.is_none() && input.remove_tags.is_none() && references.is_empty()) { return Err(invalid()); }
        self.transaction_with_hook(path,a,true,|c| {
            let note=stored_note(c,id)?.ok_or_else(||Error::new("not_found"))?;
            let nt=notetype(c,note.mid)?;
            let current=self.revision(a.context(),&revision_state(note.state.clone(),&nt))?;
            if !crate::crypto::same_digest(current.as_bytes(),input.revision.as_bytes()) { return Err(conflict()); }
            let fields=patch_fields(c,&nt,Some(&note.fields),input.fields.as_ref().unwrap_or(&Fields::default()),references)?;
            let (tags,registered)=edit_tags(c,&note.tags,input.add_tags.as_deref(),input.remove_tags.as_deref())?;
            if fields==note.fields && tags==note.tags { return Ok(WriteResult { note_id:id.to_string(),revision:current }); }
            generation::validate_type(&nt)?;
            let now=runtime.now_millis()?; let stamps=Stamps::read(c,now)?;
            let derived=super::super::notes::derive_saved_values(&fields,nt.config.sort_field_idx as usize);
            c.execute("UPDATE notes SET flds=?1,tags=?2,sfld=?3,csum=?4,mod=?5,usn=?6 WHERE id=?7",params![fields,tags,derived.sort_field,derived.checksum,stamps.seconds,stamps.usn,id]).map_err(sql_error)?;
            register_tags(c,&registered,stamps.usn)?;
            generation::generate(c,id,&nt,&fields,&tags,None,now,stamps.usn,runtime)?;
            stamps.save(c)?;
            self.write_result(c,a.context(),id,&nt)
        }, hook)
    }
    fn write_result(&self, c: &Connection, context: &Context, id: i64, nt: &Notetype) -> Result<WriteResult> {
        let note=stored_note(c,id)?.ok_or_else(unsupported)?;
        // The row must remain readable with complete fields/tags after saving.
        let result=self.note_result(context,&note,nt)?;
        let single=SearchResult { notes:vec![result],next_cursor:None,scan_incomplete:false };Self::result_json(&single)?;
        Ok(WriteResult { note_id:id.to_string(),revision:single.notes.into_iter().next().ok_or_else(unsupported)?.revision })
    }
}
