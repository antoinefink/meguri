// SPDX-License-Identifier: MIT
//! Independently authored account service and native collection transfer.
#![forbid(unsafe_op_in_unsafe_fn)]
pub mod accounts;
pub mod collection;
pub mod config;
pub mod crypto;
pub mod error;
pub mod httpapi;
pub mod limits;
pub(crate) mod media;
pub mod protocol;
pub mod storage;
pub mod webassets;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;
pub use accounts::{Identity, NativeAuthority, Service};
pub use config::Config;
pub use error::{Error, Result};
pub use storage::DataRoot;
