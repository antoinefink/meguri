// SPDX-License-Identifier: MIT
use crate::{Error, Result};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    num::NonZeroU64,
};
use url::Url;

#[derive(Clone)]
pub struct Config {
    origin: String,
    pub listen: SocketAddr,
    pub trusted_peers: HashSet<IpAddr>,
    pub temp_bytes: NonZeroU64,
}
impl Config {
    pub fn new(origin: &str, listen: SocketAddr) -> Result<Self> {
        let parsed = Url::parse(origin).map_err(|_| Error::field("public_origin"))?;
        let serialized = parsed.origin().ascii_serialization();
        // Require the exact serialized origin. This also excludes implicit
        // normalization, trailing slash, empty userinfo, default-port aliases.
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
            || serialized != origin
        {
            return Err(Error::field("public_origin"));
        }
        Ok(Self {
            origin: serialized,
            listen,
            trusted_peers: HashSet::new(),
            temp_bytes: NonZeroU64::new(2 * 1024 * 1024 * 1024).unwrap(),
        })
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn server_url(&self) -> String {
        format!("{}/", self.origin)
    }
}
