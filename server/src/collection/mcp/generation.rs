// SPDX-License-Identifier: MIT
//! Bounded, non-rendering card analysis from G2–G8. Collection strings are data;
//! this module never evaluates HTML, JavaScript, filters, links, or resources.
use super::*;
use meguri_model::{notetype::Notetype, proto::deck::DeckKind};
use rusqlite::{OptionalExtension, params};
use std::{collections::BTreeSet, sync::LazyLock};
use regex::Regex;

const MAX_SIBLINGS: usize = 2048;
pub(super) struct Sibling { pub ord: u16, pub home: i64, pub new_position: Option<i64> }
pub(super) fn siblings(c: &Connection, id: i64) -> Result<Vec<Sibling>> {
    // A row LIMIT does not bound a table scan. Require the intended single-key
    // nid index, then force it so missing/partial/wrong indexes refuse safely.
    // Never repair uploaded schema in a request.
    let indexed: bool = c.query_row("SELECT count(*)=1 AND min(name)='nid' AND min(coll)='BINARY' FROM pragma_index_xinfo('ix_cards_nid') WHERE key=1", [], |r|r.get::<_,Option<bool>>(0))
        .map_err(sql_error)?.unwrap_or(false);
    if !indexed { return Err(unsupported()); }
    let mut q=c.prepare("SELECT ord,did,odid,type,due,odue FROM cards INDEXED BY ix_cards_nid WHERE nid=?1 ORDER BY id LIMIT ?2").map_err(sql_error)?;
    let sql_limit=i64::try_from(MAX_SIBLINGS+1).map_err(|_|too_large())?;
    let mut rows=q.query(params![id,sql_limit]).map_err(sql_error)?;
    let mut result=Vec::new();
    while let Some(r)=rows.next().map_err(sql_error)? {
        if result.len()==MAX_SIBLINGS { return Err(too_large()); }
        let did:i64=r.get(1).map_err(sql_error)?; let odid:i64=r.get(2).map_err(sql_error)?;
        let kind:i64=r.get(3).map_err(sql_error)?;
        // Scheduling columns of non-new cards stay opaque and unread. New
        // position values use the signed-32 storage contract (P2/P3).
        let position=if kind==0 {
            let due:i32=r.get(4).map_err(sql_error)?; let odue:i32=r.get(5).map_err(sql_error)?;
            Some(i64::from(if odue!=0 {odue} else {due}).max(0))
        } else { None };
        result.push(Sibling { ord:r.get(0).map_err(sql_error)?,home:if odid!=0 {odid} else {did},new_position:position });
    }
    Ok(result)
}
pub(super) fn validate_type(nt: &Notetype) -> Result<()> {
    if !matches!(nt.config.kind,0|1) || nt.templates.is_empty() || nt.fields.is_empty()
        || nt.templates.iter().any(|t|t.name.is_empty() || t.name.chars().all(|c|c=='"'))
        || nt.name.replace('"',"").is_empty() || nt.config.sort_field_idx as usize>=nt.fields.len() {
        return Err(unsupported());
    }
    Ok(())
}
fn empty(value: &str) -> bool {
    static EMPTY:LazyLock<Regex>=LazyLock::new(||Regex::new(r"\A(?:[\t\n\x0b\x0c\r ]|</?(?i:br|div) ?/?>)*\z").expect("E1 grammar"));
    EMPTY.is_match(value)
}
/// Strict T11.2 analysis with iterative sections, complete T6 validation and
/// T2/T3 comments/delimiters. Missing closing delimiters are literal text.
fn renders(front: &str, names: &BTreeSet<&str>) -> bool {
    let directive="{{=<% %>=}}";
    let trimmed=front.trim_start();
    let (text,open,close)=if trimmed.starts_with(directive) {
        (trimmed.trim_start_matches(directive),"<%","%>")
    } else { (front,"{{","}}") };
    let mut sections:Vec<(&str,bool)>=Vec::new();
    let mut active=true; let mut result=false; let mut at=0;
    let next_tag=|at:usize| -> Option<(usize,usize)> {
        let start=at+text[at..].find(open)?;
        let end=start+open.len()+text[start+open.len()..].find(close)?;
        Some((start,end))
    };
    let next_comment=|at:usize| -> Option<(usize,usize)> {
        let start=at+text[at..].find("<!--")?;
        let end=start+4+text[start+4..].find("-->")?;
        Some((start,end))
    };
    // Keep future candidates cached even when the other token kind is consumed.
    let mut tag=next_tag(at); let mut comment=next_comment(at);
    loop {
        let (start,end,is_comment)=match (tag,comment) {
            (None,None)=>break,
            (Some((s,e)),None)=>(s,e,false), (None,Some((s,e)))=>(s,e,true),
            (Some((ts,te)),Some((cs,ce)))=>if cs<ts {(cs,ce,true)} else {(ts,te,false)},
        };
        if is_comment { at=end+3; } else {
            let content=text[start+open.len()..end].trim_start_matches('{').trim();
            at=end+close.len();
            if content.len()>=2 && matches!(content.as_bytes()[0],b'#'|b'^'|b'/') {
                let key=content[1..].trim_start();
                match content.as_bytes()[0] {
                    b'/' => {
                        let Some((expected,parent))=sections.pop() else { return false; };
                        if expected!=key { return false; } active=parent;
                    },
                    marker => { sections.push((key,active));active=active && (names.contains(key) == (marker==b'#')); },
                }
            } else if active && names.contains(content.rsplit(':').next().unwrap_or("")) { result=true; }
        }
        if tag.is_some_and(|(start,_)|start<at) { tag=next_tag(at); }
        if comment.is_some_and(|(start,_)|start<at) { comment=next_comment(at); }
    }
    result && sections.is_empty()
}
fn cloze_ordinals(text: &str) -> BTreeSet<u16> {
    // Each frame holds finished descendants. An unfinished outer frame drops
    // all its descendants. The depth cap drops only the extra opening marker.
    let mut frames:Vec<BTreeSet<u16>>=Vec::new(); let mut result=BTreeSet::new();
    let bytes=text.as_bytes(); let mut at=0;
    while at<bytes.len() {
        if bytes[at..].starts_with(b"{{c") {
            let mut end=at+3;
            while end<bytes.len() && (bytes[end].is_ascii_digit() || bytes[end]==b',') { end+=1; }
            if bytes[end..].starts_with(b"::") {
                let numbers:BTreeSet<u16>=text[at+3..end].split(',').filter_map(|p|p.parse::<u16>().ok()).collect();
                if !numbers.is_empty() {
                    if frames.len()<10 { frames.push(numbers.into_iter().filter(|n|*n!=0).map(|n|(n-1).min(499)).collect()); }
                    at=end+2;continue;
                }
            }
        }
        if bytes[at..].starts_with(b"}}") {
            if let Some(finished)=frames.pop() {
                if let Some(parent)=frames.last_mut() { parent.extend(finished); } else { result.extend(finished); }
            }
            at+=2;continue;
        }
        at+=1;
    }
    result
}
fn required(nt: &Notetype, fields: &str, tags: &str) -> BTreeSet<u16> {
    if nt.config.kind==1 { return fields.split('\u{1f}').flat_map(cloze_ordinals).collect(); }
    let mut names:BTreeSet<&str>=nt.fields.iter().zip(fields.split('\u{1f}')).filter(|(_,value)|!empty(value)).map(|(f,_)|f.name.as_str()).collect();
    for special in ["Type","Deck","Subdeck","Card","CardFlag","CardID","Tags"] {
        if !nt.fields.iter().any(|f|f.name==special) && (special!="Tags" || !tags.trim().is_empty()) { names.insert(special); }
    }
    nt.templates.iter().filter(|t|renders(&t.config.q_format,&names)).map(|t|t.ord as u16).collect()
}
fn usable(c: &Connection, id: i64) -> Result<bool> {
    Ok(read::deck(c,id)?.is_some_and(|d|matches!(&d.kind.kind,DeckKind::Normal(_))))
}
fn random_deck(c: &Connection, id: i64) -> Result<bool> {
    let d=read::deck(c,id)?.ok_or_else(unsupported)?;
    let DeckKind::Normal(ref normal)=d.kind.kind else { return Err(unsupported()); };
    let requested=normal.config_id;
    // Existing typed preset decoder; bound its opaque bytes before decoding.
    for key in if requested==1 {vec![1]} else {vec![requested,1]} {
        let bytes:Option<i64>=c.query_row("SELECT octet_length(name)+octet_length(config) FROM deck_config WHERE id=?1",[key],|r|r.get(0)).optional().map_err(sql_error)?;
        let Some(bytes)=bytes else { continue; };
        let bytes=usize::try_from(bytes).map_err(|_|too_large())?;
        if bytes>MAX_OBJECT_BYTES { return Err(too_large()); }
        let mut q=c.prepare("SELECT id,name,mtime_secs,usn,config FROM deck_config WHERE id=?1").map_err(sql_error)?;
        let mut rows=q.query([key]).map_err(sql_error)?;
        let preset=super::super::objects::preset(rows.next().map_err(sql_error)?.ok_or_else(unsupported)?).map_err(|_|unsupported())?;
        return match preset.config.new_card_insert_order { 0=>Ok(false),1=>Ok(true),_=>Err(unsupported()) };
    }
    Err(unsupported())
}
#[allow(clippy::too_many_arguments)]
pub(super) fn generate(c: &Connection, id: i64, nt: &Notetype, fields: &str, tags: &str, chosen: Option<i64>, now: i64, usn: i32, runtime: &impl WriteRuntime) -> Result<()> {
    let siblings=siblings(c,id)?;
    let mut ordinals=required(nt,fields,tags);
    let fallback=siblings.is_empty() && ordinals.is_empty();
    if fallback { ordinals.insert(0); }
    ordinals.retain(|ord|!siblings.iter().any(|s|s.ord==*ord));
    if ordinals.is_empty() { return Ok(()); }
    if !usable(c,1)? { return Err(unsupported()); }
    let shared=siblings.first().filter(|first|siblings.iter().all(|s|s.home==first.home)).map(|s|s.home);
    let last=write::config(c,&format!("_nt_{}_lastDeck",nt.id.0))?.and_then(|v|v.as_i64());
    let sibling_position=siblings.iter().find_map(|s|s.new_position);
    let counter=if sibling_position.is_none() {
        Some(write::config(c,"nextPos")?.and_then(|v|v.as_u64()).and_then(|n|u32::try_from(n).ok()).unwrap_or(0))
    } else { None };
    let mut random=None;
    for ord in ordinals {
        let override_id=if nt.config.kind==0 && !fallback { nt.templates.get(ord as usize).map(|t|t.config.target_deck_id).filter(|id|*id>0) } else { None };
        let mut deck=None;
        let placement=if let Some(chosen)=chosen {vec![override_id,Some(chosen),Some(1)]} else {vec![override_id,shared,last,Some(1)]};
        for candidate in placement.into_iter().flatten() { if usable(c,candidate)? {deck=Some(candidate);break;} }
        let deck=deck.ok_or_else(unsupported)?;
        let position=if let Some(position)=sibling_position { position as u32 } else {
            let p=counter.ok_or_else(unsupported)?;
            if random_deck(c,deck)? {
                let value=match random {Some(value)=>value,None=>runtime.random_position(p)?};
                if value==0 || value>=p.max(1000) { return Err(unsupported()); }
                random=Some(value);value
            } else {p}
        };
        runtime.permit_new_card()?;
        let card=write::allocate(c,"cards",now)?;
        c.execute("INSERT INTO cards(id,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data) VALUES(?1,?2,?3,?4,?5,?6,0,0,?7,0,0,0,0,0,0,0,0,'{}')",
            params![card,id,deck,ord,now.div_euclid(1000),usn,position as i32]).map_err(sql_error)?;
    }
    if let Some(counter)=counter {
        let value=counter.wrapping_add(1).to_string().into_bytes();
        c.execute("INSERT INTO config(key,usn,mtime_secs,val) VALUES('nextPos',?1,?2,?3) ON CONFLICT(key) DO UPDATE SET usn=excluded.usn,mtime_secs=excluded.mtime_secs,val=excluded.val",
            params![usn,now.div_euclid(1000),value]).map_err(sql_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_nonrendering_analysis_vectors() {
        let names=BTreeSet::from(["A","Card"]);
        for (front,expected) in [("{{A}}",true),("{{#A}}static{{/A}}",false),("{{^B}}{{text:A}}{{/B}}",true),("{{^A}}{{A}}{{/A}}",false),("<!--{{A}}-->",false),("{{A}}{{#B}}",false),("{{=<% %>=}}<%A%>",true),("{{#A}}{{A}}{{/B}}",false),("{{A",false)] {
            assert_eq!(renders(front,&names),expected,"{front}");
        }
        assert_eq!(cloze_ordinals("{{c2::a {{c4::b}}}} {{c1,3::list}} {{c700::x}}"),BTreeSet::from([0,1,2,3,499]));
        assert!(cloze_ordinals("{{c2::a {{c4::b}}}").is_empty());
        assert!(cloze_ordinals("{{c0::label}} {{c65536::invalid}} {{c1::open").is_empty());
        for value in ["","<br />\n</DIV>","\t"] { assert!(empty(value)); }
        for value in ["&nbsp;","<span></span>","<br  >","\u{a0}"] { assert!(!empty(value)); }
    }
}
