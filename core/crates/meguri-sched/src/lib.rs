//! Pure scheduling: day clock, queues, learning steps, FSRS, fuzz, load
//! balancer, answer rows, memory states and optimizer training. No executor,
//! undo or event access; those live in meguri-engine.

pub mod states;
pub mod timing;
