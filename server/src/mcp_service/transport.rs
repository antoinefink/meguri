// SPDX-License-Identifier: MIT
//! Application-owned transport seams; protocol metadata never selects authority.
use super::*;
use crate::mcp_transport::{self as transport, Access, AuthorizationRequest, BoxFuture,
    DispatchFailure, ToolCall, ToolFailure, ToolOutput, AuthorizationDecision};
use axum::{http::HeaderMap, response::{IntoResponse, Response}};
use serde_json::json;

/// Accept exactly one credential and pass its unchanged token to OAuth. Neither
/// cookies nor native credentials nor request-supplied account selectors apply.
fn bearer(headers: &HeaderMap) -> std::result::Result<&str, oauth::BearerFailure> {
    let mut values = headers.get_all("authorization").iter();
    let Some(value) = values.next() else { return Err(oauth::BearerFailure::Missing); };
    if values.next().is_some() { return Err(oauth::BearerFailure::Invalid); }
    let value = value.to_str().map_err(|_| oauth::BearerFailure::Invalid)?;
    let (scheme, token) = value.split_once(' ').ok_or(oauth::BearerFailure::Invalid)?;
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty()
        || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b))
    { return Err(oauth::BearerFailure::Invalid); }
    Ok(token)
}
impl McpService {
    fn denial(&self, failure: oauth::BearerFailure, write: bool) -> Response {
        let challenge = self.service.mcp_challenge(failure, write);
        transport::authorization_response(
            (challenge.status, [("www-authenticate", challenge.value)]).into_response(), AuthorizationDecision::Challenge)
    }
}
impl transport::Authorizer<oauth::Authority> for McpService {
    fn authorize<'a>(&'a self, request: AuthorizationRequest<'a>)
        -> BoxFuture<'a, std::result::Result<oauth::Authority, Response>> {
        Box::pin(async move {
            let write = request.required == Access::Write;
            let token = bearer(request.headers).map_err(|failure| self.denial(failure, write))?;
            // Resolve the actual grant before scope selection, so client identity
            // and effective permissions always come from the verified bearer.
            let authority = self.service.mcp_authority(token, false).await
                .map_err(|error| match error.code {
                    "invalid_token" => self.denial(oauth::BearerFailure::Invalid, write),
                    _ => transport::authorization_response(axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response(),
                        AuthorizationDecision::Unavailable),
                })?;
            if write && !authority.can_write() {
                if authority.client_id() == "https://chatgpt.com/oauth/client.json"
                    && request.tool_name.is_some() && request.rpc_id.is_some() {
                    let challenge = self.service.mcp_challenge(oauth::BearerFailure::InsufficientScope, true);
                    let mut result = json!({"isError":true,
                        "content":[{"type":"text","text":"Additional collection permission required"}],
                        "_meta":{"mcp/www_authenticate":[challenge.value]}});
                    if request.headers.get("mcp-protocol-version").and_then(|v| v.to_str().ok())
                        == Some(transport::SUPPORTED_REVISIONS[0]) {
                        result["resultType"] = json!("complete");
                        result["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(transport::SUPPORTED_REVISIONS[0]);
                    }
                    return Err(transport::authorization_response(axum::Json(json!({"jsonrpc":"2.0","id":request.rpc_id,
                        "result":result})).into_response(), AuthorizationDecision::StepUp));
                }
                return Err(self.denial(oauth::BearerFailure::InsufficientScope, true));
            }
            Ok(authority)
        })
    }
}
fn failure(error: Error) -> ToolFailure {
    match error.code {
        "not_found" => ToolFailure::NotFound,
        "conflict" => ToolFailure::Conflict,
        "invalid_input" => ToolFailure::InvalidInput,
        "result_too_large" => ToolFailure::ResultTooLarge,
        "busy" => ToolFailure::Busy,
        "attachment_invalid" => ToolFailure::AttachmentInvalid,
        "attachment_unavailable" => ToolFailure::AttachmentUnavailable,
        "attachment_conflict" => ToolFailure::AttachmentConflict,
        "unsupported_collection" => ToolFailure::UnsupportedCollection,
        // Never reclassify unknown/storage/post-commit failures as retryable.
        _ => ToolFailure::Internal,
    }
}
impl McpService {
    fn dispatch_failure(&self, error: Error, write: bool) -> DispatchFailure {
        match error.code {
            "invalid_token" => DispatchFailure::Challenge(self.denial(oauth::BearerFailure::Invalid, write)),
            "insufficient_scope" => DispatchFailure::Challenge(self.denial(oauth::BearerFailure::InsufficientScope, write)),
            _ => match error.mcp_detail {
                Some(detail) => DispatchFailure::Diagnosed { failure: failure(error), detail },
                None => DispatchFailure::Tool(failure(error)),
            },
        }
    }
}
impl transport::Backend<oauth::Authority> for McpService {
    fn dispatch(&self, authority: oauth::Authority, call: ToolCall)
        -> BoxFuture<'_, std::result::Result<ToolOutput, DispatchFailure>> {
        Box::pin(async move {
            let write = call.access() == Access::Write;
            Ok(match call {
                ToolCall::DownloadCollection => {
                    let (download_url, expires) = self.download_collection(authority).await.map_err(|e|self.dispatch_failure(e,write))?;
                    let expires_at = time::OffsetDateTime::from_unix_timestamp(expires).map_err(|_|DispatchFailure::Tool(ToolFailure::Internal))?
                        .format(&time::format_description::well_known::Rfc3339).map_err(|_|DispatchFailure::Tool(ToolFailure::Internal))?;
                    ToolOutput::Download { download_url, expires_at }
                }
                ToolCall::PrepareMediaUpload => {
                    let (upload_id, upload_url, expires) = self.prepare_media_upload(authority).await.map_err(|e|self.dispatch_failure(e,write))?;
                    let expires_at = time::OffsetDateTime::from_unix_timestamp(expires).map_err(|_|DispatchFailure::Tool(ToolFailure::Internal))?
                        .format(&time::format_description::well_known::Rfc3339).map_err(|_|DispatchFailure::Tool(ToolFailure::Internal))?;
                    ToolOutput::Upload {upload_id,upload_url,expires_at}
                }
                ToolCall::SearchNotes(input) => {
                    let out = self.search_notes(authority, SearchInput {
                        query: input.query, deck_id: input.deck_id, tag: input.tag,
                        note_id: input.note_id, limit: input.limit.map(|v| v as usize), cursor: input.cursor,
                    }).await.map_err(|e| self.dispatch_failure(e, write))?;
                    ToolOutput::Notes { next_cursor: out.next_cursor, notes: out.notes.into_iter().map(|n| transport::Note {
                        note_id:n.note_id, note_type_id:n.note_type_id, fields:n.fields, tags:n.tags, revision:n.revision,
                    }).collect() }
                }
                ToolCall::ListDecksAndNoteTypes(input) => {
                    use transport::inputs::CatalogKind;
                    let out = self.list_decks_and_note_types(authority, CatalogInput {
                        kind:input.kind.map(|k| match k { CatalogKind::All=>mcp::CatalogKind::All,
                            CatalogKind::Deck=>mcp::CatalogKind::Deck, CatalogKind::NoteType=>mcp::CatalogKind::NoteType }),
                        limit:input.limit.map(|v| v as usize), cursor:input.cursor,
                    }).await.map_err(|e| self.dispatch_failure(e, write))?;
                    ToolOutput::Catalog { next_cursor:out.next_cursor,
                        decks:out.decks.into_iter().map(|d| transport::Deck { deck_id:d.deck_id, name:d.name, filtered:d.filtered }).collect(),
                        note_types:out.note_types.into_iter().map(|n| transport::NoteType { note_type_id:n.note_type_id, name:n.name, field_names:n.field_names }).collect() }
                }
                ToolCall::CreateNote(input) => {
                    let plain = CreateInput { deck_id:input.deck_id, note_type_id:input.note_type_id, fields:mcp::Fields(input.fields), tags:input.tags };
                    let out = if input.files.is_some() || input.uploads.is_some() {
                        self.write_with_media(authority,input.files,input.file_fields,input.uploads,move |backend,path,a,refs,hook|
                            backend.create_note_with_media(path,a,plain,&Runtime,refs,Some(hook))).await
                    } else { self.create_note(authority,plain).await }.map_err(|e| self.dispatch_failure(e,write))?;
                    ToolOutput::Changed { note_id:out.note_id, revision:out.revision }
                }
                ToolCall::EditNote(input) => {
                    let plain = EditInput { note_id:input.note_id, revision:input.revision, fields:input.fields.map(mcp::Fields), add_tags:input.add_tags, remove_tags:input.remove_tags };
                    let out = if input.files.is_some() || input.uploads.is_some() {
                        self.write_with_media(authority,input.files,input.file_fields,input.uploads,move |backend,path,a,refs,hook|
                            backend.edit_note_with_media(path,a,plain,&Runtime,refs,Some(hook))).await
                    } else { self.edit_note(authority,plain).await }.map_err(|e| self.dispatch_failure(e,write))?;
                    ToolOutput::Changed { note_id:out.note_id, revision:out.revision }
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_mapping_stays_distinct_from_storage_failure() {
        assert!(matches!(failure(mcp::busy()),ToolFailure::Busy));
        assert!(matches!(failure(Error::new("unsupported_collection")),ToolFailure::UnsupportedCollection));
        for code in ["storage_unavailable", "provision_unavailable", "media_integrity", "generated_unknown_fault"] {
            assert!(matches!(failure(Error::new(code)),ToolFailure::Internal));
        }
    }
    #[test]
    fn diagnostic_phases_preserve_generic_errors_and_observed_details() {
        let erased=Error::from(std::io::Error::other("generated private path/error"))
            .mcp_storage_detail(Detail::CollectionOpen);
        assert_eq!(erased.code,"storage_unavailable");
        assert_eq!(erased.mcp_detail,Some(Detail::CollectionOpen));
        assert_eq!(failure(erased),ToolFailure::Internal);
        let locked=mcp::busy().with_mcp_detail(Detail::SqliteOpenBusy)
            .mcp_storage_detail(Detail::CollectionWorker);
        assert_eq!(locked.mcp_detail,Some(Detail::SqliteOpenBusy));
        assert_eq!(failure(locked),ToolFailure::Busy);
        let invalid=Error::new("invalid_input").mcp_storage_detail(Detail::CollectionStorage);
        assert_eq!(invalid.mcp_detail,None);
        assert_eq!(failure(invalid),ToolFailure::InvalidInput);
    }
    #[test]
    fn bearer_is_single_exact_and_has_no_cookie_fallback() {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "__Host-meguri-session=generated".parse().unwrap());
        assert!(matches!(bearer(&headers), Err(oauth::BearerFailure::Missing)));
        for value in ["Basic generated", "Bearer", "Bearer ", "Bearer  generated", "Bearer generated extra", "Bearer Bearer generated", "Bearer generated,other"] {
            headers.insert("authorization", value.parse().unwrap());
            assert!(matches!(bearer(&headers), Err(oauth::BearerFailure::Invalid)));
        }
        headers.insert("authorization", "bEaReR generated_exact-token".parse().unwrap());
        assert_eq!(bearer(&headers).ok(), Some("generated_exact-token"));
        headers.append("authorization", "Bearer second".parse().unwrap());
        assert!(matches!(bearer(&headers), Err(oauth::BearerFailure::Invalid)));
    }
}
