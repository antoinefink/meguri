use meguri_model::{card::CardQueue, proto::deck_config::DeckConfigMessage};

pub fn allowed(answered: CardQueue, sibling: CardQueue, preset: &DeckConfigMessage) -> bool {
    match sibling {
        CardQueue::New => preset.bury_new,
        CardQueue::Review => {
            preset.bury_reviews
                && matches!(
                    answered,
                    CardQueue::Learning | CardQueue::Preview | CardQueue::InterdayLearning | CardQueue::Review
                )
        }
        CardQueue::InterdayLearning => {
            preset.bury_interday_learning
                && matches!(
                    answered,
                    CardQueue::Learning | CardQueue::Preview | CardQueue::InterdayLearning
                )
        }
        _ => false,
    }
}
