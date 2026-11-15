// SPDX-License-Identifier: MIT
//! Account/service substrate. Native wire framing and compatible stores start in U5.
#![forbid(unsafe_op_in_unsafe_fn)]
pub mod accounts;
pub mod config;
pub mod crypto;
pub mod error;
pub mod httpapi;
pub mod limits;
pub mod storage;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;
pub use accounts::{Identity, NativeAuthority, Service};
pub use config::Config;
pub use error::{Error, Result};
pub use storage::DataRoot;
