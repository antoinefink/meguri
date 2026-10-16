//! The bounded job pool (`engine/jobs` J2): a fixed set of worker threads
//! from the thread registry, at user-initiated QoS (the user started these
//! jobs and watches their progress; at utility, Apple platforms coalesce
//! their timers), taking queued work in FIFO order.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::threads::{Qos, ThreadRegistry};

type Work = Box<dyn FnOnce() + Send>;

pub(crate) struct JobPool {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Work>,
    stop: bool,
}

impl JobPool {
    pub(crate) fn new(threads: &ThreadRegistry, workers: usize) -> io::Result<JobPool> {
        let shared = Arc::new(Shared::default());
        for n in 1..=workers {
            let shared = Arc::clone(&shared);
            threads.spawn(format!("meguri-job-{n}"), Qos::UserInitiated, move |activity| {
                while let Some(work) = shared.next() {
                    let _busy = activity.busy();
                    work();
                }
            })?;
        }
        Ok(JobPool { shared })
    }

    pub(crate) fn run(&self, work: impl FnOnce() + Send + 'static) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .queue
            .push_back(Box::new(work));
        self.shared.ready.notify_one();
    }
}

impl Shared {
    /// The next work, waiting for it; `None` once the pool stops.
    fn next(&self) -> Option<Work> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(work) = state.queue.pop_front() {
                return Some(work);
            }
            if state.stop {
                return None;
            }
            state = self.ready.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Drop for JobPool {
    /// The workers finish the queued work, then end.
    fn drop(&mut self) {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner).stop = true;
        self.shared.ready.notify_all();
    }
}
