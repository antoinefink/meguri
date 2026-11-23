//! Note type records a deck package import writes (`package/apkg-import`
//! I7, `package/notetype-merge` J4.5): the save normalization as an
//! unchecked save, with the stamps left to the caller, and the template
//! rewrite of a merge's field renames. Pure: nothing here reads or writes.

use std::collections::HashSet;

use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_text::fold::folded;

use super::rewrite::{Map, Side, Target, rewrite};
use super::{InvalidNotetype, NotetypeKind, names, write};

/// N4 steps 1 to 8 on an incoming record, with no template check (V1's
/// unchecked save): added, copied and updated note types of an import
/// without a merge. `others` are the names step 7 must differ from.
pub fn normalize_incoming(notetype: &mut Notetype, others: &[(NotetypeId, String)]) -> Result<(), InvalidNotetype> {
    for field in &mut notetype.fields {
        field.prior_ord = None;
    }
    names::normalize(notetype, others)?;
    notetype.config.reqs = write::card_requirements(notetype);
    Ok(())
}

/// J4.5: N4 steps 1 to 5, 7 and 8 on a merged record, `sort_index` (J4.4)
/// standing in for step 6: a position in the merged list, lowered to the
/// last one when past the end.
pub fn normalize_merged(
    notetype: &mut Notetype,
    others: &[(NotetypeId, String)],
    sort_index: u32,
) -> Result<(), InvalidNotetype> {
    for field in &mut notetype.fields {
        field.prior_ord = None;
    }
    names::normalize(notetype, others)?;
    let last = u32::try_from(notetype.fields.len().saturating_sub(1)).unwrap_or(u32::MAX);
    notetype.config.sort_field_idx = sort_index.min(last);
    notetype.config.reqs = write::card_requirements(notetype);
    Ok(())
}

/// `notetypes/field-rename` F3 with `structure-changes` S4.3's append rule
/// on every template text of `notetype`, for fields whose name changed from
/// `renames`' first member to its second. Call it on the normalized record,
/// whose first field S4.3 names.
pub fn rewrite_renamed_fields(notetype: &mut Notetype, renames: &[(String, String)]) {
    let map: Map<'_> = renames
        .iter()
        .filter(|(before, after)| before != after)
        .map(|(before, after)| (before.as_str(), Target::Renamed(after.as_str())))
        .collect();
    if map.is_empty() {
        return;
    }
    let cloze = NotetypeKind::of(notetype) == NotetypeKind::Cloze;
    let first = notetype.fields.first().map(|field| field.name.clone()).unwrap_or_default();
    for template in &mut notetype.templates {
        let config = &mut template.config;
        config.q_format = rewrite(&config.q_format, &map, &first, cloze, Side::Front);
        config.a_format = rewrite(&config.a_format, &map, &first, cloze, Side::Back);
        config.q_format_browser = rewrite(&config.q_format_browser, &map, &first, cloze, Side::BrowserFront);
        config.a_format_browser = rewrite(&config.a_format_browser, &map, &first, cloze, Side::BrowserBack);
    }
    notetype.config.reqs = write::card_requirements(notetype);
}

/// N4 step 7 alone: `name` with `+` appended until no note type of
/// `others` other than `id` has a name equal under unicase.
pub fn unique_name(name: &str, id: NotetypeId, others: &[(NotetypeId, String)]) -> String {
    names::unique_among(name, id, others)
}

/// Whether two names are the same under unicase.
pub fn same_name(a: &str, b: &str) -> bool {
    folded(a) == folded(b)
}

/// The names of `others` under unicase, for callers that test many names.
pub fn folded_names(others: &[(NotetypeId, String)]) -> HashSet<String> {
    others.iter().map(|(_, name)| folded(name)).collect()
}
