// SPDX-License-Identifier: MIT
//! Browser-only card reads. MCP's tools and search contract remain independent.
use super::*;
use regex::{Regex, RegexBuilder};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageInput {
    #[serde(default)] pub query: String,
    pub field: Option<String>, pub deck_id: Option<String>,
    pub cursor: Option<String>, pub limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DetailInput { pub card_id: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Create {
    pub generation: String,
    pub deck_id: String, pub note_type_id: String, pub fields: Fields, pub tags: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Delete { pub note_id: String, pub revision: String, pub confirmation: String }
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct Fragment { pub text: String, pub matched: bool }
#[derive(Serialize)]
pub(crate) struct Row {
    pub card_id: String, pub note_id: String, pub title: String,
    pub field: Option<String>, pub excerpt: Vec<Fragment>,
    pub deck: String, pub template: String,
}
#[derive(Serialize)]
pub(crate) struct CardPage { pub cards: Vec<Row>, pub next_cursor: Option<String> }
#[derive(Serialize)]
pub(crate) struct Detail {
    pub card: Row, pub note: NoteResult, pub field_names: Vec<String>,
    pub sibling_ids: Vec<String>, pub confirmation: String,
}

fn hidden_attribute(tag: &str, name: &str) -> bool {
    let mut rest = &tag[name.len()..];
    while !rest.is_empty() {
        rest = rest.trim_start();
        let end = rest.find(|c:char|c.is_whitespace() || matches!(c,'='|'/'|'>')).unwrap_or(rest.len());
        if end == 0 { break; }
        let attr = &rest[..end]; rest = rest[end..].trim_start();
        let mut value = "";
        if let Some(tail) = rest.strip_prefix('=') {
            rest = tail.trim_start();
            if rest.starts_with(['\'', '"']) {
                let quote = rest.as_bytes()[0] as char; rest = &rest[1..];
                let end = rest.find(quote).unwrap_or(rest.len());
                value = &rest[..end]; rest = rest.get(end+1..).unwrap_or("");
            } else {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                value = &rest[..end]; rest = &rest[end..];
            }
        }
        if attr == "hidden" { return true; }
        if attr == "style" && value.split(';').any(|rule|rule.split_once(':').is_some_and(|(key,value)| {
            let key=key.trim(); let value=value.split('!').next().unwrap_or("").trim();
            (key=="display" && value=="none") || (key=="visibility" && matches!(value,"hidden"|"collapse"))
        })) { return true; }
    }
    false
}
fn raw_hidden(name: &str) -> bool { matches!(name,"script"|"style"|"template"|"head"|"audio"|"video") }
fn void_tag(name: &str) -> bool { matches!(name,"area"|"base"|"br"|"col"|"embed"|"hr"|"img"|"input"|"link"|"meta"|"param"|"source"|"track"|"wbr") }

// A bounded text projection, never a card renderer. Attributes, media names,
// comments and executable/style contents cannot become search hits or titles.
pub(crate) fn visible_text(raw: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    let mut hidden: Vec<String> = Vec::new();
    while at < raw.len() {
        let tail = &raw[at..];
        if tail.starts_with("<!--") {
            at += tail.find("-->").map_or(tail.len(), |n| n + 3); continue;
        }
        if tail.starts_with('<') {
            let mut quote = None;
            let mut end = None;
            for (i, ch) in tail.char_indices().skip(1) {
                if let Some(q) = quote { if ch == q { quote = None; } }
                else if matches!(ch, '\'' | '"') { quote = Some(ch); }
                else if ch == '>' { end = Some(i); break; }
            }
            if let Some(end) = end {
                let tag = tail[1..end].trim().to_ascii_lowercase();
                let closing = tag.starts_with('/');
                let name = tag.trim_start_matches('/').split(|ch: char| !ch.is_ascii_alphanumeric()).next().unwrap_or("");
                if closing {
                    if hidden.last().is_some_and(|n| n == name) { hidden.pop(); }
                } else if !void_tag(name) && !tag.ends_with('/') {
                    let hide = match hidden.last() {
                        None => raw_hidden(name) || hidden_attribute(&tag,name),
                        Some(parent) if raw_hidden(parent) => name=="template" && parent=="template",
                        Some(_) => true,
                    };
                    if hide { hidden.push(name.into()); }
                }
                if hidden.is_empty() && matches!(name, "br" | "div" | "p" | "li" | "tr" | "hr") { out.push(' '); }
                at += end + 1; continue;
            }
        }
        if tail.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("[sound:")) {
            if let Some(end) = tail.find(']') { at += end + 1; continue; }
        }
        let ch = tail.chars().next().expect("nonempty");
        if hidden.is_empty() { out.push(ch); }
        at += ch.len_utf8();
    }
    // Decode entities separately: one unknown entity must not hide valid ones.
    static ENTITY: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"&(?:#[xX][0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);").unwrap());
    let decoded = ENTITY.replace_all(&out, |c: &regex::Captures<'_>| if &c[0] == "&apos;" { "'".into() } else { htmlescape::decode_html(&c[0]).unwrap_or_else(|_|c[0].to_owned()) });
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn short(text: &str, max: usize) -> String {
    let mut chars = text.chars(); let mut out: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() { out.push('…'); } out
}
fn excerpt(text: &str, query: Option<&Regex>) -> Vec<Fragment> {
    let start = query.and_then(|q|q.find(text)).map_or(0, |m|text[..m.start()].chars().count().saturating_sub(45));
    let mut boundaries: Vec<_> = text.char_indices().map(|(i,_)|i).collect(); boundaries.push(text.len());
    let end = (start + 180).min(boundaries.len()-1);
    let slice = &text[boundaries[start]..boundaries[end]];
    let mut out = Vec::new();
    if start > 0 { out.push(Fragment { text: "…".into(), matched:false }); }
    let mut at = 0;
    if let Some(q) = query {
        for m in q.find_iter(text) {
            let left=m.start().max(boundaries[start]);
            let right=m.end().min(boundaries[end]);
            if left>=right {continue;}
            let left=left-boundaries[start]; let right=right-boundaries[start];
            if left>at {out.push(Fragment {text:slice[at..left].into(), matched:false});}
            out.push(Fragment {text:slice[left..right].into(), matched:true}); at=right;
        }
    }
    if at<slice.len() {out.push(Fragment {text:slice[at..].into(), matched:false});}
    if end<boundaries.len()-1 {out.push(Fragment {text:"…".into(),matched:false});} out
}
struct Card { id:i64, nid:i64, did:i64, ord:u32 }
fn card(c:&Connection, id:i64) -> Result<Card> {
    c.query_row("SELECT id,nid,did,ord FROM cards WHERE id=?1",[id],|r|Ok(Card {id:r.get(0)?,nid:r.get(1)?,did:r.get(2)?,ord:r.get(3)?}))
        .optional().map_err(sql_error)?.ok_or_else(||Error::new("not_found"))
}
fn siblings(c:&Connection, id:i64) -> Result<Vec<String>> {
    // Qualify the index before using it, as in the shared note transactions.
    drop(generation::prepare_siblings(c)?);
    let mut q=c.prepare("SELECT id FROM cards INDEXED BY ix_cards_nid WHERE nid=?1 ORDER BY id LIMIT 2049").map_err(sql_error)?;
    let ids=q.query_map([id],|r|r.get::<_,i64>(0)).map_err(sql_error)?.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_error)?;
    if ids.len()>2048 {return Err(too_large());}
    Ok(ids.into_iter().map(|id|id.to_string()).collect())
}
fn row(c:&Connection, card:&Card, nt:&meguri_model::notetype::Notetype, fields:&str, query:Option<&Regex>, field:Option<&str>) -> Result<Row> {
    let values:Vec<_>=fields.split('\u{1f}').map(visible_text).collect();
    if values.len()!=nt.fields.len() {return Err(unsupported());}
    let matched = nt.fields.iter().zip(&values).find(|(f,v)|field.is_none_or(|name|f.name==name) && query.is_some_and(|q|q.is_match(v)));
    let selected = matched.or_else(||if query.is_none() {nt.fields.iter().zip(&values).find(|(f,_)|field.is_none_or(|name|f.name==name))} else {None});
    let deck=read::deck(c,card.did)?.ok_or_else(unsupported)?;
    let template=if nt.config.kind==1 {nt.templates.first()} else {nt.templates.iter().find(|t|t.ord==card.ord)}.ok_or_else(unsupported)?;
    Ok(Row {card_id:card.id.to_string(), note_id:card.nid.to_string(),
        title:short(values.get(nt.config.sort_field_idx as usize).ok_or_else(unsupported)?,120),
        field:selected.map(|(f,_)|f.name.clone()), excerpt:selected.map_or_else(Vec::new,|(_,v)|excerpt(v,query)),
        deck:deck.human_name(),template:template.name.clone()})
}
impl Backend {
    pub(crate) fn browse_generation(&self, a:&impl Authority) -> Result<String> {
        self.page_token(a.context(),"browse-generation","",0,0)
    }
    pub(crate) fn check_browse_generation(&self,a:&impl Authority,token:&str) -> Result<()> {
        self.page(Some(token),a.context(),"browse-generation","")?.ok_or_else(invalid)?; Ok(())
    }
    pub(crate) fn browse_page<A:Authority>(&self,path:&Path,a:&A,input:PageInput) -> Result<CardPage> {
        input::bounded_input(&input)?;
        if input.query.len()>1024 || input.field.as_ref().is_some_and(|f|f.is_empty()||f.len()>1024) {return Err(invalid());}
        let limit=input::limit(input.limit)?;
        let deck=input.deck_id.as_deref().map(input::id).transpose()?;
        let query=if input.query.is_empty() {None} else {Some(RegexBuilder::new(&regex::escape(&input.query)).case_insensitive(true).build().map_err(|_|invalid())?)};
        self.transaction(path,a,false,|c| {
            let stamp:(i64,i64)=c.query_row("SELECT mod,usn FROM col WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sql_error)?;
            let shape=tokens::shape(&(stamp,&input.query,&input.field,&input.deck_id,limit))?;
            // A changed collection is a stale page, never an exhausted result.
            let page=self.page(input.cursor.as_deref(),a.context(),"browse-cards",&shape).map_err(|e|if e.code=="invalid_input" && input.cursor.is_some(){conflict()}else{e})?;
            let mut after=page.map_or(0,|p|p.after);
            let mut result=CardPage {cards:vec![],next_cursor:None};
            let mut work=0;
            for _ in 0..MAX_CANDIDATES {
                // Integer primary-key traversal bounds sparse filters too; no
                // OFFSET, result-wide join/sort, or query-language expansion.
                let candidate:Option<(i64,i64,i64,i64)>=c.query_row("SELECT id,nid,did,odid FROM cards WHERE id>?1 ORDER BY id LIMIT 1",[after],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sql_error)?;
                let Some((id,nid,did,odid))=candidate else {return Ok(result);};
                if deck.is_none_or(|d|d==if odid!=0 {odid}else{did}) {
                    let note=read::stored_note(c,nid)?.ok_or_else(unsupported)?;
                    if work>0 && work+note.fields.len()>MAX_PAGE_WORK_BYTES {break;}
                    work+=note.fields.len();
                    let nt=read::notetype(c,note.mid)?;
                    let values:Vec<_>=note.fields.split('\u{1f}').collect();
                    if values.len()!=nt.fields.len(){return Err(unsupported());}
                    let matches=nt.fields.iter().zip(values).any(|(f,v)|input.field.as_ref().is_none_or(|name|f.name==*name) && query.as_ref().is_none_or(|q|q.is_match(&visible_text(v))));
                    if matches {
                        result.cards.push(row(c,&card(c,id)?,&nt,&note.fields,query.as_ref(),input.field.as_deref())?);
                        if size(&result)?+TOKEN_RESERVE>MAX_RESULT_BYTES {
                            result.cards.pop(); if result.cards.is_empty(){return Err(too_large());} break;
                        }
                    }
                }
                after=id;
                if result.cards.len()==limit {break;}
            }
            let more=c.query_row("SELECT id FROM cards WHERE id>?1 ORDER BY id LIMIT 1",[after],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?.is_some();
            if more {result.next_cursor=Some(self.page_token(a.context(),"browse-cards",&shape,0,after)?);}
            Ok(result)
        })
    }
    pub(crate) fn browse_detail<A:Authority>(&self,path:&Path,a:&A,input:DetailInput) -> Result<Detail> {
        let id=input::id(&input.card_id)?;
        self.transaction(path,a,false,|c| {
            let card=card(c,id)?; let stored=read::stored_note(c,card.nid)?.ok_or_else(||Error::new("not_found"))?;
            let nt=read::notetype(c,stored.mid)?;
            let note=self.note_result(a.context(),&stored,&nt)?;
            let sibling_ids=siblings(c,card.nid)?;
            let confirmation=self.delete_confirmation(a.context(),&note.revision,&sibling_ids)?;
            let result=Detail {card:row(c,&card,&nt,&stored.fields,None,None)?,note,field_names:nt.fields.iter().map(|f|f.name.clone()).collect(),sibling_ids,confirmation};
            Self::result_json(&result)?; Ok(result)
        })
    }
    fn delete_confirmation(&self,context:&Context,revision:&str,siblings:&[String]) -> Result<String> {
        let hash: [u8;32]=Sha256::digest(serde_json::to_vec(&(revision,siblings)).map_err(|_|unsupported())?).into();
        self.revision(context,&hash)
    }
    pub(crate) fn browse_delete<A:Authority>(&self,path:&Path,a:&A,input:Delete,runtime:&impl WriteRuntime) -> Result<()> {
        let id=input::id(&input.note_id)?;
        if input.revision.len()>128 || input.confirmation.len()>128 {return Err(invalid());}
        self.transaction(path,a,true,|c| {
            let stored=read::stored_note(c,id)?.ok_or_else(conflict)?;
            let nt=read::notetype(c,stored.mid)?;
            let note=self.note_result(a.context(),&stored,&nt)?;
            let ids=siblings(c,id)?;
            let token=self.delete_confirmation(a.context(),&note.revision,&ids)?;
            if !crate::crypto::same_digest(token.as_bytes(),input.confirmation.as_bytes()) || note.revision!=input.revision {return Err(conflict());}
            let stamps=write::Stamps::read(c,runtime.now_millis()?)?;
            for item in ids.iter().map(|s|input::id(s).map(|id|(id,0))).chain(std::iter::once(Ok((id,1)))) {
                let (oid,kind)=item?;
                c.execute("INSERT INTO graves(oid,type,usn) VALUES(?1,?2,?3) ON CONFLICT(oid,type) DO UPDATE SET usn=excluded.usn",params![oid,kind,stamps.usn]).map_err(sql_error)?;
            }
            c.execute("DELETE FROM cards WHERE nid=?1",[id]).map_err(sql_error)?;
            c.execute("DELETE FROM notes WHERE id=?1",[id]).map_err(sql_error)?;
            stamps.save(c)
        })
    }
}
