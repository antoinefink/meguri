//! Synthetic source-contract vectors only; no collection, connection or provider.
use meguri_sync::cloud::entity_codec::*;

fn hex(text: &str) -> Vec<u8> {
    let compact: String = text.split_whitespace().collect();
    compact
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b: u8| match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => panic!("invalid fixture"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
}
fn fields() -> EntityFields {
    EntityFields {
        kind: Kind::Note,
        key: id_key(0).to_vec(),
        origin: [0; 16],
        erev: 1,
        generation: None,
        tombstone: false,
        deleted_at_ms: 0,
        writer: [0; 16],
        base_sha256: None,
    }
}
fn supported(bytes: &[u8]) -> Entity {
    match Entity::decode(bytes).unwrap() {
        DecodeOutcome::Supported(entity) => entity,
        _ => panic!("unexpected format"),
    }
}
fn name(entity: &Entity) -> String {
    entity_name(&[0; 32], entity.fields.kind, &entity.fields.key).unwrap()
}
fn accept(entity: &Entity, asset: AssetInput<'_>) -> Result<Received, ReceiveError> {
    receive(&[0; 32], &name(entity), &entity.encode().unwrap(), asset)
}

#[test]
fn independently_computed_names_and_signed_keys() {
    // Fixed messages: kind byte followed by the listed exact bytes. Constants
    // were computed with Python's standard HMAC/SHA-256 and RFC4648 encoders.
    assert_eq!(
        entity_name(&[0; 32], Kind::Note, &[0; 8]).unwrap(),
        "ebm47dncv66g74yjuyk3w2yqtqy"
    );
    assert_eq!(
        entity_name(&[0; 32], Kind::Card, &[0; 8]).unwrap(),
        "eegewvjunxjaw2tden7dnhz5634"
    );
    assert_eq!(
        entity_name(&[0; 32], Kind::Note, &[255; 8]).unwrap(),
        "ekaafyyz3dllwjbeifh2f2lg62u"
    );
    assert_eq!(
        entity_name(&[0; 32], Kind::Tag, b"strasse").unwrap(),
        "edsofjrj35oxbbmz2dr6vfdh33a"
    );
    assert_eq!(
        entity_name(&[0; 32], Kind::MediaName, b"photo.jpg").unwrap(),
        "ev64ijpgjgezausnwytfse2aozm"
    );
    assert_eq!(id_key(0), [0; 8]);
    assert_eq!(id_key(-1), [255; 8]);
    assert_eq!(id_key(i64::MIN), [128, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(id_key(i64::MAX), [127, 255, 255, 255, 255, 255, 255, 255]);
    assert_eq!(id_key(256), [0, 0, 0, 0, 0, 0, 1, 0]);
    for code in 1..=9 {
        let kind = Kind::try_from(code).unwrap();
        let key = if code <= 6 {
            &b"\0\0\0\0\0\0\0\0"[..]
        } else {
            &b"synthetic"[..]
        };
        let value = entity_name(&[0; 32], kind, key).unwrap();
        assert_eq!(value.len(), 27);
        assert!(
            value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
        );
    }
    assert_eq!(entity_name(&[0; 32], Kind::Note, &[0; 7]), Err(CodecError::InvalidKey));
    assert_eq!(entity_name(&[0; 32], Kind::Tag, &[255]), Err(CodecError::InvalidKey));
    // Canonicalization is deliberately supplied by the caller. The codec must
    // neither fold stored spelling nor substitute media lowercase for unicase.
    assert_ne!(
        entity_name(&[0; 32], Kind::Tag, b"strasse"),
        entity_name(&[0; 32], Kind::Tag, "straße".as_bytes())
    );
    assert_ne!(
        entity_name(&[0; 32], Kind::Config, b"A"),
        entity_name(&[0; 32], Kind::Config, b"a")
    );
}

#[test]
fn published_crypto_and_base32_vectors() {
    use hmac::{Hmac, KeyInit, Mac};
    // RFC4231 test case 1; HMAC's zero padding makes this a 32-byte key too.
    let mut key = [0; 32];
    key[..20].fill(0x0b);
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(&key).unwrap();
    mac.update(b"Hi There");
    assert_eq!(
        &mac.finalize().into_bytes()[..],
        hex("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
    );
    for (bytes, expected) in [
        (b"f".as_slice(), "MY"),
        (b"fo", "MZXQ"),
        (b"foo", "MZXW6"),
        (b"foobar", "MZXW6YTBOI"),
    ] {
        assert_eq!(data_encoding::BASE32_NOPAD.encode(bytes), expected);
    }
    assert_eq!(
        sha256(b""),
        <[u8; 32]>::try_from(hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")).unwrap()
    );
}

#[test]
fn all_nine_exact_tag_vectors_have_no_payload_format_field() {
    let cases = vec![
        (
            Payload::Note(Note {
                id: 1,
                guid: vec![255],
                mid: 1,
                modified: 1,
                tags: vec![254],
                flds: vec![253],
                sfld: Stored::Integer(0),
                csum: 1,
                flags: 1,
                data: vec![252],
                nt_struct_gen: 1,
            }),
            hex("0801 1201ff 1801 2001 2a01fe 3201fd 3a020800 4001 4801 5201fc 5801"),
        ),
        (
            Payload::Card(Card {
                id: 1,
                nid: 1,
                did: 1,
                ord: 1,
                modified: 1,
                card_type: 1,
                queue: 1,
                due: Stored::Integer(0),
                ivl: 1,
                factor: 1,
                reps: 1,
                lapses: 1,
                left: 1,
                odue: Stored::Text(vec![]),
                odid: 1,
                flags: 1,
                data: Stored::Blob(vec![]),
                last_review_id: 1,
                nt_struct_gen: 1,
            }),
            hex(
                "0801 1001 1801 2001 2801 3001 3801 42020800 4801 5001 5801 6001 6801 72021a00 7801 800101 8a01022200 900101 980101",
            ),
        ),
        (
            Payload::Revlog(Revlog {
                id: 1,
                cid: 1,
                ease: 1,
                ivl: Stored::Integer(0),
                last_ivl: Stored::Text(vec![]),
                factor: 1,
                time: Stored::Blob(vec![]),
                review_type: 1,
            }),
            hex("0801 1001 1801 22020800 2a021a00 3001 3a022200 4001"),
        ),
        (
            Payload::Deck(Deck {
                id: 1,
                name: vec![255],
                mtime_secs: 1,
                common: vec![254],
                kind: vec![253],
                parent_id: 1,
            }),
            hex("0801 1201ff 1801 2201fe 2a01fd 3001"),
        ),
        (
            Payload::Preset(Preset {
                id: 1,
                name: vec![255],
                mtime_secs: 1,
                config: vec![254],
            }),
            hex("0801 1201ff 1801 2201fe"),
        ),
        (
            Payload::Notetype(Notetype {
                id: 1,
                name: vec![255],
                mtime_secs: 1,
                config: vec![254],
                fields: vec![Field {
                    ord: 1,
                    name: vec![253],
                    config: vec![252],
                }],
                templates: vec![Template {
                    ord: 1,
                    name: vec![251],
                    mtime_secs: 1,
                    config: vec![250],
                }],
                struct_gen: 1,
            }),
            hex("0801 1201ff 1801 2201fe 2a0808011201fd1a01fc 320a08011201fb18012201fa 3801"),
        ),
        (
            Payload::Tag(Tag {
                tag: vec![255],
                collapsed: 1,
                config: Some(vec![254]),
            }),
            hex("0a01ff 1001 1a01fe"),
        ),
        (
            Payload::Config(Config {
                key: vec![255],
                mtime_secs: 1,
                val: vec![254],
            }),
            hex("0a01ff 1001 1a01fe"),
        ),
        (
            Payload::MediaName(MediaName {
                name: vec![255],
                sha256: [1; 32],
                sha1: [2; 20],
                size: 1,
            }),
            {
                let mut bytes = hex("0a01ff 1220");
                bytes.extend([1; 32]);
                bytes.extend(hex("1a14"));
                bytes.extend([2; 20]);
                bytes.extend(hex("2001"));
                bytes
            },
        ),
    ];
    for (index, (payload, expected)) in cases.into_iter().enumerate() {
        assert_eq!(payload.kind() as usize, index + 1);
        assert_eq!(payload.encode().unwrap(), expected);
        assert_eq!(Payload::decode(payload.kind(), &expected).unwrap(), payload);
    }
}

#[test]
fn stored_defaults_and_ieee_bits_are_explicit_and_distinct() {
    let cases = [
        (Stored::Integer(0), "3a020800"),
        (Stored::Real(0.0), "3a09110000000000000000"),
        (Stored::Real(-0.0), "3a09110000000000000080"),
        (Stored::Text(vec![]), "3a021a00"),
        (Stored::Blob(vec![]), "3a022200"),
    ];
    for (stored, expected) in cases {
        let payload = Payload::Note(Note {
            sfld: stored,
            ..Note::default()
        });
        assert_eq!(payload.encode().unwrap(), hex(expected));
        assert_eq!(Payload::decode(Kind::Note, &hex(expected)).unwrap(), payload);
    }
    for bits in [
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        0x7ff8_1234_5678_9abc,
        1,
    ] {
        let payload = Payload::Note(Note {
            sfld: Stored::Real(f64::from_bits(bits)),
            ..Note::default()
        });
        assert_eq!(
            Payload::decode(Kind::Note, &payload.encode().unwrap()).unwrap(),
            payload
        );
    }
}

#[test]
fn every_stored_wrapper_and_member_is_required() {
    for (kind, bad) in [
        (Kind::Note, ""),
        (Kind::Note, "3a00"),
        (Kind::Note, "3a0408001a00"),
        (Kind::Note, "3a0408000800"),
        (Kind::Note, "3a0208003a020800"),
        (Kind::Note, "3a022a00"),
        (Kind::Note, "3a020a00"),
        (Kind::Card, ""),
        (Kind::Card, "42020800 72020800"),
        (Kind::Card, "42020800 8a01020800"),
        (Kind::Card, "72020800 8a01020800"),
        (Kind::Revlog, ""),
        (Kind::Revlog, "22020800 2a020800"),
        (Kind::Revlog, "22020800 3a020800"),
        (Kind::Revlog, "2a020800 3a020800"),
    ] {
        assert_eq!(Payload::decode(kind, &hex(bad)), Err(CodecError::NonCanonical));
    }
    assert_eq!(
        Payload::Card(Card::default()).encode().unwrap(),
        hex("42020800 72020800 8a01020800")
    );
    assert_eq!(
        Payload::Revlog(Revlog::default()).encode().unwrap(),
        hex("22020800 2a020800 3a020800")
    );
}

#[test]
fn tag_null_empty_and_raw_text_blob_are_retained() {
    assert!(Payload::Tag(Tag::default()).encode().unwrap().is_empty());
    let empty = Payload::Tag(Tag {
        config: Some(vec![]),
        ..Tag::default()
    });
    assert_eq!(empty.encode().unwrap(), hex("1a00"));
    assert_eq!(Payload::decode(Kind::Tag, &hex("1a00")).unwrap(), empty);
    for value in [
        Stored::Text(vec![0xff, 0xc0, 0, 0x80]),
        Stored::Blob(vec![0xff, 0xc0, 0, 0x80]),
    ] {
        let payload = Payload::Card(Card {
            data: value,
            ..Card::default()
        });
        assert_eq!(
            Payload::decode(Kind::Card, &payload.encode().unwrap()).unwrap(),
            payload
        );
    }
    let payload = Payload::Config(Config {
        key: b"MixedCase".to_vec(),
        mtime_secs: 0,
        val: vec![0xff, 0, 0x81],
    });
    assert_eq!(
        Payload::decode(Kind::Config, &payload.encode().unwrap()).unwrap(),
        payload
    );
}

#[test]
fn integer_boundaries_do_not_normalize_stored_values() {
    for value in [i64::MIN, -1, 0, i64::MAX] {
        let payload = Payload::Note(Note {
            id: value,
            csum: value,
            sfld: Stored::Integer(value),
            nt_struct_gen: u64::MAX,
            ..Note::default()
        });
        assert_eq!(
            Payload::decode(Kind::Note, &payload.encode().unwrap()).unwrap(),
            payload
        );
    }
    let negative = Payload::Deck(Deck {
        id: -1,
        ..Deck::default()
    });
    assert_eq!(negative.encode().unwrap(), hex("08ffffffffffffffffff01"));
    let gen_max = Payload::Note(Note {
        nt_struct_gen: u64::MAX,
        ..Note::default()
    });
    assert_eq!(gen_max.encode().unwrap(), hex("3a020800 58ffffffffffffffffff01"));
    let card = Payload::Card(Card {
        nt_struct_gen: u64::MAX,
        last_review_id: i64::MIN,
        ..Card::default()
    });
    assert_eq!(Payload::decode(Kind::Card, &card.encode().unwrap()).unwrap(), card);
    let nt = Payload::Notetype(Notetype {
        struct_gen: u64::MAX,
        ..Notetype::default()
    });
    assert_eq!(nt.encode().unwrap(), hex("38ffffffffffffffffff01"));
    let media = Payload::MediaName(MediaName {
        size: u64::MAX,
        ..MediaName::default()
    });
    let mut expected = hex("1220");
    expected.extend([0; 32]);
    expected.extend(hex("1a14"));
    expected.extend([0; 20]);
    expected.extend(hex("20ffffffffffffffffff01"));
    assert_eq!(media.encode().unwrap(), expected);
    assert_eq!(Payload::decode(Kind::MediaName, &expected).unwrap(), media);
}

#[test]
fn nested_rows_sort_by_ord_and_refuse_noncanonical_order() {
    let payload = Payload::Notetype(Notetype {
        fields: vec![
            Field {
                ord: 2,
                ..Field::default()
            },
            Field {
                ord: 0,
                ..Field::default()
            },
        ],
        templates: vec![
            Template {
                ord: 1,
                ..Template::default()
            },
            Template {
                ord: 0,
                ..Template::default()
            },
        ],
        ..Notetype::default()
    });
    let expected = hex("2a00 2a020802 3200 32020801");
    assert_eq!(payload.encode().unwrap(), expected);
    assert_eq!(
        Payload::decode(Kind::Notetype, &expected).unwrap().encode().unwrap(),
        expected
    );
    for bad in [
        "2a020802 2a00",
        "32020801 3200",
        "2a00 2a00",
        "2a020800",
        "2a051201780801",
        "2a022001",
    ] {
        assert_eq!(
            Payload::decode(Kind::Notetype, &hex(bad)),
            Err(CodecError::NonCanonical)
        );
    }
    let duplicate = Payload::Notetype(Notetype {
        fields: vec![Field::default(), Field::default()],
        ..Notetype::default()
    });
    assert_eq!(duplicate.encode(), Err(CodecError::NonCanonical));
}

#[test]
fn supported_payloads_refuse_malformed_and_noncanonical_wire() {
    for bad in [
        "0800",     // implicit default present
        "08010801", // duplicate
        "18010801", // order
        "2801",     // unknown
        "880001",   // nonminimal tag
        "088100",   // nonminimal value
        "1200",     // empty implicit bytes
        "12810061", // nonminimal length
        "0a0100",   // wrong wire for int64
        "08",
        "1280",
        "120261",                 // truncation
        "08ffffffffffffffffff02", // uint64 overflow
        "1280808080808080808002", // length overflow
        "1280808080808080808001", // enormous declared size, tiny buffer
        "00",
        "0b",
        "0d00000000",
        "09", // invalid field/group/fixed wire
    ] {
        assert!(Payload::decode(Kind::Preset, &hex(bad)).is_err());
    }
    assert!(Payload::decode(Kind::MediaName, &[]).is_err());
    assert!(Payload::decode(Kind::MediaName, &hex("1201ff 1a00")).is_err());
}

#[test]
fn entity_all_thirteen_exact_tags_and_optional_defaults() {
    let mut metadata = fields();
    metadata.erev = u64::MAX;
    metadata.generation = Some([0; 16]);
    metadata.tombstone = true;
    metadata.deleted_at_ms = -1;
    metadata.base_sha256 = Some([0; 32]);
    let entity = Entity::build(metadata, &[]).unwrap();
    let mut expected = hex("0801 1001 1a080000000000000000 2210");
    expected.extend([0; 16]);
    expected.extend(hex("28ffffffffffffffffff01 3210"));
    expected.extend([0; 16]);
    expected.extend(hex("3801 40ffffffffffffffffff01 4a00 5220"));
    expected.extend(hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
    expected.extend(hex("6210"));
    expected.extend([0; 16]);
    expected.extend(hex("6a20"));
    expected.extend([0; 32]);
    assert_eq!(entity.encode().unwrap(), expected);
    assert_eq!(supported(&expected), entity);
    // Check field 11's exact tag independently while keeping the empty-payload
    // vector above. Integrity validation is separate from envelope serialization.
    let mut size_one = entity.clone();
    size_one.payload_size = 1;
    let writer_offset = expected.len() - 52;
    let mut expected_size_one = expected[..writer_offset].to_vec();
    expected_size_one.extend(hex("5801"));
    expected_size_one.extend_from_slice(&expected[writer_offset..]);
    assert_eq!(size_one.encode().unwrap(), expected_size_one);
    assert_eq!(supported(&expected_size_one), size_one);
    assert_eq!(
        accept(&size_one, AssetInput::Absent),
        Err(ReceiveError::Refused(Refusal::InlineIntegrity))
    );
    assert_eq!(entity.payload, Some(vec![]));
    let received = accept(&entity, AssetInput::Absent).unwrap();
    assert_eq!(received.payload, None);
    let absent = Entity::build(fields(), &[]).unwrap();
    assert_ne!(absent.encode().unwrap(), expected);
    assert_eq!(supported(&absent.encode().unwrap()).fields.generation, None);
    assert_eq!(supported(&absent.encode().unwrap()).fields.base_sha256, None);
    let mut asset_empty = absent.clone();
    asset_empty.payload = None;
    assert_ne!(asset_empty.encode().unwrap(), absent.encode().unwrap());
    assert!(accept(&asset_empty, AssetInput::Bytes(&[])).is_ok());
}

#[test]
fn exactly_full_envelope_threshold_and_one_over_include_optionals() {
    for optional in [false, true] {
        let mut metadata = fields();
        if optional {
            metadata.generation = Some([0; 16]);
            metadata.base_sha256 = Some([0; 32]);
        }
        let empty = Entity::build(metadata.clone(), &[]).unwrap();
        let overhead = if optional { 140 } else { 88 };
        assert_eq!(empty.encode().unwrap().len(), overhead);
        // At this size, field 9's length varint gains two bytes and field 11
        // adds its one-byte tag plus three-byte size. All other bytes are fixed.
        let exact_len = INLINE_ENVELOPE_BYTES - overhead - 6;
        let payload = vec![7; exact_len];
        let exact = Entity::build(metadata.clone(), &payload).unwrap();
        assert_eq!(exact.encode().unwrap().len(), INLINE_ENVELOPE_BYTES);
        assert!(exact.payload.is_some());
        let over_payload = vec![7; exact_len + 1];
        let over = Entity::build(metadata, &over_payload).unwrap();
        assert_eq!(over.payload, None);
        let mut forced_inline = over.clone();
        forced_inline.payload = Some(over_payload);
        assert_eq!(forced_inline.encode().unwrap().len(), INLINE_ENVELOPE_BYTES + 1);
        assert_eq!(supported(&forced_inline.encode().unwrap()), forced_inline);
        assert_eq!(over.payload_size, (exact_len + 1) as u64);
        assert_eq!(over.payload_sha256, forced_inline.payload_sha256);
    }
    let bytes = vec![7; INLINE_ENVELOPE_BYTES - 88 - 6];
    assert!(Entity::build(fields(), &bytes).unwrap().payload.is_some());
    let mut metadata = fields();
    metadata.generation = Some([0; 16]);
    assert!(Entity::build(metadata, &bytes).unwrap().payload.is_none());
}

#[test]
fn payload_limits_check_declared_size_without_large_allocations() {
    for size in [0, 1, MAX_PAYLOAD_BYTES - 1, MAX_PAYLOAD_BYTES] {
        assert_eq!(validate_payload_size(size), Ok(()));
    }
    for size in [MAX_PAYLOAD_BYTES + 1, u64::MAX] {
        assert_eq!(validate_payload_size(size), Err(CodecError::Oversize));
    }
    let mut entity = Entity::build(fields(), &[]).unwrap();
    entity.payload = None;
    entity.payload_size = MAX_PAYLOAD_BYTES;
    assert_eq!(supported(&entity.encode().unwrap()).payload_size, MAX_PAYLOAD_BYTES);
    entity.payload_size = MAX_PAYLOAD_BYTES + 1;
    assert_eq!(entity.encode(), Err(CodecError::Oversize));
    // The declared size is oversize; there is no allocation of declared bytes.
    let mut bytes = Entity::build(fields(), &[]).unwrap().encode().unwrap();
    let tail = bytes.split_off(bytes.len() - 18);
    bytes.extend(hex("58ffffffffffffffffff01"));
    bytes.extend(tail);
    assert_eq!(Entity::decode(&bytes), Err(CodecError::Oversize));
}

#[test]
fn inline_and_small_asset_integrity_have_distinct_classifications() {
    let payload = Payload::Note(Note::default()).encode().unwrap();
    let entity = Entity::build(fields(), &payload).unwrap();
    assert!(accept(&entity, AssetInput::Absent).is_ok());
    // Inline form uses inline bytes even if a caller also reports a bad asset.
    assert!(accept(&entity, AssetInput::Unreadable).is_ok());
    let mut asset = entity.clone();
    asset.payload = None;
    assert!(accept(&asset, AssetInput::Bytes(&payload)).is_ok());
    assert_eq!(
        accept(&asset, AssetInput::Absent),
        Err(ReceiveError::RetryableAsset(AssetProblem::Absent))
    );
    assert_eq!(
        accept(&asset, AssetInput::Unreadable),
        Err(ReceiveError::RetryableAsset(AssetProblem::Unreadable))
    );
    assert_eq!(
        accept(&asset, AssetInput::Bytes(&[])),
        Err(ReceiveError::RetryableAsset(AssetProblem::Integrity))
    );
    assert_eq!(
        accept(&asset, AssetInput::Bytes(&[1, 2, 3, 4])),
        Err(ReceiveError::RetryableAsset(AssetProblem::Integrity))
    );
    let mut bad_digest = entity.clone();
    bad_digest.payload_sha256[0] ^= 1;
    assert_eq!(
        accept(&bad_digest, AssetInput::Absent),
        Err(ReceiveError::Refused(Refusal::InlineIntegrity))
    );
    let mut bad_size = entity.clone();
    bad_size.payload_size += 1;
    assert_eq!(
        accept(&bad_size, AssetInput::Absent),
        Err(ReceiveError::Refused(Refusal::InlineIntegrity))
    );
    let mut bad_asset_digest = asset.clone();
    bad_asset_digest.payload_sha256[0] ^= 1;
    assert_eq!(
        accept(&bad_asset_digest, AssetInput::Bytes(&payload)),
        Err(ReceiveError::RetryableAsset(AssetProblem::Integrity))
    );
    let mut bad_asset_size = asset.clone();
    bad_asset_size.payload_size += 1;
    assert_eq!(
        accept(&bad_asset_size, AssetInput::Bytes(&payload)),
        Err(ReceiveError::RetryableAsset(AssetProblem::Integrity))
    );
    assert_eq!(
        receive(&[0; 32], "esynthetic", &entity.encode().unwrap(), AssetInput::Absent),
        Err(ReceiveError::Refused(Refusal::Name))
    );
    // Matching digest/size does not excuse a noncanonical payload.
    let malformed = Entity::build(fields(), &hex("3a00")).unwrap();
    assert_eq!(
        accept(&malformed, AssetInput::Absent),
        Err(ReceiveError::Refused(Refusal::Encoding(CodecError::NonCanonical)))
    );
    let mut asset_malformed = malformed.clone();
    asset_malformed.payload = None;
    assert_eq!(
        accept(&asset_malformed, AssetInput::Bytes(&hex("3a00"))),
        Err(ReceiveError::Refused(Refusal::Encoding(CodecError::NonCanonical)))
    );
}

#[test]
fn supported_envelope_canonical_refusals() {
    let valid = Entity::build(fields(), &[]).unwrap().encode().unwrap();
    let mut cases = vec![];
    let mut duplicate = valid.clone();
    duplicate.extend(hex("6210"));
    duplicate.extend([0; 16]);
    cases.push(duplicate);
    let mut unknown = valid.clone();
    unknown.extend(hex("7001"));
    cases.push(unknown);
    let mut default_size = valid.clone();
    let tail = default_size.split_off(default_size.len() - 18);
    default_size.extend(hex("5800"));
    default_size.extend(tail);
    cases.push(default_size);
    let mut nonminimal = vec![8, 0x81, 0];
    nonminimal.extend_from_slice(&valid[2..]);
    cases.push(nonminimal);
    let mut wrong_order = valid[2..4].to_vec();
    wrong_order.extend_from_slice(&valid[..2]);
    wrong_order.extend_from_slice(&valid[4..]);
    cases.push(wrong_order);
    for prefix in ["0a0101", "0800", "0880808080808080808002"] {
        let mut bad = hex(prefix);
        bad.extend_from_slice(&valid[2..]);
        cases.push(bad);
    }
    for bad in cases {
        assert!(Entity::decode(&bad).is_err());
    }
    assert!(Entity::decode(&[8]).is_err());
    for end in 0..valid.len() {
        assert!(Entity::decode(&valid[..end]).is_err());
    }
    // Strict 16/32-byte lengths, including explicit empty optional values.
    for (start, end, replacement) in [
        (14, 32, "2200"), // origin
        (70, 88, "6200"), // writer
        (36, 70, "5200"), // payload digest
    ] {
        let mut bad = valid[..start].to_vec();
        bad.extend(hex(replacement));
        bad.extend_from_slice(&valid[end..]);
        assert!(Entity::decode(&bad).is_err());
    }
    let mut bad_generation = valid[..34].to_vec();
    bad_generation.extend(hex("3200"));
    bad_generation.extend_from_slice(&valid[34..]);
    assert_eq!(Entity::decode(&bad_generation), Err(CodecError::InvalidLength));
    let mut bad_base = valid.clone();
    bad_base.extend(hex("6a00"));
    assert_eq!(Entity::decode(&bad_base), Err(CodecError::InvalidLength));
    let mut bool_two = valid[..34].to_vec();
    bool_two.extend(hex("3802"));
    bool_two.extend_from_slice(&valid[34..]);
    assert_eq!(Entity::decode(&bool_two), Err(CodecError::NonCanonical));
    let mut implicit_false = valid[..34].to_vec();
    implicit_false.extend(hex("3800"));
    implicit_false.extend_from_slice(&valid[34..]);
    assert_eq!(Entity::decode(&implicit_false), Err(CodecError::NonCanonical));
}

#[test]
fn future_format_is_classified_without_v1_payload_or_envelope_validation() {
    // Future tails may have fields/wires this build knows nothing about.
    for future in [hex("0802 ff"), hex("0802 1000 1001 7000"), hex("08ffffffff0f 00")] {
        let expected = if future[1] == 2 { 2 } else { u32::MAX };
        assert_eq!(
            Entity::decode(&future).unwrap(),
            DecodeOutcome::UnsupportedFormat(expected)
        );
        assert_eq!(
            receive(&[0; 32], "esynthetic", &future, AssetInput::Unreadable),
            Err(ReceiveError::UnsupportedFormat(expected))
        );
    }
    assert!(Entity::decode(&hex("088080808010")).is_err()); // uint32 overflow
}

#[test]
fn row_digest_clears_only_the_defined_meguri_fields() {
    let baseline = Payload::Card(Card::default());
    for changed in [
        Card {
            last_review_id: i64::MIN,
            ..Card::default()
        },
        Card {
            nt_struct_gen: u64::MAX,
            ..Card::default()
        },
        Card {
            last_review_id: i64::MAX,
            nt_struct_gen: 9,
            ..Card::default()
        },
    ] {
        let changed = Payload::Card(changed);
        assert_eq!(baseline.row_digest().unwrap(), changed.row_digest().unwrap());
        assert_ne!(baseline.payload_digest().unwrap(), changed.payload_digest().unwrap());
    }
    let scheduling = Card {
        due: Stored::Integer(1),
        ..Card::default()
    };
    assert_ne!(
        baseline.row_digest().unwrap(),
        Payload::Card(scheduling).row_digest().unwrap()
    );
    for (original, changed, row_changed) in [
        (
            Payload::Note(Note::default()),
            Payload::Note(Note {
                nt_struct_gen: 8,
                ..Note::default()
            }),
            Payload::Note(Note {
                flags: 1,
                ..Note::default()
            }),
        ),
        (
            Payload::Deck(Deck::default()),
            Payload::Deck(Deck {
                parent_id: -1,
                ..Deck::default()
            }),
            Payload::Deck(Deck {
                common: vec![1],
                ..Deck::default()
            }),
        ),
        (
            Payload::Notetype(Notetype::default()),
            Payload::Notetype(Notetype {
                struct_gen: u64::MAX,
                ..Notetype::default()
            }),
            Payload::Notetype(Notetype {
                config: vec![1],
                ..Notetype::default()
            }),
        ),
    ] {
        assert_eq!(original.row_digest().unwrap(), changed.row_digest().unwrap());
        assert_ne!(original.payload_digest().unwrap(), changed.payload_digest().unwrap());
        assert_ne!(original.row_digest().unwrap(), row_changed.row_digest().unwrap());
    }
    // The other five kinds have no Meguri fields to clear.
    for payload in [
        Payload::Revlog(Revlog::default()),
        Payload::Preset(Preset::default()),
        Payload::Tag(Tag::default()),
        Payload::Config(Config::default()),
        Payload::MediaName(MediaName::default()),
    ] {
        assert_eq!(payload.row_digest().unwrap(), payload.payload_digest().unwrap());
    }
}

#[test]
fn debug_and_errors_do_not_reveal_values() {
    let payload = Payload::Note(Note {
        guid: b"synthetic-secret".to_vec(),
        sfld: Stored::Text(b"synthetic-secret".to_vec()),
        ..Note::default()
    });
    let mut entity = Entity::build(fields(), &payload.encode().unwrap()).unwrap();
    entity.fields.key = b"synthetic-secret".to_vec();
    assert_eq!(format!("{:?}", entity), "Entity");
    assert_eq!(format!("{:?}", entity.fields), "EntityFields");
    assert_eq!(format!("{:?}", payload), "Payload::Note");
    assert_eq!(
        format!("{:?}", Stored::Text(b"synthetic-secret".to_vec())),
        "Stored::Text"
    );
    assert_eq!(
        format!("{:?}", AssetInput::Bytes(b"synthetic-secret")),
        "AssetInput::Bytes"
    );
    assert_eq!(entity.encode(), Err(CodecError::InvalidKey));
}
