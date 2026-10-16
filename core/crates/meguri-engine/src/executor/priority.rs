//! The two queues of the writer (`engine/op-model` O2): interactive ops are
//! taken first, in arrival order; background ops and maintenance tasks only
//! when no interactive op waits. Nothing preempts a running op.

use std::collections::VecDeque;

use crate::executor::source::QueueKind;
use crate::executor::writer::Task;

#[derive(Default)]
pub(crate) struct Queues {
    interactive: VecDeque<Task>,
    background: VecDeque<Task>,
}

impl Queues {
    pub(crate) fn push(&mut self, task: Task) {
        match task.meta.queue {
            QueueKind::Interactive => self.interactive.push_back(task),
            QueueKind::Background => self.background.push_back(task),
        }
    }

    /// The next task the writer runs.
    pub(crate) fn pop_next(&mut self) -> Option<Task> {
        self.interactive.pop_front().or_else(|| self.background.pop_front())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.interactive.is_empty() && self.background.is_empty()
    }
}
