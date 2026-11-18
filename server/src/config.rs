// SPDX-License-Identifier: MIT
use crate::{Error, Result};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    num::NonZeroU64,
};
use url::Url;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum LogLevel {
    #[default]
    Off,
    Info,
    Debug,
}
impl LogLevel {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "off" => Ok(Self::Off),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            _ => Err(Error::field("log_level")),
        }
    }
}

#[derive(Clone)]
pub struct Config {
    origin: String,
    pub listen: SocketAddr,
    pub trusted_peers: HashSet<IpAddr>,
    pub temp_bytes: NonZeroU64,
    pub native_compressed_bytes: NonZeroU64,
    pub native_decompressed_bytes: NonZeroU64,
    pub log_level: LogLevel,
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
            native_compressed_bytes: NonZeroU64::new(104_857_600).unwrap(),
            native_decompressed_bytes: NonZeroU64::new(314_572_800).unwrap(),
            log_level: LogLevel::Off,
        })
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn server_url(&self) -> String {
        format!("{}/", self.origin)
    }
}
