//! Internal prelaunch F11.2 derivation. No public tools/mg job is registered.
use meguri_testkit::native_review::{self, Seeds};
use std::path::Path;

pub fn main(args: &[String]) -> i32 {
    if args.len() != 8 {
        eprintln!(
            "internal usage: meguri-cli internal-native-review ROOT CASE mac|iphone|ipad REVISION SOURCE_OPEN_SEED PACKAGE_WRITE_SEED INSPECTION_ACTIVATION_SEED REOPEN_SEED"
        );
        return 2;
    }
    let seed = |index: usize| -> Result<u64, String> {
        let value = &args[index];
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err("seed must be an explicit decimal u64".into());
        }
        value.parse().map_err(|_| "seed exceeds u64".into())
    };
    let result = (|| {
        let seeds = Seeds {
            source_open: seed(4)?,
            package_write: seed(5)?,
            inspection_and_initial_activation: seed(6)?,
            reopen: seed(7)?,
        };
        native_review::derive(
            Path::new(&args[0]),
            &args[1],
            &args[2],
            seeds,
            &args[3],
            env!("MEGURI_CLI_PROFILE"),
        )
        .map_err(|e| e.to_string())
    })();
    match result {
        Ok(output) => {
            println!("{}", output.display());
            0
        }
        Err(error) => {
            eprintln!("native review derivation refused: {error}; retain any expected-replay artifacts");
            1
        }
    }
}
