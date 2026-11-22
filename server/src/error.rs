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
}
impl Error {
    pub const fn new(code: &'static str) -> Self {
        Self {
            code,
            field: None,
            retry_after: None,
        }
    }
    pub const fn field(field: &'static str) -> Self {
        Self {
            code: "validation_failed",
            field: Some(field),
            retry_after: None,
        }
    }
    pub fn limited(seconds: u64) -> Self {
        Self {
            code: "rate_limited",
            field: None,
            retry_after: Some(seconds.max(1)),
        }
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
            "attachment_conflict" | "busy" => StatusCode::CONFLICT,
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
