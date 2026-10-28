//! Batch3/4 recognition contracts; no execution support is supplied by these tests.
use meguri_testkit::{
    exec::{Executor, MeguriExecutor},
    observe,
    scenario::*,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

fn parse(steps: Value) -> Result<Scenario, FormatError> {
    Scenario::from_value(
        &json!({"id":"prepared/contracts", "clock":{"tz":"UTC","start":"2030-01-15T10:00:00"}, "steps":steps}),
    )
}
fn request(value: Value) -> Result<Scenario, FormatError> {
    parse(json!([{"observe":[value]}]))
}
fn schema() -> Value {
    // Schema bounds are metadata, outside authored scenarios' D2 restriction.
    observe::parse_json(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/schema/scenario.schema.json"),
        )
        .unwrap(),
    )
    .unwrap()
}
// Bounded helper for these fixtures, not a general schema validator. Reject new
// keywords rather than silently claim agreement with the checked-in schemas.
fn conforms(root: &Value, def: &Value, value: &Value) -> bool {
    if let Some(b) = def.as_bool() {
        return b;
    }
    for keyword in def.as_object().expect("schema object or boolean").keys() {
        match keyword.as_str() {
            "description"
            | "$ref"
            | "allOf"
            | "anyOf"
            | "oneOf"
            | "not"
            | "if"
            | "then"
            | "else"
            | "const"
            | "enum"
            | "type"
            | "minimum"
            | "maximum"
            | "minLength"
            | "pattern"
            | "minItems"
            | "maxItems"
            | "uniqueItems"
            | "items"
            | "required"
            | "minProperties"
            | "maxProperties"
            | "propertyNames"
            | "properties"
            | "additionalProperties" => {}
            _ => panic!("unsupported schema keyword: {keyword}"),
        }
    }
    if let Some(r) = def["$ref"].as_str()
        && !conforms(root, root.pointer(r.strip_prefix('#').unwrap()).unwrap(), value)
    {
        return false;
    }
    for (key, minimum, maximum) in [
        ("allOf", usize::MAX, usize::MAX),
        ("anyOf", 1, usize::MAX),
        ("oneOf", 1, 1),
    ] {
        if let Some(variants) = def[key].as_array() {
            let n = variants.iter().filter(|v| conforms(root, v, value)).count();
            if key == "allOf" {
                if n != variants.len() {
                    return false;
                }
            } else if !(minimum..=maximum).contains(&n) {
                return false;
            }
        }
    }
    if def.get("not").is_some_and(|d| conforms(root, d, value)) {
        return false;
    }
    if let Some(condition) = def.get("if") {
        let branch = if conforms(root, condition, value) {
            "then"
        } else {
            "else"
        };
        if def.get(branch).is_some_and(|d| !conforms(root, d, value)) {
            return false;
        }
    }
    if let Some(v) = def.get("const")
        && value != v
    {
        return false;
    }
    if let Some(v) = def["enum"].as_array()
        && !v.contains(value)
    {
        return false;
    }
    if let Some(t) = def.get("type") {
        let is = |t: &str| match t {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => panic!("unknown schema type"),
        };
        if !t
            .as_str()
            .map(is)
            .unwrap_or_else(|| t.as_array().unwrap().iter().any(|t| is(t.as_str().unwrap())))
        {
            return false;
        }
    }
    if let Some(n) = value.as_f64()
        && (def["minimum"].as_f64().is_some_and(|lo| n < lo) || def["maximum"].as_f64().is_some_and(|hi| n > hi))
    {
        return false;
    }
    if let Some(s) = value.as_str() {
        if def["minLength"]
            .as_u64()
            .is_some_and(|n| s.chars().count() < (n as usize))
        {
            return false;
        }
        if let Some(pattern) = def["pattern"].as_str()
            && !regex::Regex::new(pattern).unwrap().is_match(s)
        {
            return false;
        }
    }
    if let Some(items) = value.as_array() {
        if def["minItems"].as_u64().is_some_and(|n| items.len() < (n as usize))
            || def["maxItems"].as_u64().is_some_and(|n| items.len() > (n as usize))
        {
            return false;
        }
        if def["uniqueItems"] == true && items.iter().enumerate().any(|(i, v)| items[..i].contains(v)) {
            return false;
        }
        if let Some(item) = def.get("items")
            && !items.iter().all(|v| conforms(root, item, v))
        {
            return false;
        }
    }
    if let Some(map) = value.as_object() {
        if def["required"]
            .as_array()
            .is_some_and(|req| req.iter().any(|r| !map.contains_key(r.as_str().unwrap())))
        {
            return false;
        }
        if def["minProperties"].as_u64().is_some_and(|n| map.len() < (n as usize))
            || def["maxProperties"].as_u64().is_some_and(|n| map.len() > (n as usize))
        {
            return false;
        }
        for (k, v) in map {
            if let Some(names) = def.get("propertyNames")
                && !conforms(root, names, &json!(k))
            {
                return false;
            }
            if let Some(d) = def["properties"].get(k) {
                if !conforms(root, d, v) {
                    return false;
                }
            } else if let Some(extra) = def.get("additionalProperties")
                && !conforms(root, extra, v)
            {
                return false;
            }
        }
    }
    true
}
fn request_schema<'a>(schema: &'a Value, kind: &str) -> &'a Value {
    schema["$defs"]["observation_request"]["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|branch| branch["properties"].get(kind))
        .unwrap()
}
fn check_request(kind: &str, args: Value, expected: bool) {
    let root = schema();
    assert_eq!(
        conforms(&root, request_schema(&root, kind), &args),
        expected,
        "schema {kind}: {args}"
    );
    assert_eq!(
        request(json!({kind:args.clone()})).is_ok(),
        expected,
        "model {kind}: {args}"
    );
}
fn deck_export() -> Value {
    json!({"format":"apkg","name":"cards.apkg","limit":"collection","scheduling":false,"presets":false,"media":false})
}

#[test]
#[should_panic(expected = "unsupported schema keyword: multipleOf")]
fn schema_helper_rejects_unsupported_assertions_even_beside_a_ref() {
    let root = json!({"type":"integer"});
    conforms(&root, &json!({"$ref":"#","multipleOf":2}), &json!(3));
}

#[test]
fn schema_helper_enforces_max_properties_alongside_a_ref() {
    let root = json!({"type":"object"});
    let def = json!({"$ref":"#","maxProperties":1});
    assert!(conforms(&root, &def, &json!({"deck":"A"})));
    assert!(!conforms(&root, &def, &json!({"deck":"A","tag":"B"})));
    check_request("search_build", json!([{"deck":"A","tag":"B"}]), false);
}

#[test]
fn field_io_options_preserve_presence_and_add_only_the_missing_part() {
    let edit =
        |options: Value| parse(json!([{"update_notetype":{"notetype":"Pair","edits":[{"field_options":options}]}}]));
    for (extra, tag, io) in [
        (json!({}), Presence::Missing, false),
        (json!({"tag":null}), Presence::Null, true),
        (json!({"tag":4294967295_u64}), Presence::Value(u32::MAX), true),
        (json!({"prevent_deletion":false}), Presence::Missing, true),
    ] {
        let mut options = json!({"field":"Front","sticky":false});
        options
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let parsed = edit(options).unwrap();
        let Step::UpdateNotetype(args) = &parsed.steps[0] else {
            panic!()
        };
        let NotetypeEdit::FieldOptions(options) = &args.edits[0] else {
            panic!()
        };
        assert_eq!(options.tag, tag);
        let needs = parsed.needs();
        assert!(needs.ops.contains("update_notetype.field_options"));
        assert_eq!(needs.ops.contains("update_notetype.field_options.io"), io);
        assert_eq!(
            MeguriExecutor::default()
                .missing(&needs)
                .ops
                .contains("update_notetype.field_options.io"),
            io
        );
    }
    let root = schema();
    let def = &root["$defs"]["notetype_edit"]["properties"]["field_options"];
    for key in ["tag", "prevent_deletion"] {
        for value in [
            json!(null),
            json!(false),
            json!(true),
            json!(0),
            json!(4294967295_u64),
            json!(4294967296_u64),
            json!(-1),
            json!("1"),
            json!([]),
        ] {
            let options = json!({"field":"Front",key:value});
            let expected = if key == "tag" {
                value.is_null() || value.as_u64().is_some_and(|v| v <= u64::from(u32::MAX))
            } else {
                value.is_boolean()
            };
            assert_eq!(conforms(&root, def, &options), expected, "schema {options}");
            assert_eq!(edit(options.clone()).is_ok(), expected, "model {options}");
        }
    }
}

#[test]
fn package_export_forms_and_capabilities_do_not_fold_separators_or_false_flags() {
    let root = schema();
    let def = &root["$defs"]["step"]["properties"]["export"];
    let l6 = json!({"format":"apkg","name":"cards.apkg","legacy":true});
    assert!(matches!(
        PackageExportArgs::from_mapping(l6.as_object().unwrap()).unwrap(),
        PackageExportArgs::LegacyWholeCollection { .. }
    ));
    let mut e8 = deck_export();
    e8["legacy"] = json!(true);
    assert!(matches!(
        PackageExportArgs::from_mapping(e8.as_object().unwrap()).unwrap(),
        PackageExportArgs::LegacyDeck(_)
    ));
    for (args, parts) in [
        (l6, vec!["export.apkg_legacy"]),
        (deck_export(), vec!["export.apkg"]),
        (e8.clone(), vec!["export.apkg", "export.apkg.legacy"]),
    ] {
        assert!(conforms(&root, def, &args));
        let parsed = parse(json!([{"export":args}])).unwrap();
        assert_eq!(
            parsed.needs().ops,
            parts
                .into_iter()
                .chain(["export"])
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        );
    }
    for media in [None, Some(false), Some(true)] {
        for legacy in [None, Some(false), Some(true)] {
            let mut args = json!({"format":"colpkg","name":"cards.colpkg"});
            if let Some(v) = media {
                args["media"] = json!(v);
            }
            if let Some(v) = legacy {
                args["legacy"] = json!(v);
            }
            assert!(conforms(&root, def, &args));
            let needs = parse(json!([{"export":args}])).unwrap().needs();
            assert!(needs.ops.contains("export.colpkg"));
            assert_eq!(needs.ops.contains("export.colpkg.media"), media == Some(true));
            assert_eq!(needs.ops.contains("export.colpkg.legacy"), legacy == Some(true));
            let missing = MeguriExecutor::default().missing(&needs);
            assert_eq!(missing.ops.is_empty(), media != Some(true) && legacy != Some(true));
        }
    }
    for key in ["scheduling", "presets", "media", "limit"] {
        let mut args = e8.clone();
        args.as_object_mut().unwrap().remove(key);
        assert!(!conforms(&root, def, &args));
        assert!(parse(json!([{"export":args}])).is_err());
    }
    for (key, value) in [
        ("legacy", json!(false)),
        ("legacy", json!(null)),
        ("extra", json!(true)),
        ("media", json!(null)),
    ] {
        let mut args = e8.clone();
        args[key] = value;
        assert!(!conforms(&root, def, &args));
        assert!(parse(json!([{"export":args}])).is_err());
    }
    for key in ["media", "legacy"] {
        for value in [json!(null), json!(0), json!("true")] {
            let args = json!({"format":"colpkg","name":"x.colpkg",key:value});
            assert!(!conforms(&root, def, &args));
            assert!(parse(json!([{"export":args}])).is_err());
        }
    }
    for limit in [
        json!({"notes":["#c1"]}),
        json!({"cards":["#n1"]}),
        json!({"cards":[]}),
        json!({"deck":"Default","cards":["#c1"]}),
    ] {
        let mut args = deck_export();
        args["limit"] = limit;
        assert!(!conforms(&root, def, &args));
        assert!(parse(json!([{"export":args}])).is_err());
    }
}

fn new_observations() -> Vec<(&'static str, Value)> {
    vec![
        ("add_defaults", json!({"review_card":"#c1"})),
        ("notetype_last_deck", json!({"notetype":"Pair"})),
        (
            "search_build",
            json!(["whole_collection",{"negated":{"group":{"joiner":"or","items":[{"due_on":-2147483648_i64},{"note_ids":[0,9007199254740991_u64]}]}}}]),
        ),
        (
            "search_join",
            json!([{"existing":"x","item":{"tag":"a"},"joiner":"and","negate":false}]),
        ),
        (
            "search_replace",
            json!([{"existing":"x","item":{"text":"a","field_text":true}}]),
        ),
        ("tag_complete", json!({"cases":[{"input":"","limit":4294967296_u64}]})),
        ("card_stats", json!(["#c1", "#c2"])),
    ]
}
#[test]
fn new_observations_and_stats_are_language_only_with_pure_search_d8() {
    let executor = MeguriExecutor::default();
    let caps = executor.capabilities();
    assert_eq!(ObservationKind::ALL.len(), 64);
    assert_eq!(Step::NAMES.len(), 87);
    assert_eq!(ObservationKind::ALL.iter().collect::<BTreeSet<_>>().len(), 64);
    for (kind, args) in new_observations().into_iter().chain([(
        "stats",
        json!([{"scope":"collection","history":"year","parts":["context"]}]),
    )]) {
        check_request(kind, args.clone(), true);
        let value = json!({kind:args});
        let parsed = request(value.clone()).unwrap();
        let before = parsed.needs();
        assert!(executor.missing(&before).observations.contains(kind));
        assert_eq!(before, parsed.needs());
        assert_eq!(caps, executor.capabilities());
        assert_eq!(parsed.starts_from_new_collection(), !kind.starts_with("search_"));
        assert!(
            parse(json!([{"observe":[value.clone(),value]}])).is_err(),
            "duplicate {kind}"
        );
        check_request(kind, Value::Null, false);
        check_request(kind, json!({"unknown":true}), false);
    }
    for (kind, args) in new_observations().into_iter().filter(|(k, _)| k.starts_with("search_")) {
        let mixed = parse(json!([{"observe":[{kind:args},"card"]}])).unwrap();
        assert!(mixed.needs().setup.contains(NEW_COLLECTION));
    }
}

#[test]
fn defaults_and_card_stats_validate_reference_kind_and_resolution_without_reading_a_collection() {
    check_request("add_defaults", json!({}), true);
    for bad in [
        json!({"review_card":null}),
        json!({"review_card":"#n1"}),
        json!({"review_card":"#c0"}),
        json!({"preset_deck":"D"}),
    ] {
        check_request("add_defaults", bad, false);
    }
    for args in [
        json!({"notetype":"Pair"}),
        json!({"notetype":"_*"}),
        json!({"notetype":"e\u{301}"}),
        json!({"notetype":"a\tb"}),
        json!({"notetype_id":-1}),
    ] {
        check_request("notetype_last_deck", args, true);
    }
    for bad in [
        json!({}),
        json!({"notetype":""}),
        json!({"notetype":"Pair","notetype_id":1}),
        json!({"notetype_id":null}),
        json!({"notetype_id":"1"}),
        json!({"notetype_id":true}),
    ] {
        check_request("notetype_last_deck", bad, false);
    }
    for bad in [
        json!([]),
        json!(["#c1", "#c1"]),
        json!(["#n1"]),
        json!(["#c0"]),
        json!({}),
        json!(null),
    ] {
        check_request("card_stats", bad, false);
    }
    assert!(request(json!("card_stats(#c1)")).is_err());
    let defaults = AddDefaultsRequest::from_mapping(json!({"review_card":"#c1"}).as_object().unwrap()).unwrap();
    assert_eq!(
        defaults.resolve::<i64>(|_| None).unwrap_err().to_string(),
        "unknown_ref"
    );
    assert_eq!(defaults.resolve(|_| Some(0)).unwrap(), Some(0)); // deleted but assigned
    let cards = CardStatsRequest::from_list(json!(["#c1", "#c2"]).as_array().unwrap()).unwrap();
    assert_eq!(
        cards
            .resolve(|r| if r.n == 1 { Some(42) } else { None })
            .unwrap_err()
            .to_string(),
        "unknown_ref"
    );
    assert_eq!(cards.resolve(|r| Some(r.n + 10)).unwrap(), vec![11, 12]);
    let by_name = NotetypeLastDeckRequest::Notetype("Pair".into());
    assert_eq!(
        by_name.resolve(|_| None, |_| false).unwrap_err().to_string(),
        "unknown_ref"
    );
    assert_eq!(by_name.resolve(|_| Some(42), |_| false).unwrap(), 42);
    let by_id = NotetypeLastDeckRequest::NotetypeId(42);
    assert_eq!(by_id.resolve(|_| None, |_| false).unwrap(), 42);
    assert!(
        by_id
            .resolve(|_| None, |_| true)
            .unwrap_err()
            .to_string()
            .starts_with("invalid_scenario")
    );
}

#[test]
fn search_items_recurse_and_enforce_canonical_shapes_and_numeric_bounds() {
    for item in [
        json!({"template":65534}),
        json!({"answered":{"days":4294967295_u64,"button":4}}),
        json!({"rescheduled":0}),
        json!({"due_within":2147483647}),
        json!({"dupe":{"notetype_id":1,"text":""}}),
        json!({"field":{"name":"Front","text":""}}),
        json!({"flag":"any"}),
        json!({"flag":7}),
    ] {
        check_request("search_build", json!([item]), true);
    }
    for item in [
        json!({"template":65535}),
        json!({"added":0}),
        json!({"edited":4294967296_u64}),
        json!({"introduced":-1}),
        json!({"answered":{"days":1,"button":0}}),
        json!({"answered":{"days":1,"button":null}}),
        json!({"rescheduled":-1}),
        json!({"due_on":-2147483649_i64}),
        json!({"due_within":2147483648_i64}),
        json!({"note_ids":[]}),
        json!({"note_ids":[9007199254740992_u64]}),
        json!({"group":{"joiner":"and","items":[]}}),
        json!({"text":"x","field_text":null}),
        json!({"tag":"a","deck":"b"}),
        json!({"flag":8}),
        json!({"state":"buried-manually"}),
        json!({"field":{"name":"Front","text":"x","extra":true}}),
        json!({"group":{"joiner":"xor","items":["whole_collection"]}}),
    ] {
        check_request(
            "search_build",
            json!([{"negated":{"group":{"joiner":"and","items":[item]}}}]),
            false,
        );
    }
    for bad in [
        json!({"deck":"current"}),
        json!({"deck":"filtered"}),
        json!({"deck":"a::::b"}),
        json!({"deck":"e\u{301}"}),
        json!({"notetype":"e\u{301}"}),
        json!({"tag":"none"}),
        json!({"tag":"_*"}),
        json!({"field_present":"DECK"}),
        json!({"field":{"name":"Front","text":"a\tb"}}),
        json!({"literal":"a\u{85}b"}),
        json!({"dupe":{"notetype_id":1,"text":"a\u{1c}b"}}),
    ] {
        assert!(request(json!({"search_build":[bad]})).is_err());
    }
    check_request("search_build", json!([]), false);
    for bad in [
        json!([]),
        json!([{"existing":"x","item":"whole_collection","joiner":"and"}]),
        json!([{"existing":"x","item":"whole_collection","joiner":"or","negate":null}]),
        json!([{"existing":"x","item":"whole_collection","joiner":"or","negate":false,"extra":true}]),
    ] {
        check_request("search_join", bad, false);
    }
    check_request(
        "search_replace",
        json!([{"existing":"","item":"whole_collection","negate":false}]),
        false,
    );
}

#[test]
fn tag_complete_limits_include_two_to_the_32_and_reject_null_and_sequence_mappings() {
    for n in [1_u64, 4294967295, 4294967296] {
        check_request("tag_complete", json!({"cases":[{"input":"::","limit":n}]}), true);
    }
    for n in [
        json!(0),
        json!(-1),
        json!(4294967297_u64),
        json!(null),
        json!(true),
        json!("1"),
    ] {
        check_request("tag_complete", json!({"cases":[{"input":"","limit":n}]}), false);
    }
    for bad in [
        json!({"cases":[]}),
        json!({"cases":[["a",1]]}),
        json!({"cases":[{"input":"a","limit":1,"extra":true}]}),
        json!({"cases":[{"input":null,"limit":1}]}),
        json!({"cases":[{"input":"a"}]}),
    ] {
        check_request("tag_complete", bad, false);
    }
}

#[test]
fn stats_requires_nonempty_typed_requests_scopes_histories_and_canonical_parts() {
    let parts = json!([
        "reviews",
        "true_retention",
        "hours",
        "buttons",
        "future_due",
        "card_counts",
        "intervals",
        "stability",
        "difficulty",
        "retrievability",
        "added",
        "context"
    ]);
    for scope in [json!("collection"), json!({"deck":"A::B"}), json!({"search":""})] {
        for history in ["year", "all"] {
            check_request("stats", json!([{"scope":scope,"history":history,"parts":parts}]), true);
        }
    }
    for scope in [
        json!({}),
        json!("deck"),
        json!({"deck":null}),
        json!({"deck":"D","search":"x"}),
        json!({"search":1}),
    ] {
        check_request(
            "stats",
            json!([{"scope":scope,"history":"year","parts":["reviews"]}]),
            false,
        );
    }
    for bad in [
        json!([]),
        json!({}),
        json!(null),
        json!([{"scope":"collection","history":"year","parts":[]}]),
        json!([{"scope":"collection","history":"all","parts":["unknown"]}]),
        json!([{"scope":"collection","history":"month","parts":["reviews"]}]),
        json!([{"scope":"collection","history":"all","parts":["reviews"],"extra":true}]),
        json!([{"scope":"collection","parts":["context"]}]),
        json!([["collection", "all", ["context"]]]),
    ] {
        check_request("stats", bad, false);
    }
    assert!(request(json!("stats")).is_err());
}

#[test]
fn prepared_output_schemas_use_raw_stats_fields_and_restrict_builder_errors() {
    let root = observe::parse_json(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/schema/observation.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let outputs = &root["$defs"]["step_output"]["properties"];
    for kind in ["search_build", "search_join", "search_replace"] {
        assert!(conforms(
            &root,
            &outputs[kind],
            &json!([{"text":"deck:A"},{"error":"unclosed_group"}])
        ));
        for error in ["invalid_regex", "search_failed"] {
            assert!(!conforms(&root, &outputs[kind], &json!([{"error":error}])));
        }
    }
    let raw = json!({"#c1":{
        "card_id":"#c1","note_id":"#n1","deck":"Default","original_deck":null,"card_type":"new","notetype":"Pair","preset":"Default","custom_data":"",
        "added":1,"first_review":null,"latest_review":null,"due_date":null,"due_position":1,
        "interval":0,"ease":0,"reviews":0,"lapses":0,"average_secs":0.0,"total_secs":0.0,"desired_retention":null,"fsrs_retrievability":null,
        "memory_state":{"stability":1.0,"difficulty":2.0},"fsrs_params":[],"revlog":[]
    }});
    assert!(conforms(&root, &outputs["card_stats"], &raw));
    let mut extra = raw.clone();
    extra["#c1"]["display_text"] = json!("not observed");
    assert!(!conforms(&root, &outputs["card_stats"], &extra));
    let mut wrong = raw;
    wrong["#c1"]["card_id"] = json!("#n1");
    assert!(!conforms(&root, &outputs["card_stats"], &wrong));
    assert!(conforms(
        &root,
        &outputs["stats"],
        &json!([{"reviews":{"count":{"0":{"young":1}},"time":{"0":{"young":1000}}},"context":{"fsrs":true}}])
    ));
    assert!(conforms(&root, &outputs["stats"], &json!([{"reviews":{}}])));
    assert!(!conforms(
        &root,
        &outputs["stats"],
        &json!([{"reviews":{"count":{},"time":{}}}])
    ));
    assert!(!conforms(
        &root,
        &outputs["stats"],
        &json!([{"reviews":{"unknown":{}}}])
    ));
    assert!(!conforms(
        &root,
        &outputs["stats"],
        &json!([{"card_counts":{"including_inactive":{"new_cards":1}}}])
    ));
    assert!(!conforms(&root, &outputs["stats"], &json!([{"unobserved_part":{}}])));
}
