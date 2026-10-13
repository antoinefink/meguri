//! The error every export returns (KTD9). Swift sees it as the generated
//! `CoreError` enum, and CoreBridge maps it onto `BridgeError`. Area units add
//! a variant only when Swift must react to it differently from these.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Error)]
pub enum CoreError {
    /// The operation's `CancelToken` was cancelled before it finished.
    Cancelled,
    /// Another long operation holds the slot this one needs (R171).
    Busy { running_op: String },
    /// The collection cannot take this call now, for example during an
    /// exclusive phase (KTD17).
    Unavailable { reason: String },
    /// Compiled out for this platform or build: the iOS stubs of the server
    /// exports (KTD9) and the AnkiWeb switch (KTD21).
    Unsupported { feature: String },
    /// The caller passed a value the core rejects.
    InvalidArgument { message: String },
    /// A failure the caller cannot act on: a bug, or a caught panic.
    Internal { message: String },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Cancelled => f.write_str("cancelled"),
            CoreError::Busy { running_op } => write!(f, "busy: {running_op} is running"),
            CoreError::Unavailable { reason } => write!(f, "unavailable: {reason}"),
            CoreError::Unsupported { feature } => write!(f, "unsupported: {feature}"),
            CoreError::InvalidArgument { message } => write!(f, "invalid argument: {message}"),
            CoreError::Internal { message } => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for CoreError {}

impl CoreError {
    /// The message of a caught panic payload, for `Internal` errors.
    pub(crate) fn from_panic(payload: &(dyn std::any::Any + Send)) -> CoreError {
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "panic with a non-string payload".to_owned()
        };
        CoreError::Internal {
            message: format!("panic: {message}"),
        }
    }
}
