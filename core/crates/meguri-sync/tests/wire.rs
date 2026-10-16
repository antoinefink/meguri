//! The bodies of protocol v11 (specs/sync/wire-format.md): encoders and
//! decoders against the recorded goldens and recordings of the oracle
//! (`conformance/goldens/sync/wire/`, `conformance/recordings/scenarios/
//! sync/wire/`, W19), and the rules no official server exercises (loose
//! numbers, the eight-element review, the decks object form, the media
//! envelope and zips) from the spec's own tables, served by the scripted stub.
//!
//! Expected values come from the goldens, the recordings and the spec, never
//! from the code under test. JSON compares as parsed values (key order does
//! not count; number kinds do).

mod common;

use std::io::{Cursor, Read, Write};
use std::path::Path;

use common::{bytes, conformance_json, exchanges, json, served};
use meguri_model::config::ConfigEntry;
use meguri_model::ids::{CardId, DeckId, NoteId, RevlogId};
use meguri_model::note::Note;
use meguri_model::revlog::RevlogKind;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::StoredConfig;
use meguri_sync::wire::{
    self, Graves, MediaChange, MediaFile, MediaSanity, OutgoingChanges, OutgoingChunk, PendingIds, SanityAnswer,
    SanityCounts, UploadChange,
};
use meguri_sync::{DbCheckOrigin, ErrorKind, JobError, Method, client_identity};
use serde_json::{Value, json};

// --- Helpers -----------------------------------------------------------------

#[track_caller]
fn bad_response<T: std::fmt::Debug>(result: Result<T, JobError>, method: Method) -> JobError {
    let err = result.expect_err("the body does not decode");
    assert_eq!(err.kind, ErrorKind::BadResponse, "{err}");
    assert_eq!(err.method, Some(method), "{err}");
    err
}

/// The objects of a `legacy_json` golden (model-ops.md), by name.
fn golden_objects(scenario: &str, kind: &str) -> Vec<Value> {
    let golden = conformance_json(&format!("goldens/sync/wire/{scenario}.json"));
    let steps = golden["observations"]["steps"].as_array().unwrap();
    let observed = steps
        .iter()
        .find_map(|step| step.get("legacy_json"))
        .unwrap_or_else(|| panic!("{scenario}: no legacy_json observation"));
    observed[kind].as_object().unwrap().values().cloned().collect()
}

fn with_usn(object: &Value, usn: i64) -> Value {
    let mut object = object.clone();
    object["usn"] = json!(usn);
    object
}

/// A config row as the storage layer reads it: stored as a BLOB.
fn config_row(key: &str, value: &str) -> StoredConfig {
    StoredConfig {
        entry: ConfigEntry {
            key: key.to_owned(),
            usn: Usn(0),
            mtime: EpochSecs(0),
            value: value.as_bytes().to_vec(),
        },
        is_blob: true,
    }
}

/// Decoded changes, sent again by the client with `usn` (W6.2).
fn resend_changes(changes: &wire::IncomingChanges, usn: i32) -> Value {
    let config: Option<Vec<StoredConfig>> = changes
        .config
        .as_ref()
        .map(|rows| rows.iter().map(|(key, value)| config_row(key, value)).collect());
    let outgoing = OutgoingChanges {
        notetypes: &changes.notetypes,
        decks: &changes.decks,
        deck_configs: &changes.deck_configs,
        tags: &changes.tags,
        config: config.as_deref(),
        created: changes.created,
    };
    json(&wire::encode_apply_changes(&outgoing, Usn(usn)).unwrap())
}

/// The note rows of decoded chunk notes, as a sender holds them.
fn note_rows(chunk: &wire::IncomingChunk) -> Vec<Note> {
    chunk
        .notes
        .iter()
        .map(|note| Note {
            id: note.id,
            guid: note.guid.clone(),
            notetype_id: note.notetype_id,
            mtime: note.mtime,
            usn: note.usn,
            tags: note.tags.join(" "),
            fields: note.fields.clone(),
            ..Note::default()
        })
        .collect()
}

/// A decoded chunk, sent again with `usn`.
fn resend_chunk(chunk: &wire::IncomingChunk, usn: i32) -> Value {
    let notes = note_rows(chunk);
    let outgoing = OutgoingChunk {
        done: chunk.done,
        revlog: &chunk.revlog,
        cards: &chunk.cards,
        notes: &notes,
    };
    json(&wire::encode_apply_chunk(&outgoing, Usn(usn)))
}

/// Every `usn` member of the objects, or element of the tuples, in a value.
fn usns_of_changes(changes: &Value) -> Vec<i64> {
    let mut usns = Vec::new();
    for model in changes["models"].as_array().unwrap() {
        usns.push(model["usn"].as_i64().unwrap());
    }
    for list in changes["decks"].as_array().unwrap() {
        for object in list.as_array().unwrap() {
            usns.push(object["usn"].as_i64().unwrap());
        }
    }
    usns
}

// --- W6, W19: note types, decks and presets ------------------------------------

#[test]
fn the_object_goldens_decode_and_encode_as_applychanges_sends_them() {
    // W19: each golden object is what a sender puts in `applyChanges`, except
    // `usn`, which the sender replaces with the server's (W6.2).
    let notetypes = golden_objects("notetypes", "notetypes");
    let decks = golden_objects("decks", "decks");
    let presets = golden_objects("presets", "presets");
    assert_eq!((notetypes.len(), decks.len(), presets.len()), (3, 4, 2));
    let body = json!({"models": notetypes, "decks": [decks, presets], "tags": ["vocab"]});
    let changes = wire::decode_changes(&served(Method::ApplyChanges, &bytes(&body))).unwrap();
    assert_eq!(changes.notetypes.len(), 3);
    assert_eq!(changes.decks.len(), 4);
    assert_eq!(changes.deck_configs.len(), 2);
    assert_eq!(changes.tags, ["vocab"]);
    assert_eq!((changes.config.as_ref(), changes.created), (None, None));

    let sent = resend_changes(&changes, 7);
    let expected = json!({"changes": {
        "models": notetypes.iter().map(|o| with_usn(o, 7)).collect::<Vec<_>>(),
        "decks": [
            decks.iter().map(|o| with_usn(o, 7)).collect::<Vec<_>>(),
            presets.iter().map(|o| with_usn(o, 7)).collect::<Vec<_>>(),
        ],
        "tags": ["vocab"],
    }});
    assert_eq!(sent, expected);
}

#[test]
fn the_decks_object_form_reads_like_the_array_form_and_is_never_written() {
    // W6.3: Anki would also read `{"decks":[…],"config":[…]}`.
    let decks = golden_objects("decks", "decks");
    let presets = golden_objects("presets", "presets");
    let body = json!({"models": [], "decks": {"decks": decks, "config": presets}, "tags": []});
    let changes = wire::decode_changes(&served(Method::ApplyChanges, &bytes(&body))).unwrap();
    assert_eq!((changes.decks.len(), changes.deck_configs.len()), (4, 2));
    let sent = resend_changes(&changes, 3);
    assert!(sent["changes"]["decks"].is_array(), "{sent}");
    assert_eq!(sent["changes"]["decks"][0].as_array().unwrap().len(), 4);
    assert_eq!(sent["changes"]["decks"][1].as_array().unwrap().len(), 2);
}

#[test]
fn an_empty_changes_object_is_three_empty_lists() {
    // W6.1: `{"models":[],"decks":[[],[]],"tags":[]}`, nothing else.
    let none = OutgoingChanges {
        notetypes: &[],
        decks: &[],
        deck_configs: &[],
        tags: &[],
        config: None,
        created: None,
    };
    let sent = wire::encode_apply_changes(&none, Usn(4)).unwrap();
    assert_eq!(sent, br#"{"changes":{"models":[],"decks":[[],[]],"tags":[]}}"#);
}

#[test]
fn conf_and_crt_travel_only_when_sent_and_conf_keeps_each_value() {
    // W6.1: missing or null means "not sent".
    for body in [
        r#"{"models":[],"decks":[[],[]],"tags":[]}"#,
        r#"{"models":[],"decks":[[],[]],"tags":[],"conf":null,"crt":null}"#,
    ] {
        let changes = wire::decode_changes(body.as_bytes()).unwrap();
        assert_eq!((changes.config, changes.created), (None, None), "{body}");
    }
    // W6.4: any JSON value, a repeated key keeping its last value.
    let body =
        r#"{"models":[],"decks":[[],[]],"tags":[],"conf":{"a":1,"b":{"x":[1,2.5,"y"]},"a":true},"crt":1894680000}"#;
    let changes = wire::decode_changes(body.as_bytes()).unwrap();
    let config = changes.config.clone().unwrap();
    let keys: Vec<&str> = config.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["b", "a"]);
    assert_eq!(json(config[0].1.as_bytes()), json!({"x": [1, 2.5, "y"]}));
    assert_eq!(config[1].1, "true");
    assert_eq!(changes.created, Some(EpochSecs(1_894_680_000)));
    // Written as stored, without reformatting.
    let rows = [config_row("spaced", "{ \"k\" : [ 1 , 2 ] }"), config_row("n", "1.10")];
    let outgoing = OutgoingChanges {
        notetypes: &[],
        decks: &[],
        deck_configs: &[],
        tags: &[],
        config: Some(&rows),
        created: Some(EpochSecs(1_894_680_000)),
    };
    let sent = String::from_utf8(wire::encode_apply_changes(&outgoing, Usn(1)).unwrap()).unwrap();
    assert_eq!(
        sent,
        r#"{"changes":{"models":[],"decks":[[],[]],"tags":[],"conf":{"spaced":{ "k" : [ 1 , 2 ] },"n":1.10},"crt":1894680000}}"#
    );
}

#[test]
fn a_config_value_that_is_not_a_json_blob_cannot_be_sent() {
    // W6.4 and errors.md E7: db_check_required, origin local, before any
    // request.
    let mut text = config_row("curDeck", "1");
    text.is_blob = false;
    for row in [text, config_row("broken", "{\"a\":"), config_row("huge", "1e400")] {
        let rows = [row];
        let outgoing = OutgoingChanges {
            notetypes: &[],
            decks: &[],
            deck_configs: &[],
            tags: &[],
            config: Some(&rows),
            created: None,
        };
        let err = wire::encode_apply_changes(&outgoing, Usn(1)).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::DbCheckRequired {
                origin: DbCheckOrigin::Local
            },
            "{err}"
        );
        assert_eq!(err.method, None);
    }
}

#[test]
fn a_refused_object_refuses_the_whole_changes_body() {
    // W6.1: no partial set of objects; the detail names where, never the
    // body.
    let mut decks = golden_objects("decks", "decks");
    decks[1]["usn"] = json!("SECRET-TEXT");
    let body = json!({"models": [], "decks": [decks, []], "tags": []});
    let err = bad_response(wire::decode_changes(&bytes(&body)), Method::ApplyChanges);
    assert!(err.detail.starts_with("decks[0][1]"), "{err}");
    assert!(!err.detail.contains("SECRET"), "{err}");
    for body in [
        json!({"models": [], "decks": [[]], "tags": []}),
        json!({"models": [], "decks": [[], [], []], "tags": []}),
        json!({"models": {}, "decks": [[], []], "tags": []}),
        json!({"decks": [[], []], "tags": []}),
        json!({"models": [], "decks": [[], []], "tags": [1]}),
        json!({"models": [], "decks": [[], []], "tags": [], "crt": 1.5}),
        json!({"models": [], "decks": [[], []], "tags": [], "conf": []}),
    ] {
        bad_response(wire::decode_changes(&bytes(&body)), Method::ApplyChanges);
    }
}

// --- W4, W5: start and graves --------------------------------------------------

#[test]
fn start_sends_the_local_usn_newer_flag_and_null_graves() {
    // W4's example.
    assert_eq!(
        wire::encode_start(Usn(2), false),
        br#"{"minUsn":2,"lnewer":false,"graves":null}"#
    );
}

#[test]
fn graves_split_into_requests_of_250_cards_first_then_notes_then_decks() {
    // W5.2.
    assert!(Graves::default().into_chunks().is_empty());
    let graves = Graves {
        cards: (0..200).map(CardId).collect(),
        notes: (1000..1100).map(NoteId).collect(),
        decks: (5000..5010).map(DeckId).collect(),
    };
    let chunks = graves.into_chunks();
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        (chunks[0].cards.len(), chunks[0].notes.len(), chunks[0].decks.len()),
        (200, 50, 0)
    );
    assert_eq!(
        (chunks[1].cards.len(), chunks[1].notes.len(), chunks[1].decks.len()),
        (0, 50, 10)
    );
    assert_eq!(chunks[0].notes[0], NoteId(1000));
    assert_eq!(chunks[1].notes[0], NoteId(1050));
    let exact = Graves {
        cards: (0..250).map(CardId).collect(),
        ..Graves::default()
    };
    assert_eq!(exact.into_chunks().len(), 1);
}

// --- W7 to W11: chunks and tuples ----------------------------------------------

/// W8's example card tuple with element `index` replaced by `value`.
fn card_with(index: usize, value: &str) -> String {
    let mut items: Vec<String> = [
        "1894701600000",
        "1894701600000",
        "1894701600000",
        "0",
        "1894701780",
        "2",
        "1",
        "1",
        "1894702519",
        "0",
        "0",
        "1",
        "0",
        "1",
        "0",
        "0",
        "0",
        r#""{\"pos\":1}""#,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    items[index] = value.to_owned();
    format!("[{}]", items.join(","))
}

/// W10's example review tuple with element `index` replaced by `value`.
fn review_with(index: usize, value: &str) -> String {
    let mut items: Vec<&str> = vec![
        "1894701780000",
        "1894701600000",
        "2",
        "3",
        "-600",
        "0",
        "224",
        "4200",
        "0",
    ];
    items[index] = value;
    format!("[{}]", items.join(","))
}

fn chunk_of(list: &str, tuple: &str) -> wire::IncomingChunk {
    let body = format!(r#"{{"done":true,"{list}":[{tuple}]}}"#);
    wire::decode_chunk(&served(Method::Chunk, body.as_bytes())).unwrap_or_else(|err| panic!("{tuple}: {err}"))
}

#[test]
fn loose_integers_follow_w11s_table() {
    // (element, JSON, value), W11's table.
    for (index, text, expected) in [
        (8, "1.0", 1i64),
        (8, "1.9", 1),
        (8, "-0.5", 0),
        (8, "1e3", 1000),
        (8, "2147483648", -2_147_483_648),
        (8, "9223372036854775808", -1),
        (9, "-5", 0),
        (9, "4294967301", 5),
        (4, "1894701780.0", 1_894_701_780),
    ] {
        let card = &chunk_of("cards", &card_with(index, text)).cards[0];
        let value = match index {
            8 => i64::from(card.due),
            9 => i64::from(card.interval),
            _ => card.mtime.0,
        };
        assert_eq!(value, expected, "card element {index} = {text}");
    }
    let review = &chunk_of("revlog", &review_with(4, "-600")).revlog[0];
    assert_eq!(review.interval, -600);
    let review = &chunk_of("revlog", &review_with(6, "2500.0")).revlog[0];
    assert_eq!(review.ease_factor, 2500);
    // Anything but a number refuses the tuple.
    for text in [r#""1""#, "null", "true"] {
        let body = format!(r#"{{"cards":[{}]}}"#, card_with(8, text));
        bad_response(wire::decode_chunk(body.as_bytes()), Method::Chunk);
    }
}

#[test]
fn strict_elements_refuse_what_w8_to_w10_refuse() {
    for (list, tuple) in [
        ("cards", card_with(3, "1.0")),
        ("cards", card_with(5, "2.0")),
        ("cards", card_with(6, "4")),
        ("cards", card_with(7, "5")),
        ("cards", card_with(7, "-4")),
        ("cards", card_with(10, "65536")),
        ("cards", card_with(16, "256")),
        ("cards", card_with(17, "{}")),
        ("cards", card_with(0, r#""1894701600000""#)),
        // Seventeen elements.
        ("cards", card_with(17, r#""{}""#).replacen("1894701600000,", "", 1)),
        // A tuple written as an object.
        ("cards", r#"{"id":1}"#.to_owned()),
        // W9: `mod` has no loose reading, and elements 8 to 11 keep their types.
        (
            "notes",
            r#"[1,"g",2,1894701780.0,2,"","a\u001fb","","",0,""]"#.to_owned(),
        ),
        ("notes", r#"[1,"g",2,1894701780,2,"","a\u001fb",5,"",0,""]"#.to_owned()),
        (
            "notes",
            r#"[1,"g",2,1894701780,2,"","a\u001fb","","","0",""]"#.to_owned(),
        ),
        ("notes", r#"[1,"g",2,1894701780,2,"","a\u001fb","","",0]"#.to_owned()),
        ("revlog", review_with(3, "3.0")),
        ("revlog", r#"[1,2,3,4,5,6,7]"#.to_owned()),
        ("revlog", r#"[1,2,3,4,5,6,7,8,9,10]"#.to_owned()),
    ] {
        let body = format!(r#"{{"done":true,"{list}":[{tuple}]}}"#);
        bad_response(wire::decode_chunk(body.as_bytes()), Method::Chunk);
    }
}

#[test]
fn a_review_without_a_valid_type_reads_as_type_0() {
    // W10.1: eight elements, or a type that is not an integer from 0 to 5.
    let eight = "[1894701780000,1894701600000,2,3,-600,0,224,4200]";
    assert_eq!(chunk_of("revlog", eight).revlog[0].kind, RevlogKind::Learning);
    for text in [r#""1""#, "null", "7", "1.0", "-1", "{}"] {
        let review = &chunk_of("revlog", &review_with(8, text)).revlog[0];
        assert_eq!(review.kind, RevlogKind::Learning, "type {text}");
    }
    assert_eq!(
        chunk_of("revlog", &review_with(8, "5")).revlog[0].kind,
        RevlogKind::Rescheduled
    );
}

#[test]
fn the_chunk_object_defaults_and_writes_only_what_it_holds() {
    // W7.1: missing `done` is false, missing lists are empty; Meguri always
    // writes `done` and leaves empty lists out.
    let chunk = wire::decode_chunk(b"{}").unwrap();
    assert!(!chunk.done && chunk.revlog.is_empty() && chunk.cards.is_empty() && chunk.notes.is_empty());
    let empty = OutgoingChunk {
        done: true,
        revlog: &[],
        cards: &[],
        notes: &[],
    };
    assert_eq!(wire::encode_apply_chunk(&empty, Usn(3)), br#"{"chunk":{"done":true}}"#);
    for body in [r#"{"cards":null}"#, r#"{"done":1}"#, r#"{"notes":{}}"#, "[]"] {
        bad_response(wire::decode_chunk(body.as_bytes()), Method::Chunk);
    }
}

#[test]
fn a_sent_card_carries_its_rebuilt_data() {
    // W8.1: a stored "decay":0.1542 goes out as "decay":0.154.
    let tuple = card_with(
        17,
        r#""{\"pos\":1,\"s\":2.3065,\"d\":2.118,\"dr\":0.87,\"decay\":0.1542,\"lrt\":1894701780}""#,
    );
    let chunk = chunk_of("cards", &tuple);
    let sent = resend_chunk(&chunk, 9);
    assert_eq!(
        sent["chunk"]["cards"][0][17],
        json!(r#"{"pos":1,"s":2.3065,"d":2.118,"dr":0.87,"decay":0.154,"lrt":1894701780}"#)
    );
    assert_eq!(sent["chunk"]["cards"][0][5], json!(9));
}

#[test]
fn note_tags_split_at_both_spaces_and_go_out_padded() {
    // W9.1.
    assert_eq!(
        wire::note_tags("  jlpt::n5\u{3000}vocab  x "),
        ["jlpt::n5", "vocab", "x"]
    );
    assert!(wire::note_tags("").is_empty());
    let rows = [
        Note {
            id: NoteId(1),
            tags: " jlpt::n5\u{3000}vocab ".to_owned(),
            fields: "moon\u{1f}luna".to_owned(),
            ..Note::default()
        },
        Note {
            id: NoteId(2),
            ..Note::default()
        },
    ];
    let outgoing = OutgoingChunk {
        done: true,
        revlog: &[],
        cards: &[],
        notes: &rows,
    };
    let sent = String::from_utf8(wire::encode_apply_chunk(&outgoing, Usn(2))).unwrap();
    assert_eq!(
        sent,
        r#"{"chunk":{"done":true,"notes":[[1,"",0,0,2," jlpt::n5 vocab ","moon\u001fluna","","",0,""],[2,"",0,0,2,"","","","",0,""]]}}"#
    );
    let chunk = chunk_of("notes", r#"[1,"g",2,3,4," a　b  ","f","x","y",9,"z"]"#);
    assert_eq!(chunk.notes[0].tags, ["a", "b"]);
}

#[test]
fn chunks_take_one_review_note_and_card_in_turns_up_to_250() {
    // W7.2.
    let mut pending = PendingIds::new((0..300).map(RevlogId).collect(), vec![NoteId(1)], vec![]);
    let first = pending.next_chunk();
    assert_eq!((first.revlog.len(), first.notes.len(), first.cards.len()), (249, 1, 0));
    assert!(!first.done);
    let second = pending.next_chunk();
    assert_eq!((second.revlog.len(), second.notes.len()), (51, 0));
    assert!(second.done);

    let mut pending = PendingIds::new(
        (0..100).map(RevlogId).collect(),
        (0..100).map(NoteId).collect(),
        (0..100).map(CardId).collect(),
    );
    let first = pending.next_chunk();
    assert_eq!((first.revlog.len(), first.notes.len(), first.cards.len()), (84, 83, 83));
    assert_eq!(first.revlog[83], RevlogId(83));
    let second = pending.next_chunk();
    assert_eq!(
        (second.revlog.len(), second.notes.len(), second.cards.len()),
        (16, 17, 17)
    );
    assert!(second.done);

    // Objects that fill the chunks exactly leave an empty last chunk.
    let mut pending = PendingIds::new(vec![], vec![], (0..250).map(CardId).collect());
    assert!(!pending.next_chunk().done);
    let last = pending.next_chunk();
    assert!(last.done && last.is_empty());
    assert!(PendingIds::default().next_chunk().done);
}

// --- W12, W13: sanity check and finish -------------------------------------------

#[test]
fn the_sanity_check_sends_counts_and_reads_the_verdict() {
    // W12's example request.
    let counts = SanityCounts {
        cards: 4,
        notes: 3,
        revlog: 2,
        graves: 3,
        notetypes: 9,
        decks: 4,
        deck_configs: 2,
        ..SanityCounts::default()
    };
    assert_eq!(
        wire::encode_sanity_check(&counts),
        br#"{"client":[[0,0,0],4,3,2,3,9,4,2]}"#
    );
    let answer = wire::decode_sanity_answer(br#"{"status":"bad","c":[[0,0,0],4,3,2,0,9,4,2],"s":null}"#).unwrap();
    let client = SanityCounts { graves: 0, ..counts };
    assert_eq!(
        answer,
        SanityAnswer::Bad {
            client: Some(client),
            server: None
        }
    );
    // A `c` or `s` that is not a counts tuple counts as absent.
    let answer = wire::decode_sanity_answer(br#"{"status":"bad","c":[1,2],"s":[[0,0],1,2,3,4,5,6,7]}"#).unwrap();
    assert_eq!(
        answer,
        SanityAnswer::Bad {
            client: None,
            server: None
        }
    );
    for body in [
        r#"{"status":"OK"}"#,
        r#"{"status":1}"#,
        "{}",
        r#"{"status":"ok","status":"ok"}"#,
    ] {
        bad_response(wire::decode_sanity_answer(body.as_bytes()), Method::SanityCheck2);
    }
}

#[test]
fn finish_reads_a_strict_integer() {
    assert_eq!(
        wire::decode_finish(b"1894701780000").unwrap(),
        EpochMillis(1_894_701_780_000)
    );
    for body in ["1894701780000.0", "\"1\"", "null", ""] {
        bad_response(wire::decode_finish(body.as_bytes()), Method::Finish);
    }
}

#[test]
fn the_upload_answer_is_ok_or_the_servers_refusal() {
    // W14.
    wire::decode_upload_answer(b"OK").unwrap();
    let err = wire::decode_upload_answer(b"Collection too large").unwrap_err();
    assert_eq!(
        err.kind,
        ErrorKind::ServerMessage {
            text: "Collection too large".to_owned()
        }
    );
    assert_eq!(err.method, Some(Method::Upload));
}

// --- W3, W1.2: meta and the reading rules -------------------------------------------

#[test]
fn meta_asks_with_version_11_and_the_identity() {
    let request = json(&wire::encode_meta_request());
    assert_eq!(request, json!({"v": 11, "cv": client_identity()}));
    assert!(
        String::from_utf8(wire::encode_meta_request())
            .unwrap()
            .starts_with(r#"{"v":11,"cv":"meguri,"#)
    );
}

#[test]
fn meta_answers_follow_w3() {
    // W3's example.
    let example = br#"{"cont":true,"empty":false,"hostNum":0,"media_usn":0,"mod":1894701720000,"msg":"","scm":1894701660000,"ts":1894701780,"usn":2}"#;
    let meta = wire::decode_meta(example).unwrap();
    assert_eq!(meta.modified, EpochMillis(1_894_701_720_000));
    assert_eq!(meta.schema_modified, EpochMillis(1_894_701_660_000));
    assert_eq!(meta.usn, Usn(2));
    assert_eq!(meta.server_time, EpochSecs(1_894_701_780));
    assert_eq!(
        (meta.message.as_str(), meta.can_continue, meta.empty, meta.media_usn),
        ("", true, false, 0)
    );
    // `hostNum` and `empty` may be missing; unknown members are ignored.
    let meta = wire::decode_meta(
        br#"{"mod":1,"scm":2,"usn":3,"ts":4,"msg":"hi","cont":false,"media_usn":5,"extra":{"a":[1]}}"#,
    )
    .unwrap();
    assert!(!meta.empty && !meta.can_continue);
    assert_eq!(meta.message, "hi");
    for body in [
        r#"{"mod":1,"scm":2,"usn":3,"ts":4,"msg":"","cont":true}"#,
        r#"{"mod":1.0,"scm":2,"usn":3,"ts":4,"msg":"","cont":true,"media_usn":5}"#,
        r#"{"mod":1,"scm":2,"usn":3,"ts":4,"msg":null,"cont":true,"media_usn":5}"#,
        r#"{"mod":1,"scm":2,"usn":3,"ts":4,"msg":"","cont":true,"media_usn":5,"empty":null}"#,
        r#"{"mod":1,"scm":2,"usn":2147483648,"ts":4,"msg":"","cont":true,"media_usn":5}"#,
        r#"{"mod":1,"mod":1,"scm":2,"usn":3,"ts":4,"msg":"","cont":true,"media_usn":5}"#,
        r#"[1]"#,
        r#"{"mod":1,"scm":2,"usn":3,"ts":4,"msg":"\ud800","cont":true,"media_usn":5}"#,
        r#"{"mod":1,"scm":2,"usn":3,"ts":4,"msg":"","cont":true,"media_usn":5,"x":1e400}"#,
        "{\"mod\":1,",
    ] {
        bad_response(wire::decode_meta(body.as_bytes()), Method::Meta);
    }
    bad_response(wire::decode_meta(&[0xff, 0xfe]), Method::Meta);
}

#[test]
fn nesting_deeper_than_127_levels_refuses_the_whole_body() {
    // W1.2: the outermost value is level 1.
    let nested = |levels: usize| {
        format!(
            r#"{{"mod":1,"scm":2,"usn":3,"ts":4,"msg":"","cont":true,"media_usn":5,"x":{}{}}}"#,
            "[".repeat(levels),
            "]".repeat(levels)
        )
    };
    wire::decode_meta(nested(126).as_bytes()).unwrap();
    bad_response(wire::decode_meta(nested(127).as_bytes()), Method::Meta);
    // Far deeper input refuses without exhausting the stack.
    bad_response(wire::decode_meta(nested(1_000_000).as_bytes()), Method::Meta);
}

// --- W19: the recorded exchanges --------------------------------------------------

/// The decompressed body of a recorded request or answer, as bytes.
fn recorded(body: &Value) -> Vec<u8> {
    if let Some(value) = body.get("json") {
        bytes(value)
    } else if let Some(text) = body.get("text") {
        text.as_str().unwrap().as_bytes().to_vec()
    } else {
        panic!("not a JSON or text body: {body}")
    }
}

#[test]
fn the_recorded_exchanges_decode_and_the_client_side_encodes_as_recorded() {
    // Every request and answer body of the oracle playing both sides
    // (W19): decoders read each answer, and each request a client builds
    // comes out of Meguri's encoders as recorded.
    let mut covered = std::collections::BTreeSet::new();
    for scenario in ["sync/wire/records-down", "sync/wire/records-up"] {
        // The server's usn from each actor's latest `meta` (W6.2).
        let mut meta_usn = std::collections::HashMap::new();
        for exchange in exchanges(scenario) {
            let actor = exchange["actor"].as_str().unwrap().to_owned();
            let path = exchange["path"].as_str().unwrap();
            let request = &exchange["request"]["body"];
            let answer = &exchange["response"]["body"];
            let at = format!("{scenario} #{} {path}", exchange["seq"]);
            covered.insert(path.to_owned());
            match path {
                "/sync/hostKey" => {
                    let key = meguri_sync::decode_login_answer(&recorded(answer)).unwrap();
                    assert_eq!(key.as_str(), answer["json"]["key"].as_str().unwrap(), "{at}");
                }
                "/sync/upload" => wire::decode_upload_answer(&recorded(answer)).unwrap(),
                "/sync/download" => {}
                "/sync/meta" => {
                    let meta = wire::decode_meta(&recorded(answer)).unwrap();
                    let expected = &answer["json"];
                    assert_eq!(meta.modified.0, expected["mod"].as_i64().unwrap(), "{at}");
                    assert_eq!(meta.schema_modified.0, expected["scm"].as_i64().unwrap(), "{at}");
                    assert_eq!(i64::from(meta.usn.0), expected["usn"].as_i64().unwrap(), "{at}");
                    assert_eq!(meta.server_time.0, expected["ts"].as_i64().unwrap(), "{at}");
                    assert_eq!(
                        i64::from(meta.media_usn),
                        expected["media_usn"].as_i64().unwrap(),
                        "{at}"
                    );
                    assert_eq!(meta.empty, expected["empty"].as_bool().unwrap(), "{at}");
                    assert!(meta.can_continue, "{at}");
                    meta_usn.insert(actor, meta.usn.0);
                }
                "/sync/start" => {
                    let sent = wire::encode_start(
                        Usn(request["json"]["minUsn"].as_i64().unwrap() as i32),
                        request["json"]["lnewer"].as_bool().unwrap(),
                    );
                    assert_eq!(json(&sent), request["json"], "{at}");
                    let graves = wire::decode_graves(&recorded(answer)).unwrap();
                    assert_eq!(
                        json(&wire::encode_apply_graves(&graves)),
                        json!({"chunk": answer["json"]}),
                        "{at}"
                    );
                }
                "/sync/applyGraves" => {
                    let graves = wire::decode_graves(&bytes(&request["json"]["chunk"])).unwrap();
                    assert_eq!(json(&wire::encode_apply_graves(&graves)), request["json"], "{at}");
                    assert_eq!(answer["json"], Value::Null, "{at}");
                }
                "/sync/applyChanges" => {
                    let usn = meta_usn[&actor];
                    let sent = wire::decode_changes(&bytes(&request["json"]["changes"])).unwrap();
                    assert_eq!(resend_changes(&sent, usn), request["json"], "{at}");
                    // The answer's objects carry what the server stores.
                    let received = wire::decode_changes(&recorded(answer)).unwrap();
                    let usns = usns_of_changes(&answer["json"]);
                    let stored = usns.first().map_or(0, |usn| *usn as i32);
                    assert!(usns.iter().all(|usn| *usn == i64::from(stored)), "{at}");
                    assert_eq!(resend_changes(&received, stored)["changes"], answer["json"], "{at}");
                }
                "/sync/chunk" => {
                    let chunk = wire::decode_chunk(&recorded(answer)).unwrap();
                    let usns: Vec<i64> = ["cards", "revlog", "notes"]
                        .iter()
                        .flat_map(|list| {
                            let index = if *list == "notes" {
                                4
                            } else if *list == "cards" {
                                5
                            } else {
                                2
                            };
                            answer["json"][list]
                                .as_array()
                                .map(|tuples| tuples.iter().map(|t| t[index].as_i64().unwrap()).collect())
                                .unwrap_or_else(Vec::new)
                        })
                        .collect();
                    let stored = usns.first().map_or(0, |usn| *usn as i32);
                    assert_eq!(resend_chunk(&chunk, stored)["chunk"], answer["json"], "{at}");
                }
                "/sync/applyChunk" => {
                    let usn = meta_usn[&actor];
                    let chunk = wire::decode_chunk(&bytes(&request["json"]["chunk"])).unwrap();
                    assert_eq!(resend_chunk(&chunk, usn), request["json"], "{at}");
                }
                "/sync/sanityCheck2" => {
                    let client = &request["json"]["client"];
                    let count = |index: usize| client[index].as_u64().unwrap() as u32;
                    let counts = SanityCounts {
                        due: [0, 0, 0],
                        cards: count(1),
                        notes: count(2),
                        revlog: count(3),
                        graves: count(4),
                        notetypes: count(5),
                        decks: count(6),
                        deck_configs: count(7),
                    };
                    assert_eq!(json(&wire::encode_sanity_check(&counts)), request["json"], "{at}");
                    assert_eq!(
                        wire::decode_sanity_answer(&recorded(answer)).unwrap(),
                        SanityAnswer::Ok,
                        "{at}"
                    );
                }
                "/sync/finish" => {
                    let finished = wire::decode_finish(&recorded(answer)).unwrap();
                    assert_eq!(finished.0, answer["json"].as_i64().unwrap(), "{at}");
                }
                other => panic!("{at}: unexpected method {other}"),
            }
        }
    }
    for path in [
        "/sync/meta",
        "/sync/start",
        "/sync/applyGraves",
        "/sync/applyChanges",
        "/sync/chunk",
        "/sync/applyChunk",
        "/sync/sanityCheck2",
        "/sync/finish",
    ] {
        assert!(covered.contains(path), "the recordings hold no {path}");
    }
}

// --- W15 to W17: media ---------------------------------------------------------

#[test]
fn media_requests_carry_their_arguments() {
    // W16.
    assert_eq!(json(&wire::encode_begin()), json!({"v": client_identity()}));
    assert_eq!(wire::encode_media_changes(12), br#"{"lastUsn":12}"#);
    assert_eq!(
        wire::encode_download_files(&["sun.mp3".to_owned(), "moon.jpg".to_owned()]),
        br#"{"files":["sun.mp3","moon.jpg"]}"#
    );
    assert_eq!(wire::encode_media_sanity(7), br#"{"local":7}"#);
}

#[test]
fn media_answers_read_their_data() {
    // W16 with the official server's envelope, through the stub.
    let begin = served(Method::MediaBegin, br#"{"data":{"usn":0,"sk":"0123"},"err":""}"#);
    assert_eq!(wire::decode_begin(&begin).unwrap(), 0);
    let changes = served(
        Method::MediaChanges,
        br#"{"data":[["sun.mp3",3,"0123456789abcdef0123456789abcdef01234567"],["old.png",4,""]],"err":""}"#,
    );
    assert_eq!(
        wire::decode_media_changes(&changes).unwrap(),
        [
            MediaChange {
                name: "sun.mp3".to_owned(),
                usn: 3,
                sha1: "0123456789abcdef0123456789abcdef01234567".to_owned()
            },
            MediaChange {
                name: "old.png".to_owned(),
                usn: 4,
                sha1: String::new()
            },
        ]
    );
    assert_eq!(
        wire::decode_upload_changes(br#"{"data":[25,40],"err":""}"#).unwrap(),
        (25, 40)
    );
    assert_eq!(
        wire::decode_media_sanity(br#"{"data":"OK","err":""}"#).unwrap(),
        MediaSanity::Ok
    );
    assert_eq!(
        wire::decode_media_sanity(br#"{"data":"mediaSanity","err":""}"#).unwrap(),
        MediaSanity::Mismatch
    );
}

#[test]
fn the_media_envelope_follows_w15() {
    // 1. Data that reads wins whatever `err` holds.
    assert_eq!(
        wire::decode_upload_changes(br#"{"data":[1,2],"err":"ignored"}"#).unwrap(),
        (1, 2)
    );
    // 2. Otherwise a string `err` is the server's refusal, exactly as sent.
    for body in [
        r#"{"err":"media sync is disabled"}"#,
        r#"{"data":null,"err":"media sync is disabled"}"#,
        r#"{"data":[1],"err":"media sync is disabled"}"#,
    ] {
        let served = served(Method::UploadChanges, body.as_bytes());
        let err = wire::decode_upload_changes(&served).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::ServerMessage {
                text: "media sync is disabled".to_owned()
            },
            "{body}"
        );
        assert_eq!(err.method, Some(Method::UploadChanges));
    }
    let err = wire::decode_media_sanity(br#"{"data":"maybe","err":""}"#).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ServerMessage { text: String::new() });
    // 3. Otherwise the body does not decode.
    for body in [
        r#"{"data":[1]}"#,
        r#"{"err":5}"#,
        "{}",
        "[]",
        r#"{"data":[1,2],"data":[1,2],"err":""}"#,
    ] {
        bad_response(wire::decode_upload_changes(body.as_bytes()), Method::UploadChanges);
    }
    bad_response(wire::decode_begin(br#"{"data":{"sk":"x"}}"#), Method::MediaBegin);
    bad_response(
        wire::decode_media_changes(br#"{"data":[["a",1]]}"#),
        Method::MediaChanges,
    );
}

/// A zip as a server might send it: `(entry name, bytes, deflated)`.
fn zip_of(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data, deflated) in entries {
        let method = if *deflated {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        };
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A new, empty staging directory for decoded files.
fn staging() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// How many entries `dir` holds.
fn staged_count(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

/// A decoded file's real name and the bytes of its staged file.
fn name_and_bytes(file: &MediaFile) -> (String, Vec<u8>) {
    (file.name.clone(), std::fs::read(file.file.path()).unwrap())
}

/// Whether the staged file holds `len` zero bytes, read a MiB at a time.
fn holds_zeros(file: &MediaFile, len: usize) -> bool {
    let mut reader = std::fs::File::open(file.file.path()).unwrap();
    let mut piece = vec![0u8; MIB];
    let mut total = 0;
    loop {
        let read = reader.read(&mut piece).unwrap();
        if read == 0 {
            return total == len;
        }
        if piece[..read].iter().any(|&byte| byte != 0) {
            return false;
        }
        total += read;
    }
}

#[test]
fn a_download_zip_maps_its_entries_through_meta() {
    // W17.1: stored and deflated entries, `_meta` naming each.
    let moon = [7u8; 5000];
    let zip = zip_of(&[
        ("0", b"sun bytes".as_slice(), false),
        ("1", moon.as_slice(), true),
        (
            "_meta",
            br#"{"0":"sun.mp3","1":"moon.jpg","2":"gone.png"}"#.as_slice(),
            false,
        ),
    ]);
    let dir = staging();
    let files = wire::decode_download_zip(&served(Method::DownloadFiles, &zip), dir.path()).unwrap();
    assert_eq!(
        files.iter().map(name_and_bytes).collect::<Vec<_>>(),
        [
            ("sun.mp3".to_owned(), b"sun bytes".to_vec()),
            ("moon.jpg".to_owned(), vec![7u8; 5000]),
        ]
    );
    assert_eq!(files.iter().map(|file| file.file.size()).collect::<Vec<_>>(), [9, 5000]);
    // Each file is staged in the given directory, under a name that is
    // neither its entry's nor its real one.
    for file in &files {
        let path = file.file.path();
        assert_eq!(path.parent(), Some(dir.path()), "{}", path.display());
        let staged = path.file_name().unwrap().to_str().unwrap();
        assert!(!["0", "1", "sun.mp3", "moon.jpg"].contains(&staged), "{staged}");
    }
    assert_eq!(staged_count(dir.path()), 2);

    // A file entry `_meta` does not map, no `_meta`, a `_meta` that is not
    // an object of strings, or no zip at all refuse the whole body.
    let a = b"a".as_slice();
    for zip in [
        zip_of(&[
            ("0", a, false),
            ("9", a, false),
            ("_meta", br#"{"0":"a.mp3"}"#.as_slice(), false),
        ]),
        zip_of(&[("0", a, false)]),
        zip_of(&[("0", a, false), ("_meta", br#"{"0":1}"#.as_slice(), false)]),
        zip_of(&[("0", a, false), ("_meta", br#"["a.mp3"]"#.as_slice(), false)]),
        b"PK not a zip".to_vec(),
        Vec::new(),
    ] {
        bad_response(file_count(&zip), Method::DownloadFiles);
    }
}

#[test]
fn a_zip_refused_after_some_files_expanded_leaves_nothing_staged() {
    // E6: a failed decode applies nothing. Entry `1`'s bytes no longer match
    // their checksum, which shows only once entry `0` is staged.
    let mut zip = zip_of(&[
        ("0", b"sun bytes".as_slice(), false),
        ("1", b"moon bytes".as_slice(), false),
        ("_meta", br#"{"0":"sun.mp3","1":"moon.jpg"}"#.as_slice(), false),
    ]);
    let at = zip.windows(10).position(|window| window == b"moon bytes").unwrap();
    zip[at] = b'M';
    bad_response(file_count(&zip), Method::DownloadFiles);
}

#[test]
fn a_staged_file_goes_with_its_handle_unless_persisted() {
    let zip = zip_of(&[
        ("0", b"sun".as_slice(), false),
        ("1", b"moon".as_slice(), false),
        ("_meta", br#"{"0":"sun.mp3","1":"moon.jpg"}"#.as_slice(), false),
    ]);
    let dir = staging();
    let mut files = wire::decode_download_zip(&zip, dir.path()).unwrap();
    let moon = files.pop().unwrap();
    let sun = files.pop().unwrap();
    let sun_path = sun.file.path().to_owned();
    let media = staging();
    let kept = media.path().join("moon.jpg");
    moon.file.persist(&kept).unwrap();
    assert_eq!(std::fs::read(&kept).unwrap(), b"moon");
    assert!(sun_path.exists());
    drop(sun);
    assert!(!sun_path.exists());
    assert_eq!(staged_count(dir.path()), 0);
    assert_eq!(std::fs::read(&kept).unwrap(), b"moon");
}

#[test]
fn a_file_that_cannot_be_staged_is_a_local_failure() {
    // errors.md E7: the answer decodes; the device cannot store it.
    let zip = zip_of(&[
        ("0", b"sun".as_slice(), false),
        ("_meta", br#"{"0":"sun.mp3"}"#.as_slice(), false),
    ]);
    let dir = staging();
    let err = wire::decode_download_zip(&zip, &dir.path().join("missing")).unwrap_err();
    assert_eq!(err.kind, ErrorKind::LocalStorage, "{err}");
    assert_eq!(err.method, Some(Method::DownloadFiles), "{err}");
}

const MIB: usize = 1024 * 1024;

/// A download zip whose entry `0` holds `stored` as is, entry `1` `zeros`
/// zero bytes deflated (written a MiB at a time, so the test never holds
/// them), and `_meta` naming both.
fn padded_zero_zip(stored: &[u8], zeros: usize) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let as_is = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("0", as_is).unwrap();
    zip.write_all(stored).unwrap();
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("1", deflated).unwrap();
    let piece = vec![0u8; MIB];
    let mut left = zeros;
    while left > 0 {
        let size = left.min(MIB);
        zip.write_all(&piece[..size]).unwrap();
        left -= size;
    }
    zip.start_file("_meta", as_is).unwrap();
    zip.write_all(br#"{"0":"noise.bin","1":"silence.wav"}"#).unwrap();
    zip.finish().unwrap().into_inner()
}

/// How many files a download zip decodes to, into a new staging directory: a
/// failing test prints no file bytes. A zip that does not decode leaves the
/// directory empty.
fn file_count(zip: &[u8]) -> Result<usize, JobError> {
    let dir = staging();
    let result = wire::decode_download_zip(zip, dir.path()).map(|files| files.len());
    if result.is_err() {
        assert_eq!(staged_count(dir.path()), 0, "a refused zip left files staged");
    }
    result
}

/// Bytes deflate cannot shrink (xorshift).
fn incompressible(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

#[test]
fn a_zip_bomb_is_refused() {
    // errors.md E6: a decoder allocates only in proportion to the bytes
    // received. A GiB of zeros deflates to about a MiB.
    let bomb = padded_zero_zip(b"", 1024 * MIB);
    assert!(bomb.len() < 2 * MIB, "{}", bomb.len());
    bad_response(file_count(&bomb), Method::DownloadFiles);
}

#[test]
fn entries_that_expand_too_far_together_are_refused() {
    // E6 over the whole zip: 40 entries of 4 MiB of zeros, each about 4 KiB
    // deflated and each well under the file limit.
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let zeros = vec![0u8; 4 * MIB];
    let mut meta = serde_json::Map::new();
    for index in 0..40 {
        zip.start_file(index.to_string(), deflated).unwrap();
        zip.write_all(&zeros).unwrap();
        meta.insert(index.to_string(), json!(format!("{index}.wav")));
    }
    zip.start_file("_meta", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&bytes(&Value::Object(meta))).unwrap();
    let zip = zip.finish().unwrap().into_inner();
    assert!(zip.len() < MIB / 2, "{}", zip.len());
    bad_response(file_count(&zip), Method::DownloadFiles);

    // One such entry beside 64 KiB of stored bytes expands about 60 times:
    // a deflated file a server may well send.
    let noise = incompressible(64 * 1024);
    let dir = staging();
    let files = wire::decode_download_zip(&padded_zero_zip(&noise, 4 * MIB), dir.path()).unwrap();
    assert_eq!(name_and_bytes(&files[0]), ("noise.bin".to_owned(), noise));
    assert_eq!(files[1].name, "silence.wav");
    assert!(holds_zeros(&files[1], 4 * MIB), "{} bytes", files[1].file.size());
}

#[test]
fn a_downloaded_file_over_100_mib_is_refused_and_one_of_100_mib_is_not() {
    // W17.2: no server keeps a file over 100 MiB (R1565: Meguri never sends
    // one). 2 MiB of stored bytes keep the zip's expansion in proportion
    // (E6), so only the file's own size can refuse it.
    let noise = incompressible(2 * MIB);
    let dir = staging();
    let files = wire::decode_download_zip(&padded_zero_zip(&noise, 100 * MIB), dir.path()).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[1].file.size(), 100 * MIB as u64);
    assert!(holds_zeros(&files[1], 100 * MIB));
    drop(files);
    bad_response(
        file_count(&padded_zero_zip(&noise, 100 * MIB + 1)),
        Method::DownloadFiles,
    );
}

#[test]
fn a_padded_zip_under_the_expansion_ratio_is_refused_past_25_files_of_100_mib() {
    // R1852: whatever the ratio allows, one answer's files together stay
    // within 25 names (R1577) of at most 100 MiB each (W17.2). 25 deflated
    // entries of 100 MiB of zeros, beside 24 MiB of stored bytes that keep
    // the zip under 100 times its size: no entry over 100 MiB, the files
    // 24 MiB past the limit. It fails before any file is staged.
    let mut template = zip::ZipWriter::new(Cursor::new(Vec::new()));
    template
        .start_file(
            "zeros",
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    let piece = vec![0u8; MIB];
    for _ in 0..100 {
        template.write_all(&piece).unwrap();
    }
    let template = template.finish().unwrap().into_inner();
    let mut template = zip::ZipArchive::new(Cursor::new(template)).unwrap();

    let padding = incompressible(24 * MIB);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "0",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(&padding).unwrap();
    let mut meta = serde_json::Map::new();
    meta.insert("0".to_owned(), json!("padding.bin"));
    for index in 1..=25 {
        zip.raw_copy_file_rename(template.by_index(0).unwrap(), index.to_string())
            .unwrap();
        meta.insert(index.to_string(), json!(format!("{index}.wav")));
    }
    zip.start_file("_meta", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&bytes(&Value::Object(meta))).unwrap();
    let zip = zip.finish().unwrap().into_inner();
    let expanded = 24 * MIB + 25 * 100 * MIB;
    assert!(expanded < 100 * zip.len(), "{}", zip.len());

    bad_response(file_count(&zip), Method::DownloadFiles);
}

#[test]
fn an_upload_zip_names_additions_by_batch_position_and_lists_every_change() {
    // W17.2's example.
    let zip = wire::encode_upload_zip(&[
        UploadChange::Add {
            name: "sun.mp3",
            data: b"sun",
        },
        UploadChange::Delete { name: "old.png" },
        UploadChange::Add {
            name: "moon.jpg",
            data: b"moon",
        },
    ])
    .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        entries.push((entry.name().unwrap().into_owned(), data));
    }
    let meta = entries.iter().find(|(name, _)| name == "_meta").unwrap();
    assert_eq!(
        json(&meta.1),
        json!([["sun.mp3", "0"], ["old.png", null], ["moon.jpg", "2"]])
    );
    let mut files: Vec<_> = entries.iter().filter(|(name, _)| name != "_meta").cloned().collect();
    files.sort();
    assert_eq!(
        files,
        [("0".to_owned(), b"sun".to_vec()), ("2".to_owned(), b"moon".to_vec())]
    );
}
