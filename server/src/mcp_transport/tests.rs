// SPDX-License-Identifier: MIT
//! Disposable loopback protocol fixtures only; no accounts, credentials or data
//! from a real installation. These do not establish hosted-client compatibility.
use super::*;
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
struct FixtureAuthority(&'static str);
struct Fixture {
    calls: Mutex<Vec<(&'static str, &'static str)>>,
}
impl Authorizer<FixtureAuthority> for Fixture {
    fn authorize<'a>(&'a self, request: AuthorizationRequest<'a>)
        -> BoxFuture<'a, Result<FixtureAuthority, Response>> {
        Box::pin(async move {
            let (account, write) = match request.headers.get("authorization").and_then(|h| h.to_str().ok()) {
                Some("Bearer generated-fixture-a") => ("a", true),
                Some("Bearer generated-fixture-b") => ("b", true),
                Some("Bearer generated-fixture-read") => ("a", false),
                _ => return Err(StatusCode::UNAUTHORIZED.into_response()),
            };
            if request.required == Access::Write && !write {
                let mut response = StatusCode::FORBIDDEN.into_response();
                response.headers_mut().insert("www-authenticate",
                    "Bearer error=\"insufficient_scope\", scope=\"collection:read collection:write\"".parse().unwrap());
                return Err(response);
            }
            Ok(FixtureAuthority(account))
        })
    }
}
impl Backend<FixtureAuthority> for Fixture {
    fn dispatch(&self, authority: FixtureAuthority, call: ToolCall)
        -> BoxFuture<'_, Result<ToolOutput, DispatchFailure>> {
        Box::pin(async move {
            let (name, result) = match call {
                ToolCall::SearchNotes(input) => {
                    assert_eq!(input.note_id.as_deref(), Some("9007199254740993"));
                    ("search_notes", Ok(ToolOutput::Notes {
                        notes:vec![Note {
                            note_id:"9007199254740993".into(), note_type_id:"1".into(),
                            fields:BTreeMap::from([("Front".into(), format!("generated {} <b>data</b>", authority.0))]),
                            tags:vec!["generated".into()], revision:"generated-revision".into(),
                        }], next_cursor:None,
                    }))
                }
                ToolCall::ListDecksAndNoteTypes(_) => ("list_decks_and_note_types", Ok(ToolOutput::Catalog {
                    decks:vec![Deck { deck_id:"1".into(), name:"Generated deck".into(), filtered:false }],
                    note_types:vec![NoteType { note_type_id:"1".into(), name:"Generated type".into(),
                        field_names:vec!["Front".into(), "Back".into()] }], next_cursor:None,
                })),
                ToolCall::CreateNote(_) => ("create_note", Ok(ToolOutput::Changed {
                    note_id:"9007199254740994".into(), revision:"generated-created".into(),
                })),
                ToolCall::EditNote(_) => ("edit_note", Err(ToolFailure::Conflict.into())),
                ToolCall::DownloadCollection => ("download_collection", Ok(ToolOutput::Download {
                    download_url:"https://generated.example.test/mcp/download#download=fixture".into(), expires_at:"2026-11-23T14:00:00Z".into(),
                })),
                ToolCall::PrepareMediaUpload => ("prepare_media_upload", Ok(ToolOutput::Upload {
                    upload_id:"u".repeat(43),upload_url:"https://generated.example.test/mcp/upload#upload=fixture".into(),expires_at:"2026-11-22T14:00:00Z".into(),
                })),
            };
            self.calls.lock().unwrap().push((authority.0, name));
            result
        })
    }
}
struct Running {
    url: String,
    fixture: Arc<Fixture>,
    control: Control,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}
impl Running {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = crate::Config::new("https://mcp-fixture.invalid", address).unwrap();
        let fixture = Arc::new(Fixture { calls:Mutex::new(Vec::new()) });
        let (router, control) = router(&config, fixture.clone(), fixture.clone());
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        Self { url:format!("http://{address}/mcp"), fixture, control, shutdown:Some(shutdown), task }
    }
    fn request(&self, revision: &str, bearer: &str) -> reqwest::RequestBuilder {
        reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap()
            .post(&self.url).header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", revision).bearer_auth(bearer)
    }
    fn message(&self, revision: &str, bearer: &str, method: &str, mut params: Value)
        -> reqwest::RequestBuilder {
        let mut request = self.request(revision, bearer);
        if revision == SUPPORTED_REVISIONS[0] {
            params["_meta"] = modern_meta();
            request = request.header("Mcp-Method", method);
            if method == "tools/call" {
                request = request.header("Mcp-Name", params["name"].as_str().unwrap());
            }
        }
        request.json(&body(method, params))
    }
    async fn stop(mut self) {
        self.control.close();
        self.shutdown.take().unwrap().send(()).unwrap();
        self.task.await.unwrap();
        self.control.idle().await;
    }
}
fn modern_meta() -> Value {
    json!({(wire::VERSION):SUPPORTED_REVISIONS[0],
        (wire::CLIENT_INFO):{"name":"Generated fixture", "version":"1"},
        (wire::CLIENT_CAPABILITIES):{}})
}
#[test]
fn attachment_wire_shapes_reject_missing_null_and_duplicate_members_before_dispatch() {
    let valid_file = json!({"download_url":"https://files.oaiusercontent.com/generated","file_id":"generated"});
    for (name,arguments) in [
        ("create_note",json!({"deck_id":"1","note_type_id":"1","fields":{},"files":[valid_file.clone()],"file_fields":["Front"]})),
        ("edit_note",json!({"note_id":"1","revision":"generated","files":[valid_file.clone()],"file_fields":["Front"]})),
        ("prepare_media_upload",json!({})),
    ] {
        let value = body("tools/call",json!({"name":name,"arguments":arguments}));
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(wire::validate(&axum::http::HeaderMap::new(),value,&bytes).is_ok());
    }
    for arguments in [
        json!({"note_id":"1","revision":"generated","files":[{"download_url":"x"}],"file_fields":["Front"]}),
        json!({"note_id":"1","revision":"generated","files":[{"download_url":"x","file_id":"f","file_name":null}],"file_fields":["Front"]}),
        json!({"note_id":"1","revision":"generated","uploads":null}),
    ] {
        let value = body("tools/call",json!({"name":"edit_note","arguments":arguments}));
        let bytes = serde_json::to_vec(&value).unwrap();
        let response = wire::validate(&axum::http::HeaderMap::new(),value,&bytes).err().unwrap();
        assert_eq!(response.extensions().get::<Diagnostic>().unwrap().outcome,
            Outcome::InvalidParameters(ValidationError::Arguments));
    }
    let duplicate = br#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"edit_note","arguments":{"note_id":"1","revision":"generated","uploads":[{"upload_id":"u","field":"Front","field":"Back"}]}}}"#;
    assert!(super::json::parse(duplicate).is_err());
}
fn body(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":7, "method":method, "params":params})
}
async fn rpc(r: &Running, revision: &str, bearer: &str, method: &str, params: Value) -> Value {
    let response = r.message(revision, bearer, method, params).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("mcp-session-id").is_none());
    assert!(response.headers()["content-type"].to_str().unwrap().starts_with("application/json"));
    response.json().await.unwrap()
}
fn same_content(value: &Value) {
    assert_eq!(serde_json::from_str::<Value>(value["result"]["content"][0]["text"].as_str().unwrap()).unwrap(),
        value["result"]["structuredContent"]);
}

#[tokio::test]
async fn both_lifecycles_list_real_descriptors_and_bind_each_call_to_its_bearer() {
    let r = Running::start().await;
    for revision in SUPPORTED_REVISIONS {
        let identity = if revision == "2026-07-28" {
            rpc(&r, revision, "generated-fixture-a", "server/discover", json!({})).await
        } else {
            let identity = rpc(&r, revision, "generated-fixture-a", "initialize", json!({
                "protocolVersion":revision, "capabilities":{},
                "clientInfo":{"name":"Generated fixture", "version":"1"}
            })).await;
            assert_eq!(identity["result"]["protocolVersion"], revision);
            let notification = r.request(revision, "generated-fixture-a")
                .json(&json!({"jsonrpc":"2.0", "method":"notifications/initialized"}))
                .send().await.unwrap();
            assert_eq!(notification.status(), StatusCode::ACCEPTED);
            identity
        };
        if revision == SUPPORTED_REVISIONS[0] {
            assert_eq!(identity["result"]["resultType"], "complete");
            assert_eq!(identity["result"]["supportedVersions"], json!(SUPPORTED_REVISIONS));
            assert_eq!(identity["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "Meguri");
            assert!(identity["result"].get("serverInfo").is_none());
        } else { assert_eq!(identity["result"]["serverInfo"]["name"], "Meguri"); }
        assert_eq!(identity["result"]["capabilities"], json!({"tools":{}}));
        let listed = rpc(&r, revision, "generated-fixture-a", "tools/list", json!({})).await;
        if revision == SUPPORTED_REVISIONS[0] {
            assert_eq!(listed["result"]["resultType"], "complete");
            assert_eq!(listed["result"]["_meta"][wire::VERSION], revision);
        }
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 6);
        for expected in descriptors::descriptors() {
            let actual = tools.iter().find(|v| v["name"] == expected["name"]).unwrap();
            for key in ["inputSchema", "annotations", "securitySchemes", "_meta"] {
                assert_eq!(actual[key], expected[key]);
            }
        }
        for account in ["a", "b"] {
            let result = rpc(&r, revision, &format!("generated-fixture-{account}"), "tools/call",
                json!({"name":"search_notes", "arguments":{"note_id":"9007199254740993"}})).await;
            if revision == SUPPORTED_REVISIONS[0] {
                assert_eq!(result["result"]["resultType"], "complete");
                assert_eq!(result["result"]["_meta"][wire::VERSION], revision);
            }
            assert_eq!(result["result"]["isError"], false);
            assert_eq!(result["result"]["structuredContent"]["notes"][0]["note_id"], "9007199254740993");
            assert_eq!(result["result"]["structuredContent"]["notes"][0]["fields"]["Front"],
                format!("generated {account} <b>data</b>"));
            same_content(&result);
        }
    }
    assert_eq!(*r.fixture.calls.lock().unwrap(), vec![("a","search_notes"), ("b","search_notes"),
        ("a","search_notes"), ("b","search_notes")]);
    r.stop().await;
}

#[tokio::test]
async fn scope_denial_precedes_dispatch_and_tool_failures_are_results() {
    let r = Running::start().await;
    let create = json!({"name":"create_note", "arguments":{"deck_id":"1", "note_type_id":"1",
        "fields":{"Front":"generated"}}});
    let denied = r.message("2026-07-28", "generated-fixture-read", "tools/call", create.clone()).send().await.unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(denied.headers()["www-authenticate"].to_str().unwrap().contains("collection:read collection:write"));
    assert!(r.fixture.calls.lock().unwrap().is_empty());
    let created = rpc(&r, "2026-07-28", "generated-fixture-a", "tools/call", create).await;
    assert_eq!(created["result"]["structuredContent"]["note_id"], "9007199254740994");
    same_content(&created);
    let conflict = rpc(&r, "2026-07-28", "generated-fixture-b", "tools/call", json!({
        "name":"edit_note", "arguments":{"note_id":"1", "revision":"generated-stale", "fields":{}}
    })).await;
    assert!(conflict.get("error").is_none());
    assert_eq!(conflict["result"]["isError"], true);
    assert_eq!(conflict["result"]["structuredContent"]["error"]["code"], "conflict");
    same_content(&conflict);
    let invalid = rpc(&r, "2026-07-28", "generated-fixture-a", "tools/call", json!({
        "name":"create_note", "arguments":{"account_id":"b", "deck_id":"1", "note_type_id":"1", "fields":{}}
    })).await;
    assert_eq!(invalid["error"]["code"], -32602);
    assert!(invalid.get("result").is_none());
    assert_eq!(r.fixture.calls.lock().unwrap().len(), 2);
    r.stop().await;
}

#[tokio::test]
async fn exact_origin_post_only_no_resources_and_duplicate_member_rejection() {
    let r = Running::start().await;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
    let response = client.get(&r.url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["allow"], "POST");
    for origin in ["null", "https://other.invalid", "https://mcp-fixture.invalid/"] {
        let response = r.request("2026-07-28", "generated-fixture-a").header("origin", origin)
            .json(&body("server/discover", json!({}))).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = r.message("2026-07-28", "generated-fixture-a", "tools/list", json!({}))
        .header("origin", "https://mcp-fixture.invalid").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resources = r.message("2026-07-28", "generated-fixture-a", "resources/list", json!({}))
        .send().await.unwrap();
    assert_eq!(resources.status(), StatusCode::NOT_FOUND);
    assert_eq!(resources.json::<Value>().await.unwrap()["error"]["code"], -32601);
    let duplicate = r.request("2026-07-28", "generated-fixture-a")
        .header("content-type", "application/json")
        .body(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"create_note","arguments":{"deck_id":"1","note_type_id":"1","fields":{"Front":"one","Front":"two"}}}}"#)
        .send().await.unwrap();
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    assert_eq!(duplicate.json::<Value>().await.unwrap()["error"]["code"], -32600);
    assert!(r.fixture.calls.lock().unwrap().is_empty());
    r.stop().await;
}

#[test]
fn named_inputs_preserve_i64_ids_and_enforce_cross_field_constraints() {
    let parse = |name, v:Value| inputs::parse(name, v.as_object().unwrap().clone());
    assert!(parse("search_notes", json!({"note_id":"9223372036854775807"})).is_ok());
    for args in [json!({}), json!({"note_id":"9223372036854775808"}), json!({"note_id":42}),
        json!({"note_id":"01"}), json!({"note_id":"0"}), json!({"note_id":"1","limit":1}),
        json!({"note_id":"1","tag":"x"}), json!({"query":"x","limit":51}),
        json!({"query":"x","cursor":null}), json!({"query":"x","account_id":"b"})] {
        assert!(parse("search_notes", args).is_err());
    }
    assert!(parse("search_notes", json!({"query":"literal:*", "tag":"x", "limit":50})).is_ok());
    assert!(parse("edit_note", json!({"note_id":"1","revision":"generated"})).is_err());
    assert!(parse("edit_note", json!({"note_id":"1","revision":"generated","fields":{}})).is_ok());
    assert!(parse("list_decks_and_note_types", json!({"kind":"anything"})).is_err());
}

#[test]
fn duplicate_structure_and_serialization_limits_are_bounded() {
    assert!(json::parse(br#"{"a":{"x":1,"x":2}}"#).is_err());
    assert!(json::parse(format!("{}0{}", "[".repeat(18), "]".repeat(18)).as_bytes()).is_err());
    assert!(json::parse(&serde_json::to_vec(&vec![0; 4097]).unwrap()).is_err());
    assert_eq!(json::encode(&"abc", 5).unwrap(), br#""abc""#);
    assert!(json::encode(&"abcd", 5).is_err());
    let busy = ToolFailure::Busy.value();
    assert_eq!(busy["error"]["code"], "busy");
    assert_eq!(busy["error"]["write_committed"], false);
    assert_eq!(busy["error"]["retryable"], true);
}

#[tokio::test]
async fn modern_headers_are_required_exact_and_name_sentinel_is_decoded() {
    let r = Running::start().await;
    let modern = |method: &str, mut params: Value| {
        params["_meta"] = modern_meta();
        body(method, params)
    };
    let call = modern("tools/call", json!({"name":"search_notes",
        "arguments":{"note_id":"9007199254740993"}}));
    for (method, name) in [(None, None), (Some("tools/list"), Some("search_notes")),
        (Some("tools/call"), None), (Some("tools/call"), Some("Search_notes")),
        (Some("tools/call"), Some("=?base64?invalid?="))] {
        let mut request = r.request(SUPPORTED_REVISIONS[0], "generated-fixture-a");
        if let Some(method) = method { request = request.header("Mcp-Method", method); }
        if let Some(name) = name { request = request.header("Mcp-Name", name); }
        let response = request.json(&call).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["id"], 7);
        assert_eq!(error["error"]["code"], -32020);
    }
    assert!(r.fixture.calls.lock().unwrap().is_empty());
    let response = r.request(SUPPORTED_REVISIONS[0], "generated-fixture-a")
        .header("mCp-MeThOd", "tools/call")
        .header("mCp-NaMe", "=?base64?c2VhcmNoX25vdGVz?=")
        .json(&call).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["result"]["resultType"], "complete");
    same_content(&result);
    r.stop().await;
}

#[tokio::test]
async fn stateless_requests_preserve_ids_reject_batches_and_unknown_notifications() {
    let r = Running::start().await;
    let mut ping = body("ping", json!({"_meta":modern_meta()}));
    ping["id"] = json!("generated-opaque-id");
    // No discovery or session is necessary for a modern request.
    let response = r.request(SUPPORTED_REVISIONS[0], "generated-fixture-a")
        .header("Mcp-Method", "ping").json(&ping).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap()["id"], "generated-opaque-id");
    let response = r.request("unsupported", "generated-fixture-a")
        .json(&body("ping", json!({}))).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value = response.json().await.unwrap();
    assert_eq!(error["error"]["code"], -32022);
    assert_eq!(error["error"]["data"], json!({"supported":SUPPORTED_REVISIONS,"requested":"unsupported","reason":"unsupported_version"}));
    let response = r.request(SUPPORTED_REVISIONS[1], "generated-fixture-a")
        .json(&json!([body("ping", json!({}))])).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], -32600);
    for revision in SUPPORTED_REVISIONS {
        let mut notification = body("notifications/unknown", json!({}));
        notification.as_object_mut().unwrap().remove("id");
        let mut request = r.request(revision, "generated-fixture-a");
        if revision == SUPPORTED_REVISIONS[0] {
            notification["params"]["_meta"] = modern_meta();
            request = request.header("Mcp-Method", "notifications/unknown");
        }
        let response = request.json(&notification).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], -32601);
    }
    r.stop().await;
}

#[tokio::test]
async fn legacy_initialize_negotiates_only_its_proposal_and_rejects_old_calls() {
    let r=Running::start().await;
    let initialize=|proposal:&str| body("initialize",json!({"protocolVersion":proposal,
        "capabilities":{},"clientInfo":{"name":"Generated negotiation","version":"1"}}));
    for proposal in ["2024-11-05","2025-11-25","2099-01-01"] {
        for header in [None,Some(proposal)] {
            let mut request=reqwest::Client::new().post(&r.url).bearer_auth("generated-fixture-a");
            if let Some(header)=header { request=request.header("mcp-protocol-version",header); }
            let response=request.json(&initialize(proposal)).send().await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            assert_eq!(response.json::<Value>().await.unwrap()["result"]["protocolVersion"],SUPPORTED_REVISIONS[1]);
        }
    }
    for (proposal,header) in [("2024-11-05","2025-11-25"),("2025-11-25","2024-11-05"),("2024-11-05","2024-10-07")] {
        let response=r.request(header,"generated-fixture-a").json(&initialize(proposal)).send().await.unwrap();
        assert_eq!(response.status(),StatusCode::BAD_REQUEST);
        assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"],-32020);
    }
    for method in ["ping","tools/list","notifications/initialized"] {
        let mut message=body(method,json!({}));
        if method=="notifications/initialized" { message.as_object_mut().unwrap().remove("id"); }
        let response=r.request("2024-11-05","generated-fixture-a").json(&message).send().await.unwrap();
        assert_eq!(response.status(),StatusCode::BAD_REQUEST);
        let error=response.json::<Value>().await.unwrap();
        assert_eq!(error["error"]["code"],-32022);
        assert_eq!(error["error"]["data"],json!({"supported":SUPPORTED_REVISIONS,"requested":"2024-11-05","reason":"unsupported_version"}));
    }
    // After a lower proposal, both currently supported lanes remain usable.
    for revision in SUPPORTED_REVISIONS {
        let result=rpc(&r,revision,"generated-fixture-a","tools/list",json!({})).await;
        assert_eq!(result["result"]["tools"].as_array().unwrap().len(),6);
    }
    r.stop().await;
}

#[tokio::test]
async fn actionable_validation_preserves_http_rpc_codes_and_id_without_dispatch() {
    let r = Running::start().await;
    for revision in SUPPORTED_REVISIONS {
        for (name, arguments, reason, message) in [
            ("search_notes", json!({"limit":20}), "missing_filter",
                "Provide query, deck_id or tag to search notes, or provide note_id alone for an exact lookup."),
            ("search_notes", json!({"note_id":"1","query":"generated-private-filter","cursor":"generated-private-cursor"}),
                "exact_id_combination", ValidationError::ExactIdCombination.message()),
            ("search_notes", json!({"query":"generated-private-filter","limit":51}),
                "invalid_pagination", ValidationError::Page.message()),
            ("search_notes", json!({"note_id":123}), "invalid_arguments", ValidationError::Arguments.message()),
            ("list_decks_and_note_types", json!({"kind":"generated-private-kind"}),
                "invalid_catalog_kind", ValidationError::CatalogKind.message()),
            ("create_note", json!({"deck_id":"0","note_type_id":"1","fields":{}}),
                "invalid_create", ValidationError::Create.message()),
            ("create_note", json!({"deck_id":"1","note_type_id":"1","fields":[]}),
                "invalid_arguments", ValidationError::Arguments.message()),
            ("edit_note", json!({"note_id":"1","revision":"generated-private-revision"}),
                "missing_change", ValidationError::NoChange.message()),
            ("edit_note", json!({"note_id":"0","revision":"generated-private-revision","fields":{}}),
                "invalid_edit", ValidationError::Edit.message()),
        ] {
            let response = rpc(&r, revision, "generated-fixture-a", "tools/call", json!({"name":name,"arguments":arguments})).await;
            assert_eq!(response["id"], 7);
            assert_eq!(response["error"]["code"], -32602);
            assert_eq!(response["error"]["data"]["reason"], reason);
            assert_eq!(response["error"]["message"], message);
            assert!(response.get("result").is_none());
            assert!(!response.to_string().contains("generated-private"));
        }
    }
    let response = r.request(SUPPORTED_REVISIONS[1], "generated-fixture-a")
        .header("content-type", "application/json").body("{").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error = response.json::<Value>().await.unwrap();
    assert_eq!(error["error"]["code"], -32700);
    assert_eq!(error["error"]["data"]["reason"], "invalid_json");
    assert!(r.fixture.calls.lock().unwrap().is_empty());
    r.stop().await;
}

#[test]
fn tool_error_messages_are_mirrored_and_busy_alone_promises_retry_without_a_write() {
    for (failure, code) in [
        (ToolFailure::NotFound, "not_found"), (ToolFailure::Conflict, "conflict"),
        (ToolFailure::InvalidInput, "invalid_input"), (ToolFailure::ResultTooLarge, "result_too_large"),
        (ToolFailure::UnsupportedCollection, "unsupported_collection"), (ToolFailure::Busy, "busy"),
        (ToolFailure::Internal, "internal_error"),
    ] {
        let value = json!({"result":tool_result(failure.value(), true)});
        let error = &value["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], code);
        assert!(!error["message"].as_str().unwrap().is_empty());
        assert_eq!(value["result"]["isError"], true);
        same_content(&value);
        if failure == ToolFailure::Busy {
            assert_eq!(error["retryable"], true);
            assert_eq!(error["retry_after_ms"], 1000);
            assert_eq!(error["write_committed"], false);
        } else {
            for key in ["retryable", "retry_after_ms", "write_committed"] { assert!(error.get(key).is_none()); }
        }
    }
    assert_ne!(ToolFailure::Internal.value(), ToolFailure::UnsupportedCollection.value());
}

#[test]
fn typed_terminal_diagnostics_distinguish_http_success_errors_and_challenges_at_all_levels() {
    use crate::config::LogLevel;
    let invalid = body("tools/call", json!({"name":"search_notes","arguments":{"limit":20}}));
    let bytes = serde_json::to_vec(&invalid).unwrap();
    let response = wire::validate(&axum::http::HeaderMap::new(), invalid, &bytes).err().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.extensions().get::<Diagnostic>().unwrap().outcome,
        Outcome::InvalidParameters(ValidationError::MissingFilter));
    for (response, expected) in [
        (authorization_response(StatusCode::OK.into_response(), AuthorizationDecision::StepUp), Outcome::StepUp),
        (authorization_response(StatusCode::UNAUTHORIZED.into_response(), AuthorizationDecision::Challenge), Outcome::Challenge),
        (authorization_response(StatusCode::SERVICE_UNAVAILABLE.into_response(), AuthorizationDecision::Unavailable), Outcome::Internal),
        (authorization_outcome(StatusCode::OK.into_response(), Stage::Authorization, DiagnosticOperation::Edit), Outcome::StepUp),
        (authorization_outcome(StatusCode::FORBIDDEN.into_response(), Stage::Dispatch, DiagnosticOperation::Edit), Outcome::Challenge),
        (internal_error(Value::Null), Outcome::Internal),
    ] { assert_eq!(response.extensions().get::<Diagnostic>().unwrap().outcome, expected); }
    for (outcome, label) in [
        (Outcome::Success, "success"),
        (Outcome::InvalidParameters(ValidationError::MissingFilter), "invalid_parameters"),
        (Outcome::ToolError(ToolFailure::Conflict), "tool_error"),
        (Outcome::StepUp, "step_up"), (Outcome::Challenge, "challenge"),
        (Outcome::Internal, "failure"),
    ] {
        let diagnostic = Diagnostic { stage:Stage::Dispatch, operation:DiagnosticOperation::Edit, outcome, detail:None };
        assert!(diagnostic.record(LogLevel::Off, StatusCode::OK, 17).is_none());
        assert_eq!(diagnostic.record(LogLevel::Info, StatusCode::OK, 17).is_none(), outcome == Outcome::Success);
        let debug = diagnostic.record(LogLevel::Debug, StatusCode::OK, 17).unwrap();
        assert!(debug.contains(&format!("outcome={label}")));
        assert!(debug.contains("operation=edit_note status=200"));
        assert!(debug.ends_with("elapsed_ms=17"));
        if let Some(info) = diagnostic.record(LogLevel::Info, StatusCode::OK, 17) { assert!(!info.contains("elapsed_ms")); }
    }
    for request in [
        json!({"method":"generated-private-method","id":"generated-private-id","params":{"name":"generated-private-tool"}}),
        json!({"method":"tools/call","id":"generated-private-id","params":{"name":"generated-private-tool","arguments":{"tag":"generated-private-tag"}}}),
    ] {
        let diagnostic = Diagnostic { stage:Stage::Protocol, operation:DiagnosticOperation::classify(&request),
            outcome:Outcome::InvalidParameters(ValidationError::Arguments), detail:None };
        let record = diagnostic.record(LogLevel::Debug, StatusCode::OK, 0).unwrap();
        assert!(record.contains("operation=unknown"));
        assert!(!record.contains("generated-private"));
    }
    for reason in [TransportFailure::Origin, TransportFailure::Method, TransportFailure::ContentType,
        TransportFailure::Shutdown, TransportFailure::Busy, TransportFailure::BodyLimit,
        TransportFailure::BodyRead, TransportFailure::BodyTimeout] {
        let response = transport_error(StatusCode::SERVICE_UNAVAILABLE, reason);
        assert_eq!(response.extensions().get::<Diagnostic>().unwrap().outcome, Outcome::Transport(reason));
    }
}

#[tokio::test]
async fn prepared_dispatch_carries_success_and_tool_failure_without_consuming_response_ownership() {
    const CHILD: &str = "MEGURI_MCP_DIAGNOSTIC_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "mcp_transport::tests::prepared_dispatch_carries_success_and_tool_failure_without_consuming_response_ownership"])
            .env(CHILD, "1").output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let stderr = String::from_utf8(output.stderr).unwrap();
        let records: Vec<_> = stderr.lines().filter(|line| line.starts_with("mcp_response_ready ")).collect();
        assert_eq!(records.len(), 3, "{stderr}");
        for (record, operation, outcome) in [
            (records[0], "search_notes", "success reason=none"),
            (records[1], "edit_note", "tool_error reason=conflict"),
            (records[2], "create_note", "success reason=none"),
        ] {
            assert!(record.contains(&format!("stage=dispatch operation={operation} status=200 outcome={outcome}")));
            assert!(record.rsplit_once("elapsed_ms=").unwrap().1.parse::<u128>().unwrap() >= 17);
        }
        assert!(!stderr.contains("generated-"));
        return;
    }
    let r = Running::start().await;
    let state = Arc::new(State {
        origin:"https://mcp-fixture.invalid".into(), log_level:crate::config::LogLevel::Debug,
        authorizer:r.fixture.clone(), backend:r.fixture.clone(), admission:r.control.0.clone(),
    });
    let diagnostic_request = |name: &str, arguments: Value, lease: Arc<Lease>| {
        let value = body("tools/call", json!({"name":name,"arguments":arguments}));
        let operation = DiagnosticOperation::classify(&value);
        let bytes = serde_json::to_vec(&value).unwrap();
        let dispatch = wire::validate(&axum::http::HeaderMap::new(), value, &bytes).ok().unwrap();
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(RequestAuthority { authority:FixtureAuthority("a"), dispatch, _lease:lease });
        request.extensions_mut().insert(DiagnosticContext {
            started:std::time::Instant::now() - Duration::from_millis(17), operation,
        });
        request
    };
    for (name, arguments, expected) in [
        ("search_notes", json!({"note_id":"9007199254740993"}), Outcome::Success),
        ("edit_note", json!({"note_id":"1","revision":"generated-stale","fields":{}}), Outcome::ToolError(ToolFailure::Conflict)),
    ] {
        let lease = Arc::new(Lease { _permit:r.control.0.slots.clone().try_acquire_owned().unwrap() });
        let response = dispatch(AxumState(state.clone()), diagnostic_request(name, arguments, lease.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.extensions().get::<Diagnostic>().unwrap().outcome, expected);
        let retained = retain(response, Body::empty(), Some(lease), state.clone());
        assert_eq!(r.control.0.slots.available_permits(), CAPACITY - 1);
        assert_eq!(retained.extensions().get::<Diagnostic>().unwrap().outcome, expected);
        assert!(retained.extensions().get::<DiagnosticEmitted>().is_some());
        // The outside middleware sees the same marker after synchronous retention.
        emit_diagnostic(state.log_level, std::time::Instant::now(), &retained);
        drop(retained);
        assert_eq!(r.control.0.slots.available_permits(), CAPACITY);
    }
    let lease = Arc::new(Lease { _permit:r.control.0.slots.clone().try_acquire_owned().unwrap() });
    let request = diagnostic_request("create_note", json!({"deck_id":"1", "note_type_id":"1",
        "fields":{"Front":"generated"}}), lease);
    let mut caller = Box::pin(dispatch(AxumState(state.clone()), request));
    // On this current-thread runtime, poll only far enough to spawn the worker.
    poll_fn(|cx| {
        assert!(std::future::Future::poll(caller.as_mut(), cx).is_pending());
        Poll::Ready(())
    }).await;
    assert_eq!(r.fixture.calls.lock().unwrap().len(), 2);
    drop(caller);
    tokio::time::timeout(Duration::from_secs(5), r.control.idle()).await.unwrap();
    assert_eq!(r.fixture.calls.lock().unwrap().last(), Some(&("a", "create_note")));
    drop(state);
    r.stop().await;
}

#[tokio::test]
async fn transport_rejections_keep_busy_retry_and_body_boundary() {
    let r = Running::start().await;
    let permits = r.control.0.slots.clone().acquire_many_owned(CAPACITY as u32).await.unwrap();
    let response = r.message(SUPPORTED_REVISIONS[1], "generated-fixture-a", "ping", json!({})).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "1");
    assert_eq!(response.text().await.unwrap(), TransportFailure::Busy.message());
    drop(permits);
    let response = r.request(SUPPORTED_REVISIONS[1], "generated-fixture-a")
        .header("content-type", "text/plain").body("generated-private-body").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(response.text().await.unwrap(), TransportFailure::ContentType.message());
    let mut at_limit = Body::from(vec![b' '; MAX_REQUEST_BYTES]);
    assert_eq!(read(&mut at_limit).await.unwrap().len(), MAX_REQUEST_BYTES);
    let mut over_limit = Body::from(vec![b' '; MAX_REQUEST_BYTES + 1]);
    assert_eq!(read(&mut over_limit).await.unwrap_err(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(r.fixture.calls.lock().unwrap().is_empty());
    r.stop().await;
}

#[tokio::test]
async fn diagnosed_failures_keep_access_specific_envelopes_and_fixed_terminal_detail() {
    use crate::error::McpDiagnosticDetail as Detail;
    struct FailureBackend(ToolFailure,Detail);
    impl Backend<FixtureAuthority> for FailureBackend {
        fn dispatch(&self,_:FixtureAuthority,_:ToolCall) -> BoxFuture<'_,Result<ToolOutput,DispatchFailure>> {
            Box::pin(async move {Err(DispatchFailure::Diagnosed {failure:self.0,detail:self.1})})
        }
    }
    let r=Running::start().await;
    for (name,args,access) in [
        ("search_notes",json!({"note_id":"1"}),Access::Read),
        ("list_decks_and_note_types",json!({}),Access::Read),
        ("edit_note",json!({"note_id":"1","revision":"generated-private","fields":{}}),Access::Write),
        ("create_note",json!({"deck_id":"1","note_type_id":"1","fields":{}}),Access::Write),
    ] {
        for (failure,detail,label) in [
            (ToolFailure::Busy,Detail::PublicationGate,"publication_gate"),
            (ToolFailure::Busy,Detail::NativeWriter,"native_writer"),
            (ToolFailure::Busy,Detail::SqliteOpenBusy,"sqlite_open_busy"),
            (ToolFailure::Busy,Detail::SqliteAccessBusy,"sqlite_access_busy"),
            (ToolFailure::Internal,Detail::CollectionPath,"collection_path"),
            (ToolFailure::Internal,Detail::CollectionPin,"collection_pin"),
            (ToolFailure::Internal,Detail::CollectionStorage,"collection_storage"),
            (ToolFailure::Internal,Detail::CollectionWorker,"collection_worker"),
            (ToolFailure::Internal,Detail::AuthorityStorage,"authority_storage"),
            (ToolFailure::Internal,Detail::CollectionOpen,"collection_open"),
            (ToolFailure::Internal,Detail::CollectionAccess,"collection_access"),
            (ToolFailure::UnsupportedCollection,Detail::SqliteOpen,"sqlite_open"),
            (ToolFailure::UnsupportedCollection,Detail::SqliteAccess,"sqlite_access"),
        ] {
            let state=Arc::new(State {
                origin:"https://mcp-fixture.invalid".into(),log_level:crate::config::LogLevel::Off,
                authorizer:r.fixture.clone(),backend:Arc::new(FailureBackend(failure,detail)),admission:r.control.0.clone(),
            });
            let value=body("tools/call",json!({"name":name,"arguments":args}));
            let bytes=serde_json::to_vec(&value).unwrap();
            let operation=DiagnosticOperation::classify(&value);
            let validated=wire::validate(&axum::http::HeaderMap::new(),value,&bytes).ok().unwrap();
            let lease=Arc::new(Lease {_permit:r.control.0.slots.clone().try_acquire_owned().unwrap()});
            let mut request=Request::new(Body::empty());
            request.extensions_mut().insert(RequestAuthority {authority:FixtureAuthority("a"),dispatch:validated,_lease:lease});
            request.extensions_mut().insert(DiagnosticContext {started:std::time::Instant::now(),operation});
            let response=dispatch(AxumState(state),request).await;
            assert_eq!(response.status(),StatusCode::OK);
            let diagnostic=*response.extensions().get::<Diagnostic>().unwrap();
            assert_eq!(diagnostic.operation,operation);
            assert!(response.extensions().get::<DiagnosticEmitted>().is_some());
            assert_eq!(diagnostic.detail,Some(detail));
            assert_eq!(diagnostic.outcome,Outcome::ToolError(failure));
            assert!(diagnostic.record(crate::config::LogLevel::Off,StatusCode::OK,0).is_none());
            let record=diagnostic.record(crate::config::LogLevel::Info,StatusCode::OK,0).unwrap();
            assert!(record.ends_with(&format!(" detail={label}")));
            assert!(record.contains(&format!("status=200 outcome=tool_error reason={}",failure.code())));
            assert!(!record.contains("generated-private"));
            let bytes=axum::body::to_bytes(response.into_body(),MAX_RESPONSE_BYTES).await.unwrap();
            let value:Value=serde_json::from_slice(&bytes).unwrap();
            same_content(&value);
            let error=&value["result"]["structuredContent"]["error"];
            let expected=if failure==ToolFailure::Busy && access==Access::Read {
                json!({"error":{"code":"busy","message":"The collection is busy. Try again after one second.",
                    "retryable":true,"retry_after_ms":1000}})
            } else {failure.value()};
            assert_eq!(value["result"]["structuredContent"],expected);
            assert_eq!(value["result"]["isError"],true);
            assert!(error.get("detail").is_none());
            if access==Access::Read { assert!(error.get("write_committed").is_none()); }
            if failure==ToolFailure::Busy && access==Access::Write {
                assert_eq!(error,&json!({"code":"busy",
                    "message":"The collection is busy. The write did not commit. Retry after one second.",
                    "retryable":true,"retry_after_ms":1000,"write_committed":false}));
            }
        }
    }
    r.stop().await;
}

#[tokio::test]
async fn download_discovery_read_scope_and_empty_input_for_both_revisions() {
    let r = Running::start().await;
    for revision in SUPPORTED_REVISIONS {
        let listed = rpc(&r, revision, "generated-fixture-read", "tools/list", json!({})).await;
        let tool = listed["result"]["tools"].as_array().unwrap().iter().find(|t|t["name"] == "download_collection").unwrap();
        assert_eq!(tool["securitySchemes"][0]["scopes"], json!(["collection:read"]));
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert!(tool["description"].as_str().unwrap().contains("Share download_url"));
        for arguments in [json!({"account_id":"other"}), json!({"ticket":null}), json!([])] {
            let result = rpc(&r, revision, "generated-fixture-read", "tools/call", json!({"name":"download_collection","arguments":arguments})).await;
            assert!(result.get("error").is_some());
        }
        let result = rpc(&r, revision, "generated-fixture-read", "tools/call", json!({"name":"download_collection","arguments":{}})).await;
        assert_eq!(result["result"]["isError"], false);
        let link = result["result"]["structuredContent"]["download_url"].as_str().unwrap();
        assert!(result["result"]["content"][0]["text"].as_str().unwrap().contains(link));
    }
    assert_eq!(*r.fixture.calls.lock().unwrap(), vec![("a","download_collection"),("a","download_collection")]);
    r.stop().await;
}
