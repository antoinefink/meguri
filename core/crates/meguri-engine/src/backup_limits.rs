//! Synced backup limits (package/backups B6); scheduling belongs to U127.
use meguri_storage::{ConfigValue, ReadRows, WriteScope};
use crate::{Collection, EngineError, OpResult, OpSource, OpSpec};

const FIELDS: [&str; 4] = ["daily", "weekly", "monthly", "minimum_interval_mins"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupLimits {
    pub daily: u32,
    pub weekly: u32,
    pub monthly: u32,
    pub minimum_interval_mins: u32,
}
impl Default for BackupLimits {
    fn default() -> Self {
        Self { daily: 12, weekly: 10, monthly: 9, minimum_interval_mins: 30 }
    }
}
impl BackupLimits {
    /// All four values read together; duplicate known object members refuse.
    pub fn from_stored(value: Option<&ConfigValue>) -> Self {
        fn read(value: &ConfigValue) -> Option<BackupLimits> {
            let values: Vec<&ConfigValue> = match value {
                ConfigValue::Array(values) if values.len() == 4 => values.iter().collect(),
                ConfigValue::Object(members) => {
                    let mut values = Vec::new();
                    for name in FIELDS {
                        let mut found = members.iter().filter(|(key, _)| key == name);
                        values.push(&found.next()?.1);
                        if found.next().is_some() { return None; }
                    }
                    values
                }
                _ => return None,
            };
            let mut numbers = [0; 4];
            for (out, value) in numbers.iter_mut().zip(values) {
                *out = u32::try_from(value.as_u64()?).ok()?;
            }
            Some(BackupLimits {
                daily: numbers[0], weekly: numbers[1], monthly: numbers[2], minimum_interval_mins: numbers[3],
            })
        }
        value.and_then(read).unwrap_or_default()
    }
}

pub fn read(rows: &impl ReadRows) -> Result<BackupLimits, EngineError> {
    Ok(BackupLimits::from_stored(rows.config_value("backups")?.as_ref()))
}

/// Re-derive from the writer's current object, keeping unknown members.
pub fn save_in(scope: &mut WriteScope<'_>, limits: BackupLimits) -> Result<bool, EngineError> {
    let mut value = scope.config_value("backups")?.unwrap_or(ConfigValue::Null);
    for (name, number) in FIELDS.into_iter().zip([
        limits.daily, limits.weekly, limits.monthly, limits.minimum_interval_mins,
    ]) {
        value = value.with_member(name, ConfigValue::from(u64::from(number)));
    }
    Ok(scope.set_config_ordered("backups", &value, &FIELDS)?.changed())
}

/// Settings semantics: no undo entry; only a changed key clears history.
pub fn save(collection: &Collection, limits: BackupLimits) -> Result<OpResult<bool>, EngineError> {
    collection.run(OpSpec::named("save_backup_settings", OpSource::UserNonUndoable), move |ctx| {
        save_in(ctx.scope(), limits)
    })
}
