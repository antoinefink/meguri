//! S1.1 diagnostics from the actual normal job and attached-stage merge.
use super::*;

#[test]
fn unsent_own_graves_report_each_kind_once_in_numeric_order_and_exclude_other_refusals() {
    let f = Fixture::new(1);
    baseline(&f);
    let local_card = card(&f, f.cards[0]);
    let local_note = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .note(local_card.note_id)
        .unwrap()
        .unwrap();
    let local_deck = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(DeckId(1))
        .unwrap()
        .unwrap();
    let keys = [9003, 70, 901, 70]; // repeat keys; numeric order differs from input order
    let mut cards = Vec::new();
    let mut notes = Vec::new();
    let mut decks = Vec::new();
    for key in keys {
        let mut row = local_card.clone();
        row.id = CardId(key);
        row.note_id = NoteId(key);
        row.mtime = EpochSecs(NOW / 1000 + 3);
        cards.push(row);
        let mut row = local_note.clone();
        row.id = NoteId(key);
        row.guid = format!("diagnostic-{key}");
        row.mtime = EpochSecs(NOW / 1000 + 3);
        notes.push(row);
        let mut row = local_deck.clone();
        row.id = DeckId(key);
        row.name = format!("Incoming {key}");
        row.mtime = EpochSecs(NOW / 1000 + 3);
        decks.push(row);
    }
    // A missing note with its own grave cannot reject this card's own ID.
    let mut orphan = local_card.clone();
    orphan.id = CardId(70001);
    orphan.note_id = NoteId(70);
    orphan.mtime = EpochSecs(NOW / 1000 + 3);
    cards.push(orphan.clone());
    // Pending local rows refuse older remote values for an unrelated reason.
    let mut old_card = local_card.clone();
    old_card.mtime = EpochSecs(NOW / 1000 - 10);
    cards.push(old_card);
    let mut old_note = local_note.clone();
    old_note.mtime = EpochSecs(NOW / 1000 - 10);
    notes.push(old_note);
    let mut old_deck = local_deck.clone();
    old_deck.mtime = EpochSecs(NOW / 1000 - 10);
    decks.push(old_deck);
    let encoded = meguri_sync::wire::chunk::encode_apply_chunk(
        &meguri_sync::wire::chunk::OutgoingChunk {
            done: true,
            cards: &cards,
            notes: &notes,
            revlog: &[],
        },
        Usn(5),
    );
    let chunk = json_body_bytes(&encoded)["chunk"].to_string();
    let encoded = meguri_sync::wire::changes::encode_apply_changes(
        &meguri_sync::wire::changes::OutgoingChanges {
            notetypes: &[],
            decks: &decks,
            deck_configs: &[],
            tags: &[],
            config: None,
            created: None,
        },
        Usn(5),
    )
    .unwrap();
    let changes = json_body_bytes(&encoded)["changes"].to_string();
    let retry_chunk = chunk.clone();
    let retry_changes = changes.clone();
    let collection = f.collection.clone();
    let scm = schema(&f);
    let local_id = local_card.id;
    let local_nid = local_note.id;
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/applyChanges") {
            return Reply::ok(changes.as_bytes());
        }
        if r.path().ends_with("/chunk") {
            // These pending graves were not in S or this round's sent set.
            collection
                .run(
                    OpSpec::named("post snapshot deletions", OpSource::UserUndoable),
                    move |ctx| {
                        for key in [9003, 70, 901] {
                            for kind in [GraveKind::Card, GraveKind::Note, GraveKind::Deck] {
                                ctx.scope().add_grave(key, kind, Usn::PENDING)?;
                            }
                        }
                        let mut row = ctx.scope().card(local_id)?.unwrap();
                        row.flags = 4;
                        ctx.scope().put(&row)?;
                        let mut row = ctx.scope().note(local_nid)?.unwrap();
                        row.fields = "local witness\u{1f}back".into();
                        ctx.scope().put(&row)?;
                        let mut row = ctx.scope().deck(DeckId(1))?.unwrap();
                        row.name = "Local witness".into();
                        ctx.scope().put(&row)?;
                        Ok(())
                    },
                )
                .unwrap();
            return Reply::ok(chunk.as_bytes());
        }
        default_reply(r.path(), scm, NOW)
    });
    let events = f.host.engine().stream().subscribe();
    let merge = f.start(&stub.url()).wait().unwrap().merge.unwrap();
    let event = std::iter::from_fn(|| events.try_recv())
        .find(|e| e.seq == merge.seq.unwrap())
        .expect("merge event");
    let meguri_engine::EventKind::Changes(changes) = &event.kind else {
        panic!("expected Changes")
    };
    assert_eq!(changes.skipped_incoming_cards, [CardId(70), CardId(901), CardId(9003)]);
    assert_eq!(changes.skipped_incoming_notes, [NoteId(70), NoteId(901), NoteId(9003)]);
    assert_eq!(changes.skipped_incoming_decks, [DeckId(70), DeckId(901), DeckId(9003)]);
    assert_eq!(merge.skipped.len(), 9, "staging deduplicates repeated incoming keys");
    assert_eq!(changes.cards.iter().map(|r| r.id).collect::<Vec<_>>(), [orphan.id]);
    assert!(changes.notes.is_empty());
    assert!(changes.decks.is_empty());
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    for key in [70, 901, 9003] {
        assert!(rows.card(CardId(key)).unwrap().is_none());
        assert!(rows.note(NoteId(key)).unwrap().is_none());
        assert!(rows.deck(DeckId(key)).unwrap().is_none());
        for kind in [GraveKind::Card, GraveKind::Note, GraveKind::Deck] {
            assert_eq!(
                rows.row_image(Table::Graves, &RowKey::Pair(key, kind.stored()))
                    .unwrap()
                    .unwrap()[2]
                    .as_i64(),
                Some(-1)
            );
        }
    }
    assert_eq!(rows.card(orphan.id).unwrap().unwrap().note_id, NoteId(70));
    assert_eq!(rows.card(local_id).unwrap().unwrap().flags, 4);
    assert_eq!(rows.note(local_nid).unwrap().unwrap().fields, "local witness\u{1f}back");
    assert_eq!(rows.deck(DeckId(1)).unwrap().unwrap().name, "Local witness");
    drop(rows);
    // The next round sends those graves. Repeated incoming keys now apply;
    // neither the live pending grave nor the previous round is sufficient to skip.
    let events = f.host.engine().stream().subscribe();
    let next_stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/applyChanges") {
            return Reply::ok(retry_changes.as_bytes());
        }
        if r.path().ends_with("/chunk") {
            return Reply::ok(retry_chunk.as_bytes());
        }
        default_reply(r.path(), scm, NOW)
    });
    let next = f.start(&next_stub.url()).wait().unwrap().merge.unwrap();
    let event = std::iter::from_fn(|| events.try_recv())
        .find(|e| e.seq == next.seq.unwrap())
        .unwrap();
    let meguri_engine::EventKind::Changes(changes) = &event.kind else {
        panic!("expected Changes")
    };
    assert!(changes.skipped_incoming_cards.is_empty());
    assert!(changes.skipped_incoming_notes.is_empty());
    assert!(changes.skipped_incoming_decks.is_empty());
    assert!(next.skipped.is_empty());
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    for key in [70, 901, 9003] {
        assert!(rows.card(CardId(key)).unwrap().is_some());
        assert!(rows.note(NoteId(key)).unwrap().is_some());
        assert!(rows.deck(DeckId(key)).unwrap().is_some());
    }
}
