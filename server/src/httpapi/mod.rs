// SPDX-License-Identifier: MIT
pub(crate) mod ingress;
mod oauth;
#[cfg(test)]
mod tests;

use crate::{Error, Result, Service, accounts::BrowserAuthority};
use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpListener;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    token: String,
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Password {
    current_password: String,
    new_password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reset {
    token: String,
    new_password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Serialize)]
struct Envelope<T> {
    data: T,
}
fn data<T: Serialize>(value: T, created: bool) -> Response {
    (
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(Envelope { data: value }),
    )
        .into_response()
}
const COOKIE: &str = "__Host-meguri-session";
fn set_cookie(response: &mut Response, session: Option<(&str, i64)>) {
    let cookie = match session {
        Some((value, expires_at)) => {
            let remaining = (expires_at - crate::accounts::now()).max(0);
            format!("{COOKIE}={value}; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age={remaining}")
        }
        None => format!("{COOKIE}=; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    };
    response
        .headers_mut()
        .insert("set-cookie", cookie.parse().expect("base64 cookie"));
}
fn cookie(headers: &HeaderMap) -> Result<String> {
    let mut found = None;
    for header in headers.get_all("cookie").iter() {
        let header = header
            .to_str()
            .map_err(|_| Error::new("authentication_failed"))?;
        for item in header.split(';') {
            if let Some((name, value)) = item.trim().split_once('=') {
                if name == COOKIE {
                    if found.is_some() {
                        return Err(Error::new("authentication_failed"));
                    }
                    found = Some(value.to_owned());
                }
            }
        }
    }
    found.ok_or(Error::new("authentication_failed"))
}
fn origin(headers: &HeaderMap, service: &Service) -> Result<()> {
    let all = headers.get_all("origin");
    let mut values = all.iter();
    if values.next().and_then(|v| v.to_str().ok()) != Some(service.config.origin())
        || values.next().is_some()
    {
        return Err(Error::new("origin_rejected"));
    }
    Ok(())
}
/// Only a trusted *socket peer* can supply a single forwarded address/protocol.
/// Host and X-Forwarded-Host never participate in configured origin/URLs.
pub fn effective_peer(
    headers: &HeaderMap,
    socket: SocketAddr,
    service: &Service,
) -> (IpAddr, bool) {
    if !service.config.trusted_peers.contains(&socket.ip()) {
        return (socket.ip(), false);
    }
    let one = |name: &str| -> Option<&str> {
        if headers.get_all(name).iter().count() != 1 {
            None
        } else {
            headers.get(name)?.to_str().ok()
        }
    };
    let ip = one("x-forwarded-for")
        .and_then(|v| v.parse().ok())
        .unwrap_or(socket.ip());
    let https = one("x-forwarded-proto") == Some("https");
    (ip, https)
}
fn content_type(headers: &HeaderMap) -> Result<()> {
    let all = headers.get_all("content-type");
    let mut values = all.iter();
    let value = values
        .next()
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::new("unsupported_content_type"))?;
    if values.next().is_some() {
        return Err(Error::new("unsupported_content_type"));
    }
    let mut pieces = value.split(';');
    if !pieces
        .next()
        .is_some_and(|p| p.trim().eq_ignore_ascii_case("application/json"))
    {
        return Err(Error::new("unsupported_content_type"));
    }
    if let Some(charset) = pieces.next() {
        let (name, value) = charset
            .trim()
            .split_once('=')
            .ok_or(Error::new("unsupported_content_type"))?;
        let value = value.trim();
        if !name.trim().eq_ignore_ascii_case("charset")
            || !(value.eq_ignore_ascii_case("utf-8") || value.eq_ignore_ascii_case("\"utf-8\""))
        {
            return Err(Error::new("unsupported_content_type"));
        }
    }
    if pieces.next().is_some() {
        return Err(Error::new("unsupported_content_type"));
    }
    Ok(())
}
// Depth is measured on parsed containers, with the root object at depth one.
// This Value is preflight only: typed deserialization always uses original bytes
// so duplicate known members cannot disappear through map deduplication.
fn container_depth(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(items) => 1 + items.iter().map(container_depth).max().unwrap_or(0),
        serde_json::Value::Object(items) => {
            1 + items.values().map(container_depth).max().unwrap_or(0)
        }
        _ => 0,
    }
}
fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| Error::new("invalid_request"))?;
    if !value.is_object() || container_depth(&value) > 16 {
        return Err(Error::new("invalid_request"));
    }
    serde_json::from_slice(bytes).map_err(|_| Error::new("invalid_request"))
}
async fn authority(service: &Service, headers: &HeaderMap) -> Result<BrowserAuthority> {
    service.browser_authority(&cookie(headers)?).await
}

pub fn router(service: Arc<Service>) -> Router {
    mounted_router(service).0
}
fn mounted_router(service: Arc<Service>) -> (Router, crate::mcp_transport::Control) {
    let bridge = Arc::new(crate::mcp_service::McpService::new(service.clone()));
    let (mcp, control) = crate::mcp_transport::router(&service.config, bridge.clone(), bridge);
    let router = Router::new().fallback(handle).with_state(service).merge(mcp)
        .layer(axum::middleware::from_fn(|request: Request, next: axum::middleware::Next| async move {
            secure(next.run(request).await)
        }));
    (router, control)
}
/// Closing admission wakes unfinished body readers before graceful shutdown.
/// Never time out/drop the server future: accepted workers and response bodies
/// retain root ownership until they actually cease, including cancelled callers.
pub async fn serve(
    listener: TcpListener,
    service: Arc<Service>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let owner = service.clone();
    let stopping = service.clone();
    let (router, mcp) = mounted_router(service);
    let stopping_mcp = mcp.clone();
    let result = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        stopping_mcp.close();
        stopping.browser_ingress.close();
        stopping.native_ingress.close();
    })
    .await;
    mcp.close();
    owner.browser_ingress.close();
    owner.native_ingress.close();
    owner.browser_ingress.idle().await;
    owner.native_ingress.idle().await;
    mcp.idle().await;
    result
}
async fn handle(State(service): State<Arc<Service>>, request: Request) -> Response {
    if request.uri().path().starts_with("/sync/") || request.uri().path().starts_with("/msync/") {
        return secure(crate::protocol::handle(service, request).await);
    }
    let browser_api = request.uri().path().starts_with("/api/");
    let oauth_route = oauth::route(request.uri().path());
    let browser = browser_api || oauth_route;
    let exempt = request.method() == Method::GET && request.uri().path() == "/api/status";
    let (parts, body) = request.into_parts();
    let ticket_route = matches!(parts.uri.path(), "/api/mcp/collection-download" | "/api/mcp/media-upload");
    let machine_upload = parts.method == Method::POST && parts.uri.path() == "/api/mcp/media-upload"
        && parts.uri.query().is_none();
    // Keep the incoming receiver alive until the closing response is finished.
    // Dropping it on a refusal could enable HTTP/1 background unread-body drain.
    let response = if browser {
        let early = if (browser_api && !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS)
                && (!machine_upload || parts.headers.contains_key("origin")))
            || (oauth_route && parts.headers.contains_key("origin")) {
            origin(&parts.headers, &service)
        } else {
            Ok(())
        };
        match early {
            Err(e) => ingress::retain(api_error(e, ticket_route), body, None, service),
            Ok(()) if exempt => {
                let response = dispatch(&service, &parts, None)
                    .await
                    .unwrap_or_else(IntoResponse::into_response);
                ingress::retain(response, body, None, service)
            }
            Ok(()) => match service.browser_ingress.enter() {
                Err(e) => ingress::retain(api_error(e, ticket_route), body, None, service),
                Ok(permit) => {
                    // At most CAPACITY workers, no waiting/spawn queue. A caller's
                    // cancellation must not free its slot while slow work runs.
                    let deadline =
                        tokio::time::Instant::now() + Duration::from_secs(ingress::BODY_SECONDS);
                    let worker = tokio::spawn(async move {
                        let mut body = body;
                        let response = dispatch_browser(&service, &parts, &mut body, deadline)
                            .await
                            .unwrap_or_else(|e| api_error(e, ticket_route));
                        ingress::retain(response, body, Some(permit), service)
                    });
                    worker
                        .await
                        .unwrap_or_else(|_| api_error(Error::new("internal_error"), ticket_route))
                }
            },
        }
    } else {
        // Exact local navigation/assets only. API and native dispatch retain
        // their own envelopes, admission and authentication above.
        match web_asset(&parts) {
            Some(response) => response,
            None => dispatch(&service, &parts, None)
                .await
                .unwrap_or_else(IntoResponse::into_response),
        }
    };
    let mut response = response;
    if browser {
        response
            .headers_mut()
            .insert("connection", "close".parse().expect("static header"));
    }
    secure(response)
}
fn web_asset(parts: &axum::http::request::Parts) -> Option<Response> {
    if !matches!(parts.method, Method::GET | Method::HEAD) || parts.uri.query().is_some() {
        return None;
    }
    let asset = crate::webassets::lookup(parts.uri.path())?;
    let mut response = Response::new(if parts.method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(asset.bytes)
    });
    response.headers_mut().insert("content-type", asset.content_type.parse().unwrap());
    response.headers_mut().insert("content-length", asset.bytes.len().into());
    Some(response)
}
fn secure(mut response: Response) -> Response {
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
        ("x-frame-options", "DENY"),
        ("x-content-type-options", "nosniff"),
    ] {
        response
            .headers_mut()
            .insert(name, value.parse().expect("static header"));
    }
    response
}
async fn dispatch_browser(
    service: &Arc<Service>,
    parts: &axum::http::request::Parts,
    body: &mut Body,
    deadline: tokio::time::Instant,
) -> Result<Response> {
    if oauth::route(parts.uri.path()) {
        return oauth::dispatch(service, parts, body, deadline).await;
    }
    if parts.uri.query().is_some() {
        return Err(Error::new("invalid_request"));
    }
    if parts.method == Method::GET && parts.uri.path() == "/api/mcp/collection-download" {
        return collection_download(service, parts).await;
    }
    if parts.method != Method::POST {
        return dispatch(service, parts, None).await;
    }
    if !mutation(parts.uri.path()) {
        return Err(Error::new("route_not_found"));
    }
    if parts.uri.path() == "/api/mcp/media-upload" {
        return media_upload(service, parts, body, deadline).await;
    }
    content_type(&parts.headers)?;
    if !service.admission_open().await? {
        return Err(Error::new("admission_closed"));
    }
    let bytes = service.browser_ingress.read(body, deadline).await?;
    dispatch(service, parts, Some(&bytes)).await
}
// Retain the common admission, unread body and private response headers. Ticket
// failures disclose neither OAuth metadata nor whether a credential ever existed.
fn api_error(error: Error, ticket_route: bool) -> Response {
    if ticket_route && matches!(error.code, "authentication_failed" | "capability_invalid" | "invalid_token") {
        let mut response = Error::new("authentication_failed").into_response();
        response.headers_mut().insert("www-authenticate", "Bearer".parse().unwrap());
        response
    } else if ticket_route && error.code == "conflict" {
        let mut response = error.into_response();
        *response.status_mut() = StatusCode::CONFLICT;
        response
    } else {
        error.into_response()
    }
}
async fn collection_download(service: &Arc<Service>, parts: &axum::http::request::Parts) -> Result<Response> {
    let token = crate::mcp_service::bearer(&parts.headers).map_err(|_| Error::new("authentication_failed"))?;
    crate::crypto::bearer(token).map_err(|_| Error::new("authentication_failed"))?;
    if !service.admission_open().await? { return Err(Error::new("admission_closed")); }
    let socket = parts.extensions.get::<ConnectInfo<SocketAddr>>().ok_or(Error::new("internal_error"))?.0;
    let (ip, _) = effective_peer(&parts.headers, socket, service);
    service.attempts.capability(ip, token)?;
    service.redeem_collection(token).await
}
/// Fixed route/header avoids capabilities or uploaded names in request paths.
async fn media_upload(service: &Service, parts: &axum::http::request::Parts, body: &mut Body, deadline: tokio::time::Instant) -> Result<Response> {
    if parts.headers.get_all("content-type").iter().count() != 1
        || parts.headers.get("content-type").and_then(|v|v.to_str().ok()) != Some("application/octet-stream") {
        return Err(Error::new("unsupported_content_type"));
    }
    let id = crate::mcp_service::bearer(&parts.headers).map_err(|_| Error::new("authentication_failed"))?;
    crate::crypto::bearer(id).map_err(|_| Error::new("authentication_failed"))?;
    if !service.admission_open().await? { return Err(Error::new("admission_closed")); }
    let socket = parts.extensions.get::<ConnectInfo<SocketAddr>>().ok_or(Error::new("internal_error"))?.0;
    let (ip,_) = effective_peer(&parts.headers,socket,service);
    service.attempts.capability(ip,id)?;
    service.check_media_upload(id).await?;
    let permit = service.media_upload_slot()?;
    let bytes = service.browser_ingress.read_limit(body,deadline,crate::mcp_service::attachments::FILE_BYTES).await?;
    service.accept_media_upload(id.to_owned(),bytes,permit).await?;
    Ok(data(serde_json::json!({"uploaded":true}),false))
}
fn mutation(path: &str) -> bool {
    matches!(
        path,
        "/api/mcp/media-upload"
            | "/api/setup"
            | "/api/invitations/claim"
            | "/api/login"
            | "/api/logout"
            | "/api/password"
            | "/api/admin/invitations"
            | "/api/resets/claim"
            | "/api/mcp/authorization/preview"
            | "/api/mcp/authorization/read"
            | "/api/mcp/authorization/decide"
    ) || path
        .strip_prefix("/api/admin/accounts/")
        .and_then(|s| s.split_once('/'))
        .is_some_and(|(id, action)| !id.is_empty() && matches!(action, "reset" | "reset-data" | "disable" | "delete"))
        || path.strip_prefix("/api/me/mcp-grants/").and_then(|s| s.split_once('/'))
            .is_some_and(|(id, action)| !id.is_empty() && action == "revoke")
}
async fn dispatch(
    service: &Service,
    parts: &axum::http::request::Parts,
    bytes: Option<&[u8]>,
) -> Result<Response> {
    let path = parts.uri.path();
    let method = &parts.method;
    // Browser and health routes never act as a native or SPA fallback.
    let browser = path.starts_with("/api/");
    if !browser && path != "/health" {
        return Err(Error::new("route_not_found"));
    }
    if browser && !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        origin(&parts.headers, service)?;
    }
    if parts.uri.query().is_some() {
        return Err(Error::new("invalid_request"));
    }
    if method == Method::GET {
        return match path {
            "/health" => {
                let open = service.admission_open().await?;
                if !open {
                    return Err(Error::new("admission_closed"));
                }
                Ok(data(
                    serde_json::json!({"state":"account_substrate","collection_compatibility":false}),
                    false,
                ))
            }
            "/api/status" => Ok(data(
                serde_json::json!({"setup_required":service.status().await?}),
                false,
            )),
            "/api/me" => {
                let secret = cookie(&parts.headers)?;
                let a = service.browser_authority(&secret).await?;
                let (identity, expires_at) = service.browser_me(a).await?;
                let mut response = data(
                    serde_json::json!({"id":identity.id,"username":identity.username,"role":identity.role,"server_url":service.config.server_url()}),
                    false,
                );
                set_cookie(&mut response, Some((&secret, expires_at)));
                Ok(response)
            }
            "/api/me/storage" => {
                let a = authority(service, &parts.headers).await?;
                Ok(data(serde_json::json!(service.own_storage(a).await?), false))
            }
            "/api/me/mcp-grants" => {
                let a = authority(service, &parts.headers).await?;
                Ok(data(service.oauth_grants(a).await?, false))
            }
            "/api/admin/accounts" => {
                let a = authority(service, &parts.headers).await?;
                Ok(data(
                    serde_json::json!({"accounts":service.accounts(a).await?}),
                    false,
                ))
            }
            _ => Err(Error::new("route_not_found")),
        };
    }
    if method != Method::POST {
        return Err(Error::new("route_not_found"));
    }
    content_type(&parts.headers)?;
    let socket = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .ok_or(Error::new("internal_error"))?
        .0;
    let (ip, _https) = effective_peer(&parts.headers, socket, service);
    let bytes = bytes.ok_or(Error::new("invalid_request"))?;
    match path {
        "/api/mcp/authorization/preview" => {
            let body: crate::accounts::oauth::ReadRequest = parse(bytes)?;
            service.attempts.capability(ip, &body.request)?;
            Ok(data(service.oauth_consent_preview(&body.request).await?, false))
        }
        "/api/mcp/authorization/read" => {
            let body: crate::accounts::oauth::ReadRequest = parse(bytes)?;
            service.attempts.capability(ip, &body.request)?;
            let a = authority(service, &parts.headers).await?;
            Ok(data(service.oauth_consent_read(a, &body.request).await?, false))
        }
        "/api/mcp/authorization/decide" => {
            let body: crate::accounts::oauth::DecisionRequest = parse(bytes)?;
            service.attempts.capability(ip, &body.request)?;
            let a = authority(service, &parts.headers).await?;
            Ok(data(serde_json::json!({"redirect_to":service.oauth_consent_decide(a, &body.request, &body.decision).await?}), false))
        }
        "/api/setup" | "/api/invitations/claim" => {
            let body: Claim = parse(bytes)?;
            Ok(data(
                service
                    .claim(
                        ip,
                        if path == "/api/setup" {
                            "setup"
                        } else {
                            "invite"
                        },
                        body.token,
                        &body.username,
                        body.password,
                    )
                    .await?,
                true,
            ))
        }
        "/api/login" => {
            let body: Login = parse(bytes)?;
            let (identity, secret, expires_at) = service
                .browser_login_session(ip, &body.username, body.password)
                .await?;
            let mut response = data(identity, false);
            set_cookie(&mut response, Some((&secret, expires_at)));
            Ok(response)
        }
        "/api/logout" => {
            let _: Empty = parse(bytes)?;
            service.logout(&cookie(&parts.headers)?).await?;
            let mut response = data((), false);
            set_cookie(&mut response, None);
            Ok(response)
        }
        "/api/password" => {
            let body: Password = parse(bytes)?;
            let a = authority(service, &parts.headers).await?;
            service
                .change_password(ip, a, body.current_password, body.new_password)
                .await?;
            let mut response = data((), false);
            set_cookie(&mut response, None);
            Ok(response)
        }
        "/api/admin/invitations" => {
            let _: Empty = parse(bytes)?;
            let a = authority(service, &parts.headers).await?;
            Ok(data(service.invitation(a).await?, true))
        }
        "/api/resets/claim" => {
            let body: Reset = parse(bytes)?;
            service
                .reset_claim(ip, body.token, body.new_password)
                .await?;
            Ok(data((), false))
        }
        _ => {
            if let Some((id, "revoke")) = path.strip_prefix("/api/me/mcp-grants/").and_then(|s| s.split_once('/')) {
                if !id.is_empty() {
                    let _: Empty = parse(bytes)?;
                    let a = authority(service, &parts.headers).await?;
                    return Ok(data(service.oauth_revoke(a, id.into()).await?, false));
                }
            }
            if let Some(suffix) = path.strip_prefix("/api/admin/accounts/") {
                if let Some((id, action)) = suffix.split_once('/') {
                    if !id.is_empty() && matches!(action, "reset" | "reset-data" | "disable" | "delete") {
                        let _: Empty = parse(bytes)?;
                        let a = authority(service, &parts.headers).await?;
                        return if action == "reset" {
                            Ok(data(service.reset_link(a, id.into()).await?, true))
                        } else if action == "reset-data" {
                            Ok(data(service.reset_data(a, id.into()).await?, false))
                        } else {
                            if action == "delete" {
                                service.delete_member(a, id.into()).await?;
                            } else {
                                service.disable(a, id.into()).await?;
                            }
                            Ok(data((), false))
                        };
                    }
                }
            }
            Err(Error::new("route_not_found"))
        }
    }
}
