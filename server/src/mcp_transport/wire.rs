// SPDX-License-Identifier: MIT
//! Tools-only wire validation. No identity is inferred from protocol metadata.
use super::{SUPPORTED_REVISIONS, ToolCall, descriptors, inputs, rpc_error};
use axum::{http::{HeaderMap, StatusCode}, response::{IntoResponse, Response}};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};

pub(super) const VERSION: &str = "io.modelcontextprotocol/protocolVersion";
pub(super) const CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
pub(super) const CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

#[derive(Clone)]
pub(super) enum Operation {
    Discover,
    Initialize,
    Initialized,
    Ping,
    List,
    Call { name: String, call: ToolCall },
}
#[derive(Clone)]
pub(super) struct Dispatch {
    pub id: Value,
    pub modern: bool,
    pub operation: Operation,
}
impl Dispatch {
    pub fn access(&self) -> descriptors::Access {
        match &self.operation {
            Operation::Call { call, .. } => call.access(),
            _ => descriptors::Access::Read,
        }
    }
    pub fn name(&self) -> Option<&str> {
        match &self.operation { Operation::Call { name, .. } => Some(name), _ => None }
    }
}
fn singleton<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, ()> {
    let mut values = headers.get_all(name).iter();
    let first = values.next().map(|v| v.to_str().map_err(|_| ())).transpose()?;
    if values.next().is_some() { return Err(()); }
    Ok(first)
}
fn members(object: &Map<String, Value>, allowed: &[&str]) -> bool {
    object.keys().all(|k| allowed.contains(&k.as_str()))
}
fn client_info(value: Option<&Value>) -> bool {
    value.and_then(Value::as_object).is_some_and(|v|
        ["name", "version"].iter().all(|k| v.get(*k).and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())))
}
fn decoded_name(value: &str) -> Result<String, ()> {
    if let Some(encoded) = value.strip_prefix("=?base64?") {
        let encoded = encoded.strip_suffix("?=").ok_or(())?;
        String::from_utf8(STANDARD.decode(encoded).map_err(|_| ())?).map_err(|_| ())
    } else { Ok(value.to_owned()) }
}
fn unsupported_version(id: Value, requested: &str) -> Response {
    (StatusCode::BAD_REQUEST, axum::Json(json!({
        "jsonrpc":"2.0", "id":id,
        "error":{"code":-32022, "message":"Unsupported protocol version",
            "data":{"supported":SUPPORTED_REVISIONS,"requested":requested}}
    }))).into_response()
}

pub(super) fn validate(headers: &HeaderMap, value: Value, bytes: &[u8]) -> Result<Dispatch, Response> {
    let object = value.as_object().ok_or_else(||
        rpc_error(StatusCode::BAD_REQUEST, Value::Null, -32600, "Invalid request"))?;
    let id = object.get("id").filter(|v| v.is_string() || v.as_i64().is_some()
        || v.as_u64().is_some()).cloned().unwrap_or(Value::Null);
    let invalid = || rpc_error(StatusCode::BAD_REQUEST, id.clone(), -32600, "Invalid request");
    let mismatch = || rpc_error(StatusCode::BAD_REQUEST, id.clone(), -32020, "Protocol header mismatch");
    let params_error = || rpc_error(StatusCode::OK, id.clone(), -32602, "Invalid parameters");
    if !members(object, &["jsonrpc", "id", "method", "params"])
        || object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || (object.contains_key("id") && id.is_null()) { return Err(invalid()); }
    let method = object.get("method").and_then(Value::as_str).ok_or_else(invalid)?;
    let empty = Map::new();
    let params = match object.get("params") {
        Some(v) => v.as_object().ok_or_else(params_error)?,
        None => &empty,
    };
    let version = singleton(headers, "mcp-protocol-version").map_err(|_| mismatch())?;
    if method != "initialize" && version.is_some_and(|v| !SUPPORTED_REVISIONS.contains(&v)) {
        return Err(unsupported_version(id, version.expect("present unsupported version")));
    }
    let meta = params.get("_meta").map(|v| v.as_object().ok_or_else(params_error)).transpose()?;
    let meta_version = meta.and_then(|m| m.get(VERSION)).and_then(Value::as_str);
    let modern = version == Some(SUPPORTED_REVISIONS[0])
        || (version.is_none() && meta_version == Some(SUPPORTED_REVISIONS[0]));
    if modern {
        if version != Some(SUPPORTED_REVISIONS[0]) || meta_version != version
            || singleton(headers, "mcp-method").map_err(|_| mismatch())? != Some(method) {
            return Err(mismatch());
        }
        if !client_info(meta.and_then(|m| m.get(CLIENT_INFO)))
            || !meta.and_then(|m| m.get(CLIENT_CAPABILITIES)).is_some_and(Value::is_object) {
            return Err(params_error());
        }
        let name = singleton(headers, "mcp-name").map_err(|_| mismatch())?;
        if method == "tools/call" {
            let name = name.ok_or_else(mismatch).and_then(|s| decoded_name(s).map_err(|_| mismatch()))?;
            if params.get("name").and_then(Value::as_str) != Some(name.as_str()) {
                return Err(mismatch());
            }
        } else if name.is_some() { return Err(mismatch()); }
    } else if let Some(meta_version) = meta_version {
        if meta_version != SUPPORTED_REVISIONS[1] { return Err(mismatch()); }
    }
    let operation = match method {
        "server/discover" if modern => {
            if !members(params, &["_meta"]) { return Err(params_error()); }
            Operation::Discover
        }
        "initialize" if !modern => {
            if !members(params, &["protocolVersion", "capabilities", "clientInfo", "_meta"])
                || !params.get("capabilities").is_some_and(Value::is_object)
                || !client_info(params.get("clientInfo")) { return Err(params_error()); }
            let requested = params.get("protocolVersion").and_then(Value::as_str).ok_or_else(params_error)?;
            // Legacy initialize negotiates our supported legacy revision even
            // for an unsupported proposal. Its optional header must still agree
            // with that proposal; later requests get no old-version exemption.
            if version.is_some_and(|v| v != requested) {
                return Err(mismatch());
            }
            Operation::Initialize
        }
        "notifications/initialized" if !modern && !object.contains_key("id") => {
            if !members(params, &["_meta"]) { return Err(params_error()); }
            Operation::Initialized
        }
        "ping" => {
            if !members(params, &["_meta"]) { return Err(params_error()); }
            Operation::Ping
        }
        "tools/list" => {
            // The four descriptors form one complete page; no invented cursor.
            if !members(params, &["_meta"]) { return Err(params_error()); }
            Operation::List
        }
        "tools/call" => {
            if !members(params, &["name", "arguments", "_meta"]) { return Err(params_error()); }
            let name = params.get("name").and_then(Value::as_str).ok_or_else(params_error)?;
            if descriptors::access(name).is_none() { return Err(params_error()); }
            // Parse original argument bytes before transport map conversion.
            // The bounded JSON preflight has also rejected nested duplicates.
            backend_arguments(bytes, name).map_err(|_| params_error())?;
            let arguments = match params.get("arguments") {
                Some(v) => v.as_object().ok_or_else(params_error)?.clone(),
                None => Map::new(),
            };
            let call = inputs::parse(name, arguments).map_err(|_| params_error())?;
            Operation::Call { name:name.to_owned(), call }
        }
        _ => return Err(rpc_error(StatusCode::NOT_FOUND, id, -32601, "Method not found")),
    };
    if id.is_null() && !matches!(operation, Operation::Initialized) { return Err(invalid()); }
    Ok(Dispatch { id, modern, operation })
}

fn backend_arguments(bytes: &[u8], name: &str) -> Result<(), ()> {
    use crate::collection::mcp;
    use serde::Deserialize;
    use serde_json::value::RawValue;
    #[derive(Deserialize)]
    struct Envelope<'a> { #[serde(borrow)] params: Arguments<'a> }
    #[derive(Deserialize)]
    struct Arguments<'a> { #[serde(borrow)] arguments: Option<&'a RawValue> }
    let envelope: Envelope<'_> = serde_json::from_slice(bytes).map_err(|_| ())?;
    let raw = envelope.params.arguments.map_or("{}", RawValue::get).as_bytes();
    match name {
        "search_notes" => mcp::Backend::parse::<mcp::SearchInput>(raw).map(|_| ()),
        "list_decks_and_note_types" => mcp::Backend::parse::<mcp::CatalogInput>(raw).map(|_| ()),
        "create_note" => mcp::Backend::parse::<mcp::CreateInput>(raw).map(|_| ()),
        "edit_note" => mcp::Backend::parse::<mcp::EditInput>(raw).map(|_| ()),
        _ => return Err(()),
    }.map_err(|_| ())
}
