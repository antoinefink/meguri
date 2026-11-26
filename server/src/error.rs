// SPDX-License-Identifier: MIT
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub field: Option<&'static str>,
    pub retry_after: Option<u64>,
    pub(crate) mcp_detail: Option<McpDiagnosticDetail>,
}
impl Error {
    pub const fn new(code: &'static str) -> Self {
        Self {
            code,
            field: None,
            retry_after: None,
            mcp_detail: None,
        }
    }
    pub const fn field(field: &'static str) -> Self {
        Self {
            code: "validation_failed",
            field: Some(field),
            retry_after: None,
            mcp_detail: None,
        }
    }
    pub fn limited(seconds: u64) -> Self {
        Self {
            code: "rate_limited",
            field: None,
            retry_after: Some(seconds.max(1)),
            mcp_detail: None,
        }
    }
    pub(crate) fn with_mcp_detail(mut self, detail: McpDiagnosticDetail) -> Self {
        self.mcp_detail = Some(detail);
        self
    }
    /// An erased storage cause permits a phase label, never a guessed cause.
    pub(crate) fn mcp_storage_detail(mut self, detail: McpDiagnosticDetail) -> Self {
        if self.code == "storage_unavailable" && self.mcp_detail.is_none() {
            self.mcp_detail = Some(detail);
        }
        self
    }
    pub fn status(&self) -> StatusCode {
        match self.code {
            "authentication_failed" => StatusCode::UNAUTHORIZED,
            "permission_denied" | "origin_rejected" | "capability_invalid" => StatusCode::FORBIDDEN,
            "username_unavailable"
            | "claim_in_progress"
            | "sole_administrator"
            | "stale_authority" => StatusCode::CONFLICT,
            "validation_failed" => StatusCode::UNPROCESSABLE_ENTITY,
            "rate_limited" => StatusCode::TOO_MANY_REQUESTS,
            "provision_unavailable" | "storage_unavailable" | "admission_closed" => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            "attachment_invalid" | "attachment_unavailable" => StatusCode::BAD_REQUEST,
            "browse_conflict" | "attachment_conflict" | "busy" => StatusCode::CONFLICT,
            "invalid_request" => StatusCode::BAD_REQUEST,
            "unsupported_content_type" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "request_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
            "route_not_found" => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::new("storage_unavailable")
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::new("storage_unavailable")
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}
impl std::error::Error for Error {}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Detail {
            code: &'static str,
            #[serde(skip_serializing_if = "Option::is_none")]
            field: Option<&'static str>,
        }
        #[derive(Serialize)]
        struct Body {
            error: Detail,
        }
        let mut response = (
            self.status(),
            Json(Body {
                error: Detail {
                    code: self.code,
                    field: self.field,
                },
            }),
        )
            .into_response();
        if let Some(seconds) = self.retry_after {
            response.headers_mut().insert(
                "retry-after",
                seconds.to_string().parse().expect("integer header"),
            );
        }
        response
    }
}

/// MCP terminal diagnostics only. Never serialize into a client error body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpDiagnosticDetail {
    McpQueue, PublicationGate, NativeWriter, CollectionPath, CollectionPin,
    CollectionStorage, CollectionWorker, AuthorityStorage, CollectionOpen,
    CollectionAccess, SqliteOpen, SqliteOpenBusy, SqliteAccess, SqliteAccessBusy,
    AttachmentClient, AttachmentUrlLength, AttachmentUrlCharacters, AttachmentUrlParse,
    AttachmentUrlScheme, AttachmentUrlHost, AttachmentUrlPort, AttachmentUrlUserinfo,
    AttachmentUrlFragment, AttachmentUrlRawAt, AttachmentDns, AttachmentAddress,
    AttachmentHttpClient, AttachmentHttpRequest, AttachmentHttpResponse,
    AttachmentTimeout, AttachmentBody,
}
impl McpDiagnosticDetail {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::McpQueue => "mcp_queue",
            Self::PublicationGate => "publication_gate", Self::NativeWriter => "native_writer",
            Self::CollectionPath => "collection_path", Self::CollectionPin => "collection_pin",
            Self::CollectionStorage => "collection_storage", Self::CollectionWorker => "collection_worker",
            Self::AuthorityStorage => "authority_storage", Self::CollectionOpen => "collection_open",
            Self::CollectionAccess => "collection_access", Self::SqliteOpen => "sqlite_open",
            Self::SqliteOpenBusy => "sqlite_open_busy", Self::SqliteAccess => "sqlite_access",
            Self::SqliteAccessBusy => "sqlite_access_busy",
            Self::AttachmentClient => "attachment_client",
            Self::AttachmentUrlLength => "attachment_url_length",
            Self::AttachmentUrlCharacters => "attachment_url_characters",
            Self::AttachmentUrlParse => "attachment_url_parse",
            Self::AttachmentUrlScheme => "attachment_url_scheme",
            Self::AttachmentUrlHost => "attachment_url_host",
            Self::AttachmentUrlPort => "attachment_url_port",
            Self::AttachmentUrlUserinfo => "attachment_url_userinfo",
            Self::AttachmentUrlFragment => "attachment_url_fragment",
            Self::AttachmentUrlRawAt => "attachment_url_raw_at",
            Self::AttachmentDns => "attachment_dns", Self::AttachmentAddress => "attachment_address",
            Self::AttachmentHttpClient => "attachment_http_client",
            Self::AttachmentHttpRequest => "attachment_http_request",
            Self::AttachmentHttpResponse => "attachment_http_response",
            Self::AttachmentTimeout => "attachment_timeout", Self::AttachmentBody => "attachment_body",
        }
    }
}
