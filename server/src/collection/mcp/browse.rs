// SPDX-License-Identifier: MIT
//! Browser-only card reads. MCP's tools and search contract remain independent.
use super::*;
use regex::{Regex, RegexBuilder};
use super::browse_projection as projection;
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
    #[serde(skip_serializing_if="Option::is_none")]
    pub unavailable: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct CardPage {
    pub cards: Vec<Row>, pub next_cursor: Option<String>,
    pub examined_through: Option<String>,
}
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

// Only a letter (optionally after /) can begin a tag. Comparisons remain text.
fn tag_start(tail: &str) -> bool {
    let body = tail.strip_prefix("</").or_else(||tail.strip_prefix('<')).unwrap_or("");
    body.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
}

// Both success and malformed input consume the examined span. In particular an
// unterminated quoted tag never causes another scan of the remaining suffix.
fn tag_end(tail: &str) -> std::result::Result<usize, usize> {
    let mut quote = None;
    for (i, ch) in tail.char_indices().skip(1) {
        if let Some(q) = quote {
            if ch == q { quote = None; }
        } else {
            match ch {
                '\'' | '"' => quote = Some(ch),
                '>' => return Ok(i),
                '<' => return Err(i),
                _ => {},
            }
        }
    }
    Err(tail.len())
}

// A bounded text projection, never a card renderer. Decode entities after
// tokenization so escaped markup stays literal. Each source byte is scanned a
// constant number of times, including malformed markup and sound tokens.
pub(crate) fn visible_text(raw: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    let mut hidden: Vec<String> = Vec::new();
    let mut hidden_paragraph = None;
    while at < raw.len() {
        let tail = &raw[at..];
        if let Some(name) = hidden.last().filter(|name| matches!(name.as_str(), "script" | "style")) {
            // Raw text has no attribute quotes, comments or nested tags. Only
            // its own end tag matters, even after a comparison or lone quote.
            let length = name.len() + 2;
            let start = tail.as_bytes().windows(length).enumerate().find_map(|(i, bytes)| {
                (bytes.starts_with(b"</") && bytes[2..].eq_ignore_ascii_case(name.as_bytes())
                    && tail.as_bytes().get(i + length).is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>'))).then_some(i)
            });
            let Some(start) = start else { break; };
            let Some(end) = tail[start..].find('>') else { break; };
            at += start + end + 1;
            hidden.pop();
            continue;
        }
        if tail.starts_with("<!--") {
            if let Some(end) = tail.find("-->") { at += end + 3; continue; }
            if hidden.is_empty() { out.push_str(tail); }
            break;
        }
        if tag_start(tail) {
            let end = match tag_end(tail) {
                Ok(end) => end,
                Err(consumed) => {
                    if hidden.is_empty() { out.push_str(&tail[..consumed]); }
                    at += consumed;
                    continue;
                },
            };
            let tag = tail[1..end].to_ascii_lowercase();
            let closing = tag.starts_with('/');
            let body = tag.strip_prefix('/').unwrap_or(&tag);
            let name_end = body.find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | ':')).unwrap_or(body.len());
            let name = &body[..name_end];
            // A name must end at a real tag delimiter, not punctuation in text.
            if body[name_end..].starts_with(|ch: char| !ch.is_whitespace() && ch != '/') {
                if hidden.is_empty() { out.push_str(&tail[..end+1]); }
                at += end+1;
                continue;
            }
            // Starting a paragraph implicitly ends the previous paragraph and
            // its hidden descendants. Remember its index to avoid stack scans.
            if !closing && name == "p" {
                if let Some(index) = hidden_paragraph.take() { hidden.truncate(index); }
            }
            if closing {
                if hidden.last().is_some_and(|n| n == name) {
                    hidden.pop();
                    if name == "p" { hidden_paragraph = None; }
                }
            } else if !void_tag(name) && !tag.ends_with('/') {
                let hide = match hidden.last() {
                    None => raw_hidden(name) || hidden_attribute(&tag, name),
                    Some(parent) if raw_hidden(parent) => matches!(name, "script" | "style") || (name == "template" && parent == "template"),
                    Some(_) => true,
                };
                if hide {
                    if name == "p" { hidden_paragraph = Some(hidden.len()); }
                    hidden.push(name.into());
                }
            }
            if hidden.is_empty() && matches!(name, "br" | "div" | "p" | "li" | "tr" | "hr") { out.push(' '); }
            at += end+1;
            continue;
        }
        if tail.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("[sound:")) {
            let end = tail[7..].find(['[', ']']).map(|n|n+7).unwrap_or(tail.len());
            if tail.as_bytes().get(end) == Some(&b']') { at += end+1; continue; }
            if hidden.is_empty() { out.push_str(&tail[..end]); }
            at += end;
            continue;
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
            if m.start() >= boundaries[end] { break; }
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
struct Card { id:i64, nid:i64, did:i64, ord:i64 }
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
fn unavailable_row(card: &Card, message: &str) -> Row {
    Row { card_id:card.id.to_string(), note_id:card.nid.to_string(), title:"Content unavailable".into(),
        field:None, excerpt:vec![], deck:format!("Deck {}",card.did), template:format!("Template {}",card.ord),
        unavailable:Some(format!("Card {} · Note {}. {message}",card.id,card.nid)) }
}
fn row(c:&Connection, card:&Card, nt:&projection::NoteType, values:&[String], query:Option<&Regex>, field:Option<&str>) -> Result<Row> {
    if values.len()!=nt.fields.len() {return Err(projection::unavailable());}
    let matched = nt.fields.iter().zip(values).find(|(f,v)|field.is_none_or(|name|f.as_str()==name) && query.is_some_and(|q|q.is_match(v)));
    let selected = matched.or_else(||if query.is_none() {nt.fields.iter().zip(values).find(|(f,_)|field.is_none_or(|name|f.as_str()==name))} else {None});
    let deck=projection::deck(c,card.did)?;
    let template=if nt.kind==1 {nt.templates.first()} else {nt.templates.iter().find(|(ord,_)|i64::from(*ord)==card.ord)}.ok_or_else(projection::unavailable)?;
    Ok(Row {card_id:card.id.to_string(), note_id:card.nid.to_string(),
        title:short(values.get(nt.sort).ok_or_else(projection::unavailable)?,120),
        field:selected.map(|(f,_)|f.clone()), excerpt:selected.map_or_else(Vec::new,|(_,v)|excerpt(v,query)),
        deck:deck.name,template:projection::short(&template.1),unavailable:None})
}
enum CandidateProjection {
    BudgetReached,
    NoMatch,
    Matched(Row),
}

fn project_candidate(c: &Connection, card: &Card, input: &PageInput, query: Option<&Regex>, work: &mut usize) -> Result<CandidateProjection> {
    // Browse needs only bounded field text, never the MCP edit hash, tags,
    // guid, sfld or data. Large opaque note members do not block card listing.
    let (mid, bytes) = projection::note_head(c,card.nid)?;
    let nt = projection::notetype(c, mid)?;
    if input.field.as_ref().is_some_and(|name| !nt.fields.contains(name)) {
        return Ok(CandidateProjection::NoMatch);
    }
    if bytes <= MAX_PAGE_WORK_BYTES && *work > 0 && *work + bytes > MAX_PAGE_WORK_BYTES {
        return Ok(CandidateProjection::BudgetReached);
    }
    if bytes > MAX_PAGE_WORK_BYTES {
        // Include unchecked cards during search too. An unavailable projection
        // is explicit uncertainty, never evidence of an exhausted search.
        let placeholders = vec![String::new(); nt.fields.len()];
        let mut unavailable = row(c, card, &nt, &placeholders, None, None)?;
        unavailable.title = "Content unavailable".into();
        unavailable.field = None;
        unavailable.excerpt.clear();
        unavailable.unavailable = Some("This note exceeds the browser text limit. Its fields were not searched. Open it in a synced app.".into());
        return Ok(CandidateProjection::Matched(unavailable));
    }
    *work += bytes;
    let fields=projection::note_fields(c,card.nid)?;
    let values: Vec<_> = fields.split('\u{1f}').map(visible_text).collect();
    if values.len() != nt.fields.len() { return Err(projection::unavailable()); }
    let matches = nt.fields.iter().zip(&values).any(|(field, value)|
        input.field.as_ref().is_none_or(|name| field.as_str() == name)
        && query.is_none_or(|q| q.is_match(value)));
    if !matches { return Ok(CandidateProjection::NoMatch); }
    Ok(CandidateProjection::Matched(row(c, card, &nt, &values, query, input.field.as_deref())?))
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum DetailResult { Available(Detail), Unavailable { card:Row } }
#[derive(Serialize)]
pub(crate) struct Catalog { decks:Vec<CatalogDeck>, note_types:Vec<CatalogType>, next_cursor:Option<String> }
#[derive(Serialize)]
struct CatalogDeck { deck_id:String, name:String, filtered:bool, #[serde(skip_serializing_if="Option::is_none")] unavailable:Option<String> }
#[derive(Serialize)]
struct CatalogType { note_type_id:String, name:String, field_names:Vec<String>, #[serde(skip_serializing_if="Option::is_none")] unavailable:Option<String> }

impl Backend {
    pub(crate) fn browse_catalog<A:Authority>(&self,path:&Path,a:&A,input:CatalogInput) -> Result<Catalog> {
        input::bounded_input(&input)?;
        let limit=input::limit(input.limit)?; let kind=input.kind.unwrap_or_default();
        let shape=tokens::shape(&(kind,limit))?;
        let page=self.page(input.cursor.as_deref(),a.context(),"browse-catalog",&shape)?;
        let (mut phase,mut after)=page.map_or((if kind==input::CatalogKind::NoteType {1} else {0},0),|p|(p.kind,p.after));
        if (kind==input::CatalogKind::Deck && phase!=0) || (kind==input::CatalogKind::NoteType && phase!=1) {return Err(invalid());}
        self.transaction(path,a,false,|c| {
            let mut result=Catalog {decks:vec![],note_types:vec![],next_cursor:None};
            let mut count=0;
            loop {
                let table=if phase==0 {"decks"} else {"notetypes"};
                let id=c.query_row(&format!("SELECT id FROM {table} WHERE id>?1 ORDER BY id LIMIT 1"),[after],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?;
                let Some(id)=id else {
                    if phase==0 && kind==input::CatalogKind::All {phase=1;after=0;continue;} break;
                };
                if count==limit {result.next_cursor=Some(self.page_token(a.context(),"browse-catalog",&shape,phase,after)?);break;}
                if phase==0 {
                    let item=match projection::deck(c,id) {
                        Ok(d)=>CatalogDeck {deck_id:id.to_string(),name:d.name,filtered:d.filtered,unavailable:None},
                        Err(e) if projection::is_content_error(&e)=>CatalogDeck {deck_id:id.to_string(),name:format!("Deck {id} · Unavailable"),filtered:false,unavailable:Some("Deck metadata is unavailable.".into())},
                        Err(e)=>return Err(e),
                    };result.decks.push(item);
                } else {
                    let item=match projection::notetype(c,id) {
                        Ok(nt)=>CatalogType {note_type_id:id.to_string(),name:nt.name,field_names:nt.fields,unavailable:None},
                        Err(e) if projection::is_content_error(&e)=>CatalogType {note_type_id:id.to_string(),name:format!("Note type {id} · Unavailable"),field_names:vec![],unavailable:Some("Note type metadata is unavailable.".into())},
                        Err(e)=>return Err(e),
                    };result.note_types.push(item);
                }
                if size(&result)?+TOKEN_RESERVE>MAX_RESULT_BYTES {
                    if phase==0 {result.decks.pop();} else {result.note_types.pop();}
                    if count==0 {return Err(too_large());}
                    result.next_cursor=Some(self.page_token(a.context(),"browse-catalog",&shape,phase,after)?);break;
                }
                after=id;count+=1;
            } Ok(result)
        })
    }
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
            let mut result=CardPage {cards:vec![],next_cursor:None,examined_through:if after==0 {None} else {Some(after.to_string())}};
            let mut work=0;
            for _ in 0..MAX_CANDIDATES {
                // Integer primary-key traversal bounds sparse filters too; no
                // OFFSET, result-wide join/sort, or query-language expansion.
                let candidate = c.query_row(
                    "SELECT id,nid,did,ord,odid FROM cards WHERE id>?1 ORDER BY id LIMIT 1", [after],
                    |r| Ok((Card { id:r.get(0)?, nid:r.get(1)?, did:r.get(2)?, ord:r.get(3)? }, r.get::<_,i64>(4)?)),
                ).optional().map_err(sql_error)?;
                let Some((card,odid))=candidate else {return Ok(result);};
                let id=card.id;
                if deck.is_none_or(|d|d==if odid!=0 {odid}else{card.did}) {
                    let projected = match project_candidate(c,&card,&input,query.as_ref(),&mut work) {
                        Err(e) if projection::is_content_error(&e) => CandidateProjection::Matched(unavailable_row(&card,"Its content could not be inspected and was not searched. Open it in a synced app.")),
                        other => other?,
                    };
                    match projected {
                        CandidateProjection::BudgetReached => break,
                        CandidateProjection::NoMatch => {},
                        CandidateProjection::Matched(row) => {
                            result.cards.push(row);
                            if size(&result)?+TOKEN_RESERVE>MAX_RESULT_BYTES {
                                result.cards.pop();
                                if result.cards.is_empty(){return Err(too_large());}
                                break;
                            }
                        },
                    }
                }
                after=id;
                result.examined_through=Some(id.to_string());
                if result.cards.len()==limit {break;}
            }
            let more=c.query_row("SELECT id FROM cards WHERE id>?1 ORDER BY id LIMIT 1",[after],|r|r.get::<_,i64>(0)).optional().map_err(sql_error)?.is_some();
            if more {result.next_cursor=Some(self.page_token(a.context(),"browse-cards",&shape,0,after)?);}
            Ok(result)
        })
    }
    pub(crate) fn browse_detail<A:Authority>(&self,path:&Path,a:&A,input:DetailInput) -> Result<DetailResult> {
        let id=input::id(&input.card_id)?;
        self.transaction(path,a,false,|c| {
            let card=card(c,id)?;
            let detail = (|| {
                let (mid,bytes)=projection::note_head(c,card.nid)?;
                if bytes>MAX_RECORD_BYTES {return Err(too_large());}
                let labels=projection::notetype(c,mid)?;
                projection::deck(c,card.did)?;
                let _=projection::note_fields(c,card.nid)?;
                let stored=read::stored_note(c,card.nid)?.ok_or_else(projection::unavailable)?;
                let nt=read::notetype(c,stored.mid)?;
                let note=self.note_result(a.context(),&stored,&nt)?;
                let sibling_ids=siblings(c,card.nid)?;
                let confirmation=self.delete_confirmation(a.context(),&note.revision,&sibling_ids)?;
                let values:Vec<_>=stored.fields.split('\u{1f}').map(visible_text).collect();
                let result=Detail {card:row(c,&card,&labels,&values,None,None)?,note,field_names:nt.fields.iter().map(|f|f.name.clone()).collect(),sibling_ids,confirmation};
                Self::result_json(&result)?; Ok(result)
            })();
            match detail {
                Ok(detail) => Ok(DetailResult::Available(detail)),
                Err(e) if projection::is_content_error(&e) => Ok(DetailResult::Unavailable {card:unavailable_row(&card,"This content is unavailable for browser editing. Open it in a synced app.")}),
                Err(e) => Err(e),
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_comparisons_escaped_markup_accents_and_hidden_content() {
        let raw = "<p>3 < 5 and 7 > 6; café &amp; café.</p> \
            &lt;b&gt;literal&lt;/b&gt; &unknown; &eacute; \
            <span title='3 > 2'>shown</span><span hidden>secret</span> \
            <script>const x = '<audio>secret';</script>[sound:secret.mp3]";
        let text = visible_text(raw);
        assert_eq!(text, "3 < 5 and 7 > 6; café & café. <b>literal</b> &unknown; é shown");
        let query = RegexBuilder::new("CAFÉ").case_insensitive(true).build().unwrap();
        let fragments = excerpt(&text, Some(&query));
        assert_eq!(fragments.iter().filter(|part|part.matched).map(|part|part.text.as_str()).collect::<Vec<_>>(), ["café", "café"]);
        assert_eq!(fragments.iter().map(|part|part.text.as_str()).collect::<String>(), text);
    }

    #[test]
    fn malformed_tokens_have_safe_literal_fallback_without_suffix_rescans() {
        for raw in ["<a ".repeat(10_000), "[sound:unfinished ".repeat(10_000), "<a title='".repeat(10_000), "<!--unfinished".into()] {
            assert_eq!(visible_text(&raw), raw.trim_end());
        }
        assert_eq!(visible_text("x <a <b>café</b> y [sound:bad [sound:ok.mp3] done"), "x <a café y [sound:bad done");
        assert_eq!(visible_text("3 < 5 and 7 > 6; <42> <a,b>"), "3 < 5 and 7 > 6; <42> <a,b>");
    }

    #[test]
    fn raw_script_and_style_end_tags_survive_markup_comparisons_and_lone_quotes() {
        for name in ["script", "style"] {
            for body in ["if (a<b && c>d) secret", "a<b ' secret", "a<b \" secret", "<!-- <a title='secret", "</scripted></styled>secret"] {
                let raw = format!("before <{name}>{body}</{name}> visible");
                assert_eq!(visible_text(&raw), "before visible", "{raw}");
                let raw = format!("<head><{name}>{body}</{name}></head>visible");
                assert_eq!(visible_text(&raw), "visible", "{raw}");
            }
            let raw = format!("<{name}>{}</{} >visible", "a<b ' secret ".repeat(10_000), name.to_ascii_uppercase());
            assert_eq!(visible_text(&raw), "visible");
        }
    }

    #[test]
    fn new_paragraph_ends_hidden_paragraph_without_revealing_hidden_ancestors() {
        assert_eq!(visible_text("<p hidden>secret<p>visible</p>"), "visible");
        assert_eq!(visible_text("<p hidden><span>secret<p>visible</p>"), "visible");
        assert_eq!(visible_text("<div hidden><p>secret<p>also secret</p></div>visible"), "visible");
        let raw = format!("{}<p>visible</p>", "<p hidden><span>secret".repeat(10_000));
        assert_eq!(visible_text(&raw), "visible");
    }
}
