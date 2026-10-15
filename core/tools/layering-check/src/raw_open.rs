//! Finds identifiers named in crates that may not use them: the engine's raw
//! open entry point outside the crates allowed to call it, and the
//! storage-only builder in the crates denied it.
//!
//! Sources are tokenized, so the identifier in a comment or a string literal
//! does not count. A file that does not tokenize is searched as plain text.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{TokenStream, TokenTree};

use crate::edges::Edges;
use crate::metadata::Metadata;
use crate::{Error, Rule, Violation};

pub(crate) fn scan(meta: &Metadata, edges: &Edges) -> Result<Vec<Violation>, Error> {
    let rule = &edges.raw_open;
    let mut out = scan_ident(
        meta,
        &rule.ident,
        |name| !rule.allowed.iter().any(|a| a == name),
        |package, rel, line| {
            Violation::new(
                Rule::RawOpen,
                format!("{package}:{rel}:{line}"),
                format!(
                    "{package} names the engine's raw open entry point `{}` at {rel}:{line}; edges.toml [raw_open] \
                     allows it only in {}. Open collections through meguri_host::CoreHost::open.",
                    rule.ident,
                    rule.allowed.join(", ")
                ),
            )
        },
    )?;
    if let Some(builder) = &edges.storage_builder {
        out.extend(scan_ident(
            meta,
            &builder.ident,
            |name| builder.denied.iter().any(|d| d == name),
            |package, rel, line| {
                Violation::new(
                    Rule::StorageBuilder,
                    format!("{package}:{rel}:{line}"),
                    format!(
                        "{package} names the storage-only builder `{}` at {rel}:{line}; edges.toml \
                         [storage_builder] keeps it out of {}. The live collection is written only through \
                         the executor's write scopes.",
                        builder.ident,
                        builder.denied.join(", ")
                    ),
                )
            },
        )?);
    }
    Ok(out)
}

/// Every place `ident` appears as an identifier in the workspace crates
/// `checked` selects.
fn scan_ident(
    meta: &Metadata,
    ident: &str,
    checked: impl Fn(&str) -> bool,
    violation: impl Fn(&str, &str, usize) -> Violation,
) -> Result<Vec<Violation>, Error> {
    let mut out = Vec::new();
    for package in &meta.packages {
        if !meta.workspace_members.contains(&package.id) || !checked(&package.name) {
            continue;
        }
        let Some(root) = package.manifest_path.parent() else {
            continue;
        };
        let mut files = Vec::new();
        rust_files(root, &mut files)?;
        files.sort();
        for file in files {
            let text = fs::read_to_string(&file).map_err(|e| Error::new(format!("reading {}: {e}", file.display())))?;
            let rel = file.strip_prefix(root).unwrap_or(&file).display().to_string();
            for line in ident_lines(&text, ident) {
                out.push(violation(&package.name, &rel, line));
            }
        }
    }
    Ok(out)
}

/// `.rs` files under a crate directory, skipping `target/` and hidden directories.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    let entries = fs::read_dir(dir).map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let kind = entry
            .file_type()
            .map_err(|e| Error::new(format!("reading {}: {e}", path.display())))?;
        if kind.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_files(&path, out)?;
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// 1-based lines where `ident` appears as an identifier.
fn ident_lines(text: &str, ident: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    match TokenStream::from_str(text) {
        Ok(tokens) => walk(tokens, ident, &mut lines),
        Err(_) => {
            for (i, line) in text.lines().enumerate() {
                if contains_word(line, ident) {
                    lines.push(i + 1);
                }
            }
        }
    }
    lines
}

fn walk(tokens: TokenStream, ident: &str, lines: &mut Vec<usize>) {
    for token in tokens {
        match token {
            TokenTree::Ident(i) if unraw(&i.to_string()) == ident => lines.push(i.span().start().line),
            TokenTree::Group(g) => walk(g.stream(), ident, lines),
            _ => {}
        }
    }
}

/// An identifier without its raw prefix: `r#open_raw` names `open_raw`.
fn unraw(ident: &str) -> &str {
    ident.strip_prefix("r#").unwrap_or(ident)
}

fn contains_word(line: &str, word: &str) -> bool {
    let is_ident = |c: char| c == '_' || c.is_alphanumeric();
    line.match_indices(word).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + word.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}
