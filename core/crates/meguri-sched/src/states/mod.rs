//! Answer states of a card (specs/sched/). So far: interval fuzz
//! (`sched/fuzz`) and the FSRS answer states of a review card with the load
//! balancer off (`sched/fsrs-transitions` T1-T5), written for the
//! floating-point probe (`conformance/float-determinism`). U45 adds learning
//! steps, the other card kinds, short-term scheduling and labels.

pub mod fsrs_glue;
pub mod fuzz;
pub mod interval_limits;
pub mod labels;
pub mod learning;
pub mod leech;
pub mod review;
pub mod steps;
