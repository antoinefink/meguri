//! Independent K5.3/K5.4/K7/K19.2/K20 vectors; no service or runtime fixture.
use meguri_sync::cloud::{
    control_codec::DecodeOutcome,
    entity_codec::{AssetInput, AssetProblem, CodecError, ReceiveError, Refusal, sha256},
    generation_codec::{
        Generation, GenerationState, LayoutMap, MAX_GENERATION_BYTES, MAX_MEMBER_LIST_BYTES, Member, MemberList,
        MemberListView, MemberRef, TypeChange,
    },
};

// The existing PeakAlloc is process-wide. This binary has concurrent tests, so
// count only successful allocation requests on the measuring test's thread.
// Total requested bytes conservatively bound peak live allocation, including
// temporary allocations and reallocations; frees cannot hide a transient peak.
mod decode_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct Alloc;
    thread_local! {
        static REQUESTED: Cell<Option<usize>> = const { Cell::new(None) };
    }

    fn added(bytes: usize) {
        let _ = REQUESTED.try_with(|requested| {
            if let Some(total) = requested.get() {
                requested.set(Some(total.saturating_add(bytes)));
            }
        });
    }

    // SAFETY: all pointers, sizes and layouts are forwarded unchanged to System.
    // The const-initialized TLS Cell requires no allocation or destructor, and
    // accounting never dereferences an allocation or changes its ownership.
    unsafe impl GlobalAlloc for Alloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: the caller supplies the valid layout required by alloc.
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() {
                added(layout.size());
            }
            ptr
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            // SAFETY: the caller supplies the valid layout required by alloc_zeroed.
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if !ptr.is_null() {
                added(layout.size());
            }
            ptr
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller supplies a live pointer and its original layout.
            unsafe { System.dealloc(ptr, layout) };
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            // SAFETY: the caller supplies a live pointer, its layout and valid new size.
            let new = unsafe { System.realloc(ptr, layout, new_size) };
            if !new.is_null() {
                // Count the whole successful request even for in-place growth.
                added(new_size);
            }
            new
        }
    }

    pub fn measure<T>(decode: impl FnOnce() -> T) -> (T, usize) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                REQUESTED.with(|requested| requested.set(None));
            }
        }
        REQUESTED.with(|requested| assert!(requested.replace(Some(0)).is_none()));
        let reset = Reset;
        let result = decode();
        let requested = REQUESTED.with(|requested| requested.get().unwrap());
        drop(reset);
        (result, requested)
    }
}

#[global_allocator]
static ALLOC: decode_alloc::Alloc = decode_alloc::Alloc;

const CLAIMED: &str = "0801 1210 000102030405060708090a0b0c0d0e0f 2001";
const A: &str = "eaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "eaaaaaaaaaaaaaaaaaaaaaaaaae";
// 27 ASCII bytes: e + 26 a's. Literal bytes do not call the production encoder.
const MEMBER: &str = "0a1b 65 6161616161 6161616161 6161616161 6161616161 6161616161 61 108001";
const MEMBER_LIST: &str = "0a20 0a1b 65 6161616161 6161616161 6161616161 6161616161 6161616161 61 108001";
const MAP: &str = "3a19 08ffffffffffffffffff01 1802 2207 0100feffffff0f 2a0102";
const CHANGE: &str = "420e 0801 1002 1803 2004 2a0101 320100";

fn hex(literal: &str) -> Vec<u8> {
    let compact: String = literal.split_whitespace().collect();
    assert_eq!(compact.len() % 2, 0);
    compact
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b| match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => panic!("invalid literal byte"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
}
fn generation() -> Generation {
    Generation {
        generation_id: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        reasons: Vec::new(),
        state: GenerationState::Claimed,
        anchors: Vec::new(),
        members: Vec::new(),
        maps: Vec::new(),
        type_changes: Vec::new(),
        members_sha256: None,
        members_size: None,
    }
}
fn layout() -> LayoutMap {
    LayoutMap {
        notetype_id: 0,
        from_struct_gen: 0,
        to_struct_gen: 0,
        field_map: Vec::new(),
        template_map: Vec::new(),
    }
}
fn change() -> TypeChange {
    TypeChange {
        old_notetype_id: 0,
        old_struct_gen: 0,
        new_notetype_id: 0,
        new_struct_gen: 0,
        field_map: Vec::new(),
        template_map: Vec::new(),
    }
}
fn tail(literal: &str) -> Vec<u8> {
    let mut bytes = hex(CLAIMED);
    bytes.extend(hex(literal));
    bytes
}
fn assert_vector(value: Generation, bytes: Vec<u8>) {
    assert_eq!(value.encode().unwrap(), bytes);
    assert_eq!(Generation::decode(&bytes), Ok(DecodeOutcome::Supported(value)));
}
fn asset(bytes: &[u8]) -> Result<MemberListView<'_>, ReceiveError> {
    MemberList::decode_asset(AssetInput::Bytes(bytes), &sha256(bytes), bytes.len() as u64)
}
fn assert_members(bytes: &[u8], expected: &MemberList) {
    let view = asset(bytes).unwrap();
    assert_eq!(view.len(), expected.members.len());
    assert_eq!(view.is_empty(), expected.members.is_empty());
    assert!(view.iter().eq(expected.members.iter().map(|member| MemberRef {
        record_name: &member.record_name,
        erev: member.erev,
    })));
}

#[test]
fn literal_all_states_all_reasons_and_nested_messages() {
    for (state, literal) in [
        (GenerationState::Claimed, "2001"),
        (GenerationState::Complete, "2002"),
        (GenerationState::Abandoned, "2003"),
    ] {
        let mut bytes = hex(CLAIMED);
        bytes.truncate(20);
        bytes.extend(hex(literal));
        assert_vector(Generation { state, ..generation() }, bytes);
    }
    let value = Generation {
        reasons: (1..=10).collect(),
        anchors: vec![A.to_owned()],
        members: vec![Member {
            record_name: A.to_owned(),
            erev: 128,
        }],
        maps: vec![LayoutMap {
            notetype_id: -1,
            to_struct_gen: 2,
            field_map: vec![-1, 0, i32::MAX],
            template_map: vec![1],
            ..layout()
        }],
        type_changes: vec![TypeChange {
            old_notetype_id: 1,
            old_struct_gen: 2,
            new_notetype_id: 3,
            new_struct_gen: 4,
            field_map: vec![-1],
            template_map: vec![0],
        }],
        ..generation()
    };
    let mut bytes = hex(CLAIMED);
    bytes.truncate(20);
    bytes.extend(hex(
        "1a0a0102030405060708090a 2001 2a1b 65 6161616161 6161616161 6161616161 6161616161 6161616161 61 3220",
    ));
    bytes.extend(hex(MEMBER));
    bytes.extend(hex(MAP));
    bytes.extend(hex(CHANGE));
    assert_vector(value, bytes);
    let list = MemberList {
        members: vec![Member {
            record_name: A.to_owned(),
            erev: 128,
        }],
    };
    assert_eq!(list.encode().unwrap(), hex(MEMBER_LIST));
    assert_members(&hex(MEMBER_LIST), &list);
}

#[test]
fn optional_default_presence_and_empty_messages_are_preserved() {
    // Present all-zero digest and present size zero are distinct from absent.
    let bytes = tail("4a20 0000000000000000000000000000000000000000000000000000000000000000 5000");
    assert_vector(
        Generation {
            members_sha256: Some([0; 32]),
            members_size: Some(0),
            ..generation()
        },
        bytes,
    );
    assert_vector(generation(), hex(CLAIMED));
    // No invented nonzero id/counter or monotonic generation constraint.
    assert_vector(
        Generation {
            maps: vec![layout()],
            type_changes: vec![change()],
            ..generation()
        },
        tail("3a00 4200"),
    );
    // Member erev zero is nonoptional and omitted.
    let mut bytes = hex("321d 0a1b 65 6161616161 6161616161 6161616161 6161616161 6161616161 61");
    let mut expected = hex(CLAIMED);
    expected.append(&mut bytes);
    assert_vector(
        Generation {
            members: vec![Member {
                record_name: A.to_owned(),
                erev: 0,
            }],
            ..generation()
        },
        expected,
    );
    assert_eq!(MemberList { members: Vec::new() }.encode(), Ok(Vec::new()));
    assert_members(&[], &MemberList { members: Vec::new() });
    // Known SHA-256 of empty content, independent of the production hash helper.
    assert!(
        MemberList::decode_asset(
            AssetInput::Bytes(&[]),
            &hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .try_into()
                .unwrap(),
            0
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn signed_ids_unsigned_counters_and_packed_sint32_boundaries() {
    for (id, literal) in [
        (0, "3a00"),
        (1, "3a020801"),
        (-1, "3a0b08ffffffffffffffffff01"),
        (i64::MIN, "3a0b0880808080808080808001"),
        (i64::MAX, "3a0a08ffffffffffffffff7f"),
    ] {
        assert_vector(
            Generation {
                maps: vec![LayoutMap {
                    notetype_id: id,
                    ..layout()
                }],
                ..generation()
            },
            tail(literal),
        );
    }
    assert_vector(
        Generation {
            maps: vec![LayoutMap {
                from_struct_gen: u64::MAX,
                to_struct_gen: u64::MAX,
                ..layout()
            }],
            ..generation()
        },
        tail("3a16 10ffffffffffffffffff01 18ffffffffffffffffff01"),
    );
    assert_vector(
        Generation {
            type_changes: vec![TypeChange {
                old_notetype_id: i64::MIN,
                old_struct_gen: u64::MAX,
                new_notetype_id: i64::MAX,
                new_struct_gen: u64::MAX,
                ..change()
            }],
            ..generation()
        },
        tail("422b 0880808080808080808001 10ffffffffffffffffff01 18ffffffffffffffff7f 20ffffffffffffffffff01"),
    );
    assert_vector(
        Generation {
            maps: vec![LayoutMap {
                field_map: vec![-1, 0, 1, i32::MAX],
                template_map: vec![-1],
                ..layout()
            }],
            ..generation()
        },
        tail("3a0d 2208010002feffffff0f 2a0101"),
    );
    assert_vector(
        Generation {
            type_changes: vec![TypeChange {
                field_map: vec![-1, i32::MAX],
                template_map: vec![-1, 0],
                ..change()
            }],
            ..generation()
        },
        tail("420c 2a0601feffffff0f 32020100"),
    );
    let mut bytes =
        hex("0a28 0a1b 65 6161616161 6161616161 6161616161 6161616161 6161616161 61 10ffffffffffffffffff01");
    let list = MemberList {
        members: vec![Member {
            record_name: A.to_owned(),
            erev: u64::MAX,
        }],
    };
    assert_eq!(list.encode().unwrap(), bytes);
    assert_members(&bytes, &list);
    bytes[0] = 0x32;
    let mut envelope = hex(CLAIMED);
    envelope.extend(bytes);
    assert_vector(
        Generation {
            members: vec![Member {
                record_name: A.to_owned(),
                erev: u64::MAX,
            }],
            ..generation()
        },
        envelope,
    );
}

#[test]
fn sorted_unique_fields_and_signed_pair_order_are_enforced() {
    let sorted = Generation {
        reasons: vec![1, 10],
        anchors: vec![A.to_owned(), B.to_owned()],
        members: vec![
            Member {
                record_name: A.to_owned(),
                erev: 0,
            },
            Member {
                record_name: B.to_owned(),
                erev: 0,
            },
        ],
        maps: vec![
            LayoutMap {
                notetype_id: -1,
                ..layout()
            },
            layout(),
        ],
        type_changes: vec![
            TypeChange {
                old_notetype_id: -1,
                new_notetype_id: i64::MAX,
                ..change()
            },
            TypeChange {
                new_notetype_id: -1,
                ..change()
            },
            change(),
        ],
        ..generation()
    };
    // This check supplements literal vectors with a multi-entry sorting case.
    assert_eq!(
        Generation::decode(&sorted.encode().unwrap()),
        Ok(DecodeOutcome::Supported(sorted.clone()))
    );
    for duplicate in [false, true] {
        let mut value = sorted.clone();
        if duplicate {
            value.reasons[1] = value.reasons[0];
        } else {
            value.reasons.reverse();
        }
        assert!(value.encode().is_err());
        value = sorted.clone();
        if duplicate {
            value.anchors[1] = value.anchors[0].clone();
        } else {
            value.anchors.reverse();
        }
        assert!(value.encode().is_err());
        value = sorted.clone();
        if duplicate {
            value.members[1] = value.members[0].clone();
        } else {
            value.members.reverse();
        }
        assert!(value.encode().is_err());
        value = sorted.clone();
        if duplicate {
            value.maps[1] = value.maps[0].clone();
        } else {
            value.maps.reverse();
        }
        assert!(value.encode().is_err());
        value = sorted.clone();
        if duplicate {
            value.type_changes[1] = value.type_changes[0].clone();
        } else {
            value.type_changes.reverse();
        }
        assert!(value.encode().is_err());
    }
    for suffix in [
        "3a020801 3a00",
        "3a00 3a00",
        "42021801 4200",
        "4200 4200",
        "42020801 4200",
    ] {
        assert!(Generation::decode(&tail(suffix)).is_err());
    }
    for reasons in [
        "1a020101",
        "1a020201",
        "1a0100",
        "1a010b",
        "1a058180808010",
        "1801",
        "1a00",
        "1a01011a0102",
    ] {
        let mut bytes = hex(CLAIMED);
        bytes.splice(20..20, hex(reasons));
        assert!(Generation::decode(&bytes).is_err());
    }
    let member = hex(MEMBER_LIST);
    let mut duplicates = member.clone();
    duplicates.extend(&member);
    assert!(asset(&duplicates).is_err());
    // Reverse two otherwise valid members, and reverse/duplicate inline anchors.
    let mut second = member.clone();
    second[30] = b'e'; // final base32 character: a -> e
    let mut ascending = member.clone();
    ascending.extend(&second);
    assert_members(
        &ascending,
        &MemberList {
            members: sorted
                .members
                .iter()
                .map(|member| Member {
                    record_name: member.record_name.clone(),
                    erev: 128,
                })
                .collect(),
        },
    );
    let mut descending = second;
    descending.extend(&member);
    assert!(asset(&descending).is_err());
    let mut anchors = member[..31].to_vec();
    anchors.drain(0..2); // remove MemberList wrapper, retain string field
    anchors[0] = 0x2a;
    for duplicate in [false, true] {
        let mut first = anchors.clone();
        if !duplicate {
            first[28] = b'e';
        }
        first.extend(&anchors);
        let mut bytes = hex(CLAIMED);
        bytes.extend(first);
        assert!(Generation::decode(&bytes).is_err());
    }
}

#[test]
fn required_identity_state_and_storage_combinations_are_checked() {
    for range in [0..2, 2..20, 20..22] {
        let mut bytes = hex(CLAIMED);
        bytes.drain(range);
        assert!(Generation::decode(&bytes).is_err());
    }
    for length in [0, 15, 17] {
        let mut bytes = hex("0801 12");
        bytes.push(length);
        bytes.extend(vec![0; length as usize]);
        bytes.extend(hex("2001"));
        assert_eq!(Generation::decode(&bytes), Err(CodecError::InvalidLength));
    }
    for state in ["00", "04", "8180808010"] {
        let mut bytes = hex(CLAIMED);
        bytes.truncate(21);
        bytes.extend(hex(state));
        assert!(Generation::decode(&bytes).is_err());
    }
    for suffix in [
        "5000",                                                                  // size without digest
        "4a20 0000000000000000000000000000000000000000000000000000000000000000", // digest without size
        "4a00 5000",
        "4a1f 00000000000000000000000000000000000000000000000000000000000000 5000",
        "4a21 000000000000000000000000000000000000000000000000000000000000000000 5000",
    ] {
        assert!(Generation::decode(&tail(suffix)).is_err());
    }
    let mut value = generation();
    value.members_size = Some(0);
    assert!(value.encode().is_err());
    value.members_sha256 = Some([0; 32]);
    value.members.push(Member {
        record_name: A.to_owned(),
        erev: 0,
    });
    assert!(value.encode().is_err());
    let mut bytes = tail("3220");
    bytes.extend(hex(MEMBER));
    bytes.extend(hex(
        "4a20 0000000000000000000000000000000000000000000000000000000000000000 5000",
    ));
    assert!(Generation::decode(&bytes).is_err());
    let asset_envelope = tail("4a20 0000000000000000000000000000000000000000000000000000000000000000 5000");
    for range in [22..56, 56..58] {
        let mut bytes = asset_envelope.clone();
        let field = bytes[range.clone()].to_vec();
        bytes.splice(range.end..range.end, field);
        assert!(Generation::decode(&bytes).is_err());
    }
}

#[test]
fn noncanonical_nested_fields_maps_and_record_names_are_rejected() {
    for suffix in [
        "3a020800",
        "3a021000",
        "3a021800", // explicit scalar defaults
        "3a0422002a00",
        "3a022200",                     // explicit empty packed lists
        "3a0420012002",                 // unpacked map
        "3a052201012200",               // duplicate packed field
        "3a052203818000",               // nonminimal packed -1
        "3a03220103",                   // -2
        "3a072205ffffffff0f",           // i32::MIN
        "3a0722058080808010",           // uint32 overflow
        "3a03220180",                   // truncated packed varint
        "3a0c220affffffffffffffffff01", // uint64::MAX does not fit sint32
        "3a020a00",
        "3a021200",
        "3a023001", // wire types/unknown tag
        "3a0408010801",
        "3a0410010801", // duplicate/reorder
        "42020800",
        "42021000",
        "42021800",
        "42022000",
        "42022a00",
        "42023200",
        "4203280101",
        "42032a0103",
        "42073205ffffffff0f",
        "420732058080808010",
        "420438012801",
        "420408010801",
        "420418010801",
        "42040a001001",
    ] {
        assert!(
            Generation::decode(&tail(suffix)).is_err(),
            "accepted nested malformed vector {suffix}"
        );
    }
    for field in [true, false] {
        for invalid in [-2, i32::MIN] {
            let mut map = layout();
            let mut pair = change();
            if field {
                map.field_map.push(invalid);
                pair.field_map.push(invalid);
            } else {
                map.template_map.push(invalid);
                pair.template_map.push(invalid);
            }
            assert!(
                Generation {
                    maps: vec![map],
                    ..generation()
                }
                .encode()
                .is_err()
            );
            assert!(
                Generation {
                    type_changes: vec![pair],
                    ..generation()
                }
                .encode()
                .is_err()
            );
        }
    }
    for name in [
        "",
        "eabc",
        "eaaaaaaaaaaaaaaaaaaaaaaaaab",
        "Eaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "eaaaaaaaaaaaaaaaaaaaaaaaaa0",
    ] {
        assert!(
            MemberList {
                members: vec![Member {
                    record_name: name.to_owned(),
                    erev: 0
                }]
            }
            .encode()
            .is_err()
        );
        assert!(
            Generation {
                anchors: vec![name.to_owned()],
                ..generation()
            }
            .encode()
            .is_err()
        );
    }
    let mut bad_utf8 = hex(MEMBER_LIST);
    bad_utf8[4] = 0xff;
    assert!(asset(&bad_utf8).is_err());
    for suffix in ["1000", "1001", "1801", "0a00", "00"] {
        let mut bytes = hex(MEMBER);
        bytes.extend(hex(suffix));
        // All these members are less than 128 bytes, so a literal single-byte
        // wrapper length independently frames the deliberately corrupt body.
        let mut list = vec![0x0a, bytes.len() as u8];
        list.extend(bytes);
        assert!(asset(&list).is_err());
    }
    for literal in ["0a00", "0a021001", "0a040a001001", "1001", "0a8000", "0a03ff"] {
        assert!(asset(&hex(literal)).is_err());
    }
}

#[test]
fn duplicates_unknown_defaults_nonminimal_varints_and_trailing_data_are_rejected() {
    for range in [0..2, 2..20, 20..22] {
        let mut bytes = hex(CLAIMED);
        let field = bytes[range.clone()].to_vec();
        bytes.splice(range.end..range.end, field);
        assert!(Generation::decode(&bytes).is_err());
    }
    for suffix in ["5801", "2001", "2000", "0801", "00", "ff", "3a80", "2a00", "3200"] {
        assert!(Generation::decode(&tail(suffix)).is_err());
    }
    for (range, replacement) in [(0..1, "8800"), (1..2, "8100"), (3..4, "9000"), (21..22, "8100")] {
        let mut bytes = hex(CLAIMED);
        bytes.splice(range, hex(replacement));
        assert!(Generation::decode(&bytes).is_err());
    }
    for index in [0, 2, 20] {
        let mut bytes = hex(CLAIMED);
        bytes[index] ^= 2;
        assert!(Generation::decode(&bytes).is_err());
    }
    let bytes = hex(CLAIMED);
    for end in 0..bytes.len() {
        assert!(Generation::decode(&bytes[..end]).is_err());
    }
    // Length/key varints inside the member asset, plus malformed suffixes.
    for (range, replacement) in [(0..1, "8a00"), (1..2, "a000"), (3..4, "9b00")] {
        let mut bytes = hex(MEMBER_LIST);
        bytes.splice(range, hex(replacement));
        assert!(asset(&bytes).is_err());
    }
    let mut bytes = hex(MEMBER_LIST);
    bytes[1] = 0x21; // adjust the wrapper so only erev is noncanonical
    bytes.splice(32..34, hex("808100"));
    assert!(asset(&bytes).is_err());
}

#[test]
fn asset_integrity_retryable_facts_and_exact_declared_limit() {
    let bytes = hex(MEMBER_LIST);
    let digest = sha256(&bytes);
    for (input, problem) in [
        (AssetInput::Absent, AssetProblem::Absent),
        (AssetInput::Unreadable, AssetProblem::Unreadable),
    ] {
        assert_eq!(
            MemberList::decode_asset(input, &digest, bytes.len() as u64),
            Err(ReceiveError::RetryableAsset(problem))
        );
    }
    assert_eq!(
        MemberList::decode_asset(AssetInput::Bytes(&bytes), &[0; 32], bytes.len() as u64),
        Err(ReceiveError::RetryableAsset(AssetProblem::Integrity))
    );
    for size in [0, bytes.len() as u64 - 1, bytes.len() as u64 + 1] {
        assert_eq!(
            MemberList::decode_asset(AssetInput::Bytes(&bytes), &digest, size),
            Err(ReceiveError::RetryableAsset(AssetProblem::Integrity))
        );
    }
    for size in [MAX_MEMBER_LIST_BYTES - 1, MAX_MEMBER_LIST_BYTES] {
        // At the declared ceiling the next fact is still the missing asset;
        // no artificial 100 MB allocation is needed to check this boundary.
        assert_eq!(
            MemberList::decode_asset(AssetInput::Absent, &digest, size),
            Err(ReceiveError::RetryableAsset(AssetProblem::Absent))
        );
        assert_vector(
            Generation {
                members_sha256: Some([0; 32]),
                members_size: Some(size),
                ..generation()
            },
            tail(if size == MAX_MEMBER_LIST_BYTES {
                "4a20 0000000000000000000000000000000000000000000000000000000000000000 5080808032"
            } else {
                "4a20 0000000000000000000000000000000000000000000000000000000000000000 50ffffff31"
            }),
        );
    }
    assert_eq!(
        MemberList::decode_asset(AssetInput::Absent, &digest, MAX_MEMBER_LIST_BYTES + 1),
        Err(ReceiveError::Refused(Refusal::Encoding(CodecError::Oversize)))
    );
    assert_eq!(
        Generation {
            members_sha256: Some([0; 32]),
            members_size: Some(MAX_MEMBER_LIST_BYTES + 1),
            ..generation()
        }
        .encode(),
        Err(CodecError::Oversize)
    );
    assert_eq!(
        Generation::decode(&tail(
            "4a20 0000000000000000000000000000000000000000000000000000000000000000 5081808032"
        )),
        Err(CodecError::Oversize)
    );
    // Integrity succeeds here; malformed wire is then a refusal, not a retry.
    assert_eq!(
        asset(&hex("1001")),
        Err(ReceiveError::Refused(Refusal::Encoding(CodecError::NonCanonical)))
    );
}

#[test]
fn exact_ceiling_canonical_member_asset_decodes_with_bounded_allocation() {
    // Prove the witness observes real allocations before constructing the asset.
    let (probe, observed) = decode_alloc::measure(|| std::hint::black_box(vec![0u8; 4096]));
    assert!(observed >= probe.capacity());
    assert!(observed >= 4096);
    drop(probe);

    assert_eq!(MAX_MEMBER_LIST_BYTES, 104_857_600);
    const CEILING: usize = 104_857_600;
    const MEMBERS: usize = CEILING / 31;
    // A zero-erev entry is 31 bytes: wrapper (2), name header (2), name (27).
    // The seven remaining bytes are the last member's tag and six-byte erev.
    assert_eq!(CEILING % 31, 7);
    const ALPHABET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";
    // Reserve the later over-limit byte too, avoiding a fixture reallocation.
    let mut bytes = Vec::with_capacity(CEILING + 1);
    for index in 0..MEMBERS {
        let last = index == MEMBERS - 1;
        let mut name = [b'2'; 27];
        name[0] = b'e';
        name[26] = b'a'; // canonical zero low padding bits
        let mut digits = index;
        for slot in name[21..26].iter_mut().rev() {
            *slot = ALPHABET[digits % 32];
            digits /= 32;
        }
        assert_eq!(digits, 0);
        // Independent literal framing; do not use the production encoder.
        bytes.extend_from_slice(&[0x0a, if last { 36 } else { 29 }, 0x0a, 27]);
        bytes.extend_from_slice(&name);
        if last {
            bytes.extend_from_slice(&[0x10, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]);
        }
    }
    assert_eq!(bytes.len(), CEILING);
    let digest = sha256(&bytes);

    // Fixture and its expected digest are prepared before measurement. Only
    // synchronous decode (including its integrity hash) runs in this window.
    let (decoded, decode_requested) =
        decode_alloc::measure(|| MemberList::decode_asset(AssetInput::Bytes(&bytes), &digest, CEILING as u64));
    let view = decoded.unwrap();
    assert_eq!(view.len(), MEMBERS);
    assert!(!view.is_empty());
    let ((count, final_erev, borrowed), iter_requested) = decode_alloc::measure(|| {
        let start = bytes.as_ptr() as usize;
        let end = start + bytes.len();
        let mut count = 0;
        let mut final_erev = 0;
        let mut borrowed = true;
        for member in view.iter() {
            let ptr = member.record_name.as_ptr() as usize;
            borrowed &= ptr >= start && ptr + member.record_name.len() <= end;
            count += 1;
            final_erev = member.erev;
        }
        (count, final_erev, borrowed)
    });
    assert_eq!(count, MEMBERS);
    assert_eq!(final_erev, 1 << 35);
    assert!(borrowed);
    let requested = decode_requested.saturating_add(iter_requested);
    assert!(requested < 64 * 1024, "decode/iteration requested {requested} bytes");
    assert!(bytes.capacity().saturating_add(requested) < 300_000_000);

    // The next actual byte exceeds the same full protocol limit, before hash
    // or wire validation; no second full-size fixture is necessary.
    bytes.push(0);
    assert_eq!(
        MemberList::decode_asset(AssetInput::Bytes(&bytes), &digest, bytes.len() as u64),
        Err(ReceiveError::Refused(Refusal::Encoding(CodecError::Oversize)))
    );
}

#[test]
fn exact_envelope_size_boundaries_and_future_format_probe() {
    // 22 fixed envelope bytes + 4 outer map bytes + 4 packed-field header bytes.
    let entries = MAX_GENERATION_BYTES - 30;
    for (n, literal_header) in [(entries - 1, "3ae5ff2f22e1ff2f"), (entries, "3ae6ff2f22e2ff2f")] {
        let mut expected = tail(literal_header);
        expected.resize(expected.len() + n, 0);
        assert_eq!(expected.len(), n + 30);
        assert_vector(
            Generation {
                maps: vec![LayoutMap {
                    field_map: vec![0; n],
                    ..layout()
                }],
                ..generation()
            },
            expected,
        );
    }
    assert_eq!(
        Generation {
            maps: vec![LayoutMap {
                field_map: vec![0; entries + 1],
                ..layout()
            }],
            ..generation()
        }
        .encode(),
        Err(CodecError::Oversize)
    );
    let mut over = hex("0801");
    over.resize(MAX_GENERATION_BYTES + 1, 0xff);
    assert_eq!(Generation::decode(&over), Err(CodecError::Oversize));
    // V1 at the cap reaches parsing; the next byte crosses the preallocation cap.
    over.pop();
    assert_eq!(Generation::decode(&over), Err(CodecError::NonCanonical));
    for (literal, format) in [("0802", 2), ("08ffffffff0f", u32::MAX)] {
        let mut bytes = hex(literal);
        assert_eq!(Generation::decode(&bytes), Ok(DecodeOutcome::UnsupportedFormat(format)));
        bytes.resize(MAX_GENERATION_BYTES + 1, 0xff);
        assert_eq!(Generation::decode(&bytes), Ok(DecodeOutcome::UnsupportedFormat(format)));
    }
    for literal in ["", "0800", "08", "0a0102", "088080808010", "08ffffffffffffffffff02"] {
        assert!(Generation::decode(&hex(literal)).is_err());
    }
}

#[test]
fn debug_output_keeps_opaque_facts_private() {
    assert_eq!(format!("{:?}", generation()), "Generation");
    assert_eq!(
        format!(
            "{:?}",
            Member {
                record_name: A.to_owned(),
                erev: 1
            }
        ),
        "Member"
    );
    assert_eq!(format!("{:?}", MemberList { members: Vec::new() }), "MemberList");
    let bytes = hex(MEMBER_LIST);
    let view = asset(&bytes).unwrap();
    assert_eq!(format!("{view:?}"), "MemberListView");
    assert_eq!(format!("{:?}", view.iter().next().unwrap()), "MemberRef");
    assert_eq!(format!("{:?}", layout()), "LayoutMap");
    assert_eq!(format!("{:?}", change()), "TypeChange");
}
