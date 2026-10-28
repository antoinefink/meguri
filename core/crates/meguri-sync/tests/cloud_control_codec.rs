//! Independent K5.2/K5.3 wire vectors; no provider, storage or publication policy.
use meguri_sync::cloud::{
    control_codec::{
        DecodeOutcome, Epoch, EpochReason, EpochState, KindCount, MAX_CONTROL_BYTES, MediaRoot, epoch, media_root,
    },
    entity_codec::{CodecError, Kind},
};

// Literal protobuf bytes, not output from either codec. Epoch id is 00..0f,
// record key 20..3f, media key a0..bf. The two zero int64 fields are omitted.
const PUBLISHING: &str = "
    0801 1210 000102030405060708090a0b0c0d0e0f
    1a20 202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
    2812 3001 3801";
const COMPLETE: &str = "
    0801 1210 000102030405060708090a0b0c0d0e0f
    1a20 202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
    2812 3002 3801
    4a02 0801
    4a05 0809 108001";
const ROOT: &str = "
    0801 1220 a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf";

fn hex(literal: &str) -> Vec<u8> {
    let compact: String = literal.split_whitespace().collect();
    assert_eq!(compact.len() % 2, 0, "incomplete literal byte");
    compact
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("invalid literal byte"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
}
fn publishing() -> Epoch {
    Epoch {
        epoch_id: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        record_key: [
            32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58,
            59, 60, 61, 62, 63,
        ],
        crt: 0,
        ver: 18,
        state: EpochState::Publishing,
        reason: EpochReason::Initial,
        published_at_ms: 0,
        counts: Vec::new(),
    }
}
fn root() -> MediaRoot {
    MediaRoot {
        media_key: [
            160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180,
            181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191,
        ],
        created_at_ms: 0,
    }
}
fn complete() -> Epoch {
    Epoch {
        state: EpochState::Complete,
        counts: vec![
            KindCount {
                kind: Kind::Note,
                live: 0,
            },
            KindCount {
                kind: Kind::MediaName,
                live: 128,
            },
        ],
        ..publishing()
    }
}

#[test]
fn literal_publishing_complete_and_media_root() {
    let publishing = publishing();
    assert_eq!(publishing.encode().unwrap(), hex(PUBLISHING));
    assert_eq!(
        Epoch::decode(&hex(PUBLISHING)),
        Ok(DecodeOutcome::Supported(publishing))
    );
    let complete = complete();
    assert_eq!(complete.encode().unwrap(), hex(COMPLETE));
    assert_eq!(Epoch::decode(&hex(COMPLETE)), Ok(DecodeOutcome::Supported(complete)));
    let root = root();
    assert_eq!(root.encode().unwrap(), hex(ROOT));
    assert_eq!(MediaRoot::decode(&hex(ROOT)), Ok(DecodeOutcome::Supported(root)));
}

#[test]
fn all_reason_codes_have_independent_literal_values() {
    for (reason, literal) in [
        (EpochReason::Initial, "3801"),
        (EpochReason::ReplaceCloud, "3802"),
        (EpochReason::Restore, "3803"),
        (EpochReason::CollectionPackageImport, "3804"),
        (EpochReason::ManualAnkiImport, "3805"),
    ] {
        let value = Epoch { reason, ..publishing() };
        let mut expected = hex(PUBLISHING);
        expected.truncate(58);
        expected.extend(hex(literal));
        assert_eq!(value.encode().unwrap(), expected);
        assert_eq!(Epoch::decode(&expected), Ok(DecodeOutcome::Supported(value)));
    }
}

#[test]
fn signed_boundaries_use_int64_not_zigzag_and_zero_is_omitted() {
    // Each literal tail independently spells both int64 fields and the intervening
    // required schema/state/reason. Negative int64 is a ten-byte unsigned varint.
    for (value, epoch_tail, root_tail) in [
        (0, "2812 3001 3801", ""),
        (1, "2001 2812 3001 3801 4001", "1801"),
        (
            -1,
            "20ffffffffffffffffff01 2812 3001 3801 40ffffffffffffffffff01",
            "18ffffffffffffffffff01",
        ),
        (
            i64::MIN,
            "2080808080808080808001 2812 3001 3801 4080808080808080808001",
            "1880808080808080808001",
        ),
        (
            i64::MAX,
            "20ffffffffffffffff7f 2812 3001 3801 40ffffffffffffffff7f",
            "18ffffffffffffffff7f",
        ),
    ] {
        let value_epoch = Epoch {
            crt: value,
            published_at_ms: value,
            ..publishing()
        };
        let mut expected = hex(PUBLISHING);
        expected.truncate(54);
        expected.extend(hex(epoch_tail));
        assert_eq!(value_epoch.encode().unwrap(), expected);
        assert_eq!(Epoch::decode(&expected), Ok(DecodeOutcome::Supported(value_epoch)));
        let value_root = MediaRoot {
            created_at_ms: value,
            ..root()
        };
        let mut expected = hex(ROOT);
        expected.extend(hex(root_tail));
        assert_eq!(value_root.encode().unwrap(), expected);
        assert_eq!(MediaRoot::decode(&expected), Ok(DecodeOutcome::Supported(value_root)));
    }
}

#[test]
fn counts_use_kind_domain_order_and_canonical_zero_and_max_live() {
    let mut expected = hex(PUBLISHING);
    expected[57] = 2;
    // Complete with no entries is representable; there is no all-nine wire rule.
    let mut value = Epoch {
        state: EpochState::Complete,
        ..publishing()
    };
    assert_eq!(value.encode().unwrap(), expected);
    assert_eq!(Epoch::decode(&expected), Ok(DecodeOutcome::Supported(value.clone())));
    for (kind, literal) in [
        (Kind::Note, "4a020801"),
        (Kind::Card, "4a020802"),
        (Kind::Revlog, "4a020803"),
        (Kind::Deck, "4a020804"),
        (Kind::Preset, "4a020805"),
        (Kind::Notetype, "4a020806"),
        (Kind::Tag, "4a020807"),
        (Kind::Config, "4a020808"),
        (Kind::MediaName, "4a020809"),
    ] {
        value.counts.push(KindCount { kind, live: 0 });
        expected.extend(hex(literal));
    }
    assert_eq!(value.encode().unwrap(), expected);
    assert_eq!(Epoch::decode(&expected), Ok(DecodeOutcome::Supported(value)));

    let value = Epoch {
        state: EpochState::Complete,
        counts: vec![KindCount {
            kind: Kind::Card,
            live: u64::MAX,
        }],
        ..publishing()
    };
    let mut expected = hex(PUBLISHING);
    expected[57] = 2;
    expected.extend(hex("4a0d080210ffffffffffffffffff01"));
    assert_eq!(value.encode().unwrap(), expected);
    assert_eq!(Epoch::decode(&expected), Ok(DecodeOutcome::Supported(value)));
}

#[test]
fn wrappers_keep_exact_initial_complete_and_media_bytes() {
    let e = complete();
    assert_eq!(
        epoch(
            e.epoch_id,
            e.record_key,
            0,
            0,
            &[(Kind::Note, 0), (Kind::MediaName, 128)]
        )
        .unwrap(),
        hex(COMPLETE)
    );
    let mut empty = hex(PUBLISHING);
    empty[57] = 2;
    assert_eq!(epoch(e.epoch_id, e.record_key, 0, 0, &[]).unwrap(), empty);
    assert_eq!(media_root(root().media_key, 0).unwrap(), hex(ROOT));
    let mut signed = hex(PUBLISHING);
    signed.truncate(54);
    signed.extend(hex("20ffffffffffffffffff01 2812 3002 3801 4001"));
    assert_eq!(epoch(e.epoch_id, e.record_key, -1, 1, &[]).unwrap(), signed);
    let mut timed_root = hex(ROOT);
    timed_root.extend(hex("18ffffffffffffffffff01"));
    assert_eq!(media_root(root().media_key, -1).unwrap(), timed_root);
}

#[test]
fn fixed_lengths_and_required_nondefault_fields_are_enforced() {
    // Alter both length and actual content, so these are well-formed protobufs.
    for (len_at, start, length) in [(3, 4, 16_usize), (21, 22, 32)] {
        for delta in [-1_i32, 1] {
            let mut bytes = hex(PUBLISHING);
            bytes[len_at] = (length as i32 + delta) as u8;
            if delta < 0 {
                bytes.remove(start);
            } else {
                bytes.insert(start, 0);
            }
            assert_eq!(Epoch::decode(&bytes), Err(CodecError::InvalidLength));
        }
    }
    for delta in [-1_i32, 1] {
        let mut bytes = hex(ROOT);
        bytes[3] = (32 + delta) as u8;
        if delta < 0 {
            bytes.remove(4);
        } else {
            bytes.insert(4, 0);
        }
        assert_eq!(MediaRoot::decode(&bytes), Err(CodecError::InvalidLength));
    }
    for range in [0..2, 2..20, 20..54, 54..56, 56..58, 58..60] {
        let mut bytes = hex(PUBLISHING);
        bytes.drain(range);
        assert!(Epoch::decode(&bytes).is_err());
    }
    for range in [0..2, 2..36] {
        let mut bytes = hex(ROOT);
        bytes.drain(range);
        assert!(MediaRoot::decode(&bytes).is_err());
    }
}

#[test]
fn invalid_states_reasons_schemas_and_typed_count_combinations_are_refused() {
    for (index, invalid) in [(55, 0), (55, 17), (55, 19), (57, 0), (57, 3), (59, 0), (59, 6)] {
        let mut bytes = hex(PUBLISHING);
        bytes[index] = invalid;
        assert_eq!(Epoch::decode(&bytes), Err(CodecError::NonCanonical));
    }
    for (index, literal) in [(55, "9280808010"), (57, "8180808010"), (59, "8180808010")] {
        let mut bytes = hex(PUBLISHING);
        bytes.splice(index..index + 1, hex(literal));
        assert_eq!(Epoch::decode(&bytes), Err(CodecError::NonCanonical));
    }
    let mut value = complete();
    value.state = EpochState::Publishing;
    assert_eq!(value.encode(), Err(CodecError::NonCanonical));
    let mut bytes = hex(COMPLETE);
    bytes[57] = 1;
    assert_eq!(Epoch::decode(&bytes), Err(CodecError::NonCanonical));
    value = complete();
    value.ver = 19;
    assert_eq!(value.encode(), Err(CodecError::NonCanonical));
    value = complete();
    value.counts.reverse();
    assert_eq!(value.encode(), Err(CodecError::NonCanonical));
    value.counts = vec![
        KindCount {
            kind: Kind::Note,
            live: 0
        };
        2
    ];
    assert_eq!(value.encode(), Err(CodecError::NonCanonical));
    value.counts = vec![
        KindCount {
            kind: Kind::Note,
            live: 0
        };
        10
    ];
    assert_eq!(value.encode(), Err(CodecError::NonCanonical));
    assert_eq!(
        epoch([0; 16], [0; 32], 0, 0, &[(Kind::Card, 1), (Kind::Note, 1)]),
        Err(CodecError::NonCanonical)
    );
}

#[test]
fn malformed_and_noncanonical_kind_count_messages_are_refused() {
    for suffix in [
        "4a00", // missing kind
        "4a020800",
        "4a02080a",              // invalid kind domain
        "4a06088180808010",      // uint32 overflow cannot alias kind 1
        "4a0408011000",          // explicit nonoptional live zero
        "4a0408010801",          // duplicate kind
        "4a06080110011001",      // duplicate live
        "4a0408011801",          // unknown field
        "4a0408011001 4a020801", // duplicate entry
        "4a020809 4a020801",     // descending entries
        "4a03088100",            // nonminimal kind
        "4a050801108100",        // nonminimal live
        "4a840008011001",        // nonminimal submessage length
        "4a020a00",              // wrong kind wire type
        "4a0408011200",          // wrong live wire type
        "4a020801 4a020802 4a020803 4a020804 4a020805 4a020806 4a020807 4a020808 4a020809 4a020809",
    ] {
        let mut bytes = hex(PUBLISHING);
        bytes[57] = 2;
        bytes.extend(hex(suffix));
        assert!(Epoch::decode(&bytes).is_err(), "accepted malformed count suffix");
    }
}

#[test]
fn v1_unknown_duplicate_reordered_wrong_wire_and_default_scalar_fields_are_refused() {
    for suffix in ["5001", "3801", "4000", "40014001", "40010801", "4200", "00", "4a80"] {
        let mut bytes = hex(PUBLISHING);
        bytes.extend(hex(suffix));
        assert!(Epoch::decode(&bytes).is_err());
    }
    let mut explicit_crt = hex(PUBLISHING);
    explicit_crt.splice(54..54, hex("2000"));
    assert_eq!(Epoch::decode(&explicit_crt), Err(CodecError::NonCanonical));
    for suffix in ["2001", "1800", "18011801", "18010801", "1a00", "00"] {
        let mut bytes = hex(ROOT);
        bytes.extend(hex(suffix));
        assert!(MediaRoot::decode(&bytes).is_err());
    }
    // Duplicate every singular field while preserving its wire representation.
    for range in [0..2, 2..20, 20..54, 54..56, 56..58, 58..60] {
        let mut bytes = hex(PUBLISHING);
        let field = bytes[range.clone()].to_vec();
        bytes.splice(range.end..range.end, field);
        assert_eq!(Epoch::decode(&bytes), Err(CodecError::NonCanonical));
    }
    for range in [0..2, 2..36] {
        let mut bytes = hex(ROOT);
        let field = bytes[range.clone()].to_vec();
        bytes.splice(range.end..range.end, field);
        assert_eq!(MediaRoot::decode(&bytes), Err(CodecError::NonCanonical));
    }
    let mut duplicate_crt = hex(PUBLISHING);
    duplicate_crt.splice(54..54, hex("20012001"));
    assert!(Epoch::decode(&duplicate_crt).is_err());
    // Key/epoch length-delimited fields must not accept integer wire types.
    for index in [2, 20] {
        let mut bytes = hex(PUBLISHING);
        bytes[index] &= !7;
        assert!(Epoch::decode(&bytes).is_err());
    }
    let mut bytes = hex(ROOT);
    bytes[2] = 0x10;
    assert!(MediaRoot::decode(&bytes).is_err());
}

#[test]
fn nonminimal_varints_and_truncated_fields_are_refused() {
    for (range, literal) in [(0..1, "8800"), (1..2, "8100"), (3..4, "9000"), (55..56, "9200")] {
        let mut bytes = hex(PUBLISHING);
        bytes.splice(range, hex(literal));
        assert_eq!(Epoch::decode(&bytes), Err(CodecError::NonCanonical));
    }
    for (range, literal) in [(0..1, "8800"), (1..2, "8100"), (3..4, "a000")] {
        let mut bytes = hex(ROOT);
        bytes.splice(range, hex(literal));
        assert_eq!(MediaRoot::decode(&bytes), Err(CodecError::NonCanonical));
    }
    // Cut at each offset in fixtures ending with a required nondefault field.
    // A cut before a default scalar or repeated field can instead be valid.
    let bytes = hex(PUBLISHING);
    for end in 0..bytes.len() {
        assert!(Epoch::decode(&bytes[..end]).is_err());
    }
    let bytes = hex(ROOT);
    for end in 0..bytes.len() {
        assert!(MediaRoot::decode(&bytes[..end]).is_err());
    }
    for suffix in ["4080", "40ffffffffffffffffffff02", "4affffffffffffffffff01", "4a0208"] {
        let mut bytes = hex(PUBLISHING);
        bytes.extend(hex(suffix));
        assert!(Epoch::decode(&bytes).is_err());
    }
    for suffix in ["1880", "18ffffffffffffffffffff02"] {
        let mut bytes = hex(ROOT);
        bytes.extend(hex(suffix));
        assert!(MediaRoot::decode(&bytes).is_err());
    }
}

#[test]
fn format_attention_is_distinct_and_v1_input_is_bounded_before_body_parsing() {
    for (literal, format) in [("0802", 2), ("08ffffffff0f", u32::MAX)] {
        let bytes = hex(literal);
        assert_eq!(Epoch::decode(&bytes), Ok(DecodeOutcome::UnsupportedFormat(format)));
        assert_eq!(MediaRoot::decode(&bytes), Ok(DecodeOutcome::UnsupportedFormat(format)));
    }
    // A future body is deliberately not interpreted against V1 field rules.
    let mut future = hex("0802");
    future.resize(MAX_CONTROL_BYTES, 0xff);
    assert_eq!(Epoch::decode(&future), Ok(DecodeOutcome::UnsupportedFormat(2)));
    assert_eq!(MediaRoot::decode(&future), Ok(DecodeOutcome::UnsupportedFormat(2)));
    future.push(0xff);
    assert_eq!(Epoch::decode(&future), Ok(DecodeOutcome::UnsupportedFormat(2)));
    assert_eq!(MediaRoot::decode(&future), Ok(DecodeOutcome::UnsupportedFormat(2)));
    let mut oversized = hex("0801");
    oversized.resize(MAX_CONTROL_BYTES + 1, 0xff);
    assert_eq!(Epoch::decode(&oversized), Err(CodecError::Oversize));
    assert_eq!(MediaRoot::decode(&oversized), Err(CodecError::Oversize));
    for literal in ["", "0800", "08", "0a0102", "088080808010", "08ffffffffffffffffff02"] {
        let bytes = hex(literal);
        assert!(Epoch::decode(&bytes).is_err());
        assert!(MediaRoot::decode(&bytes).is_err());
    }
}

#[test]
fn debug_and_errors_do_not_disclose_key_material() {
    assert_eq!(format!("{:?}", publishing()), "Epoch");
    assert_eq!(format!("{:?}", root()), "MediaRoot");
    assert_eq!(
        format!("{:?}", DecodeOutcome::Supported(publishing())),
        "Supported(Epoch)"
    );
    assert_eq!(
        format!("{:?}", DecodeOutcome::Supported(root())),
        "Supported(MediaRoot)"
    );
    let mut bytes = hex(COMPLETE);
    bytes.extend(hex("5001"));
    let error = Epoch::decode(&bytes).unwrap_err();
    assert_eq!(format!("{error:?}"), "NonCanonical");
    assert_eq!(error.to_string(), "non-canonical entity encoding");
    let mut bytes = hex(ROOT);
    bytes[3] = 31;
    bytes.pop();
    let error = MediaRoot::decode(&bytes).unwrap_err();
    assert_eq!(format!("{error:?}"), "InvalidLength");
    assert_eq!(error.to_string(), "invalid entity field length");
}
