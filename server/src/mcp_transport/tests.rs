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
        assert_eq!(tools.len(), 4);
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
    assert_eq!(error["error"]["data"], json!({"supported":SUPPORTED_REVISIONS,"requested":"unsupported"}));
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
        assert_eq!(error["error"]["data"],json!({"supported":SUPPORTED_REVISIONS,"requested":"2024-11-05"}));
    }
    // After a lower proposal, both currently supported lanes remain usable.
    for revision in SUPPORTED_REVISIONS {
        let result=rpc(&r,revision,"generated-fixture-a","tools/list",json!({})).await;
        assert_eq!(result["result"]["tools"].as_array().unwrap().len(),4);
    }
    r.stop().await;
}
