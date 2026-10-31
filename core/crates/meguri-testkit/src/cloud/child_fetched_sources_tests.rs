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
        };
        let bytes = b"small actual transport source".to_vec();
        let record = super::super::Record {
            name: "source".into(),
            record_type: "MGEntity".into(),
            fields: BTreeMap::from([
                ("e".into(), super::super::Field::EncryptedBytes(vec![1, 2, 3])),
                (
                    "pa".into(),
                    super::super::Field::Asset(super::super::Asset::synthetic(bytes.clone())),
                ),
            ]),
            system_fields: super::super::SystemFields::from_bytes(vec![4, 5]),
        };
        let callback = driver
            .stage_fetched(
                "original-sdk".into(),
                vec![record],
                "collection".into(),
                vec![6],
                Some(7),
            )
            .unwrap();
        let path = callback.records[0].asset_path.clone().unwrap();
        let failed = terminal.is_err();
        let worker = std::thread::spawn(move || {
            let Input::Fetched(original) = read_frame::<Input>(&mut peer).unwrap() else {
                panic!("original fetched command")
            };
            let source = original.records[0].asset_path.as_ref().unwrap();
            assert_eq!(std::fs::read(source).unwrap(), bytes);
            assert_eq!(original.instance, "original-sdk");
            assert_eq!(original.exchange, Some(7));
            write_frame(&mut peer, &ChildMessage::Paused(IncomingStage::FetchBeforeInbox)).unwrap();
            assert!(matches!(
                read_frame::<Input>(&mut peer).unwrap(),
                Input::DrainIncoming(1)
            ));
            write_frame(&mut peer, &ChildMessage::Paused(IncomingStage::ApplyAfterCommit)).unwrap();
            assert!(matches!(read_frame::<Input>(&mut peer).unwrap(), Input::ContinueWork));
            write_frame(&mut peer, &ChildMessage::Reply(Ok(serde_json::json!(3)))).unwrap();
            assert!(source.exists());
            assert!(matches!(read_frame::<Input>(&mut peer).unwrap(), Input::ContinueWork));
            assert!(source.exists());
            write_frame(&mut peer, &ChildMessage::Reply(terminal)).unwrap();
        });
        let error = driver.call::<()>(Input::Fetched(callback.clone())).unwrap_err();
        assert!(is_work_paused(error.as_ref()));
        let error = driver.call::<usize>(Input::DrainIncoming(1)).unwrap_err();
        assert!(is_work_paused(error.as_ref()));
        assert!(path.exists());
        driver.resume_incoming_work().unwrap();
        assert!(
            path.exists(),
            "another callback's terminal reply cannot release this source"
        );
        // The callback resumes with the original command/path even if its SDK
        // has retired. Its terminal error must survive the other queued reply.
        driver.resume_incoming_work().unwrap();
        assert!(
            !path.exists(),
            "terminal success/error/cancellation releases the source"
        );
        assert_eq!(driver.call::<usize>(Input::DrainIncoming(1)).unwrap(), 3);
        assert_eq!(driver.call::<()>(Input::Fetched(callback)).is_err(), failed);
        assert!(driver.completed_work.is_empty());
        worker.join().unwrap();
    }
}
