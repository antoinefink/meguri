//! Admission (`engine/lifecycle` L4): which submitted ops the executor queues,
//! parks or refuses in the current collection state. The lifecycle sets the
//! admission; privileged handles (the open steps', the exclusive job's)
//! carry the token that gets their ops in when nothing else is admitted.

use crate::error::EngineError;

/// The token of a privileged handle; valid only while its phase lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Privilege(pub(crate) u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    /// `Ready`: every op.
    Open,
    /// Opening or an exclusive phase: only ops with this token.
    Privileged { privilege: Privilege, reason: String },
    /// Closing, closed or failed: nothing.
    Closed { reason: String },
}

pub(crate) enum Decision {
    Queue,
    Park,
    Refuse(EngineError),
}

impl Admission {
    pub(crate) fn decide(&self, privilege: Option<Privilege>, park: bool) -> Decision {
        let reason = match self {
            Admission::Open => return Decision::Queue,
            Admission::Privileged { privilege: allowed, .. } if privilege == Some(*allowed) => {
                return Decision::Queue;
            }
            Admission::Privileged { reason, .. } | Admission::Closed { reason } => reason,
        };
        if park {
            Decision::Park
        } else {
            Decision::Refuse(EngineError::unavailable(reason.clone()))
        }
    }
}
