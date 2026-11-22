// SPDX-License-Identifier: MIT
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::collections::BTreeMap;

pub(crate) const READ: &str = "collection:read";
pub(crate) const BOTH: &str = "collection:read collection:write";

/// Reviewed published identities from M2. All support public-client code and
/// rotating refresh grants with no client secret. Strings never cause a fetch.
#[derive(Clone, Copy)]
pub(crate) struct Client {
    pub id: &'static str,
    pub redirect: &'static str,
    pub name: &'static str,
    pub direct_files: bool,
}
const CLIENTS: &[Client] = &[
    Client { id: "https://chatgpt.com/oauth/client.json", redirect: "https://chatgpt.com/connector_platform_oauth_redirect", name: "ChatGPT", direct_files:true },
    Client { id: "https://claude.ai/oauth/mcp-oauth-client-metadata", redirect: "https://claude.ai/api/mcp/auth_callback", name: "Claude", direct_files:false },
];
pub(crate) fn client(id: &str) -> Result<Client> {
    CLIENTS.iter().find(|c| c.id == id).copied().ok_or(Error::new("invalid_client"))
}
pub(crate) fn scope(value: &str) -> Result<bool> {
    let mut read = false;
    let mut write = false;
    for item in value.split(' ') {
        match item {
            READ if !read => read = true,
            "collection:write" if !write => write = true,
            _ => return Err(Error::new("invalid_scope")),
        }
    }
    if !read { return Err(Error::new("invalid_scope")); }
    Ok(write)
}
pub(crate) fn scopes(write: bool) -> &'static str { if write { BOTH } else { READ } }

/// URL parsing alone also accepts dot segments, default ports and escaped paths.
/// M2 permits only scheme/host case aliases, so compare the original components.
pub(crate) fn resource(origin: &str, value: &str) -> Result<String> {
    let invalid = || Error::new("invalid_target");
    let expected_host = origin.strip_prefix("https://").ok_or_else(invalid)?;
    let parsed = url::Url::parse(value).map_err(|_| invalid())?;
    let (scheme, rest) = value.split_once("://").ok_or_else(invalid)?;
    let (host, path) = rest.split_once('/').ok_or_else(invalid)?;
    if !scheme.eq_ignore_ascii_case("https") || !host.eq_ignore_ascii_case(expected_host)
        || path != "mcp" || parsed.username() != "" || parsed.password().is_some()
        || parsed.query().is_some() || parsed.fragment().is_some()
    { return Err(invalid()); }
    Ok(format!("{origin}/mcp"))
}

/// Bounded UTF-8 form decoding. Unknown OAuth extensions are ignored; known
/// protocol fields retain duplicate and malformed escape rejection.
/// The existing url dependency supplies encoding; no HTTP client is involved.
pub(crate) fn form(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    if bytes.len() > 16 * 1024 { return Err(Error::new("request_too_large")); }
    fn decode(bytes: &[u8]) -> Result<String> {
        let mut result = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' => {
                    let hex = bytes.get(i + 1..i + 3).ok_or(Error::new("invalid_request"))?;
                    let digit = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
                    result.push(digit(hex[0]).zip(digit(hex[1])).map(|(a,b)| a*16+b).ok_or(Error::new("invalid_request"))?);
                    i += 3;
                }
                b'+' => { result.push(b' '); i += 1; }
                c => { result.push(c); i += 1; }
            }
        }
        let value = String::from_utf8(result).map_err(|_| Error::new("invalid_request"))?;
        if value.contains('\0') { return Err(Error::new("invalid_request")); }
        Ok(value)
    }
    let mut result = BTreeMap::new();
    for pair in bytes.split(|b| *b == b'&') {
        let split = pair.iter().position(|b| *b == b'=').ok_or(Error::new("invalid_request"))?;
        let name = decode(&pair[..split])?;
        let value = decode(&pair[split+1..])?;
        if name.is_empty() { return Err(Error::new("invalid_request")); }
        if ["response_type","client_id","redirect_uri","resource","scope","code_challenge",
            "code_challenge_method","state","grant_type","code","code_verifier","refresh_token"]
            .contains(&name.as_str()) && result.insert(name, value).is_some()
        { return Err(Error::new("invalid_request")); }
    }
    Ok(result)
}
pub(crate) fn required<'a>(p: &'a BTreeMap<String,String>, name: &str) -> Result<&'a str> {
    p.get(name).map(String::as_str).filter(|s| !s.is_empty()).ok_or(Error::new("invalid_request"))
}
pub(crate) fn challenge(value: &str) -> Result<()> {
    if URL_SAFE_NO_PAD.decode(value).is_ok_and(|v| v.len() == 32 && URL_SAFE_NO_PAD.encode(&v) == value) {
        Ok(())
    } else { Err(Error::new("invalid_request")) }
}
pub(crate) fn verifier(value: &str) -> Result<String> {
    if !(43..=128).contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b)) {
        return Err(Error::new("invalid_grant"));
    }
    Ok(URL_SAFE_NO_PAD.encode(crypto::digest(value.as_bytes())))
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Authorization {
    pub client: String,
    pub redirect: String,
    pub resource: String,
    pub write: bool,
    pub challenge: String,
    pub state: Option<String>,
}
impl Authorization {
    pub fn parse(origin: &str, p: &BTreeMap<String,String>) -> Result<Self> {
        let c = client(required(p,"client_id")?)?;
        if required(p,"redirect_uri")? != c.redirect { return Err(Error::new("invalid_request")); }
        if required(p,"response_type")? != "code" { return Err(Error::new("unsupported_response_type")); }
        if required(p,"code_challenge_method")? != "S256" { return Err(Error::new("invalid_request")); }
        let challenge_value = required(p,"code_challenge")?;
        challenge(challenge_value)?;
        Ok(Self { client:c.id.into(), redirect:c.redirect.into(), resource:resource(origin,required(p,"resource")?)?,
            write:scope(p.get("scope").map_or(READ, String::as_str))?, challenge:challenge_value.into(), state:p.get("state").cloned() })
    }
    pub fn callback(&self, origin: &str, key: &str, value: &str) -> String {
        callback(origin, &self.redirect, self.state.as_deref(), key, value)
    }
}
pub(crate) fn callback(origin: &str, redirect: &str, state: Option<&str>, key: &str, value: &str) -> String {
    let mut url = url::Url::parse(redirect).expect("reviewed callback");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair(key, value).append_pair("iss", origin);
        if let Some(state) = state { query.append_pair("state", state); }
    }
    url.into()
}

/// Select capabilities only from the reviewed exact client binding on the grant.
pub(crate) fn direct_file_client(authority: &super::Authority) -> bool {
    authority.can_write() && client(authority.client_id()).is_ok_and(|c|c.direct_files)
}
