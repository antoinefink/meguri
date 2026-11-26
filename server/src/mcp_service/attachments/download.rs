// SPDX-License-Identifier: MIT
//! Private restricted egress: never include reqwest errors or URLs in diagnostics.
use super::*;
use std::net::{IpAddr, SocketAddr};
const HOST: &str = "files.oaiusercontent.com";
fn failed(detail: Detail) -> Error { unavailable().with_mcp_detail(detail) }
fn request_failed(error: reqwest::Error, stage: Detail) -> Error {
    failed(if error.is_timeout() { Detail::AttachmentTimeout } else { stage })
}
pub(super) fn target(value: &str) -> Result<url::Url> {
    if value.len() > 4096 { return Err(failed(Detail::AttachmentUrlLength)); }
    if value.bytes().any(|b|b.is_ascii_control() || b == b'\\') { return Err(failed(Detail::AttachmentUrlCharacters)); }
    let url = url::Url::parse(value).map_err(|_| failed(Detail::AttachmentUrlParse))?;
    if url.scheme() != "https" { return Err(failed(Detail::AttachmentUrlScheme)); }
    if url.host_str() != Some(HOST) { return Err(failed(Detail::AttachmentUrlHost)); }
    if url.port_or_known_default() != Some(443) { return Err(failed(Detail::AttachmentUrlPort)); }
    if !url.username().is_empty() || url.password().is_some() { return Err(failed(Detail::AttachmentUrlUserinfo)); }
    if url.fragment().is_some() { return Err(failed(Detail::AttachmentUrlFragment)); }
    // The third raw slash-separated segment can include a no-path query, as well
    // as empty user information normalized away by parsing. Retain the check and order.
    if value.split('/').nth(2).is_some_and(|a|a.contains('@')) { return Err(failed(Detail::AttachmentUrlRawAt)); }
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
        let resolved = tokio::net::lookup_host((HOST,443)).await.map_err(|_| failed(Detail::AttachmentDns))?;
        let addresses = checked_addresses(resolved)?;
        // Per-request DNS overrides pin every connection to checked answers.
        // The URL host remains intact for SNI and certificate hostname checks.
        let client = reqwest::Client::builder().https_only(true).no_proxy()
            .redirect(reqwest::redirect::Policy::none()).no_gzip().no_brotli().no_deflate().no_zstd()
            .timeout(Duration::from_secs(15)).resolve_to_addrs(HOST,&addresses)
            .build().map_err(|_| failed(Detail::AttachmentHttpClient))?;
        let response = client.get(url).header("accept-encoding","identity").send().await
            .map_err(|e| request_failed(e,Detail::AttachmentHttpRequest))?;
        collect(response,maximum).await
    }).await.map_err(|_| failed(Detail::AttachmentTimeout))?
}
fn checked_addresses(resolved: impl Iterator<Item = SocketAddr>) -> Result<Vec<SocketAddr>> {
    let mut addresses = Vec::new();
    for address in resolved {
        if addresses.len() >= 16 || !public(address.ip()) { return Err(failed(Detail::AttachmentAddress)); }
        if !addresses.contains(&address) { addresses.push(address); }
    }
    if addresses.is_empty() { return Err(failed(Detail::AttachmentAddress)); }
    Ok(addresses)
}
async fn collect(mut response: reqwest::Response, maximum: usize) -> Result<Vec<u8>> {
    if response.status() != reqwest::StatusCode::OK || response.headers().contains_key("content-encoding")
        || response.content_length().is_some_and(|n| n == 0 || n > maximum as u64) {
        return Err(failed(Detail::AttachmentHttpResponse));
    }
    let expected = response.content_length();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| request_failed(e,Detail::AttachmentBody))? {
        if chunk.len() > maximum - bytes.len() { return Err(invalid().with_mcp_detail(Detail::AttachmentBody)); }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() || expected.is_some_and(|n|n != bytes.len() as u64) { return Err(failed(Detail::AttachmentBody)); }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_untrusted_urls_and_special_addresses_without_io() {
        for (value,detail) in [
            (format!("{}\\", "x".repeat(4096)),Detail::AttachmentUrlLength),
            ("https://files.oaiusercontent.com/x\n".into(),Detail::AttachmentUrlCharacters),
            ("http:\\untrusted.example.test".into(),Detail::AttachmentUrlCharacters),
            ("".into(),Detail::AttachmentUrlParse),
            ("https://[invalid-host".into(),Detail::AttachmentUrlParse),
            ("https://files.oaiusercontent.com:invalid/x".into(),Detail::AttachmentUrlParse),
            ("http://user@untrusted.example.test:444/x#secret".into(),Detail::AttachmentUrlScheme),
            ("https://files.oaiusercontent.com.evil.test:444/x#secret".into(),Detail::AttachmentUrlHost),
            ("https://127.0.0.1/x".into(),Detail::AttachmentUrlHost),
            ("https://user@files.oaiusercontent.com:444/x#secret".into(),Detail::AttachmentUrlPort),
            ("https://user@files.oaiusercontent.com/x#secret".into(),Detail::AttachmentUrlUserinfo),
            ("https://:secret@files.oaiusercontent.com/x".into(),Detail::AttachmentUrlUserinfo),
            ("https://files.oaiusercontent.com/x#secret".into(),Detail::AttachmentUrlFragment),
            ("https://files.oaiusercontent.com/x#".into(),Detail::AttachmentUrlFragment),
            ("https://@files.oaiusercontent.com/x#secret".into(),Detail::AttachmentUrlFragment),
            ("https://@files.oaiusercontent.com/x".into(),Detail::AttachmentUrlRawAt),
            ("https://:@files.oaiusercontent.com/x".into(),Detail::AttachmentUrlRawAt),
        ] {
            let error=target(&value).unwrap_err();
            assert_eq!(error,failed(detail));
            assert_eq!(error.to_string(),"attachment_unavailable");
        }
        for value in ["https://files.oaiusercontent.com/generated?secret=fixture",
            "https://FILES.OAIUSERCONTENT.COM:443/generated?secret=fixture",
            "https://files.oaiusercontent.com/x@fixture?secret=@fixture"] {
            assert!(target(value).is_ok());
        }
        let prefix="https://files.oaiusercontent.com/";
        assert!(target(&format!("{prefix}{}","x".repeat(4096-prefix.len()))).is_ok());
        for ip in ["0.0.0.0","10.0.0.1","100.64.0.1","127.0.0.1","169.254.169.254","172.16.0.1","192.168.1.1","192.0.2.1","198.18.0.1","198.51.100.1","203.0.113.1","224.0.0.1","255.255.255.255","::1","::ffff:8.8.8.8","64:ff9b::808:808","2001:db8::1","2002:808:808::1","fc00::1","fe80::1","3fff::1"] { assert!(!public(ip.parse().unwrap()),"{ip}"); }
        for ip in ["8.8.8.8","1.1.1.1","2606:4700:4700::1111"] { assert!(public(ip.parse().unwrap())); }
    }
    #[test]
    fn raw_at_detail_is_distinct_from_parsed_userinfo() {
        for value in ["https://files.oaiusercontent.com?secret=@fixture",
            "https://@files.oaiusercontent.com/x", "https://:@files.oaiusercontent.com/x"] {
            let parsed=url::Url::parse(value).unwrap();
            assert!(parsed.username().is_empty());
            assert!(parsed.password().is_none());
            assert_eq!(target(value).unwrap_err(),failed(Detail::AttachmentUrlRawAt));
        }
        assert_eq!(target("https://user:secret@files.oaiusercontent.com/x").unwrap_err(),
            failed(Detail::AttachmentUrlUserinfo));
        assert_eq!(target("https://files.oaiusercontent.com?secret=@fixture#fragment").unwrap_err(),
            failed(Detail::AttachmentUrlFragment));
        assert!(target("https://files.oaiusercontent.com?secret=fixture").is_ok());
        assert!(target("https://files.oaiusercontent.com/?secret=@fixture").is_ok());
    }
    #[test]
    fn address_failures_have_only_the_fixed_address_detail() {
        for addresses in [vec![],vec!["127.0.0.1:443".parse().unwrap()],
            vec!["8.8.8.8:443".parse().unwrap(),"192.168.1.1:443".parse().unwrap()]] {
            assert_eq!(checked_addresses(addresses.into_iter()).unwrap_err(),failed(Detail::AttachmentAddress));
        }
        let address="8.8.8.8:443".parse().unwrap();
        assert_eq!(checked_addresses([address,address].into_iter()).unwrap(),vec![address]);
    }
    #[tokio::test]
    async fn response_failures_are_distinct_from_requests_and_never_include_input_data() {
        for status in [206,302,403,500] {
            let response=axum::http::Response::builder().status(status)
                .header("location","https://generated-secret.example.test/private")
                .body(b"private response bytes".to_vec()).unwrap().into();
            assert_eq!(collect(response,1024).await.unwrap_err(),failed(Detail::AttachmentHttpResponse));
        }
        let encoded=axum::http::Response::builder().header("content-encoding","identity")
            .body(vec![1u8]).unwrap().into();
        assert_eq!(collect(encoded,1024).await.unwrap_err(),failed(Detail::AttachmentHttpResponse));
        for bytes in [vec![],vec![1u8;1025]] {
            let response=axum::http::Response::new(bytes).into();
            assert_eq!(collect(response,1024).await.unwrap_err(),failed(Detail::AttachmentHttpResponse));
        }
        let bytes=b"generated bytes".to_vec();
        assert_eq!(collect(axum::http::Response::new(bytes.clone()).into(),1024).await.unwrap(),bytes);
        for detail in [Detail::AttachmentHttpRequest,Detail::AttachmentBody] {
            let error=reqwest::Client::new().get("https://[generated-private-error").build().unwrap_err();
            let error=request_failed(error,detail);
            assert_eq!(error,failed(detail));
            assert_eq!(error.to_string(),"attachment_unavailable");
        }
    }
}
