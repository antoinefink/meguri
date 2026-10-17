//! Append-only device-local recovery log, deleted-notes-log L1–L5.
use meguri_model::note::Note;
use std::{fs::OpenOptions, io::Write, path::Path};

pub fn append(collection: &Path, notes: &[Note]) -> std::io::Result<()> {
    if notes.is_empty() {
        return Ok(());
    }
    let path = collection.with_file_name("deleted.txt");
    let (mut file, fresh) = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => (file, true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            (OpenOptions::new().append(true).open(&path)?, false)
        }
        Err(e) => return Err(e),
    };
    if fresh {
        file.write_all(b"#guid column:1\n#notetype column:2\n#nid\tmid\tfields\n")?;
    }
    let mut ordered: Vec<_> = notes.iter().collect();
    ordered.sort_by_key(|note| note.id);
    for note in ordered {
        writeln!(
            file,
            "{}\t{}\t{}",
            note.guid,
            note.notetype_id.0,
            note.fields.replace('\u{1f}', "\t")
        )?;
    }
    file.flush()
}
