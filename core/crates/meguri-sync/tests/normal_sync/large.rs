use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
fn start_review(f: &Fixture) -> (meguri_engine::SessionId, meguri_engine::session::Head) {
    match f.host.review().start(DeckId(2), None).unwrap() {
        meguri_engine::session::StartResult::Started { session_id, head, .. } => (session_id, head),
        other => panic!("{other:?}"),
    }
}
fn answer_head(
    f: &Fixture,
    session: meguri_engine::SessionId,
    head: meguri_engine::session::Head,
) -> meguri_engine::session::Head {
    let item = &head.items[0];
    match f
        .host
        .review()
        .answer(
            session,
            item.card_id,
            meguri_testkit::normal_support::Rating::Good,
            item.state_token,
            EpochMillis(f.clock.now_ms()),
            3000,
        )
        .unwrap()
    {
        meguri_engine::session::AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        other => panic!("{other:?}"),
    }
}
fn large_fixture() -> Fixture {
    let f = Fixture::new(150005);
    let ids = f.cards[150000..].to_vec();
    f.collection
        .run(
            OpSpec::named("review deck fixture", OpSource::UserUndoable),
            move |ctx| {
                let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
                deck.id = DeckId(2);
                deck.name = "Review during merge".into();
                ctx.scope().put(&deck)?;
                for id in ids {
                    let mut c = ctx.scope().card(id)?.unwrap();
                    c.deck_id = DeckId(2);
                    ctx.scope().put(&c)?;
                }
                Ok(())
            },
        )
        .unwrap();
    baseline(&f);
    f
}
fn large_server(f: &Fixture) -> SyncStub {
    let ids = Arc::new(f.cards[..150000].to_vec());
    let prototype = card(f, ids[0]);
    let cursor = AtomicUsize::new(0);
    let scm = schema(f);
    let clock = f.clock.clone();
    SyncStub::serve(move |r| {
        if r.path().ends_with("start") {
            cursor.store(0, Ordering::SeqCst);
        }
        if r.path().ends_with("/chunk") {
            let start = cursor.fetch_add(250, Ordering::SeqCst);
            let mut rows = Vec::new();
            for at in start..(start + 250).min(ids.len()) {
                let mut c = prototype.clone();
                c.id = ids[at];
                c.note_id = NoteId(ids[at].0);
                c.flags = 2;
                c.usn = Usn(5);
                rows.push(c);
            }
            let body = meguri_sync::wire::chunk::encode_apply_chunk(
                &meguri_sync::wire::chunk::OutgoingChunk {
                    done: rows.len() < 250,
                    cards: &rows,
                    notes: &[],
                    revlog: &[],
                },
                Usn(5),
            );
            return Reply::ok(json_body_bytes(&body)["chunk"].to_string().as_bytes());
        }
        default_reply(r.path(), scm, clock.now_ms())
    })
}
#[test]
fn suspend_two_seconds_with_five_queued_answers_rolls_back_merge_at_150k() {
    let f = large_fixture();
    let (session, mut head) = start_review(&f);
    let server = large_server(&f);
    let token = Arc::new(Mutex::new(None::<Arc<meguri_engine::CancelToken>>));
    let hook_token = token.clone();
    let first = AtomicBool::new(true);
    let (entered, merging) = mpsc::sync_channel(1);
    let _hook = normal::test_hooks::install(Arc::new(move |point| {
        if matches!(
            point,
            normal::test_hooks::Point::Merged {
                table: Table::Cards,
                ..
            }
        ) && first.swap(false, Ordering::SeqCst)
        {
            entered.send(()).unwrap();
            loop {
                if hook_token.lock().unwrap().as_ref().is_some_and(|t| t.is_cancelled()) {
                    break;
                }
                std::thread::yield_now();
            }
        }
    }));
    let task = f.start(&server.url());
    *token.lock().unwrap() = Some(task.handle.cancel.clone());
    merging.recv_timeout(Duration::from_secs(120)).unwrap();
    for _ in 0..5 {
        head = answer_head(&f, session, head);
    }
    let began = meguri_engine::monotonic::now();
    assert_eq!(
        f.host.engine().suspend(Duration::from_secs(2)).unwrap(),
        meguri_engine::SuspendOutcome::Suspended
    );
    assert!(meguri_engine::monotonic::now() - began <= Duration::from_secs(2));
    assert_eq!(task.wait().unwrap_err().kind, ErrorKind::Interrupted);
    f.host.engine().resume().unwrap();
    assert_eq!(card(&f, f.cards[0]).flags, 0);
    for id in &f.cards[150000..] {
        assert_eq!(card(&f, *id).reps, 1);
    }
    // A new ordinary op plus successful next round proves no write transaction
    // or attachment escaped rollback. The same 150k payload is replayed.
    f.collection
        .run(OpSpec::named("after rollback", OpSource::Rollover), |_| Ok(()))
        .unwrap();
    drop(_hook);
    let next = large_server(&f);
    f.start(&next.url()).wait().unwrap();
    assert_eq!(card(&f, f.cards[0]).flags, 2);
}
#[test]
fn merge_150k_updates_preserves_real_overlay_answers_and_write_budget_at_150k() {
    let f = large_fixture();
    let (session, mut head) = start_review(&f);
    f.collection.record_durability_events();
    let server = large_server(&f);
    let first = AtomicBool::new(true);
    let (entered, merging) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let released = Mutex::new(released);
    let _hook = normal::test_hooks::install(Arc::new(move |point| {
        if matches!(
            point,
            normal::test_hooks::Point::Merged {
                table: Table::Cards,
                ..
            }
        ) && first.swap(false, Ordering::SeqCst)
        {
            entered.send(()).unwrap();
            released.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
        }
    }));
    let task = f.start(&server.url());
    merging.recv_timeout(Duration::from_secs(120)).unwrap();
    // Coordinate one real accepted answer at the writer boundary. Further
    // answers follow the actual 2 s cadence while the real merge is running.
    let mut answered = vec![head.items[0].card_id];
    head = answer_head(&f, session, head);
    let declared = f
        .collection
        .write_delay()
        .expect("150k merge declares writer delay")
        .declared;
    let (finished, done) = mpsc::sync_channel(1);
    let worker = f
        .host
        .engine()
        .threads()
        .spawn_owned(
            "normal merge waiter",
            meguri_engine::Qos::UserInitiated,
            "jobs",
            move |_| {
                finished.send(task.wait()).unwrap();
            },
        )
        .unwrap();
    release.send(()).unwrap();
    let outcome = loop {
        match done.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => break result.unwrap(),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !head.items.is_empty() {
                    answered.push(head.items[0].card_id);
                    head = answer_head(&f, session, head);
                }
            }
            Err(e) => panic!("merge waiter: {e}"),
        }
    };
    worker.join().unwrap();
    let merged = outcome.merge.unwrap();
    assert_eq!(merged.write_budget, declared);
    assert!(merged.timing.rows >= 150000);
    eprintln!(
        "150k merge: declared {:?}, observed {:?}, rows {}",
        merged.write_budget, merged.timing.write, merged.timing.rows
    );
    let committed = meguri_storage::counting_vfs::clock::now();
    loop {
        assert!(
            meguri_storage::counting_vfs::clock::now() - committed <= Duration::from_secs(1),
            "overlay answers not durable within 1 s after merge"
        );
        let applied = answered.iter().all(|id| card(&f, *id).reps == 1);
        if applied
            && f.vfs.records().iter().any(|r| {
                r.filename.ends_with("-wal")
                    && r.result == 0
                    && r.started >= committed
                    && r.completed <= committed + Duration::from_secs(1)
            })
        {
            break;
        }
        std::thread::yield_now();
    }
    for id in answered {
        assert_eq!(card(&f, id).reps, 1);
    }
    assert_eq!(card(&f, f.cards[0]).flags, 2);
}
