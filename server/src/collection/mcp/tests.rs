// SPDX-License-Identifier: MIT
//! Disposable generated collections only. These verify the collection seam, not
//! the adapter's real publication barrier, OAuth binding, or mixed-peer safety.
use super::*;
use meguri_model::{proto::{ProtoBlob, deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck}, deck_config::DeckConfigMessage, notetype::{NotetypeConfig, FieldConfig, TemplateConfig}}};
use rusqlite::params;
use std::{cell::Cell, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[cfg(not(feature = "test-hooks"))]
#[path = "tests/size_scaling.rs"]
mod size_scaling;

struct Access { context: Context, fail: Cell<bool>, checks: Cell<usize>, admissions: Cell<usize> }
impl Access {
    fn new() -> Self { Self { context:Context { account:"generated-account".into(),publication:"generated-publication".into() },fail:Cell::new(false),checks:Cell::new(0),admissions:Cell::new(0) } }
}
impl Authority for Access {
    type Guard=();
    fn context(&self)->&Context { &self.context }
    fn admit(&self,_write:bool)->Result<()> { self.admissions.set(self.admissions.get()+1);Ok(()) }
    fn recheck(&self,_guard:&())->Result<()> { self.checks.set(self.checks.get()+1);if self.fail.get() {Err(Error::new("permission_denied"))} else {Ok(())} }
}
struct Runtime(i64);
impl WriteRuntime for Runtime {
    fn now_millis(&self)->Result<i64> { Ok(self.0) }
    fn guid_random_u64(&self)->Result<u64> { Ok(1700000000000) }
    // P4's published vector for p=1. Tests with due-order decks never call it.
    fn random_position(&self,p:u32)->Result<u32> { assert_eq!(p,1);Ok(825) }
}
#[test]
fn media_hook_authority_loss_rolls_back_create_and_edit_with_guard_still_held() {
    use std::rc::Rc;
    struct Held(Rc<Cell<bool>>);
    impl Drop for Held { fn drop(&mut self) { self.0.set(false); } }
    struct Guarded { access: Access, held: Rc<Cell<bool>> }
    impl Authority for Guarded {
        type Guard = Held;
        fn context(&self) -> &Context { &self.access.context }
        fn admit(&self, write: bool) -> Result<Held> {
            self.access.admit(write)?;
            assert!(!self.held.replace(true));
            Ok(Held(self.held.clone()))
        }
        fn recheck(&self, guard: &Held) -> Result<()> {
            assert!(Rc::ptr_eq(&self.held,&guard.0));
            assert!(self.held.get());
            self.access.recheck(&())
        }
    }
    for create in [true,false] {
        let f = Fixture::new();
        let original = f.create("original",vec![],1000).unwrap();
        let before = f.exact(&original.note_id);
        let stamps = f.stamp();
        let positions = write::config(&f.db,"nextPos").unwrap();
        let a = Guarded { access:Access::new(),held:Rc::new(Cell::new(false)) };
        let mut hook = || {
            assert!(a.held.get());
            assert_eq!(a.access.checks.get(),1);
            a.access.fail.set(true);
            Ok(())
        };
        let references = [("Back".into(),"[sound:meguri-generated.mp3]".into())];
        let result = if create {
            f.backend.create_note_with_media(&f.path,&a,CreateInput { deck_id:"1".into(),note_type_id:"10".into(),
                fields:Fields(BTreeMap::from([("Front".into(),"new".into())])),tags:Some(vec!["new-tag".into()]) },
                &Runtime(2000),&references,Some(&mut hook))
        } else {
            f.backend.edit_note_with_media(&f.path,&a,EditInput { note_id:original.note_id.clone(),revision:original.revision,
                fields:None,add_tags:Some(vec!["new-tag".into()]),remove_tags:None },
                &Runtime(2000),&references,Some(&mut hook))
        };
        assert_eq!(result.unwrap_err().code,"permission_denied");
        assert_eq!(a.access.checks.get(),2); assert!(!a.held.get());
        assert_eq!(f.exact(&original.note_id),before); assert_eq!(f.stamp(),stamps);
        assert_eq!(write::config(&f.db,"nextPos").unwrap(),positions);
        let counts:(i64,i64,i64) = f.db.query_row("SELECT (SELECT count(*) FROM notes),(SELECT count(*) FROM cards),(SELECT count(*) FROM tags)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(counts,(1,1,0));
    }
}
struct Fixture { _temp:tempfile::TempDir,path:PathBuf,db:rusqlite::Connection,backend:Backend,access:Access }
impl Fixture {
    fn new()->Self {
        let temp=tempfile::tempdir().unwrap();let path=temp.path().join("generated.anki2");
        std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
        super::super::schema::create_blank(&path,123,456).unwrap();
        let db=super::super::schema::open(&path,true).unwrap();
        let common=ProtoBlob::new(DeckCommon::default()).to_bytes().into_owned();
        let mut normal=NormalDeck::default();normal.config_id=1;
        let kind=ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))).to_bytes().into_owned();
        db.execute("INSERT INTO decks VALUES(1,'Generated',0,0,?1,?2)",params![common,kind]).unwrap();
        let preset=ProtoBlob::new(DeckConfigMessage::default()).to_bytes().into_owned();
        db.execute("INSERT INTO deck_config VALUES(1,'Generated preset',0,0,?1)",[preset]).unwrap();
        let config=ProtoBlob::new(NotetypeConfig::default()).to_bytes().into_owned();
        db.execute("INSERT INTO notetypes VALUES(10,'Generated type',0,0,?1)",[config]).unwrap();
        for (ord,name) in [(0,"Front"),(1,"Back"),(2,"Reverse")] {
            let config=ProtoBlob::new(FieldConfig::default()).to_bytes().into_owned();
            db.execute("INSERT INTO fields VALUES(10,?1,?2,?3)",params![ord,name,config]).unwrap();
        }
        for (ord,front) in [(0,"{{Front}}"),(1,"{{#Reverse}}{{Back}}{{/Reverse}} ")] {
            let mut template=TemplateConfig::default();template.q_format=front.into();
            let config=ProtoBlob::new(template).to_bytes().into_owned();
            db.execute("INSERT INTO templates VALUES(10,?1,?2,0,0,?3)",params![ord,format!("Generated {ord}"),config]).unwrap();
        }
        db.execute("INSERT INTO config VALUES('nextPos',0,0,?1)",[b"1".as_slice()]).unwrap();
        Self {_temp:temp,path,db,backend:Backend::new([7;32]),access:Access::new()}
    }
    fn create(&self,front:&str,tags:Vec<String>,time:i64)->Result<WriteResult> {
        self.backend.create_note(&self.path,&self.access,CreateInput {deck_id:"1".into(),note_type_id:"10".into(),fields:Fields(BTreeMap::from([("Front".into(),front.into()),("Back".into(),"generated answer".into())])),tags:Some(tags)},&Runtime(time))
    }
    fn exact(&self,id:&str)->NoteResult {
        self.backend.search_notes(&self.path,&self.access,SearchInput {note_id:Some(id.into()),..Default::default()}).unwrap().notes.remove(0)
    }
    fn edit(&self,id:&str,revision:&str,fields:&[(&str,&str)])->Result<WriteResult> {
        self.backend.edit_note(&self.path,&self.access,EditInput {note_id:id.into(),revision:revision.into(),fields:Some(Fields(fields.iter().map(|(k,v)|(k.to_string(),v.to_string())).collect())),add_tags:None,remove_tags:None},&Runtime(1000))
    }
    fn stamp(&self)->(i64,i32,i64,i64,i64) {
        self.db.query_row("SELECT mod,usn,crt,scm,ls FROM col WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap()
    }
}
#[test]
fn keyed_reads_use_existing_indexes_and_ignore_unrelated_invalid_objects() {
    let f=Fixture::new();let created=f.create("generated",vec![],9007199254740993).unwrap();
    // A validation scan or migration would reject this unrelated object.
    f.db.execute("INSERT INTO notetypes VALUES(99,'Unrelated',0,0,x'ff')",[]).unwrap();
    f.db.execute("CREATE TABLE generated_padding(payload BLOB)",[]).unwrap();
    f.db.execute("INSERT INTO generated_padding VALUES(zeroblob(20000000))",[]).unwrap();
    let before=f.stamp();let first=f.exact(&created.note_id);let second=f.exact(&created.note_id);
    assert_eq!(first,second);assert_eq!(first.note_id,"9007199254740993");assert_eq!(before,f.stamp());
    for sql in ["SELECT id FROM notes WHERE id=1000","SELECT id FROM notetypes WHERE id=10","SELECT name FROM fields WHERE ntid=10 ORDER BY ord","SELECT name FROM templates WHERE ntid=10 ORDER BY ord","SELECT ord FROM cards INDEXED BY ix_cards_nid WHERE nid=1000 ORDER BY id"] {
        let mut q=f.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let details=q.query_map([],|r|r.get::<_,String>(3)).unwrap().collect::<std::result::Result<Vec<_>,_>>().unwrap().join(" ");
        assert!(details.contains("SEARCH"),"{details}");assert!(!details.contains("SCAN"),"{details}");
    }
    assert_eq!(f.access.admissions.get(),f.access.checks.get());
}
#[test]
fn creation_and_patch_have_native_stamps_and_preserve_scheduling_and_omitted_bytes() {
    let f=Fixture::new();let first=f.create("cafe\u{301}\r\n",vec!["lang::Word".into(),"LANG".into()],1000).unwrap();
    assert_eq!(f.exact(&first.note_id).fields["Front"],"café\n");
    assert_eq!(f.exact(&first.note_id).tags,vec!["lang","lang::Word"]);
    assert_eq!(f.stamp(),(1000,1,123,456,0));
    assert_eq!(f.db.query_row("SELECT guid FROM notes",[],|r|r.get::<_,String>(0)).unwrap(),"c~MJPzD");
    // Existing cards keep every identity, scheduling and opaque-data value.
    f.db.execute("UPDATE cards SET queue=-1,due=17,reps=8,flags=3,data='opaque'",[]).unwrap();
    f.db.execute("UPDATE notes SET flags=27,data='opaque note',flds=?1",["café\n\u{1f}e\u{301}\0raw\u{1f}"]).unwrap();
    let loaded=f.exact(&first.note_id);
    let changed=f.edit(&first.note_id,&loaded.revision,&[("Reverse","yes")]).unwrap();
    assert_ne!(changed.revision,loaded.revision);
    let note=f.exact(&first.note_id);assert_eq!(note.fields["Back"],"e\u{301}\0raw");
    assert_eq!(f.stamp(),(1001,2,123,456,0));
    assert_eq!(f.db.query_row("SELECT count(*) FROM cards",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    let old:(i64,i64,i64,String)=f.db.query_row("SELECT due,reps,flags,data FROM cards WHERE ord=0",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();assert_eq!(old,(17,8,3,"opaque".into()));
    assert_eq!(f.db.query_row("SELECT due FROM cards WHERE ord=1",[],|r|r.get::<_,i64>(0)).unwrap(),17);
    let opaque:(i64,String)=f.db.query_row("SELECT flags,data FROM notes",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();assert_eq!(opaque,(27,"opaque note".into()));
    assert_eq!(f.db.query_row("SELECT count(*) FROM notes WHERE usn>=1",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(f.db.query_row("SELECT count(*) FROM cards WHERE usn>=1",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(super::super::schema::sync_stamp(&f.db,999).unwrap(),1001);
    assert_eq!(super::super::schema::sync_stamp(&f.db,1001).unwrap(),1001);
}
#[test]
fn same_second_revisions_stale_noops_layout_and_context_are_checked() {
    let mut f=Fixture::new();let first=f.create("one",vec![],1000).unwrap();
    let same=f.edit(&first.note_id,&first.revision,&[("Front","one")]).unwrap();assert_eq!(same.revision,first.revision);let stamp=f.stamp();
    let changed=f.edit(&first.note_id,&first.revision,&[("Front","two")]).unwrap();
    assert_ne!(changed.revision,first.revision);assert_eq!(f.stamp().0,stamp.0+1);
    assert_eq!(f.edit(&first.note_id,&first.revision,&[("Front","two")]).unwrap_err().code,"conflict");
    f.db.execute("UPDATE fields SET name='Prompt' WHERE ntid=10 AND ord=0",[]).unwrap();
    assert_eq!(f.edit(&first.note_id,&changed.revision,&[]).unwrap_err().code,"conflict");
    let current=f.exact(&first.note_id).revision;
    f.access.context.account="another-generated-account".into();
    assert_ne!(f.exact(&first.note_id).revision,current);
    assert_eq!(f.edit(&first.note_id,&current,&[]).unwrap_err().code,"conflict");
    f.access.context.publication="replacement".into();
    assert_eq!(f.edit(&first.note_id,&current,&[]).unwrap_err().code,"conflict");
}
#[test]
fn paging_advances_examined_candidates_and_never_skips_a_match_for_fit() {
    let f=Fixture::new();
    for i in 0..205 { f.create(if i==204 {"needle"} else {"other"},vec![],1000+i).unwrap(); }
    let first=f.backend.search_notes(&f.path,&f.access,SearchInput {query:Some("NEEDLE".into()),..Default::default()}).unwrap();
    assert!(first.notes.is_empty());assert!(first.next_cursor.is_some());
    let second=f.backend.search_notes(&f.path,&f.access,SearchInput {query:Some("NEEDLE".into()),cursor:first.next_cursor,..Default::default()}).unwrap();
    assert_eq!(second.notes.len(),1);assert!(second.next_cursor.is_none());
    // JSON escaping makes three of these records exceed a page's byte budget.
    let g=Fixture::new();let text="\"".repeat(120000);
    for i in 0..3 { g.create(&text,vec![],2000+i).unwrap(); }
    let mut cursor=None;let mut seen=Vec::new();
    loop {
        let page=g.backend.search_notes(&g.path,&g.access,SearchInput {deck_id:Some("1".into()),cursor,..Default::default()}).unwrap();
        assert!(Backend::result_json(&page).unwrap().len()<=MAX_RESULT_BYTES);
        seen.extend(page.notes.into_iter().map(|n|n.note_id));cursor=page.next_cursor;
        if cursor.is_none() {break;}
    }
    assert_eq!(seen,vec!["2000","2001","2002"]);
}
#[test]
fn typed_catalog_and_handles_are_bound_to_shape_and_publication() {
    let mut f=Fixture::new();
    let first=f.backend.list_decks_and_note_types(&f.path,&f.access,CatalogInput {limit:Some(1),..Default::default()}).unwrap();
    assert_eq!(first.decks[0].deck_id,"1");assert!(!first.decks[0].filtered);assert!(first.note_types.is_empty());
    let cursor=first.next_cursor.unwrap();
    let second=f.backend.list_decks_and_note_types(&f.path,&f.access,CatalogInput {limit:Some(1),cursor:Some(cursor.clone()),..Default::default()}).unwrap();
    assert_eq!(second.note_types[0].field_names,vec!["Front","Back","Reverse"]);assert!(second.next_cursor.is_none());
    assert_eq!(f.backend.list_decks_and_note_types(&f.path,&f.access,CatalogInput {limit:Some(2),cursor:Some(cursor.clone()),..Default::default()}).unwrap_err().code,"invalid_input");
    f.access.context.publication="replacement".into();
    assert_eq!(f.backend.list_decks_and_note_types(&f.path,&f.access,CatalogInput {limit:Some(1),cursor:Some(cursor),..Default::default()}).unwrap_err().code,"conflict");
}
#[test]
fn failed_final_authority_check_and_sql_failure_roll_back_all_effects() {
    let f=Fixture::new();let before=f.stamp();
    f.access.fail.set(true);
    assert_eq!(f.create("new",vec!["new-tag".into()],1000).unwrap_err().code,"permission_denied");
    assert_eq!(f.stamp(),before);
    for table in ["notes","cards","tags"] { assert_eq!(f.db.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),0); }
    assert_eq!(write::config(&f.db,"nextPos").unwrap().unwrap(),serde_json::json!(1));
    f.access.fail.set(false);
    // A real SQLite uniqueness failure occurs after note/tag effects began.
    f.db.execute_batch("CREATE UNIQUE INDEX generated_card_failure ON cards(nid)").unwrap();
    let mut template=TemplateConfig::default();template.q_format="{{Back}}".into();
    f.db.execute("UPDATE templates SET config=?1 WHERE ord=1",[ProtoBlob::new(template).to_bytes().into_owned()]).unwrap();
    assert!(f.create("new",vec!["new-tag".into()],1000).is_err());
    assert_eq!(f.stamp(),before);
    for table in ["notes","cards","tags"] { assert_eq!(f.db.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),0); }
}
#[test]
fn active_sqlite_writer_is_busy_without_retiring_or_waiting() {
    let f=Fixture::new();f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let start=std::time::Instant::now();
    let error=f.create("content",vec![],1000).unwrap_err();
    assert_eq!(error.code,"busy");assert_eq!(error.retry_after,Some(1));
    assert_eq!(error.mcp_detail,Some(Detail::SqliteAccessBusy));
    assert!(start.elapsed()<std::time::Duration::from_millis(250));
    assert!(!f.db.is_autocommit());f.db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(f.db.query_row("SELECT count(*) FROM notes",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}
#[test]
fn locked_schema_during_point_open_never_uses_default_busy_timeout() {
    let f=Fixture::new();
    f.db.pragma_update(None,"journal_mode","DELETE").unwrap();
    f.db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    for write in [false,true] {
        let start=std::time::Instant::now();
        let error=if write { f.create("content",vec![],1000).unwrap_err() } else {
            f.backend.search_notes(&f.path,&f.access,SearchInput {note_id:Some("1000".into()),..Default::default()}).unwrap_err()
        };
        assert_eq!(error.code,"busy");
        assert!(start.elapsed()<std::time::Duration::from_millis(250));
    }
    assert!(!f.db.is_autocommit());
    f.db.execute_batch("ROLLBACK").unwrap();
}
#[test]
fn point_write_leaves_checkpoint_work_outside_request_including_last_close() {
    let Fixture {_temp,path,db,backend,access}=Fixture::new();
    // Fixture setup leaves no other handle to hide a last-close checkpoint.
    db.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,true).unwrap();
    db.pragma_update(None,"wal_autocheckpoint",0).unwrap();
    drop(db);
    let main=std::fs::read(&path).unwrap();
    let c=super::super::schema::open_mcp(&path,true).unwrap();
    assert_eq!(c.pragma_query_value(None,"busy_timeout",|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(c.pragma_query_value(None,"wal_autocheckpoint",|r|r.get::<_,i64>(0)).unwrap(),0);
    drop(c);
    let result=backend.create_note(&path,&access,CreateInput {deck_id:"1".into(),note_type_id:"10".into(),fields:Fields(BTreeMap::from([("Front".into(),"generated".into())])),tags:None},&Runtime(1000)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(),main);
    let mut wal=path.as_os_str().to_os_string();wal.push("-wal");
    assert!(std::fs::metadata(Path::new(&wal)).unwrap().len()>0);
    let read=backend.search_notes(&path,&access,SearchInput {note_id:Some(result.note_id),..Default::default()}).unwrap();
    assert_eq!(read.notes[0].fields["Front"],"generated");
    assert_eq!(std::fs::read(&path).unwrap(),main);
}
#[test]
fn strict_inputs_size_limits_and_oversized_single_results() {
    for input in [r#"{"note_id":"1","note_id":"2"}"#,r#"{"query":"x","unknown":true}"#,r#"{"note_id":1}"#,r#"{"query":null}"#] {assert!(Backend::parse::<SearchInput>(input.as_bytes()).is_err());}
    assert!(Backend::parse::<CreateInput>(br#"{"deck_id":"1","note_type_id":"10","fields":{"Front":"x","Front":"y"}}"#).is_err());
    assert!(Backend::parse::<SearchInput>(&vec![b' ';MAX_REQUEST_BYTES+1]).is_err());
    let f=Fixture::new();let first=f.create("x",vec![],1000).unwrap();
    assert_eq!(f.backend.search_notes(&f.path,&f.access,SearchInput {note_id:Some(first.note_id.clone()),limit:Some(20),..Default::default()}).unwrap_err().code,"invalid_input");
    assert_eq!(f.backend.search_notes(&f.path,&f.access,SearchInput {note_id:Some("9223372036854775808".into()),..Default::default()}).unwrap_err().code,"invalid_input");
    f.db.execute("UPDATE notes SET flds=CAST(zeroblob(?1) AS TEXT)",[i64::try_from(MAX_RECORD_BYTES+1).unwrap()]).unwrap();
    assert_eq!(f.backend.search_notes(&f.path,&f.access,SearchInput {note_id:Some(first.note_id),..Default::default()}).unwrap_err().code,"result_too_large");
}
#[test]
fn canonical_tag_patches_are_exact_preserve_registry_and_refuse_overlap() {
    let f=Fixture::new();
    f.db.execute("INSERT INTO tags VALUES('German::Verb',0,0,x'0102')",[]).unwrap();
    let first=f.create("content",vec!["german::noun".into(),"a::child".into(),"a".into()],1000).unwrap();
    assert_eq!(f.exact(&first.note_id).tags,vec!["a","a::child","German::noun"]);
    let result=f.backend.edit_note(&f.path,&f.access,EditInput {note_id:first.note_id.clone(),revision:first.revision.clone(),fields:None,add_tags:Some(vec!["STRASSE".into()]),remove_tags:Some(vec!["a".into()])},&Runtime(1000)).unwrap();
    assert_eq!(f.exact(&first.note_id).tags,vec!["a::child","German::noun","STRASSE"]);
    let stamp=f.stamp();
    let failure=f.backend.edit_note(&f.path,&f.access,EditInput {note_id:first.note_id.clone(),revision:result.revision,fields:None,add_tags:Some(vec!["Straße".into()]),remove_tags:Some(vec!["STRASSE".into()])},&Runtime(1000)).unwrap_err();
    assert_eq!(failure.code,"invalid_input");assert_eq!(f.stamp(),stamp);
    let opaque:(i32,i32,Vec<u8>)=f.db.query_row("SELECT usn,collapsed,config FROM tags WHERE tag='German::Verb'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(opaque,(0,0,vec![1,2]));
}
#[test]
fn cloze_cards_fallback_random_positions_and_counter_wrap() {
    let f=Fixture::new();
    let mut cloze=NotetypeConfig::default();cloze.kind=1;
    let config=ProtoBlob::new(cloze).to_bytes().into_owned();
    f.db.execute("UPDATE notetypes SET config=?1 WHERE id=10",[config]).unwrap();
    let mut random_preset=DeckConfigMessage::default();random_preset.new_card_insert_order=1;
    let preset=ProtoBlob::new(random_preset).to_bytes().into_owned();
    f.db.execute("UPDATE deck_config SET config=?1",[preset]).unwrap();
    let first=f.create("{{c3::a {{c2::b}}}} {{c501::x}} {{c700::y}}",vec![],1000).unwrap();
    let mut q=f.db.prepare("SELECT ord,due,usn FROM cards ORDER BY ord").unwrap();
    let cards=q.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).unwrap().collect::<std::result::Result<Vec<_>,_>>().unwrap();
    assert_eq!(cards,vec![(1,825,0),(2,825,0),(499,825,0)]);
    let changed=f.edit(&first.note_id,&first.revision,&[("Front","no clozes")]).unwrap();
    assert_ne!(changed.revision,first.revision);
    assert_eq!(f.db.query_row("SELECT count(*) FROM cards",[],|r|r.get::<_,i64>(0)).unwrap(),3);
    let preset=ProtoBlob::new(DeckConfigMessage::default()).to_bytes().into_owned();
    f.db.execute("UPDATE deck_config SET config=?1",[preset]).unwrap();
    f.db.execute("UPDATE config SET val=?1 WHERE key='nextPos'",[b"4294967295".as_slice()]).unwrap();
    let fallback=f.create("plain",vec![],2000).unwrap();
    let card:(i64,i64)=f.db.query_row("SELECT ord,due FROM cards WHERE nid=?1",[fallback.note_id.parse::<i64>().unwrap()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(card,(0,-1));assert_eq!(write::config(&f.db,"nextPos").unwrap().unwrap(),serde_json::json!(0));
}
#[test]
fn stamp_overflow_invalid_fields_missing_notes_and_identity_cap_have_no_partial_effects() {
    let f=Fixture::new();let first=f.create("content",vec![],1000).unwrap();let stamp=f.stamp();
    assert_eq!(f.edit(&first.note_id,&first.revision,&[("Unknown","value")]).unwrap_err().code,"invalid_input");
    assert_eq!(f.edit("99",&first.revision,&[]).unwrap_err().code,"not_found");assert_eq!(f.stamp(),stamp);
    f.db.execute("UPDATE col SET usn=2147483647",[]).unwrap();let before=f.stamp();
    assert_eq!(f.edit(&first.note_id,&first.revision,&[("Front","changed")]).unwrap_err().code,"unsupported_collection");assert_eq!(f.stamp(),before);assert_eq!(f.exact(&first.note_id).fields["Front"],"content");
    f.db.execute("UPDATE col SET usn=1,mod=9223372036854775807",[]).unwrap();
    assert_eq!(f.create("new",vec![],1000).unwrap_err().code,"unsupported_collection");
    // I2 chooses a free current candidate despite a distant high ID. When the
    // candidate is occupied, I3 fills the first gap within its one-day cap.
    f.db.execute("INSERT INTO notes SELECT 999999999999,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data FROM notes WHERE id=1000",[]).unwrap();
    assert_eq!(write::allocate(&f.db,"notes",2000).unwrap(),2000);
    assert_eq!(write::allocate(&f.db,"notes",1000).unwrap(),1001);
}

#[test]
fn sibling_index_absence_refuses_deck_search_and_writes_without_repair() {
    let f=Fixture::new();let first=f.create("one",vec![],1000).unwrap();
    let note=f.exact(&first.note_id);let stamp=f.stamp();
    f.db.execute_batch("DROP INDEX ix_cards_nid").unwrap();
    let schema:i64=f.db.pragma_query_value(None,"schema_version",|r|r.get(0)).unwrap();
    let before:(i64,i64,i64)=f.db.query_row("SELECT (SELECT count(*) FROM notes),(SELECT count(*) FROM cards),(SELECT count(*) FROM tags)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(f.backend.search_notes(&f.path,&f.access,SearchInput {deck_id:Some("1".into()),..Default::default()}).unwrap_err().code,"unsupported_collection");
    assert_eq!(f.create("new",vec!["new-tag".into()],2000).unwrap_err().code,"unsupported_collection");
    assert_eq!(f.edit(&first.note_id,&first.revision,&[("Front","changed")]).unwrap_err().code,"unsupported_collection");
    assert_eq!(f.exact(&first.note_id),note);
    assert_eq!(f.edit(&first.note_id,&first.revision,&[]).unwrap().revision,first.revision);
    assert_eq!(f.stamp(),stamp);
    assert_eq!(write::config(&f.db,"nextPos").unwrap().unwrap(),serde_json::json!(2));
    let after:(i64,i64,i64)=f.db.query_row("SELECT (SELECT count(*) FROM notes),(SELECT count(*) FROM cards),(SELECT count(*) FROM tags)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(after,before);
    assert_eq!(f.db.pragma_query_value::<i64,_>(None,"schema_version",|r|r.get(0)).unwrap(),schema);
    // A same-named unrelated or partial index must not turn into an index scan.
    for sql in ["CREATE INDEX ix_cards_nid ON cards(did)","CREATE INDEX ix_cards_nid ON cards(nid) WHERE nid=999"] {
        f.db.execute_batch(sql).unwrap();
        assert_eq!(generation::siblings(&f.db,1000).err().unwrap().code,"unsupported_collection");
        f.db.execute_batch("DROP INDEX ix_cards_nid").unwrap();
    }
    f.db.execute_batch("DROP TABLE cards").unwrap();
    assert_eq!(f.exact(&first.note_id),note);
    assert_eq!(f.edit(&first.note_id,&first.revision,&[("Front","changed")]).unwrap_err().code,"unsupported_collection");
    assert_eq!(f.exact(&first.note_id),note);
}

#[test]
fn indexed_sibling_sample_keeps_its_row_bound() {
    let f=Fixture::new();f.create("one",vec![],1000).unwrap();
    f.db.execute_batch("WITH RECURSIVE n(id) AS (VALUES(2000) UNION ALL SELECT id+1 FROM n WHERE id<4046) INSERT INTO cards SELECT n.id,c.nid,c.did,c.ord,c.mod,c.usn,c.type,c.queue,c.due,c.ivl,c.factor,c.reps,c.lapses,c.left,c.odue,c.odid,c.flags,c.data FROM n CROSS JOIN cards c WHERE c.id=1000").unwrap();
    assert_eq!(generation::siblings(&f.db,1000).unwrap().len(),2048);
    f.db.execute_batch("INSERT INTO cards SELECT 5000,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data FROM cards WHERE id=1000").unwrap();
    assert_eq!(generation::siblings(&f.db,1000).err().unwrap().code,"result_too_large");
}

#[test]
fn omitted_tag_patches_preserve_legacy_bytes_registry_and_durable_noops() {
    for legacy in [" z a "," a a "," TAG tag Tag ","  Z\u{3000}a  a "] {
        let f=Fixture::new();let first=f.create("one",vec![],1000).unwrap();
        f.db.execute("UPDATE notes SET tags=?1",[legacy]).unwrap();
        f.db.execute("INSERT INTO tags VALUES('Tag',17,0,x'0102')",[]).unwrap();
        let original=f.exact(&first.note_id);
        let snapshot=|| {
            // Reopen to observe durable values, including opaque registry bytes.
            let db=super::super::schema::open(&f.path,false).unwrap();
            let note:(String,String,i64,i32)=db.query_row("SELECT tags,flds,mod,usn FROM notes WHERE id=1000",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
            let tags=db.prepare("SELECT tag,usn,collapsed,config FROM tags ORDER BY tag").unwrap().query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i32>(1)?,r.get::<_,i32>(2)?,r.get::<_,Option<Vec<u8>>>(3)?))).unwrap().collect::<std::result::Result<Vec<_>,_>>().unwrap();
            (note,tags,f.stamp(),write::config(&db,"nextPos").unwrap())
        };
        let before=snapshot();
        for fields in [&[][..],&[("Front","one")][..]] {
            let result=f.edit(&first.note_id,&original.revision,fields).unwrap();
            assert_eq!(result.revision,original.revision);
            assert_eq!(snapshot(),before,"{legacy:?}");
            assert_eq!(f.exact(&first.note_id),original);
        }
        // A real field edit also leaves omitted legacy tags and registry alone.
        f.edit(&first.note_id,&original.revision,&[("Front","changed")]).unwrap();
        let after=snapshot();assert_eq!(after.0.0,legacy);assert_eq!(after.1,before.1);
    }
}

#[test]
fn sqlite_diagnostics_distinguish_open_from_access_without_error_text() {
    for code in [rusqlite::ErrorCode::DatabaseBusy,rusqlite::ErrorCode::DatabaseLocked] {
        let raw=||rusqlite::Error::SqliteFailure(rusqlite::ffi::Error {
            code,extended_code:0,
        },Some("generated private SQLite/path/content".into()));
        let open=sql_open_error(raw());
        let access=sql_error(raw());
        let config=sql_open_storage_error(raw());
        assert_eq!((config.code,config.mcp_detail),("busy",Some(Detail::SqliteOpenBusy)));
        assert_eq!((open.code,open.mcp_detail),("busy",Some(Detail::SqliteOpenBusy)));
        assert_eq!((access.code,access.mcp_detail),("busy",Some(Detail::SqliteAccessBusy)));
    }
    let raw=||rusqlite::Error::InvalidQuery;
    assert_eq!(sql_open_error(raw()).mcp_detail,Some(Detail::SqliteOpen));
    assert_eq!(sql_error(raw()).mcp_detail,Some(Detail::SqliteAccess));
    assert_eq!(sql_open_error(raw()).code,"unsupported_collection");
    assert_eq!(sql_error(raw()).code,"unsupported_collection");
    let config=sql_open_storage_error(raw());
    assert_eq!((config.code,config.mcp_detail),("storage_unavailable",Some(Detail::SqliteOpen)));
}
