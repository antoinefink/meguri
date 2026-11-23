// SPDX-License-Identifier: MIT
//! Fixed metadata only; never format requests, errors, headers or response bodies.
use super::{inputs::ValidationError, ToolFailure};
use crate::error::McpDiagnosticDetail;
use axum::{http::StatusCode, response::Response};
use crate::config::LogLevel;
use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation { Unknown, Discover, Initialize, Initialized, Ping, List, Search, Catalog, Create, Edit, Download }
impl Operation {
    pub fn classify(value: &serde_json::Value) -> Self {
        match value.get("method").and_then(serde_json::Value::as_str) {
            Some("server/discover") => Self::Discover,
            Some("initialize") => Self::Initialize,
            Some("notifications/initialized") => Self::Initialized,
            Some("ping") => Self::Ping,
            Some("tools/list") => Self::List,
            Some("tools/call") => match value["params"]["name"].as_str() {
                Some("search_notes") => Self::Search,
                Some("list_decks_and_note_types") => Self::Catalog,
                Some("create_note") => Self::Create,
                Some("edit_note") => Self::Edit,
                Some("download_collection") => Self::Download,
                _ => Self::Unknown,
            },
            _ => Self::Unknown,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown", Self::Discover => "discover", Self::Initialize => "initialize",
            Self::Initialized => "initialized", Self::Ping => "ping", Self::List => "tools_list",
            Self::Search => "search_notes", Self::Catalog => "list_decks_and_note_types",
            Self::Create => "create_note", Self::Edit => "edit_note", Self::Download => "download_collection",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransportFailure { Origin, Method, ContentType, Shutdown, Busy, BodyLimit, BodyRead, BodyTimeout }
impl TransportFailure {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Origin => "origin_rejected", Self::Method => "method_rejected",
            Self::ContentType => "content_type", Self::Shutdown => "shutdown",
            Self::Busy => "admission_busy", Self::BodyLimit => "body_limit",
            Self::BodyRead => "body_read", Self::BodyTimeout => "body_timeout",
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::Origin => "Use the configured server origin.", Self::Method => "Use POST for MCP requests.",
            Self::ContentType => "Send an application/json request body.",
            Self::Shutdown => "The server is shutting down. Reconnect when it is available.",
            Self::Busy => "The server is busy. Retry after one second.",
            Self::BodyLimit => "Reduce the request body to at most 1 MiB.",
            Self::BodyRead => "Send a complete JSON request body.",
            Self::BodyTimeout => "Send the complete request body within ten seconds.",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage { Transport, Protocol, Authorization, Dispatch, Response }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Success, Transport(TransportFailure), InvalidParameters(ValidationError),
    Protocol(ValidationError), ToolError(ToolFailure), Challenge, StepUp, Internal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Diagnostic { pub stage: Stage, pub operation: Operation, pub outcome: Outcome, pub detail: Option<McpDiagnosticDetail> }
impl Diagnostic {
    pub fn new(stage: Stage, outcome: Outcome) -> Self { Self { stage, operation:Operation::Unknown, outcome, detail:None } }
    pub fn with_detail(mut self, detail: Option<McpDiagnosticDetail>) -> Self {
        self.detail = detail;
        self
    }
    pub fn attach(self, mut response: Response) -> Response {
        response.extensions_mut().insert(self);
        response
    }
    pub fn record(self, level: LogLevel, status: StatusCode, elapsed_ms: u128) -> Option<String> {
        if level == LogLevel::Off || (level == LogLevel::Info && self.outcome == Outcome::Success) { return None; }
        let stage = match self.stage { Stage::Transport => "transport", Stage::Protocol => "protocol",
            Stage::Authorization => "authorization", Stage::Dispatch => "dispatch", Stage::Response => "response" };
        let (outcome, reason) = match self.outcome {
            Outcome::Success => ("success", "none"),
            Outcome::Transport(v) => ("rejected", v.reason()),
            Outcome::InvalidParameters(v) => ("invalid_parameters", v.reason()),
            Outcome::Protocol(v) => ("rejected", v.reason()),
            Outcome::ToolError(v) => ("tool_error", v.code()),
            Outcome::Challenge => ("challenge", "authorization_required"),
            Outcome::StepUp => ("step_up", "additional_scope_required"),
            Outcome::Internal => ("failure", "internal_error"),
        };
        let mut record = format!("mcp_response_ready stage={stage} operation={} status={} outcome={outcome} reason={reason}",
            self.operation.label(), status.as_u16());
        if let Some(detail) = self.detail { record.push_str(&format!(" detail={}", detail.label())); }
        if level == LogLevel::Debug { record.push_str(&format!(" elapsed_ms={elapsed_ms}")); }
        Some(record)
    }
    pub fn emit(self, level: LogLevel, status: StatusCode, elapsed_ms: u128) {
        if let Some(record) = self.record(level, status, elapsed_ms) {
            let _ = writeln!(std::io::stderr().lock(), "{record}");
        }
    }
}
