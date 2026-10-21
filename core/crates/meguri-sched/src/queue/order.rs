use crate::sqlfns::hash;
use meguri_model::card::Card;

pub fn gather(cards: &mut [Card], decks: &[meguri_model::ids::DeckId], mode: i32, today: u32) {
    let salt = i64::from(today.wrapping_mul(2654435761));
    let place = |c: &Card| decks.iter().position(|d| *d == c.deck_id).unwrap();
    let signed = |id: i64| hash(&[id, salt]) as i64;
    cards.sort_by(|a, b| match mode {
        1 => a
            .due
            .cmp(&b.due)
            .then(a.ord.cmp(&b.ord))
            .then(a.deck_id.cmp(&b.deck_id))
            .then(a.id.cmp(&b.id)),
        2 => b
            .due
            .cmp(&a.due)
            .then(a.ord.cmp(&b.ord))
            .then(a.deck_id.cmp(&b.deck_id))
            .then(a.id.cmp(&b.id)),
        3 => signed(a.note_id.0).cmp(&signed(b.note_id.0)).then(a.ord.cmp(&b.ord)),
        4 => signed(a.id.0).cmp(&signed(b.id.0)),
        5 => place(a)
            .cmp(&place(b))
            .then(signed(a.note_id.0).cmp(&signed(b.note_id.0)))
            .then(a.ord.cmp(&b.ord))
            .then(a.id.cmp(&b.id)),
        _ => place(a)
            .cmp(&place(b))
            .then(a.due.cmp(&b.due))
            .then(a.ord.cmp(&b.ord))
            .then(a.id.cmp(&b.id)),
    });
}
pub fn sort_new(cards: &mut [Card], mode: i32, today: u32) {
    let h = |id: i64| hash(&[id, i64::from(today)]);
    match mode {
        1 => {}
        2 => cards.sort_by_key(|c| (c.ord, h(c.id.0))),
        3 => cards.sort_by_key(|c| (h(c.note_id.0), c.ord)),
        4 => cards.sort_by_key(|c| h(c.id.0)),
        _ => cards.sort_by_key(|c| c.ord),
    }
}
