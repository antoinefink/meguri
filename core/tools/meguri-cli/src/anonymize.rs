//! `meguri-cli anonymize`: the anonymized clone of a collection copy
//! (U15, `specs/fixtures/anonymizer.md` A2.2, R1941).
//!
//! ```text
//! meguri-cli anonymize --copy <source dir> --out <clone dir> --salt-file <file>
//! ```
//!
//! The source is only read. `--out` must not exist or be empty and must lie
//! outside the source. A salt file that does not exist is created with 32
//! bytes from the operating system's generator, readable and writable only
//! by its owner. On success it prints the report's path (exit 0); the
//! report's checks say pass, warn or fail. On failure it prints the A18
//! kind (`meguri-cli anonymize: <kind>: ...`), deletes what it wrote under
//! `--out` and exits 1 (2 for a usage error). Nothing it prints names
//! collection text.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use meguri_testkit::anonymize::{self, AnonymizeError, REPORT, Salt};

const USAGE: &str = "usage: meguri-cli anonymize --copy <source dir> --out <clone dir> --salt-file <file>";

fn fail(error: &AnonymizeError) -> i32 {
    eprintln!("meguri-cli anonymize: {error}");
    if matches!(error, AnonymizeError::Usage(_)) {
        eprintln!("{USAGE}");
        2
    } else {
        1
    }
}

/// The salt from `path`, or a new one written there when it does not exist.
fn salt(path: &Path) -> Result<Salt, AnonymizeError> {
    match fs::read(path) {
        Ok(contents) => Salt::parse(&contents),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let mut bytes = [0u8; 32];
            // The operating system's generator: the salt is a secret key,
            // so it never comes from the core's seeded randomness seam.
            fs::File::open("/dev/urandom")
                .and_then(|mut random| random.read_exact(&mut bytes))
                .map_err(|e| AnonymizeError::io("reading /dev/urandom", &e))?;
            let salt = Salt::new(bytes);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|e| AnonymizeError::io("creating the salt file", &e))?;
            file.write_all(salt.to_file_contents().as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|e| AnonymizeError::io("writing the salt file", &e))?;
            Ok(salt)
        }
        Err(e) => Err(AnonymizeError::io("reading the salt file", &e)),
    }
}

pub fn main(args: &[String]) -> i32 {
    let mut copy: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut salt_file: Option<PathBuf> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let Some(value) = rest.next() else {
            return fail(&AnonymizeError::Usage(format!("{arg} needs a value")));
        };
        let slot = match arg.as_str() {
            "--copy" => &mut copy,
            "--out" => &mut out,
            "--salt-file" => &mut salt_file,
            other => return fail(&AnonymizeError::Usage(format!("unknown argument {other}"))),
        };
        if slot.replace(PathBuf::from(value)).is_some() {
            return fail(&AnonymizeError::Usage(format!("{arg} given twice")));
        }
    }
    let (Some(copy), Some(out), Some(salt_file)) = (copy, out, salt_file) else {
        return fail(&AnonymizeError::Usage(
            "--copy, --out and --salt-file are required".into(),
        ));
    };
    let salt = match salt(&salt_file) {
        Ok(salt) => salt,
        Err(e) => return fail(&e),
    };
    match anonymize::anonymize(&copy, &out, &salt) {
        Ok(_) => {
            println!("{}", out.join(REPORT).display());
            0
        }
        Err(e) => fail(&e),
    }
}
