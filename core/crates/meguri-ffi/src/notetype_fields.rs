//! The Fields tab's two pure reads over FFI (U281), neither of which opens
//! the collection: the field names each template text refers to
//! (`render/template-syntax` T11.1), from which the tab shows a field's Used
//! On (`notetypes/workspace` W1, W3), and the check of a field name the
//! person typed (`notetypes/validation` V5).

use meguri_engine::notetypes::{FieldNameCheck, FieldNameRefusal, check_typed_field_name};
use meguri_template::ParsedTemplate;

use crate::{CoreError, callback};

/// Why V5 refuses a typed field name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeFieldNameRefusal {
    /// It starts with `#`, `^` or `/`.
    BadFirstCharacter,
    /// It holds `:`, `{` or `}`.
    BadCharacter,
    /// Another field has a name equal under unicase.
    Duplicate,
}

/// V5's answer for a typed field name.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeFieldNameCheck {
    /// Empty once `"` is removed and the ends trimmed: nothing changes.
    Ignored,
    /// The name to use: the typed text without `"`, trimmed.
    Accepted { name: String },
    Refused { refusal: NotetypeFieldNameRefusal },
}

impl From<FieldNameCheck> for NotetypeFieldNameCheck {
    fn from(check: FieldNameCheck) -> Self {
        match check {
            FieldNameCheck::Ignored => Self::Ignored,
            FieldNameCheck::Accepted(name) => Self::Accepted { name },
            FieldNameCheck::Refused(refusal) => Self::Refused {
                refusal: match refusal {
                    FieldNameRefusal::BadFirstCharacter => NotetypeFieldNameRefusal::BadFirstCharacter,
                    FieldNameRefusal::BadCharacter => NotetypeFieldNameRefusal::BadCharacter,
                    FieldNameRefusal::Duplicate => NotetypeFieldNameRefusal::Duplicate,
                },
            },
        }
    }
}

/// V5: checks `typed` against the draft's field names in order;
/// `renaming` is the position in `fields` of the field being renamed (its
/// own name does not count), none for a new field. Pure.
#[uniffi::export]
pub fn notetype_check_field_name(
    typed: String,
    fields: Vec<String>,
    renaming: Option<u32>,
) -> Result<NotetypeFieldNameCheck, CoreError> {
    callback::enter("notetype_check_field_name");
    let names: Vec<&str> = fields.iter().map(String::as_str).collect();
    let renaming = renaming.and_then(|at| usize::try_from(at).ok());
    Ok(check_typed_field_name(&typed, &names, renaming).into())
}

/// T11.1 for each text: every replacement key and every section key that
/// is not a card number, outside comments, each once in order of first
/// appearance. Special names, `FrontSide`, the empty key and names of no
/// field are kept: the caller matches them against its fields. Pure.
#[uniffi::export]
pub fn template_field_references(texts: Vec<String>) -> Result<Vec<Vec<String>>, CoreError> {
    callback::enter("template_field_references");
    Ok(texts
        .iter()
        .map(|text| {
            ParsedTemplate::parse(text)
                .field_references()
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn typed_field_names_cross_the_bridge_with_v5s_answer() {
        let fields = names(&["Front", "Back"]);
        let check =
            |typed: &str, renaming: Option<u32>| notetype_check_field_name(typed.into(), fields.clone(), renaming).unwrap();
        assert_eq!(check("  \"Ex\"tra ", None), NotetypeFieldNameCheck::Accepted { name: "Extra".into() });
        assert_eq!(check(" \" ", None), NotetypeFieldNameCheck::Ignored);
        let refused = |refusal| NotetypeFieldNameCheck::Refused { refusal };
        assert_eq!(check("#Tag", None), refused(NotetypeFieldNameRefusal::BadFirstCharacter));
        assert_eq!(check("a:b", None), refused(NotetypeFieldNameRefusal::BadCharacter));
        assert_eq!(check("back", None), refused(NotetypeFieldNameRefusal::Duplicate));
        // The renamed field's own name does not count: a case-only rename is accepted.
        assert_eq!(check("BACK", Some(1)), NotetypeFieldNameCheck::Accepted { name: "BACK".into() });
        assert_eq!(check("front", Some(1)), refused(NotetypeFieldNameRefusal::Duplicate));
    }

    #[test]
    fn references_follow_t11_1_per_text() {
        let texts = names(&[
            "{{Front}}<!--{{Hidden}}-->{{#Extra}}{{text:Back}}{{/Extra}}{{Front}}",
            "{{#c1}}{{Tags}}{{/c1}}{{^Blank}}x{{/Blank}}",
            "",
            "{{#Open}}",
        ]);
        let refs = template_field_references(texts).unwrap();
        assert_eq!(refs[0], names(&["Front", "Extra", "Back"]));
        assert_eq!(refs[1], names(&["Tags", "Blank"]));
        assert!(refs[2].is_empty());
        // A text with a structural error still lists the keys it holds.
        assert_eq!(refs[3], names(&["Open"]));
    }
}
