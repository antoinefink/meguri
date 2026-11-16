// SPDX-License-Identifier: MIT
//! Cargo consumes only an explicitly prepared, source-bound production bundle.
use std::{env, fs, path::{Path, PathBuf}};

fn files(root: &Path, relative: &Path, result: &mut Vec<PathBuf>, source: bool) {
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path).expect("missing browser input");
    assert!(!metadata.file_type().is_symlink(), "linked browser input");
    if metadata.is_dir() {
        // Match scripts/copy-source.mjs: exclude only root-level generated
        // directories, never similarly named source files or nested inputs.
        if source && relative.components().count() == 1 && matches!(relative.to_str(),
            Some("node_modules" | "dist" | ".test-build" | "playwright-report" | "test-results")) {
            return;
        }
        for entry in fs::read_dir(path).expect("read browser inputs") {
            files(root, &relative.join(entry.expect("browser entry").file_name()), result, source);
        }
    } else {
        assert!(metadata.is_file(), "non-file browser input");
        result.push(relative.to_owned());
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=MEGURI_WEB_PREPARED_DIR");
    let web = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("web");
    let prepared = PathBuf::from(env::var_os("MEGURI_WEB_PREPARED_DIR")
        .expect("prepare real browser assets through MG before compiling the server"));
    assert!(prepared.is_absolute(), "prepared browser directory must be absolute");
    println!("cargo:rerun-if-changed={}", prepared.display());
    // Watch the tree so additions/removals also invalidate Cargo's preparation.
    println!("cargo:rerun-if-changed={}", web.display());
    let mut expected = Vec::new();
    files(&web, Path::new(""), &mut expected, true);
    expected.sort();
    let mut actual = Vec::new();
    files(&prepared.join("inputs"), Path::new(""), &mut actual, false);
    actual.sort();
    assert_eq!(actual, expected, "stale prepared browser input inventory");
    for input in expected {
        assert_eq!(fs::read(web.join(&input)).unwrap(), fs::read(prepared.join("inputs").join(&input)).unwrap(), "stale prepared browser input: {}", input.display());
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut bundles = Vec::new();
    files(&prepared, Path::new("embedded"), &mut bundles, false);
    assert!(bundles.iter().any(|p| p == Path::new("embedded/index.html")), "missing browser shell");
    assert!(bundles.iter().any(|p| p.extension().is_some_and(|e| e == "js")), "missing compiled JavaScript");
    fs::create_dir_all(out.join("embedded")).unwrap();
    for input in [PathBuf::from("meguri_web_assets.rs"), PathBuf::from("assets.json")].into_iter().chain(bundles) {
        let source = prepared.join(&input);
        assert!(fs::symlink_metadata(&source).unwrap().is_file(), "invalid prepared asset");
        let bytes = fs::read(source).expect("missing prepared asset");
        assert!(!bytes.is_empty(), "empty prepared asset");
        fs::write(out.join(input), bytes).unwrap();
    }
}
