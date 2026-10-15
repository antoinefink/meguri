//! Headless driver for scenarios, generators, benchmarks, oracle comparison,
//! the private suite and the anonymized clone. Commands land with their units.

mod anonymize;
mod conformance;
mod generate;
mod private;
mod profile_extract;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = match args.split_first() {
        Some((command, rest)) if command == "conformance" => conformance::main(rest),
        Some((command, rest)) if command == "generate" => generate::main(rest),
        Some((command, rest)) if command == "profile-extract" => profile_extract::main(rest),
        Some((command, rest)) if command == "private" => private::main(rest),
        Some((command, rest)) if command == "anonymize" => anonymize::main(rest),
        Some((command, _)) if command == "--version" => {
            println!("meguri-cli {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("usage: meguri-cli conformance <command> ...   (meguri-cli conformance help)");
            eprintln!("       meguri-cli generate <size>|worst-case <name>|recipe <name> --out DIR [options]");
            eprintln!("       meguri-cli profile-extract <collection> --media-dir <dir> --tz <IANA zone> --out <file>");
            eprintln!("       meguri-cli private copy|tokens|suite ...   (the private vault, U15)");
            eprintln!(
                "       meguri-cli anonymize --copy <dir> --out <dir> --salt-file <file>   (the anonymized clone, U15)"
            );
            2
        }
    };
    std::process::exit(status);
}
