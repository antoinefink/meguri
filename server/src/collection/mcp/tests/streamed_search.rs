// SPDX-License-Identifier: MIT
use super::*;
use std::{sync::Arc, time::{Duration, Instant}};

fn request() -> SearchInput {
    SearchInput { query:Some("[a.b]".into()), tag:Some("strasse".into()), ..Default::default() }
}
fn ids(page: &SearchResult) -> Vec<&str> {
    page.notes.iter().map(|n|n.note_id.as_str()).collect()
}
fn capped(f: &Fixture, input: SearchInput, limits: read::ScanLimits) -> Result<SearchResult> {
    f.backend.test_search_notes_with_scan_limits(&f.path,&f.access,input,limits)
}
fn limits() -> read::ScanLimits {
    read::ScanLimits { candidates:2, bytes:32*1024*1024, duration:Duration::from_secs(60) }
}
fn small_fixture() -> Fixture {
    let f=Fixture::new();
    f.create("ordinary",vec![],1000).unwrap();
    let tx=f.db.unchecked_transaction().unwrap();
    for (id,text,tags) in [(1001,"ordinary",""),(1002,"[a.b]"," Straße "),
        (1003,"[a.b]"," STRASSE "),(1004,"ordinary","")] {
        tx.execute("INSERT INTO notes SELECT ?1,printf('generated-%d',?1),mid,mod,usn,?2,?3,sfld,csum,flags,data FROM notes WHERE id=1000",
            params![id,tags,format!("{text}\u{1f}answer\u{1f}")]).unwrap();
    }
    tx.commit().unwrap();
    f
}

#[test]
fn cold_first_page_finds_late_literal_in_twelve_thousand_notes_and_absence_exhausts() {
    let f=Fixture::new();
    f.create(&"x".repeat(650),vec!["unrelated".into()],1000).unwrap();
    f.db.execute_batch("WITH RECURSIVE n(id) AS (VALUES(1001) UNION ALL SELECT id+1 FROM n WHERE id<13598)
        INSERT INTO notes SELECT n.id,printf('generated-%d',n.id),mid,mod,usn,tags,flds,sfld,csum,flags,data
        FROM n CROSS JOIN notes WHERE notes.id=1000;").unwrap();
    f.create("ΣSK [a.b] deck:1",vec!["Straße".into()],13599).unwrap();
    // Unrelated layouts must not be read by the streaming filter.
    f.db.execute("UPDATE notes SET mid=999 WHERE id<13599",[]).unwrap();
    let (count,bytes):(i64,i64)=f.db.query_row("SELECT count(*),sum(octet_length(flds)+octet_length(tags)) FROM notes",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(count,12_600);assert!((8_000_000..9_000_000).contains(&bytes));
    let stamp=f.stamp();
    for input in [
        SearchInput {query:Some("σsk [A.B] DECK:1".into()),..Default::default()},
        SearchInput {tag:Some("STRASSE".into()),..Default::default()}, request(),
    ] {
        // Real production limits, no warm posting and no timing assertion.
        let page=f.backend.search_notes(&f.path,&f.access,input).unwrap();
        assert_eq!(ids(&page),["13599"]);
        assert!(!page.scan_incomplete);assert!(page.next_cursor.is_none());
    }
    let page=f.backend.search_notes(&f.path,&f.access,SearchInput {
        query:Some("absent-generated-needle".into()),..Default::default()
    }).unwrap();
    assert!(page.notes.is_empty());assert!(page.next_cursor.is_none());assert!(!page.scan_incomplete);
    assert_eq!(f.stamp(),stamp);
    assert_eq!(f.access.admissions.get(),f.access.checks.get());
}

#[test]
fn capped_scan_reports_incomplete_and_continues_strictly_after_examined_ids() {
    let f=small_fixture();
    let first=capped(&f,request(),limits()).unwrap();
    assert!(first.notes.is_empty());assert!(first.scan_incomplete);
    let cursor=first.next_cursor.clone().expect("incomplete implies cursor");
    let wire:serde_json::Value=serde_json::from_str(&Backend::result_json(&first).unwrap()).unwrap();
    assert_eq!(wire,serde_json::json!({"notes":[],"next_cursor":cursor.clone(),"scan_incomplete":true}));
    // Newly matching earlier notes cannot be repeated by the continuation.
    f.db.execute("UPDATE notes SET tags=' strasse ',flds='[a.b]\u{1f}answer\u{1f}' WHERE id=1000",[]).unwrap();
    let second=capped(&f,SearchInput {cursor:first.next_cursor,..request()},limits()).unwrap();
    assert_eq!(ids(&second),["1002","1003"]);assert!(second.scan_incomplete);
    let last=capped(&f,SearchInput {cursor:second.next_cursor,..request()},limits()).unwrap();
    assert!(last.notes.is_empty());assert!(!last.scan_incomplete);assert!(last.next_cursor.is_none());
    let wire=serde_json::to_value(&last).unwrap();assert!(wire.get("scan_incomplete").is_none());
    let full=capped(&f,SearchInput {limit:Some(1),..request()},limits()).unwrap();
    assert_eq!(ids(&full),["1000"]);assert!(!full.scan_incomplete);assert!(full.next_cursor.is_some());
    assert!(serde_json::to_value(&full).unwrap().get("scan_incomplete").is_none());
    // Existing shape binding still applies to incomplete-page cursors.
    assert_eq!(capped(&f,SearchInput {cursor:Some(cursor),limit:Some(1),..request()},limits()).unwrap_err().code,"invalid_input");
}

#[test]
fn byte_and_deadline_stops_make_progress_even_when_only_one_row_fits() {
    let f=small_fixture();
    for tiny in [
        read::ScanLimits {candidates:100,bytes:1,..limits()},
        read::ScanLimits {candidates:100,duration:Duration::ZERO,..limits()},
    ] {
        let mut cursor=None;
        let mut seen=Vec::new();
        for n in 0..5 {
            let page=capped(&f,SearchInput {cursor,..request()},tiny).unwrap();
            assert_eq!(page.scan_incomplete,n<4);
            assert_eq!(page.next_cursor.is_some(),n<4);
            seen.extend(page.notes.into_iter().map(|n|n.note_id));cursor=page.next_cursor;
        }
        assert!(cursor.is_none());assert_eq!(seen,["1002","1003"]);
    }
}

#[test]
fn exhausted_posting_capacity_falls_back_for_text_tag_and_combined_filters() {
    let f=small_fixture();
    let index={
        let tx=f.db.unchecked_transaction().unwrap();
        Arc::new(SearchIndex::test_with_posting_capacity(&tx,Instant::now()+Duration::from_secs(30),1).unwrap())
    };
    for input in [SearchInput {query:Some("[a.b]".into()),..Default::default()},
        SearchInput {tag:Some("strasse".into()),..Default::default()},request()] {
        assert!(index.candidates(None,input.tag.as_deref(),input.query.as_deref()).is_none());
        let page=f.backend.search_notes_with_index(&f.path,&f.access,input,|_|Some(index.clone())).unwrap();
        assert_eq!(ids(&page),["1002","1003"]);assert!(!page.scan_incomplete);assert!(page.next_cursor.is_none());
    }
}

#[test]
fn streaming_preserves_per_field_literals_record_caps_and_authority_errors() {
    let f=small_fixture();
    // Neither a separator-spanning query nor a tag substring is a match.
    for input in [SearchInput {query:Some("[a.b]answer".into()),..Default::default()},
        SearchInput {tag:Some("stras".into()),..Default::default()}] {
        let page=f.backend.search_notes(&f.path,&f.access,input).unwrap();
        assert!(page.notes.is_empty());assert!(!page.scan_incomplete);assert!(page.next_cursor.is_none());
    }
    // An incomplete result is still subject to the final authority check.
    f.access.fail.set(true);
    assert_eq!(capped(&f,request(),limits()).unwrap_err().code,"permission_denied");
    f.access.fail.set(false);
    for column in ["flds","tags","data"] {
        let value=format!("\0{}","x".repeat(if column=="data" {MAX_OBJECT_BYTES} else {MAX_RECORD_BYTES}));
        f.db.execute(&format!("UPDATE notes SET {column}=?1 WHERE id=1000"),[value]).unwrap();
        assert_eq!(f.backend.search_notes(&f.path,&f.access,request()).unwrap_err().code,"result_too_large");
        f.db.execute("UPDATE notes SET flds='ordinary\u{1f}answer\u{1f}',tags='',data='' WHERE id=1000",[]).unwrap();
    }
    f.db.execute_batch("ALTER TABLE notes RENAME TO generated_missing_notes").unwrap();
    assert_eq!(capped(&f,request(),limits()).unwrap_err().code,"unsupported_collection");
}

#[test]
fn cold_scan_refuses_noninteger_note_type_id_in_nonmatching_note() {
    let f=small_fixture();
    f.db.execute("UPDATE notes SET mid='generated-invalid-mid' WHERE id=1000",[]).unwrap();
    let error=f.backend.search_notes(&f.path,&f.access,request()).unwrap_err();
    assert_eq!((error.code,error.mcp_detail),("unsupported_collection",Some(Detail::SqliteAccess)));
}

#[test]
fn streaming_byte_fit_retries_confirmed_match_without_incomplete_flag() {
    let f=Fixture::new();let text="\"".repeat(120000);
    for id in 1000..1003 {f.create(&text,vec![],id).unwrap();}
    let input=||SearchInput {query:Some("\"".into()),..Default::default()};
    let first=f.backend.search_notes(&f.path,&f.access,input()).unwrap();
    assert_eq!(ids(&first),["1000","1001"]);assert!(!first.scan_incomplete);
    assert!(Backend::result_json(&first).unwrap().len()<=MAX_RESULT_BYTES);
    let next=f.backend.search_notes(&f.path,&f.access,SearchInput {cursor:first.next_cursor,..input()}).unwrap();
    assert_eq!(ids(&next),["1002"]);assert!(!next.scan_incomplete);assert!(next.next_cursor.is_none());
    assert!(Backend::result_json(&next).unwrap().len()<=MAX_RESULT_BYTES);
    let exact=f.backend.search_notes(&f.path,&f.access,SearchInput {note_id:Some("1000".into()),..Default::default()}).unwrap();
    let wire=serde_json::to_value(exact).unwrap();
    assert_eq!(wire.as_object().unwrap().len(),2);assert!(wire["next_cursor"].is_null());
}
