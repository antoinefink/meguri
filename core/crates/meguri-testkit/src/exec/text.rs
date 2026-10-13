//! The `text` observation kind (observations.md O4, O8): meguri-text's pure
//! functions applied to a list of inputs. It never reads or changes the
//! scenario's collection, so it needs no session state.

use std::borrow::Cow;
use std::cmp::Ordering;

use meguri_text::{checksum, field_empty, fold, html, iri, media_name, media_refs, normalize};
use serde_json::{Map, Value, json};

use super::{ExecError, Registry, Session};
use crate::scenario::{ObservationArgs, ObservationRequest, TextFunction, TextInputs, TextRequest};

/// Registers the `text` kind.
pub(super) fn register(registry: &mut Registry) {
    registry.observation("text", observe);
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn observe(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("observation text takes {fns, inputs}"));
    };
    let text = TextRequest::from_mapping(map).map_err(|e| invalid(e.to_string()))?;
    let mut value = Map::new();
    for &function in &text.fns {
        let outputs = match &text.inputs {
            TextInputs::Strings(inputs) => inputs
                .iter()
                .map(|input| apply(function, input))
                .collect::<Result<Vec<_>, _>>()?,
            TextInputs::Pairs(pairs) if function == TextFunction::UnicaseCompare => pairs
                .iter()
                .map(|(a, b)| json!(sign(fold::compare_folded(a, b))))
                .collect(),
            TextInputs::Pairs(_) => return Err(invalid(format!("{} takes strings", function.name()))),
        };
        value.insert(function.name().to_owned(), Value::Array(outputs));
    }
    Ok(Value::Object(value))
}

/// O8 "as saved": the value a note save stores for a field in a collection
/// with the default settings (note text normalization on).
fn as_saved(input: &str) -> Cow<'_, str> {
    normalize::field_as_saved(input, true)
}

fn sign(ordering: Ordering) -> i8 {
    match ordering {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

fn apply(function: TextFunction, input: &str) -> Result<Value, ExecError> {
    Ok(match function {
        TextFunction::StripMarkup => json!(html::strip_markup(input)),
        TextFunction::StripMarkupKeepMedia => json!(html::strip_markup_keep_media(input)),
        TextFunction::TextLine => json!(html::text_line(input)),
        TextFunction::TextLineKeepMedia => json!(html::text_line_keep_media(input)),
        TextFunction::MediaRefs => json!(media_refs::references(input)),
        TextFunction::IriEncode => json!(iri::encode_for_display(input)),
        TextFunction::IriDecode => json!(iri::decode_for_saving(input)),
        TextFunction::MediaFilename => {
            // The oracle sees the transform through its media store, which
            // also lowercases new names (text/media-filenames#F4).
            if input.to_lowercase() != input {
                return Err(invalid(format!(
                    "media_filename input {input:?} changes when lowercased"
                )));
            }
            json!(media_name::safe_media_name(input))
        }
        TextFunction::SortField => json!(checksum::sort_text(&as_saved(input))),
        TextFunction::Checksum => json!(checksum::note_checksum(&as_saved(input))),
        TextFunction::FieldEmpty => json!(field_empty::counts_as_empty(&as_saved(input))),
        TextFunction::UnicaseCompare => return Err(invalid("unicase_compare takes [a, b] pairs")),
    })
}
