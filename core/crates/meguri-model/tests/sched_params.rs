use meguri_model::proto::{BlobMessage, deck_config::DeckConfigMessage};

fn varint(mut value: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 128 });
        if value == 0 {
            return out;
        }
    }
}
fn with_other(other: &str) -> DeckConfigMessage {
    let mut bytes = vec![0xfa, 0x0f];
    bytes.extend(varint(other.len()));
    bytes.extend(other.as_bytes());
    DeckConfigMessage::decode(&bytes).unwrap()
}

#[test]
fn newer_json_parameter_markers_follow_exact_decoded_top_level_key_and_array_rules() {
    for (other, expected) in [
        (r#"{"fsrsParams7":[1]}"#, true),
        (r#"{"fsrsParams0007":[null]}"#, true),
        (r#"{"fsrsParams99999999999999999999999999999999999999":[[]]}"#, true),
        (r#"{"fsrsParams\u0037":[1]}"#, true),
        (r#"{"fsrsParams7":[1],"fsrsParams7":[]}"#, true),
        (r#"{"fsrsParams7":[],"fsrsParams6":[1]}"#, false),
        (r#"{"fsrsParams7":"[1]"}"#, false),
        (r#"{"fsrsParams7":null,"FSRSParams8":[1]}"#, false),
        (r#"{"nested":{"fsrsParams8":[1]}}"#, false),
        (
            r#"{"fsrsParams+7":[1],"fsrsParams７":[1],"fsrsParams7x":[1],"fsrsParams":[1]}"#,
            false,
        ),
        (r#"{"fsrsParams0000006":[1]}"#, false),
        (r#"[{"fsrsParams7":[1]}]"#, false),
        ("not JSON", false),
        (r#"{"fsrsParams7":[1e999]}"#, false),
    ] {
        let config = with_other(other);
        let before = config.encode();
        assert_eq!(config.has_later_fsrs_parameters(), expected, "{other}");
        assert_eq!(config.encode(), before);
    }
}

#[test]
fn newer_proto_parameter_markers_use_decoded_top_level_field_metadata_only() {
    for (bytes, expected) in [
        (vec![0x3a, 0], false),
        (vec![0x42, 0], false),
        (vec![0x3a, 4, 0, 0, 128, 63], true),
        (vec![0x38, 0], true),
        (vec![0x3b, 0x3c], true),
        (vec![0x43, 0x44], true),
        (vec![0x3a, 1, 0, 0x3a, 0], true),
        (vec![0xba, 0, 0x80, 0], false),
        (vec![0x9b, 0x06, 0x3a, 1, 0, 0x9c, 0x06], false),
    ] {
        let config = DeckConfigMessage::decode(&bytes).unwrap();
        assert_eq!(config.has_later_fsrs_parameters(), expected, "{bytes:x?}");
        assert_eq!(
            config
                .unknown_fields()
                .iter()
                .flat_map(|f| f.bytes())
                .copied()
                .collect::<Vec<_>>(),
            bytes
        );
    }
}
