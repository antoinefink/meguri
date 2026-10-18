//! Save normalization and shared field-count repair (N2, F1).
use meguri_storage::{ConfigValue, ReadRows, StorageError};

pub fn normalize_enabled(reader: &(impl ReadRows + ?Sized)) -> Result<bool, StorageError> {
    Ok(!matches!(
        reader.config_value("normalize_note_text")?,
        Some(ConfigValue::Bool(false))
    ))
}

pub fn normalize(reader: &impl ReadRows, values: &[String]) -> Result<Vec<String>, StorageError> {
    let enabled = normalize_enabled(reader)?;
    Ok(values
        .iter()
        .map(|v| meguri_text::normalize::field_as_saved(v, enabled).into_owned())
        .collect())
}

pub fn repair(values: &mut Vec<String>, count: usize) -> Result<(), crate::EngineError> {
    if count == 0 {
        return Err(super::invalid("a note type needs at least one field"));
    }
    if values.len() > count {
        let extra = values.drain(count..).collect::<Vec<_>>();
        for value in extra {
            values[count - 1].push_str("; ");
            values[count - 1].push_str(&value);
        }
    }
    values.resize(count, String::new());
    Ok(())
}
