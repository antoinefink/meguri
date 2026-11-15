// SPDX-License-Identifier: MIT
use meguri_sync_server::collection::{model_adapter,unicase};
use meguri_model::{proto::{BlobMessage,deck_config::DeckConfigMessage},schema11};

#[test]
fn pinned_collation_vectors_include_three_character_folding() {
    let c=rusqlite::Connection::open_in_memory().unwrap();unicase::register(&c).unwrap();
    for (a,b,expected) in [("Straße","STRASSE",true),("ﬃ","iff",true),("ﬃ","ffi",false),("İ","i\u{307}",true),("Ꟁ","ꟁ",false)] {
        assert_eq!(c.query_row("SELECT ?1 = ?2 COLLATE unicase",[a,b],|r|r.get::<_,bool>(0)).unwrap(),expected);
    }
}
#[test]
fn unchanged_protobuf_keeps_unknown_and_noncanonical_bytes() {
    let bytes=[0x42,0,0x3a,0,0x48,0x14,0xfa,0x0f,2,0xff,0xff];
    let mut preset=model_adapter::blob::<DeckConfigMessage>(&bytes).unwrap();
    assert_eq!(preset.to_bytes().as_ref(),bytes);
    preset.new_per_day=25;
    let encoded=preset.to_bytes();
    assert!(encoded.windows(2).any(|s|s==[0x3a,0]));
    assert!(encoded.windows(2).any(|s|s==[0x42,0]));
    assert!(encoded.ends_with(&[0xfa,0x0f,2,0xff,0xff]));
    DeckConfigMessage::decode(&encoded).unwrap();
    assert!(model_adapter::blob::<DeckConfigMessage>(&[0x0a,5]).is_err());
}
#[test]
fn schema_json_keeps_wide_ids_unknown_values_and_filtered_deck_kind() {
    let input=r#"{"id":9007199254740993,"name":"synthetic","mod":5,"usn":0,"lrnToday":[0,0],"revToday":[0,0],"newToday":[0,0],"timeToday":[0,0],"collapsed":false,"dyn":1,"resched":true,"terms":[["tag:synthetic",20,0]],"unknown":{"wide":9223372036854775807}}"#;
    let deck=schema11::deck_from_json(input).unwrap();
    assert_eq!(deck.id.0,9007199254740993);
    let output=schema11::deck_to_json(&deck);
    assert!(output.contains("9007199254740993"));assert!(output.contains("9223372036854775807"));
    let round=schema11::deck_from_json(&output).unwrap();
    assert_eq!(round.kind.to_bytes(),deck.kind.to_bytes());
}
