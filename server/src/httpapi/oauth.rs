// SPDX-License-Identifier: MIT
//! OAuth HTTP boundary. Browser consent uses the parent JSON/cookie/Origin path;
//! token exchange is form encoded and never authenticates with a cookie.
use super::*;
use crate::accounts::oauth::{self, Authorization};

pub(super) fn route(path: &str) -> bool {
    matches!(path,"/oauth/authorize"|"/oauth/token"|"/.well-known/oauth-protected-resource/mcp"|"/.well-known/oauth-authorization-server")
}
fn redirect(location: String) -> Result<Response> {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert("location",location.parse().map_err(|_| Error::new("internal_error"))?);
    Ok(response)
}
fn local_error() -> Response {
    (StatusCode::BAD_REQUEST, [("content-type","text/html; charset=utf-8")],
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Authorization unavailable</title><h1>Authorization unavailable</h1><p>Return to the client and start again.</p></html>").into_response()
}
fn oauth_error(error: Error) -> Response {
    let (status,code) = match error.code {
        "invalid_request"|"invalid_grant"|"invalid_scope"|"invalid_client"|"invalid_target"|"unsupported_grant_type" => (StatusCode::BAD_REQUEST,error.code),
        "rate_limited" => (StatusCode::TOO_MANY_REQUESTS,"temporarily_unavailable"),
        "storage_unavailable"|"admission_closed" => (StatusCode::SERVICE_UNAVAILABLE,"temporarily_unavailable"),
        "unsupported_content_type" => (StatusCode::UNSUPPORTED_MEDIA_TYPE,"invalid_request"),
        "request_too_large" => (StatusCode::PAYLOAD_TOO_LARGE,"invalid_request"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR,"server_error"),
    };
    let mut response=(status,Json(serde_json::json!({"error":code}))).into_response();
    if let Some(seconds)=error.retry_after {
        response.headers_mut().insert("retry-after",seconds.to_string().parse().expect("integer"));
    }
    response
}
fn form_content_type(headers: &HeaderMap) -> Result<()> {
    if headers.get_all("content-type").iter().count() != 1 { return Err(Error::new("unsupported_content_type")); }
    let value=headers.get("content-type").and_then(|h| h.to_str().ok()).ok_or(Error::new("unsupported_content_type"))?;
    let mut parts=value.split(';');
    if !parts.next().is_some_and(|v| v.trim().eq_ignore_ascii_case("application/x-www-form-urlencoded")) {
        return Err(Error::new("unsupported_content_type"));
    }
    if let Some(charset)=parts.next() {
        let Some((name,value))=charset.trim().split_once('=') else {return Err(Error::new("unsupported_content_type"))};
        if !name.trim().eq_ignore_ascii_case("charset") || !(value.trim().eq_ignore_ascii_case("utf-8") || value.trim().eq_ignore_ascii_case("\"utf-8\"")) {
            return Err(Error::new("unsupported_content_type"));
        }
    }
    if parts.next().is_some() {return Err(Error::new("unsupported_content_type"));}
    Ok(())
}
pub(super) async fn dispatch(service: &Service, parts: &axum::http::request::Parts, body: &mut Body, deadline: tokio::time::Instant) -> Result<Response> {
    let path=parts.uri.path();
    let origin=service.config.origin();
    if parts.method==Method::GET {
        match path {
            "/.well-known/oauth-protected-resource/mcp" if parts.uri.query().is_none() => return Ok(Json(serde_json::json!({
                "resource":format!("{origin}/mcp"),"authorization_servers":[origin],"scopes_supported":[oauth::READ,"collection:write"],"bearer_methods_supported":["header"]
            })).into_response()),
            "/.well-known/oauth-authorization-server" if parts.uri.query().is_none() => return Ok(Json(serde_json::json!({
                "issuer":origin,"authorization_endpoint":format!("{origin}/oauth/authorize"),"token_endpoint":format!("{origin}/oauth/token"),
                "response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],
                "code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"],
                "scopes_supported":[oauth::READ,"collection:write"],"client_id_metadata_document_supported":true,
                "authorization_response_iss_parameter_supported":true
            })).into_response()),
            "/oauth/authorize" => {
                let p=match oauth::form(parts.uri.query().unwrap_or("").as_bytes()) {Ok(p)=>p,Err(_)=>return Ok(local_error())};
                // Only this exact reviewed callback can receive an OAuth error.
                let approved=p.get("client_id").and_then(|id| oauth::client(id).ok())
                    .filter(|c| p.get("redirect_uri").is_some_and(|r| r==c.redirect));
                let request=match Authorization::parse(origin,&p) {
                    Ok(r)=>r,
                    Err(e)=>return match approved {
                        Some(c)=>redirect(oauth::callback(origin,c.redirect,p.get("state").map(String::as_str),"error",e.code)),
                        None=>Ok(local_error()),
                    },
                };
                let socket=parts.extensions.get::<ConnectInfo<SocketAddr>>().ok_or(Error::new("internal_error"))?.0;
                let (ip,_)=effective_peer(&parts.headers,socket,service);
                // The digest bucket stores neither state nor the pending handle.
                if service.attempts.capability(ip,parts.uri.query().unwrap_or("")).is_err() {
                    return redirect(request.callback(origin,"error","temporarily_unavailable"));
                }
                let failure=request.callback(origin,"error","temporarily_unavailable");
                return redirect(match service.oauth_begin(request).await {
                    Ok(location)=>location,
                    Err(_)=>failure,
                });
            }
            _=>{},
        }
    }
    if parts.method==Method::POST && path=="/oauth/token" {
        let result=async {
            if parts.uri.query().is_some() || parts.headers.contains_key("authorization") {
                return Err(Error::new("invalid_request"));
            }
            form_content_type(&parts.headers)?;
            let bytes=service.browser_ingress.read(body,deadline).await?;
            let p=oauth::form(&bytes)?;
            let socket=parts.extensions.get::<ConnectInfo<SocketAddr>>().ok_or(Error::new("internal_error"))?.0;
            let (ip,_)=effective_peer(&parts.headers,socket,service);
            let key=p.get("code").or_else(||p.get("refresh_token")).map_or("",String::as_str);
            service.attempts.capability(ip,key)?;
            service.oauth_token(p).await
        }.await;
        return Ok(match result {Ok(tokens)=>Json(tokens).into_response(),Err(e)=>oauth_error(e)});
    }
    let mut response=StatusCode::METHOD_NOT_ALLOWED.into_response();
    response.headers_mut().insert("allow",if path=="/oauth/token" {"POST"} else {"GET"}.parse().unwrap());
    Ok(response)
}
