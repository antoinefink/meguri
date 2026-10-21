//! One counted tree snapshot per call, preserving the host provider's order.
use crate::{
    callback,
    error::CoreError,
    review_session::{ReviewCounts, ReviewDeckStudyState},
    runtime,
};
use meguri_model::{ids::DeckId, proto::deck::DeckKind};
use meguri_sched::counts::deck_tree::Node;
use meguri_storage::{ReadRows, ReaderRole};

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CountedDeckNode {
    pub id: i64,
    pub name: String,
    pub filtered: bool,
    pub study_collapsed: bool,
    pub browser_collapsed: bool,
    pub counts: ReviewCounts,
    pub children: Vec<CountedDeckNode>,
}
impl From<Node> for CountedDeckNode {
    fn from(node: Node) -> Self {
        Self {
            id: node.id.0,
            name: node.name,
            filtered: node.filtered,
            study_collapsed: node.study_collapsed,
            browser_collapsed: node.browser_collapsed,
            counts: node.counts.into(),
            children: node.children.into_iter().map(Into::into).collect(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CountedDeckTree {
    pub generation: u64,
    pub next_change_at: i64,
    pub nodes: Vec<CountedDeckNode>,
}
#[uniffi::export]
pub fn deck_tree_with_counts() -> Result<CountedDeckTree, CoreError> {
    callback::enter("deck_tree_with_counts");
    let host = runtime::host()?;
    let collection = host.collection()?;
    let tree = host.queues().deck_counts(&collection)?;
    Ok(CountedDeckTree {
        generation: collection.generation().0,
        next_change_at: tree.next_change_at,
        nodes: tree.nodes.into_iter().map(Into::into).collect(),
    })
}
#[uniffi::export]
pub fn deck_study_state(deck_id: i64) -> Result<ReviewDeckStudyState, CoreError> {
    callback::enter("deck_study_state");
    let host = runtime::host()?;
    Ok(host
        .queues()
        .deck_study_state(&host.collection()?, Some(DeckId(deck_id)))?
        .into())
}
#[uniffi::export]
pub fn deck_description_html(deck_id: i64) -> Result<String, CoreError> {
    callback::enter("deck_description_html");
    let host = runtime::host()?;
    let collection = host.collection()?;
    let reader = collection.reader(ReaderRole::Query)?;
    let deck = reader
        .deck(DeckId(deck_id))
        .map_err(meguri_engine::EngineError::from)?
        .ok_or_else(|| CoreError::InvalidArgument {
            message: format!("deck {deck_id} does not exist"),
        })?;
    Ok(match &deck.kind.kind {
        DeckKind::Normal(normal) => {
            meguri_text::description::description_html(&normal.description, normal.markdown_description)
        }
        DeckKind::Filtered(_) => String::new(),
    })
}
