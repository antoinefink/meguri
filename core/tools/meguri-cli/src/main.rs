//! Headless driver for scenarios, generators, benchmarks, oracle comparison
//! and the private suite. Commands land with their units.

mod conformance;
mod generate;
mod profile_extract;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = match args.split_first() {
        Some((command, rest)) if command == "conformance" => conformance::main(rest),
        Some((command, rest)) if command == "generate" => generate::main(rest),
        Some((command, rest)) if command == "profile-extract" => profile_extract::main(rest),
        Some((command, _)) if command == "--version" => {
            println!("meguri-cli {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("usage: meguri-cli conformance <command> ...   (meguri-cli conformance help)");
            eprintln!("       meguri-cli generate <size>|worst-case <name>|recipe <name> --out DIR [options]");
            eprintln!("       meguri-cli profile-extract --collection PATH --out FILE [--media-dir DIR] [--tz IANA]");
            2
        }
    };
    std::process::exit(status);
}
