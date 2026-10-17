//! U63 T3, render/image-occlusion-format F16: strict-read every detected IO
//! field, then require an unchanged save to preserve its bytes (F11).
use std::collections::{BTreeMap, btree_map::Entry};
use std::fs;
use std::path::Path;

use meguri_engine::image_occlusion as io;
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::{OpenOptions, ReadRows, Writer, open_collection};
use meguri_testkit::private::suite::{Check, CheckCounts};
use meguri_testkit::private::vault::{COLLECTION, COLLECTION_WAL};

pub struct IoRoundtrip;

impl Check for IoRoundtrip {
    fn name(&self) -> &'static str {
        "io-roundtrip"
    }

    fn run(&self, copy: &Path) -> Result<CheckCounts, String> {
        // The storage open gate can upgrade/checkpoint. Like candidate
        // extraction, open a scratch copy so the vault copy stays intact.
        let scratch = tempfile::tempdir().map_err(|_| "creating IO scratch copy failed")?;
        fs::copy(copy.join(COLLECTION), scratch.path().join(COLLECTION)).map_err(|_| "copying IO collection failed")?;
        match fs::copy(copy.join(COLLECTION_WAL), scratch.path().join(COLLECTION_WAL)) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("copying IO collection WAL failed".into()),
        }
        let clock = FixedClock::new(0, UtcOffset::UTC);
        let random = SeededRandom::new(63);
        let collection = open_collection(&scratch.path().join(COLLECTION), &OpenOptions::new(&clock, &random))
            .map_err(|_| "opening IO scratch collection failed")?;
        let result = check_rows(collection.writer());
        let closed = collection.close();
        let counts = result?;
        closed.map_err(|_| "closing IO scratch collection failed")?;
        Ok(counts)
    }
}

fn check_rows(writer: &Writer) -> Result<CheckCounts, String> {
    let mut counts = CheckCounts::default();
    counts.breakdown.insert("round-tripped", 0);
    let mut roles = BTreeMap::new();
    let mut after = None;
    loop {
        let notes = writer
            .notes_after(after, 5_000)
            .map_err(|_| "reading IO notes failed")?;
        if notes.is_empty() {
            break;
        }
        for note in notes {
            after = Some(note.id);
            if let Entry::Vacant(entry) = roles.entry(note.notetype_id) {
                let nt = writer
                    .notetype(note.notetype_id)
                    .map_err(|_| "reading IO note type failed")?
                    .ok_or("IO note type missing")?;
                entry.insert(io::detect(&nt));
            }
            let Some(role) = roles[&note.notetype_id] else { continue };
            let Some(field) = note.field_values().nth(role.occlusions) else {
                failed(&mut counts, "missing-field");
                continue;
            };
            match io::parse(field) {
                Err(error) => failed(&mut counts, error.kind.name()),
                Ok(doc) => {
                    // An unchanged save never applies the pixel-size drop
                    // rules, so any positive finite reference size suffices.
                    match doc.write(&doc.shapes, doc.hide_all, 1., 1.) {
                        Ok(written) if written == field => {
                            counts.passed += 1;
                            *counts.breakdown.get_mut("round-tripped").expect("initialized") += 1;
                        }
                        Ok(_) => failed(&mut counts, "byte-mismatch"),
                        Err(_) => failed(&mut counts, "write-error"),
                    }
                }
            }
        }
    }
    Ok(counts)
}

fn failed(counts: &mut CheckCounts, category: &'static str) {
    counts.failed += 1;
    *counts.breakdown.entry(category).or_default() += 1;
}
