use meguri_model::{
    card::{Card, CardQueue, CardType},
    ids::DeckId,
    rng::RandomSource,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidDays {
    Empty,
    Malformed,
    TooLarge,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Days {
    pub minimum: u32,
    pub maximum: u32,
    pub reset_interval: bool,
}
impl Days {
    pub fn parse(text: &str) -> Result<Self, InvalidDays> {
        if text.is_empty() {
            return Err(InvalidDays::Empty);
        }
        let reset_interval = text.ends_with('!');
        let body = text.strip_suffix('!').unwrap_or(text);
        let parts = body.split('-').collect::<Vec<_>>();
        if parts.len() > 2
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(InvalidDays::Malformed);
        }
        let bound = |p: &str| p.parse::<u32>().map_err(|_| InvalidDays::TooLarge);
        let a = bound(parts[0])?;
        let b = if parts.len() == 2 { bound(parts[1])? } else { a };
        Ok(Self {
            minimum: a.min(b),
            maximum: a.max(b),
            reset_interval,
        })
    }
    pub fn draw(self, random: &dyn RandomSource) -> u32 {
        if self.minimum == self.maximum {
            return self.minimum;
        }
        let size = u64::from(self.maximum) - u64::from(self.minimum) + 1;
        // Rejection sampling avoids modulo bias, including the full u32 range.
        let reject = size.wrapping_neg() % size;
        loop {
            let bits = random.next_u64();
            if bits >= reject {
                return (u64::from(self.minimum) + bits % size) as u32;
            }
        }
    }
}
pub fn apply(card: &mut Card, days: u32, today: u32, boundary: i64, starting_ease: f32) {
    let interval = if card.queue == CardQueue::New || card.interval == 0 {
        0
    } else if let Some(last) = card.data.last_review_secs {
        let elapsed = if last >= boundary {
            0
        } else {
            (i128::from(boundary) - i128::from(last)) / 86_400
        };
        (elapsed as u32).wrapping_add(days)
    } else {
        let stored = if card.original_deck_id.0 != 0 {
            card.original_due
        } else {
            card.due
        };
        let due_day = if stored > 1_000_000_000 {
            i64::from(today) + (i64::from(stored) - boundary) / 86_400
        } else {
            i64::from(stored)
        };
        (i64::from(card.interval) + i64::from(today) + i64::from(days) - due_day).clamp(0, i64::from(u32::MAX)) as u32
    };
    crate::model::position::save_set_due_position(card);
    if card.original_deck_id.0 != 0 {
        card.deck_id = card.original_deck_id;
        card.original_deck_id = DeckId(0);
        card.original_due = 0;
    }
    card.card_type = CardType::Review;
    card.queue = CardQueue::Review;
    card.due = today.wrapping_add(days) as i32;
    card.interval = interval;
    if card.ease_factor == 0 {
        card.ease_factor = (starting_ease * 1000.0_f32).round() as u16;
    }
}
pub fn log_factor(card: &Card) -> u32 {
    card.data.memory_state.map_or(u32::from(card.ease_factor), |m| {
        (((m.difficulty - 1.0_f32) / 9.0_f32 + 0.1_f32) * 1000.0_f32).clamp(0.0, 65535.0) as u32
    })
}
