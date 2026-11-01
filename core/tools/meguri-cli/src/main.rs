//! Headless driver for scenarios, generators, benchmarks, oracle comparison,
//! the private suite and the anonymized clone. Commands land with their units.

mod anonymize;
mod cloud_service;
mod conformance;
mod fp_probe;
mod generate;
mod native_review;
mod private;
mod private_suite;
mod profile_extract;
mod timing;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = match args.split_first() {
        Some((command, rest)) if command == "conformance" => conformance::main(rest),
        Some((command, rest)) if command == "generate" => generate::main(rest),
        Some((command, rest)) if command == "internal-native-review" => native_review::main(rest),
        Some((command, rest)) if command == "internal-cloud-service" => cloud_service::main(rest),
        Some((command, rest)) if command == "fp-probe" => fp_probe::main(rest),
        Some((command, rest)) if command == "profile-extract" => profile_extract::main(rest),
        Some((command, rest)) if command == "private" => private::main(rest),
        Some((command, rest)) if command == "anonymize" => anonymize::main(rest),
        Some((command, rest)) if command == "timing" => timing::main(rest),
        Some((command, _)) if command == "--version" => {
            println!("meguri-cli {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("usage: meguri-cli conformance <command> ...   (meguri-cli conformance help)");
            eprintln!("       meguri-cli generate <size>|worst-case <name>|recipe <name> --out DIR [options]");
            eprintln!("       meguri-cli fp-probe eval|report ...   (meguri-cli fp-probe help)");
            eprintln!("       meguri-cli profile-extract <collection> --media-dir <dir> --tz <IANA zone> --out <file>");
            eprintln!("       meguri-cli private copy|tokens|suite ...   (the private vault, U15)");
            eprintln!(
                "       meguri-cli anonymize --copy <dir> --out <dir> --salt-file <file>   (the anonymized clone, U15)"
            );
            eprintln!("       meguri-cli timing run <dir> --out <dir> [options]   (timed operations, U18)");
            2
        }
    };
    std::process::exit(status);
}
