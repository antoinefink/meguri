// SPDX-License-Identifier: MIT
//! Private restricted egress: never include reqwest errors or URLs in diagnostics.
use super::*;
use std::net::{IpAddr, SocketAddr};
const HOST: &str = "files.oaiusercontent.com";
pub(super) fn target(value: &str) -> Result<url::Url> {
    if value.len() > 4096 || value.bytes().any(|b|b.is_ascii_control() || b == b'\\') { return Err(unavailable()); }
    let url = url::Url::parse(value).map_err(|_| unavailable())?;
    if url.scheme() != "https" || url.host_str() != Some(HOST) || url.port_or_known_default() != Some(443)
        || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some()
        || value.split('/').nth(2).is_some_and(|a|a.contains('@')) { return Err(unavailable()); }
    Ok(url)
}
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let n = u32::from(ip);
            // Includes private, shared, loopback, link-local, documentation,
            // benchmarking, multicast and reserved ranges. Fail closed on special use.
            ![(0,8),(0x0a000000,8),(0x64400000,10),(0x7f000000,8),(0xa9fe0000,16),
                (0xac100000,12),(0xc0000000,24),(0xc0000200,24),(0xc0586300,24),
                (0xc0a80000,16),(0xc6120000,15),(0xc6336400,24),(0xcb007100,24),
                (0xe0000000,3)].iter().any(|&(base,bits)| n >> (32-bits) == base >> (32-bits))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000 && !(s[0] == 0x2001 && s[1] < 0x0200)
                && !(s[0] == 0x2001 && s[1] == 0x0db8) && s[0] != 0x2002 && s[0] != 0x3ffe
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
pub(super) async fn fetch(value: &str, maximum: usize) -> Result<Vec<u8>> {
    let url = target(value)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    tokio::time::timeout_at(deadline, async move {
        let resolved = tokio::net::lookup_host((HOST,443)).await.map_err(|_| unavailable())?;
        let mut addresses = Vec::<SocketAddr>::new();
        for address in resolved {
            if addresses.len() >= 16 || !public(address.ip()) { return Err(unavailable()); }
            if !addresses.contains(&address) { addresses.push(address); }
        }
        if addresses.is_empty() { return Err(unavailable()); }
        // Per-request DNS overrides pin every connection to checked answers.
        // The URL host remains intact for SNI and certificate hostname checks.
        let client = reqwest::Client::builder().https_only(true).no_proxy()
            .redirect(reqwest::redirect::Policy::none()).no_gzip().no_brotli().no_deflate().no_zstd()
            .timeout(Duration::from_secs(15)).resolve_to_addrs(HOST,&addresses)
            .build().map_err(|_| unavailable())?;
        let mut response = client.get(url).header("accept-encoding","identity").send().await.map_err(|_| unavailable())?;
        if response.status() != reqwest::StatusCode::OK || response.headers().contains_key("content-encoding")
            || response.content_length().is_some_and(|n| n == 0 || n > maximum as u64) { return Err(unavailable()); }
        let expected = response.content_length();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if chunk.len() > maximum - bytes.len() { return Err(invalid()); }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() || expected.is_some_and(|n|n != bytes.len() as u64) { return Err(unavailable()); }
        Ok(bytes)
    }).await.map_err(|_| unavailable())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_untrusted_urls_and_special_addresses_without_io() {
        for value in ["http://files.oaiusercontent.com/x","https://files.oaiusercontent.com.evil.test/x", "https://files.oaiusercontent.com:444/x", "https://user@files.oaiusercontent.com/x", "https://@files.oaiusercontent.com/x", "https://files.oaiusercontent.com/x#secret", "https://127.0.0.1/x"] { assert!(target(value).is_err()); }
        assert!(target("https://files.oaiusercontent.com/generated?secret=fixture").is_ok());
        for ip in ["0.0.0.0","10.0.0.1","100.64.0.1","127.0.0.1","169.254.169.254","172.16.0.1","192.168.1.1","192.0.2.1","198.18.0.1","198.51.100.1","203.0.113.1","224.0.0.1","255.255.255.255","::1","::ffff:8.8.8.8","64:ff9b::808:808","2001:db8::1","2002:808:808::1","fc00::1","fe80::1","3fff::1"] { assert!(!public(ip.parse().unwrap()),"{ip}"); }
        for ip in ["8.8.8.8","1.1.1.1","2606:4700:4700::1111"] { assert!(public(ip.parse().unwrap())); }
    }
}
