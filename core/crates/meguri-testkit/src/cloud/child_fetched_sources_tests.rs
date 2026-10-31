//! Source lifetime only: the real Service/runtime fetch regressions remain separate.
use super::*;

#[test]
fn staged_sources_survive_suspended_callbacks_until_the_original_terminal_reply() {
    for terminal in [
        Ok(serde_json::Value::Null),
        Err(ChildFailure::RuntimeStaleIncarnation),
        Err(ChildFailure::classify(Box::new(runtime::RuntimeError::Engine(
            meguri_engine::EngineError::Cancelled,
        )))),
    ] {
        let (parent, mut peer) = UnixStream::pair().unwrap();
        parent.set_read_timeout(Some(IPC_TIMEOUT)).unwrap();
        peer.set_read_timeout(Some(IPC_TIMEOUT)).unwrap();
        let mut driver = ChildDriver {
            child: None,
            stream: parent,
            keys: None,
            queued: None,
            queued_response: None,
            pending_work: None,
            suspended_work: Vec::new(),
            completed_work: Vec::new(),
            paused_work: None,
            fetched_sources: BTreeMap::new(),
            asset_commands: BTreeMap::new(),
        };
        let bytes = b"small actual transport source".to_vec();
        let envelope = vec![1, 2, 3];
        let inline_envelope = vec![0xab; codec::INLINE_ENVELOPE_BYTES];
        let record = super::super::Record {
            name: "source".into(),
            record_type: "MGEntity".into(),
            fields: BTreeMap::from([
                ("e".into(), super::super::Field::EncryptedBytes(envelope.clone())),
                (
                    "pa".into(),
                    super::super::Field::Asset(super::super::Asset::synthetic(bytes.clone())),
                ),
            ]),
            system_fields: super::super::SystemFields::from_bytes(vec![4, 5]),
        };
        let inline_record = super::super::Record {
            name: "inline-source".into(),
            record_type: "MGEntity".into(),
            fields: BTreeMap::from([("e".into(), super::super::Field::EncryptedBytes(inline_envelope.clone()))]),
            system_fields: super::super::SystemFields::from_bytes(vec![8, 9]),
        };
        let callback = driver
            .stage_fetched(
                "original-sdk".into(),
                vec![record, inline_record],
                "collection".into(),
                vec![6],
                Some(7),
            )
            .unwrap();
        let path = callback.records[0].asset_path.clone().unwrap();
        let envelope_path = callback.envelopes.as_ref().unwrap().path.clone();
        assert!(callback.records.iter().all(|input| input.record.envelope.is_empty()));
        let failed = terminal.is_err();
        let worker = std::thread::spawn(move || {
            let Input::Fetched(original) = read_frame::<Input>(&mut peer).unwrap() else {
                panic!("original fetched command")
            };
            let source = original.records[0].asset_path.as_ref().unwrap();
            assert_eq!(std::fs::read(source).unwrap(), bytes);
            assert_eq!(original.instance, "original-sdk");
            assert_eq!(original.exchange, Some(7));
            assert_eq!(original.zone, "collection");
            assert_eq!(original.serialization, vec![6]);
            assert!(original.records.iter().all(|input| input.record.envelope.is_empty()));
            let envelope_source = original.envelopes.as_ref().unwrap();
            assert_eq!(envelope_source.ranges, vec![(0, 3), (3, inline_envelope.len() as u64)]);
            let check_envelopes = || {
                let hydrated =
                    incoming_driver::hydrate_fetched_records(original.records.clone(), original.envelopes.clone())
                        .unwrap();
                assert_eq!(hydrated[0].record.name, "source");
                assert_eq!(hydrated[0].record.envelope, envelope);
                assert_eq!(hydrated[0].record.system_fields, vec![4, 5]);
                assert_eq!(hydrated[0].asset_path.as_ref(), Some(source));
                assert_eq!(hydrated[1].record.name, "inline-source");
                assert_eq!(hydrated[1].record.envelope, inline_envelope);
                assert_eq!(hydrated[1].record.system_fields, vec![8, 9]);
                assert!(hydrated[1].asset_path.is_none());
            };
            check_envelopes();
            write_frame(&mut peer, &ChildMessage::Paused(IncomingStage::FetchBeforeInbox)).unwrap();
            assert!(matches!(
                read_frame::<Input>(&mut peer).unwrap(),
                Input::DrainIncoming(1)
            ));
            write_frame(&mut peer, &ChildMessage::Paused(IncomingStage::ApplyAfterCommit)).unwrap();
            assert!(matches!(read_frame::<Input>(&mut peer).unwrap(), Input::ContinueWork));
            write_frame(&mut peer, &ChildMessage::Reply(Ok(serde_json::json!(3)))).unwrap();
            assert!(source.exists());
            assert!(envelope_source.path.exists());
            check_envelopes();
            assert!(matches!(read_frame::<Input>(&mut peer).unwrap(), Input::ContinueWork));
            assert!(source.exists());
            assert!(envelope_source.path.exists());
            check_envelopes();
            write_frame(&mut peer, &ChildMessage::Reply(terminal)).unwrap();
            let Input::Fetched(empty) = read_frame::<Input>(&mut peer).unwrap() else {
                panic!("empty fetched command")
            };
            assert!(empty.records.is_empty());
            assert!(empty.envelopes.is_none());
            assert_eq!(empty.instance, "empty-sdk");
            assert_eq!(empty.zone, "empty-zone");
            assert_eq!(empty.serialization, vec![10, 11]);
            assert_eq!(empty.exchange, Some(12));
            assert!(
                incoming_driver::hydrate_fetched_records(empty.records, empty.envelopes)
                    .unwrap()
                    .is_empty()
            );
            write_frame(&mut peer, &ChildMessage::Reply(Ok(serde_json::Value::Null))).unwrap();
        });
        let error = driver.call::<()>(Input::Fetched(callback.clone())).unwrap_err();
        assert!(is_work_paused(error.as_ref()));
        let error = driver.call::<usize>(Input::DrainIncoming(1)).unwrap_err();
        assert!(is_work_paused(error.as_ref()));
        assert!(path.exists());
        assert!(envelope_path.exists());
        driver.resume_incoming_work().unwrap();
        assert!(
            path.exists(),
            "another callback's terminal reply cannot release this source"
        );
        assert!(
            envelope_path.exists(),
            "another callback's reply cannot release the envelope spool"
        );
        // The callback resumes with the original command/path even if its SDK
        // has retired. Its terminal error must survive the other queued reply.
        driver.resume_incoming_work().unwrap();
        assert!(
            !path.exists(),
            "terminal success/error/cancellation releases the source"
        );
        assert!(
            !envelope_path.exists(),
            "original terminal reply releases the envelope spool"
        );
        assert_eq!(driver.call::<usize>(Input::DrainIncoming(1)).unwrap(), 3);
        assert_eq!(driver.call::<()>(Input::Fetched(callback)).is_err(), failed);
        assert!(driver.completed_work.is_empty());
        assert!(driver.fetched_sources.is_empty());
        let empty = driver
            .stage_fetched("empty-sdk".into(), vec![], "empty-zone".into(), vec![10, 11], Some(12))
            .unwrap();
        driver.call::<()>(Input::Fetched(empty)).unwrap();
        assert!(driver.fetched_sources.is_empty());
        worker.join().unwrap();
    }
}

#[test]
fn fetched_envelope_projection_rejects_invalid_ranges_and_mixed_inline_bytes() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&[1, 2, 3]).unwrap();
    let record = FetchedAssetRecord {
        record: Saved {
            name: "source".into(),
            envelope: Vec::new(),
            system_fields: vec![4, 5],
        },
        asset_path: None,
    };
    // Out-of-order, short source and overbound lengths cannot allocate or read
    // beyond the exact callback spool. No core envelope validation lives here.
    for range in [
        (1, 2),
        (0, 4),
        (0, super::super::Limits::default().non_asset_per_record + 1),
    ] {
        let source = FetchedEnvelopeSource {
            path: file.path().to_path_buf(),
            ranges: vec![range],
        };
        assert!(incoming_driver::hydrate_fetched_records(vec![record.clone()], Some(source)).is_err());
    }
    let source = FetchedEnvelopeSource {
        path: file.path().to_path_buf(),
        ranges: vec![(0, 3)],
    };
    let mut mixed = record.clone();
    mixed.record.envelope = vec![1];
    assert!(incoming_driver::hydrate_fetched_records(vec![mixed], Some(source.clone())).is_err());
    assert!(incoming_driver::hydrate_fetched_records(vec![record], None).is_err());
    assert!(incoming_driver::hydrate_fetched_records(Vec::new(), Some(source)).is_err());
}
