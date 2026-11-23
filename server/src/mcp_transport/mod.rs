// SPDX-License-Identifier: MIT
//! Tools-only MCP adapter mounted by the existing application router.
//!
//! Contract: specs/servers/meguri-collection-mcp.md M1, M2, M4–M6.
//! Original, tools-only Streamable HTTP using the existing locked facilities.
//! Merge `router` once with the application router; retain its socket/response
//! policies. The application bridge supplies real OAuth/collection authority.
//! `Authorizer` parses the supplied headers and validates the complete required
//! scopes. Backend authority is request-local; protocol metadata is never identity.
//! The backend rechecks the grant/publication at admission, read release and write
//! commit, and bounds allocations while reading. Only fixed diagnostic metadata is logged.
//!
//! Eight nonqueued slots cover body reads, dispatch and response ownership. Reads
//! have a ten-second body-read deadline and 1 MiB cap, with depth 16 / 4096 JSON values.
//! Tool payloads are limited to 512 KiB; complete HTTP responses have a fixed cap.
//! Close `Control` at shutdown admission, join the listener, then await `idle`.
//! Dispatch runs to completion even if its HTTP caller disappears; blocking
//! workers must additionally retain their own root/barrier ownership.
//!
//! Local generated fixtures establish protocol behavior only. Real OAuth,
//! collection, hosted-client, package and full-gate acceptance remain separate.
//! The integrator must compile and run both protocol lanes through MG; modern
//! result metadata/model conformance has no independent acceptance evidence in
//! this filtered runner. The supplied discovery layout is used explicitly.
pub mod backend;
pub mod descriptors;
pub mod inputs;
mod json;
mod wire;
mod diagnostics;
#[cfg(test)]
mod tests;

pub use backend::{AuthorizationRequest, Authorizer, Backend, BoxFuture, Deck, DispatchFailure, Note, NoteType, ToolFailure, ToolOutput};
pub use descriptors::Access;
pub use inputs::ToolCall;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State as AxumState},
    http::{Method, StatusCode},
    routing::post,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use http_body::{Body as HttpBody, Frame, SizeHint};
use serde_json::{Value, json};
use std::{
    future::poll_fn,
    pin::Pin,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use diagnostics::{Diagnostic, Operation as DiagnosticOperation, Outcome, Stage, TransportFailure};
use inputs::ValidationError;

pub const SUPPORTED_REVISIONS: [&str; 2] = ["2026-07-28", "2025-11-25"];
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 512 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 3 * MAX_RESULT_BYTES + 64 * 1024;
const CAPACITY: usize = 8;
const BODY_TIMEOUT: Duration = Duration::from_secs(10);

struct Admission {
    slots: Arc<Semaphore>,
    closed: AtomicBool,
}
/// Close before server shutdown; drain after the listener and its bodies end.
/// Accepted dispatch retains its own permit even if its HTTP caller disappears.
#[derive(Clone)]
pub struct Control(Arc<Admission>);
impl Control {
    pub fn close(&self) { self.0.closed.store(true, Ordering::Release); }
    pub async fn idle(&self) {
        let _all = self.0.slots.clone().acquire_many_owned(CAPACITY as u32).await;
    }
}
struct State<A> {
    origin: String,
    log_level: crate::config::LogLevel,
    authorizer: Arc<dyn Authorizer<A>>,
    backend: Arc<dyn Backend<A>>,
    admission: Arc<Admission>,
}
struct Lease { _permit: OwnedSemaphorePermit }
// Only admission can create this context. No public constructor or body mapping.
#[derive(Clone)]
struct RequestAuthority<A> { authority: A, dispatch: wire::Dispatch, _lease: Arc<Lease> }
#[derive(Clone, Copy)]
struct DiagnosticContext { started: std::time::Instant, operation: DiagnosticOperation }
#[derive(Clone, Copy)]
struct DiagnosticEmitted;

/// Uses an already validated canonical Config origin; never Host/forwarded headers.
/// Returns an unmounted router containing ONLY the exact /mcp route.
pub fn router<A: Clone + Send + Sync + 'static>(
    config: &crate::Config,
    authorizer: Arc<dyn Authorizer<A>>,
    backend: Arc<dyn Backend<A>>,
) -> (Router, Control) {
    let admission = Arc::new(Admission {
        slots: Arc::new(Semaphore::new(CAPACITY)), closed: AtomicBool::new(false),
    });
    let state = Arc::new(State {
        origin:config.origin().to_owned(), log_level:config.log_level,
        authorizer, backend, admission:admission.clone(),
    });
    let router = Router::new().route("/mcp", post(dispatch::<A>).fallback(reject_method))
        .with_state(state.clone())
        .route_layer(middleware::from_fn_with_state(state.clone(), admit::<A>))
        .route_layer(middleware::from_fn_with_state(state, diagnose::<A>));
    (router, Control(admission))
}

fn origin_ok(headers: &axum::http::HeaderMap, origin: &str) -> bool {
    let mut values = headers.get_all("origin").iter();
    match values.next() {
        None => true,
        Some(v) => v.to_str().ok() == Some(origin) && values.next().is_none(),
    }
}
fn json_content_type(headers: &axum::http::HeaderMap) -> bool {
    let mut values = headers.get_all("content-type").iter();
    values.next().and_then(|v| v.to_str().ok()).is_some_and(|v|
        v.split(';').next().is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json")))
        && values.next().is_none()
}
async fn read(body: &mut Body) -> Result<Vec<u8>, StatusCode> {
    let mut bytes = Vec::new();
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx)).await {
        let frame = frame.map_err(|_| StatusCode::BAD_REQUEST)?;
        if let Ok(data) = frame.into_data() {
            if data.len() > MAX_REQUEST_BYTES - bytes.len() {
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
            bytes.extend_from_slice(&data);
        }
        tokio::task::yield_now().await;
    }
    Ok(bytes)
}
fn rpc_error(status: StatusCode, id: Value, code: i32, reason: ValidationError) -> Response {
    let response = (status, axum::Json(json!({"jsonrpc":"2.0", "id":id,
        "error":{"code":code, "message":reason.message(), "data":{"reason":reason.reason()}}}))).into_response();
    Diagnostic::new(Stage::Protocol, if code == -32602 { Outcome::InvalidParameters(reason) }
        else { Outcome::Protocol(reason) }).attach(response)
}
fn internal_error(id: Value) -> Response {
    Diagnostic::new(Stage::Response, Outcome::Internal).attach(
        (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"jsonrpc":"2.0", "id":id,
            "error":{"code":-32603, "message":"The MCP response could not be prepared. Check the current collection state before trying again.",
                "data":{"reason":"internal_error"}}}))).into_response())
}
fn transport_error(status: StatusCode, reason: TransportFailure) -> Response {
    Diagnostic::new(Stage::Transport, Outcome::Transport(reason)).attach((status, reason.message()).into_response())
}
async fn diagnose<A: Clone + Send + Sync + 'static>(
    AxumState(state): AxumState<Arc<State<A>>>, mut request: Request, next: Next,
) -> Response {
    let started = (state.log_level != crate::config::LogLevel::Off).then(std::time::Instant::now);
    if let Some(started) = started {
        request.extensions_mut().insert(DiagnosticContext { started, operation:DiagnosticOperation::Unknown });
    }
    let response = next.run(request).await;
    if let Some(started) = started { emit_diagnostic(state.log_level, started, &response); }
    response
}
fn emit_diagnostic(level: crate::config::LogLevel, started: std::time::Instant, response: &Response) {
    if response.extensions().get::<DiagnosticEmitted>().is_some() { return; }
    let diagnostic = response.extensions().get::<Diagnostic>().copied()
        .unwrap_or_else(|| Diagnostic::new(Stage::Response, Outcome::Internal));
    diagnostic.emit(level, response.status(), started.elapsed().as_millis());
}

async fn admit<A: Clone + Send + Sync + 'static>(
    AxumState(state): AxumState<Arc<State<A>>>, request: Request, next: Next,
) -> Response {
    let (mut parts, mut incoming) = request.into_parts();
    let early = if !origin_ok(&parts.headers, &state.origin) {
        Some(transport_error(StatusCode::FORBIDDEN, TransportFailure::Origin))
    } else if parts.method != Method::POST {
        let mut response = transport_error(StatusCode::METHOD_NOT_ALLOWED, TransportFailure::Method);
        response.headers_mut().insert("allow", "POST".parse().unwrap());
        Some(response)
    } else if !json_content_type(&parts.headers) {
        Some(transport_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, TransportFailure::ContentType))
    } else { None };
    if let Some(response) = early { return retain(response, incoming, None, state); }
    if state.admission.closed.load(Ordering::Acquire) {
        return retain(transport_error(StatusCode::SERVICE_UNAVAILABLE, TransportFailure::Shutdown), incoming, None, state);
    }
    let permit = match state.admission.slots.clone().try_acquire_owned() {
        Ok(permit) => Arc::new(Lease { _permit:permit }),
        Err(_) => {
            let mut response = transport_error(StatusCode::SERVICE_UNAVAILABLE, TransportFailure::Busy);
            response.headers_mut().insert("retry-after", "1".parse().unwrap());
            return retain(response, incoming, None, state);
        }
    };
    if state.admission.closed.load(Ordering::Acquire) {
        return retain(transport_error(StatusCode::SERVICE_UNAVAILABLE, TransportFailure::Shutdown), incoming, Some(permit), state);
    }
    let bytes = match tokio::time::timeout(BODY_TIMEOUT, read(&mut incoming)).await {
        Ok(Ok(bytes)) => bytes,
        result => {
            let status = match result { Ok(Err(status)) => status, _ => StatusCode::REQUEST_TIMEOUT };
            let reason = match status {
                StatusCode::PAYLOAD_TOO_LARGE => TransportFailure::BodyLimit,
                StatusCode::REQUEST_TIMEOUT => TransportFailure::BodyTimeout,
                _ => TransportFailure::BodyRead,
            };
            return retain(transport_error(status, reason), incoming, Some(permit), state);
        }
    };
    let value = match json::parse(&bytes) {
        Ok(value) => value,
        Err(error) => {
            let (code, reason) = if error.is_data() { (-32600, ValidationError::Envelope) }
                else { (-32700, ValidationError::Json) };
            return retain(rpc_error(StatusCode::BAD_REQUEST, Value::Null, code, reason),
                incoming, Some(permit), state);
        }
    };
    let operation = DiagnosticOperation::classify(&value);
    let validated = match wire::validate(&parts.headers, value, &bytes) {
        Ok(validated) => validated,
        Err(mut response) => {
            if let Some(diagnostic) = response.extensions_mut().get_mut::<Diagnostic>() { diagnostic.operation = operation; }
            return retain(response, incoming, Some(permit), state);
        }
    };
    let auth_request = AuthorizationRequest {
        headers:&parts.headers, required:validated.access(),
        rpc_id:if validated.id.is_null() { None } else { Some(&validated.id) },
        tool_name:validated.name(),
    };
    let authority = match state.authorizer.authorize(auth_request).await {
        Ok(authority) => authority,
        Err(response) => {
            let response = authorization_outcome(response, Stage::Authorization, operation);
            return retain(response, incoming, Some(permit), state);
        }
    };
    parts.headers.remove("authorization");
    parts.headers.remove("cookie");
    parts.extensions.insert(RequestAuthority {
        authority, dispatch:validated, _lease:permit.clone(),
    });
    if let Some(context) = parts.extensions.get_mut::<DiagnosticContext>() { context.operation = operation; }
    let mut response = next.run(Request::from_parts(parts, Body::empty())).await;
    if let Some(diagnostic) = response.extensions_mut().get_mut::<Diagnostic>() { diagnostic.operation = operation; }
    retain(response, incoming, Some(permit), state)
}

async fn reject_method() -> StatusCode { StatusCode::METHOD_NOT_ALLOWED }

async fn dispatch<A: Clone + Send + Sync + 'static>(
    AxumState(state): AxumState<Arc<State<A>>>, mut request: Request,
) -> Response {
    let Some(context) = request.extensions_mut().remove::<RequestAuthority<A>>() else {
        return internal_error(Value::Null);
    };
    let id = context.dispatch.id.clone();
    let diagnostic = request.extensions_mut().remove::<DiagnosticContext>();
    // A disconnected caller drops only its join handle. A possibly committing
    // operation, the backend owner and its admission lease run to completion.
    match tokio::spawn(async move {
        let level = state.log_level;
        let mut response = execute(state, context).await;
        if let Some(context) = diagnostic {
            if let Some(diagnostic) = response.extensions_mut().get_mut::<Diagnostic>() {
                diagnostic.operation = context.operation;
            }
            emit_diagnostic(level, context.started, &response);
            response.extensions_mut().insert(DiagnosticEmitted);
        }
        response
    }).await {
        Ok(response) => response,
        Err(_) => internal_error(id),
    }
}

fn tool_result(value: Value, is_error: bool) -> Value {
    let text = match (value.get("download_url").and_then(Value::as_str), value.get("expires_at").and_then(Value::as_str)) {
        (Some(url), Some(expires)) if !is_error => format!("[Download collection]({url})\n\nShare this link with the user. Sign in to the same Meguri account and choose Download collection before {expires}. This link can be used once."),
        _ => serde_json::to_string(&value).expect("JSON tool payload"),
    };
    json!({"content":[{"type":"text", "text":text}],
        "structuredContent":value, "isError":is_error})
}

async fn execute<A: Clone + Send + Sync + 'static>(
    state: Arc<State<A>>, context: RequestAuthority<A>,
) -> Response {
    use wire::Operation;
    let server_info = json!({"name":"Meguri", "version":env!("CARGO_PKG_VERSION")});
    let modern = context.dispatch.modern;
    let mut outcome = Outcome::Success;
    let mut detail = None;
    let mut result = match context.dispatch.operation {
        Operation::Discover => json!({"resultType":"complete", "supportedVersions":SUPPORTED_REVISIONS,
            "capabilities":{"tools":{}}, "_meta":{"io.modelcontextprotocol/serverInfo":server_info}}),
        Operation::Initialize => json!({"protocolVersion":SUPPORTED_REVISIONS[1],
            "capabilities":{"tools":{}}, "serverInfo":server_info}),
        Operation::Initialized => return Diagnostic::new(Stage::Dispatch, Outcome::Success).attach(StatusCode::ACCEPTED.into_response()),
        Operation::Ping => json!({}),
        Operation::List => json!({"tools":descriptors::descriptors()}),
        Operation::Call { call, .. } => {
            let access = call.access();
            match state.backend.dispatch(context.authority, call).await {
                Ok(output) if output.bounded() => match json::encode(&output, MAX_RESULT_BYTES) {
                    Ok(bytes) => tool_result(serde_json::from_slice(&bytes).expect("encoded tool output"), false),
                    Err(_) => {
                        outcome = Outcome::ToolError(ToolFailure::ResultTooLarge);
                        tool_result(ToolFailure::ResultTooLarge.value(), true)
                    },
                },
                Ok(_) => {
                    outcome = Outcome::ToolError(ToolFailure::ResultTooLarge);
                    tool_result(ToolFailure::ResultTooLarge.value(), true)
                },
                Err(failure) => {
                    let error = match failure {
                        DispatchFailure::Tool(error) => error,
                        DispatchFailure::Diagnosed { failure, detail: diagnosed } => {
                            detail = Some(diagnosed);
                            failure
                        },
                        DispatchFailure::Challenge(response) => return authorization_outcome(response, Stage::Dispatch, DiagnosticOperation::Unknown),
                    };
                    outcome = Outcome::ToolError(error);
                    tool_result(error.value_for_access(access), true)
                },
            }
        }
    };
    if modern {
        result["resultType"] = json!("complete");
        result["_meta"][wire::VERSION] = json!(SUPPORTED_REVISIONS[0]);
    }
    let envelope = json!({"jsonrpc":"2.0", "id":context.dispatch.id, "result":result});
    match json::encode(&envelope, MAX_RESPONSE_BYTES) {
        Ok(bytes) => Diagnostic::new(Stage::Dispatch, outcome).with_detail(detail).attach(([("content-type", "application/json")], bytes).into_response()),
        Err(_) => internal_error(envelope["id"].clone()),
    }
}

/// Application-owned decisions, captured without parsing a challenge or body.
pub(crate) enum AuthorizationDecision { Challenge, StepUp, Unavailable }
pub(crate) fn authorization_response(response: Response, decision: AuthorizationDecision) -> Response {
    let outcome = match decision {
        AuthorizationDecision::Challenge => Outcome::Challenge,
        AuthorizationDecision::StepUp => Outcome::StepUp,
        AuthorizationDecision::Unavailable => Outcome::Internal,
    };
    Diagnostic::new(Stage::Authorization, outcome).attach(response)
}
fn authorization_outcome(response: Response, stage: Stage, operation: DiagnosticOperation) -> Response {
    // Authorizers return either an OAuth denial, a tool-level step-up, or an
    // unavailable response. Classify the decision without examining its body.
    let outcome = response.extensions().get::<Diagnostic>().map(|v| v.outcome).unwrap_or_else(|| match response.status() {
        StatusCode::OK => Outcome::StepUp,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Outcome::Challenge,
        _ => Outcome::Internal,
    });
    Diagnostic { stage, operation, outcome, detail:None }.attach(response)
}

struct Retained<A> {
    response: Body,
    _incoming: Body,
    _lease: Option<Arc<Lease>>,
    _owner: Arc<State<A>>,
}
impl<A: Send + Sync + 'static> HttpBody for Retained<A> {
    type Data = Bytes;
    type Error = axum::Error;
    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        Pin::new(&mut self.response).poll_frame(cx)
    }
    fn size_hint(&self) -> SizeHint { self.response.size_hint() }
}
fn retain<A: Send + Sync + 'static>(
    mut response: Response, incoming: Body, lease: Option<Arc<Lease>>, owner: Arc<State<A>>,
) -> Response {
    response.headers_mut().insert("connection", "close".parse().unwrap());
    response.headers_mut().insert("cache-control", "no-store".parse().unwrap());
    response.headers_mut().insert("referrer-policy", "no-referrer".parse().unwrap());
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, Body::new(Retained {
        response:body, _incoming:incoming, _lease:lease, _owner:owner,
    }))
}
