//! X2-X8: exercise SQLite, with same-platform f32 expectations independent
//! of the extractor implementation. Corpus/golden coverage lives in testkit.
use rusqlite::{Connection, params, types::Value};

fn connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    meguri_search::sqlfns::register(&conn).unwrap();
    conn
}

fn real(conn: &Connection, function: &str, data: &str, args: [i64; 4]) -> Option<f64> {
    conn.query_row(
        &format!("SELECT {function}(?1,?2,?3,?4,?5)"),
        params![data, args[0], args[1], args[2], args[3]],
        |row| row.get(0),
    )
    .unwrap()
}

fn exact(actual: Option<f64>, expected: f32) {
    if expected.is_nan() {
        assert_eq!(actual, None);
    } else {
        assert_eq!(actual.unwrap().to_bits(), f64::from(expected).to_bits());
    }
}

#[test]
fn sqlite_memory_metrics_match_native_f32_calculations() {
    fn json_number(number: f32) -> String {
        if number.is_infinite() {
            "1e40".to_owned()
        } else {
            number.to_string()
        }
    }

    let database = connection();
    for stability in [0.0f32, -1.0, 0.01, 0.05, 2.5, 5.0, 100.0, f32::INFINITY] {
        for decay_parameter in [0.0f32, -0.5, 0.1542, 0.5, 2.0, 1e-40, f32::INFINITY] {
            for elapsed_seconds in [0u32, 1, 86399, 86400, 82800, 864000, u32::MAX] {
                let stability_text = json_number(stability);
                let decay_text = json_number(decay_parameter);
                let card_json = format!(r#"{{"s":{stability_text},"d":5,"decay":{decay_text},"dr":0.9,"lrt":0}}"#);
                let expected_retention = fsrs::current_retrievability(
                    fsrs::MemoryState {
                        stability,
                        difficulty: 5.0,
                    },
                    elapsed_seconds as f32 / 86400.0,
                    decay_parameter,
                );
                let sql_arguments = [35, 10, 30, i64::from(elapsed_seconds)];
                exact(
                    real(&database, "meguri_retrievability", &card_json, sql_arguments),
                    expected_retention,
                );
                let exponent = -1.0f32 / decay_parameter;
                for desired_retention in [0.0f32, -0.5, 0.9, 0.99999994, 1.0, 1.0000001, f32::INFINITY] {
                    let desired_text = json_number(desired_retention);
                    let card_json =
                        format!(r#"{{"s":{stability_text},"d":5,"decay":{decay_text},"dr":{desired_text},"lrt":0}}"#);
                    let expected_relative = -(expected_retention.max(0.0001).powf(exponent) - 1.0)
                        / (desired_retention.max(0.0001).powf(exponent) - 1.0);
                    exact(
                        real(&database, "meguri_relative_retrievability", &card_json, sql_arguments),
                        expected_relative,
                    );
                }
            }
        }
    }
}

#[test]
fn scalar_fields_are_independent_and_json_rules_are_extractor_specific() {
    let conn = connection();
    for (text, s, d, pos) in [
        (r#"{"s":0.1}"#, Some(0.1f32), None, None),
        (r#"{"d":3,"pos":4294967295}"#, None, Some(3.0), Some(4294967295i64)),
        (r#"{"s":5,"d":5,"cd":null}"#, None, None, None),
        (r#"{"s":5,"d":5,"cd":"\ud800"}"#, None, None, None),
        (r#"{"s":5,"s":6,"pos":7}"#, None, None, None),
        (r#"{"s":5,"pos":-0}"#, Some(5.0), None, None),
        (r#"{"s":5,"zz":"\ud800"}"#, Some(5.0), None, None),
        (r#"{"s":5,"zz":{"a":1e400}}"#, Some(5.0), None, None),
        (r#"{"s":5,"d":1.7976931348623158e308}"#, None, None, None),
        (r#"[7,2,3]"#, Some(2.0), Some(3.0), Some(7)),
        (r#"[7,2,3,4,5,6,null]"#, None, None, None),
        (r#"{"\u0073":16777217}"#, Some(16777216.0), None, None),
    ] {
        let got: (Option<f64>, Option<f64>, Option<i64>) = conn
            .query_row(
                "SELECT meguri_card_f32(?1,'s'),meguri_card_f32(?1,'d'),meguri_original_position(?1)",
                [text],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(got, (s.map(f64::from), d.map(f64::from), pos), "{text}");
    }
    // General CardData keeps its paired-memory and lenient cd behavior.
    assert!(
        meguri_model::card_data::CardData::parse(r#"{"s":5}"#)
            .memory_state
            .is_none()
    );
    assert!(
        meguri_model::card_data::CardData::parse(r#"{"s":5,"d":5,"cd":5}"#)
            .memory_state
            .is_some()
    );
}

#[test]
fn nesting_unknown_values_is_unbounded_but_known_values_stop_at_127() {
    let conn = connection();
    for depth in [126, 127, 130, 20_000] {
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        for key in ["zz", "lrt"] {
            let text = format!(r#"{{"s":5,"d":5,"{key}":{nested}}}"#);
            let s: Option<f64> = conn
                .query_row("SELECT meguri_card_f32(?1,'s')", [&text], |r| r.get(0))
                .unwrap();
            assert_eq!(s, (key == "zz" || depth <= 126).then_some(5.0), "{key}/{depth}");
        }
    }
}

#[test]
fn registration_flags_arities_keys_and_storage_classes_are_strict() {
    let conn = connection();
    for (name, arity) in [
        ("meguri_card_f32", 2),
        ("meguri_retrievability", 5),
        ("meguri_relative_retrievability", 5),
        ("meguri_original_position", 1),
    ] {
        let (narg, flags): (i32, i32) = conn
            .query_row(
                "SELECT narg,flags FROM pragma_function_list WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(narg, arity);
        assert_eq!(flags & (0x800 | 0x200000), 0x800 | 0x200000);
        assert!(conn.prepare(&format!("SELECT {name}()")).is_err());
    }
    for key in [
        Value::Null,
        Value::Integer(1),
        Value::Real(1.0),
        Value::Blob(vec![b's']),
        Value::Text("S".into()),
        Value::Text("pos".into()),
    ] {
        assert!(
            conn.query_row("SELECT meguri_card_f32('{}',?1)", [key], |r| r.get::<_, Option<f64>>(0))
                .is_err()
        );
    }
    for sql in [
        "SELECT meguri_card_f32(CAST(x'ff' AS TEXT),'s')",
        "SELECT meguri_card_f32(5,'s')",
        "SELECT meguri_retrievability('{\"s\":5,\"d\":5}',35.0,10,30,0)",
    ] {
        assert_eq!(conn.query_row(sql, [], |r| r.get::<_, Option<f64>>(0)).unwrap(), None);
    }
    assert_eq!(
        conn.query_row(
            "SELECT meguri_retrievability('{\"s\":5,\"d\":5,\"lrt\":0}',35,NULL,NULL,0)",
            [],
            |r| r.get::<_, Option<f64>>(0)
        )
        .unwrap(),
        Some(1.0)
    );
    assert_eq!(
        conn.query_row(
            "SELECT meguri_relative_retrievability('{\"s\":5,\"d\":5,\"dr\":0.9,\"lrt\":0}',35,10,NULL,0)",
            [],
            |r| r.get::<_, Option<f64>>(0)
        )
        .unwrap(),
        None
    );
}

#[test]
fn elapsed_branches_wrap_and_saturate_in_the_specified_order() {
    let conn = connection();
    for (args, r_seconds, rr_seconds) in [
        ([35, 20, 30, 1897293600], 15u32 * 86400, 15u32 * 86400),
        ([35, 40, 30, 1897293600], 30 * 86400, 0),
        ([35, -5, 30, 1897293600], 30 * 86400, 0),
        (
            [0, 0, 60000, 1897293600],
            60000u32.wrapping_mul(86400),
            60000u32.wrapping_mul(86400),
        ),
        ([i64::MIN, 1, 30, 1897293600], 30 * 86400, 30 * 86400),
        ([1897290000, 2, 30, 1897293600], 3602, 3600),
        ([6192257296, 0, 30, 1897293600], 3600, 3600),
        ([365000, 0, 30, 1897293600], 0, 0),
        ([365001, 0, 30, 1897293600], 1896928599, 1896928599),
    ] {
        exact(
            real(&conn, "meguri_retrievability", r#"{"s":5,"d":5}"#, args),
            fsrs::current_retrievability(
                fsrs::MemoryState {
                    stability: 5.0,
                    difficulty: 5.0,
                },
                r_seconds as f32 / 86400.0,
                0.5,
            ),
        );
        exact(
            real(&conn, "meguri_relative_retrievability", "{}", args),
            -((rr_seconds / 86400) as f32 + 0.001) / (args[1] as f32).max(1.0),
        );
    }
}

#[test]
fn sqlite_storage_samples_preserve_float_width() {
    let database = connection();
    let storage_values = [
        Value::Null,
        Value::Integer(i64::MIN),
        Value::Integer(-1),
        Value::Integer(0),
        Value::Integer(i64::MAX),
        Value::Real(35.0),
        Value::Real(f64::INFINITY),
        Value::Text("35".into()),
        Value::Text(r#"{"s":5,"d":5,"dr":1,"lrt":0}"#.into()),
        Value::Blob(vec![0, 255]),
    ];
    for case_number in 0..2000 {
        let arguments: Vec<_> = (0..5)
            .map(|argument_index| {
                let source_index = (case_number / (argument_index + 1) + argument_index * 3) % storage_values.len();
                &storage_values[source_index]
            })
            .collect();
        let (retention, relative, _position): (Option<f64>, Option<f64>, Option<i64>) = database
            .query_row(
                "SELECT meguri_retrievability(?1,?2,?3,?4,?5),meguri_relative_retrievability(?1,?2,?3,?4,?5),meguri_original_position(?1)",
                rusqlite::params_from_iter(arguments),
                |result| Ok((result.get(0)?, result.get(1)?, result.get(2)?)),
            )
            .unwrap();
        for returned_number in [retention, relative].into_iter().flatten() {
            assert!(!returned_number.is_nan());
            assert_eq!(returned_number, f64::from(returned_number as f32));
        }
    }
}

#[test]
fn seeded_json_and_arbitrary_text_bytes_never_panic_or_raise_sql_errors() {
    use meguri_model::rng::{RandomSource, SeededRandom};
    let conn = connection();
    let random = SeededRandom::new(82);
    let atoms = [
        "null",
        "true",
        "false",
        "0",
        "-0",
        "1e40",
        "1e400",
        "1e-99999999",
        r#""\ud800""#,
        r#""x""#,
        "{}",
        "[]",
    ];
    for index in 0..1000 {
        let mut bytes: Vec<u8> = (0..random.next_u64() % 128).map(|_| random.next_u64() as u8).collect();
        if index % 3 != 0 {
            let value = atoms[random.next_u64() as usize % atoms.len()];
            let depth = random.next_u64() as usize % 150;
            let key = ["s", "d", "pos", "lrt", "dr", "decay", "cd", "zz"][random.next_u64() as usize % 8];
            bytes = format!(
                r#"{{"s":5,"d":5,"{key}":{}{value}{}}}"#,
                "[".repeat(depth),
                "]".repeat(depth)
            )
            .into_bytes();
            if index % 7 == 0 {
                bytes.pop();
            }
        }
        let argument = || match random.next_u64() % 5 {
            0 => Value::Integer(random.next_u64() as i64),
            1 => Value::Real(random.next_u64() as f64),
            2 => Value::Text((random.next_u64() as i64).to_string()),
            3 => Value::Null,
            _ => Value::Blob(vec![random.next_u64() as u8]),
        };
        conn.query_row("SELECT meguri_card_f32(CAST(?1 AS TEXT),'s'),meguri_card_f32(CAST(?1 AS TEXT),'d'),meguri_card_f32(CAST(?1 AS TEXT),'dr'),meguri_card_f32(CAST(?1 AS TEXT),'decay'),meguri_retrievability(CAST(?1 AS TEXT),?2,?3,?4,?5),meguri_relative_retrievability(CAST(?1 AS TEXT),?2,?3,?4,?5),meguri_original_position(CAST(?1 AS TEXT))",params![bytes,argument(),argument(),argument(),argument()],|row| {
            for col in 0..6 {
                if let Some(value) = row.get::<_,Option<f64>>(col)? {
                    assert!(!value.is_nan());
                    assert_eq!(value,f64::from(value as f32));
                }
            }
            let _: Option<i64> = row.get(6)?;
            Ok(())
        }).unwrap();
    }
}
