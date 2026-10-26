use std::process::Command;
#[test]
fn native_review_internal_cli_requires_all_explicit_seeds_and_rejects_overflow() {
    let cli = env!("CARGO_BIN_EXE_meguri-cli");
    assert_eq!(
        Command::new(cli).arg("internal-native-review").status().unwrap().code(),
        Some(2)
    );
    for invalid in ["18446744073709551616", "-1", "+1", ""] {
        let output = Command::new(cli)
            .args([
                "internal-native-review",
                "/nonexistent",
                "case",
                "mac",
                "0000000000000000000000000000000000000000",
                "1",
                "2",
                invalid,
                "4",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8(output.stderr).unwrap().contains("seed"));
    }
}
