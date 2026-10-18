//! N4, N6, N7: one raw representation and bounded, exact churn diagnostics.
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_testkit::dump::preservation::{assert_changes_allowed, differences, pending, raw_dump};
use serde_json::json;

#[test]
fn dump_differences_stop_at_columns_and_escape_row_keys_without_numeric_tolerance() {
    let before = json!({"config": {"a/b~c": {"val": {"sha256": "abc"}, "usn": 1}}, "notes": {"1": {"guid": "g"}}});
    let after = json!({"config": {"a/b~c": {"val": {"sha256": "def"}, "usn": 1.0}}, "notes": {"2": {"guid": "g"}}});
    let diff = differences(&before, &after);
    assert_eq!(
        diff.iter().map(|d| d.pointer.as_str()).collect::<Vec<_>>(),
        ["/config/a~1b~0c/usn", "/config/a~1b~0c/val", "/notes/1", "/notes/2"]
    );
    assert_eq!(diff[1].expected, Some(json!({"sha256": "abc"})));
    assert!(assert_changes_allowed(&before, &after, &["/config/*/*", "/notes/*"]).is_ok());
    let error = assert_changes_allowed(&before, &after, &["/config/*/val", "/notes/*"]).unwrap_err();
    assert!(error.contains("/config/a~1b~0c/usn") && error.contains("1.0"));
    assert!(
        assert_changes_allowed(&before, &after, &["/config/*", "/notes/*"]).is_err(),
        "a wildcard covers one segment only"
    );
}

#[test]
fn change_diagnostics_show_twenty_changes_and_the_total() {
    let before = json!({"notes": {}});
    let rows = (0..23)
        .map(|i| (i.to_string(), json!({"guid": "g"})))
        .collect::<serde_json::Map<_, _>>();
    let after = json!({"notes": rows});
    let error = assert_changes_allowed(&before, &after, &[]).unwrap_err();
    assert_eq!(error.lines().filter(|s| s.starts_with('/')).count(), 20);
    assert!(error.contains("23"));
}

#[test]
fn raw_dump_reads_all_twelve_tables_and_keeps_every_blob_byte() {
    let mut tables = Vec::new();
    let dumped = raw_dump(|sql| {
        let table = sql.strip_prefix("SELECT * FROM ").unwrap();
        tables.push(table.to_owned());
        Ok(if table == "notes" {
            RawRows {
                columns: vec!["id".into(), "guid".into(), "data".into()],
                rows: vec![vec![
                    SqlValue::Integer(7),
                    SqlValue::Text("stored-guid".into()),
                    SqlValue::Blob(vec![0, 255]),
                ]],
            }
        } else {
            let keys = match table {
                "fields" | "templates" => vec!["ntid", "ord"],
                "config" => vec!["KEY"],
                "graves" => vec!["oid", "type"],
                "tags" => vec!["tag"],
                _ => vec!["id"],
            };
            RawRows {
                columns: keys.into_iter().map(str::to_owned).collect(),
                rows: vec![],
            }
        })
    })
    .unwrap();
    assert_eq!(tables.len(), 12);
    assert_eq!(dumped["notes"]["7"]["guid"], "stored-guid");
    assert_eq!(
        dumped["notes"]["7"]["data"],
        json!({"sha256": "06eb7d6a69ee19e5fbdf749018d3d2abfa04bcbd1365db312eb86dc7169389b8"})
    );
}

#[test]
fn pending_reads_all_twelve_counters_and_refuses_wrong_shapes() {
    let rows = |values| RawRows {
        columns: vec![],
        rows: vec![values],
    };
    let counts = pending(|sql| {
        assert!(sql.contains("SELECT scm > ls FROM col"));
        Ok(rows(vec![SqlValue::Integer(0); 12]))
    })
    .unwrap();
    assert_eq!(counts, [0; 12]);
    let mut values = vec![SqlValue::Integer(0); 12];
    values[11] = SqlValue::Integer(1);
    assert_eq!(pending(|_| Ok(rows(values.clone()))).unwrap()[11], 1);
    assert!(pending(|_| Ok(rows(vec![SqlValue::Integer(0); 11]))).is_err());
    assert!(pending(|_| Ok(rows(vec![SqlValue::Real(0.0); 12]))).is_err());
}
