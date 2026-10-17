//! Cloning a note type (`specs/notetypes/records.md` N7, R905, R916).

use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

/// The clone of `source` before its add: named `name` or `<source name>
/// copy`, `original_id` cleared, its template rows' `mtime_secs` and `usn`
/// 0, everything else as in the source, field and template ids, `other`
/// bytes and unknown protobuf fields included. Its id, stamps and card
/// requirements are the add's (N5, N6).
pub fn cloned(source: &Notetype, name: Option<&str>) -> Notetype {
    let mut clone = source.clone();
    clone.id = NotetypeId(0);
    clone.name = name.map_or_else(|| format!("{} copy", source.name), str::to_owned);
    clone.config.original_id = None;
    for template in &mut clone.templates {
        template.mtime = EpochSecs(0);
        template.usn = Usn(0);
    }
    clone
}
