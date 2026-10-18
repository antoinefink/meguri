//! Strict typed config reads (G1/G2). Persistence remains U23's writer:
//! equal values preserve original bytes and stamps. Read-only keys have no setter.
use meguri_storage::{ConfigValue, ReadRows, StorageError, WriteScope, Written};

pub struct SchedConfig<'a, R: ReadRows> {
    rows: &'a R,
}

macro_rules! bool_key {
    ($get:ident, $key:literal, $default:expr) => {
        pub fn $get(&self) -> Result<bool, StorageError> {
            Ok(self
                .rows
                .config_value($key)?
                .and_then(|v| v.as_bool())
                .unwrap_or($default))
        }
    };
}
macro_rules! u32_key {
    ($get:ident, $key:literal, $default:expr) => {
        pub fn $get(&self) -> Result<u32, StorageError> {
            Ok(self
                .integer($key)?
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or($default))
        }
    };
}
macro_rules! string_key {
    ($get:ident, $key:literal, $default:expr) => {
        pub fn $get(&self) -> Result<String, StorageError> {
            Ok(self
                .rows
                .config_value($key)?
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| $default.into()))
        }
    };
}

impl<'a, R: ReadRows> SchedConfig<'a, R> {
    pub fn new(rows: &'a R) -> Self {
        Self { rows }
    }
    fn integer(&self, key: &str) -> Result<Option<i128>, StorageError> {
        Ok(match self.rows.config_value(key)? {
            Some(ConfigValue::Integer(v)) => Some(v),
            _ => None,
        })
    }
    u32_key!(collapse_time, "collapseTime", 1200);
    u32_key!(last_unburied, "lastUnburied", 0);
    u32_key!(next_position, "nextPos", 0);
    u32_key!(time_limit, "timeLim", 0);
    bool_key!(new_cards_ignore_review_limit, "newCardsIgnoreReviewLimit", false);
    bool_key!(apply_all_parent_limits, "applyAllParentLimits", false);
    bool_key!(load_balancer_enabled, "loadBalancerEnabled", true);
    bool_key!(fsrs, "fsrs", false);
    bool_key!(fsrs_short_term_with_steps, "fsrsShortTermWithStepsEnabled", false);
    bool_key!(fsrs_health_check, "fsrsHealthCheck", true);
    bool_key!(fsrs_legacy_evaluate, "fsrsLegacyEvaluate", false);
    bool_key!(restore_position_reviewer, "restorePositionReviewer", true);
    bool_key!(restore_position_browser, "restorePositionBrowser", true);
    bool_key!(reset_counts_reviewer, "resetCountsReviewer", false);
    bool_key!(reset_counts_browser, "resetCountsBrowser", false);
    bool_key!(random_order_reposition, "randomOrderReposition", false);
    bool_key!(shift_position_of_existing_cards, "shiftPositionOfExistingCards", false);
    bool_key!(sched2021, "sched2021", false);
    string_key!(set_due_reviewer, "setDueReviewer", "1");
    string_key!(set_due_browser, "setDueBrowser", "0");
    string_key!(card_state_customizer, "cardStateCustomizer", "");
    pub fn rollover_hour(&self) -> Result<u8, StorageError> {
        Ok(crate::timing::rollover_hour(self.integer("rollover")?).unwrap_or(crate::timing::DEFAULT_ROLLOVER_HOUR))
    }
    pub fn creation_offset(&self) -> Result<Option<i32>, StorageError> {
        Ok(crate::timing::creation_offset(self.integer("creationOffset")?))
    }
    pub fn local_offset(&self) -> Result<i32, StorageError> {
        Ok(crate::timing::local_offset(self.integer("localOffset")?))
    }
    pub fn last_fsrs_optimize(&self) -> Result<i32, StorageError> {
        Ok(self
            .integer("lastFsrsOptimize")?
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or(0))
    }
    /// Open policy accepts only the integer 2; all other forms mean v1.
    pub fn scheduler_version(&self) -> Result<u8, StorageError> {
        Ok(if self.integer("schedVer")? == Some(2) { 2 } else { 1 })
    }
}

macro_rules! setter {
    ($name:ident, $key:literal, $ty:ty, $value:expr) => {
        pub fn $name(scope: &mut WriteScope<'_>, value: $ty) -> Result<Written, StorageError> {
            scope.set_config($key, &($value)(value))
        }
    };
}

setter!(set_collapse_time, "collapseTime", u32, |v| ConfigValue::Integer(
    i128::from(v)
));
setter!(set_rollover_hour, "rollover", u8, |v: u8| ConfigValue::Integer(
    i128::from(v.min(23))
));
setter!(set_time_limit, "timeLim", u32, |v| ConfigValue::Integer(i128::from(v)));
setter!(
    set_new_cards_ignore_review_limit,
    "newCardsIgnoreReviewLimit",
    bool,
    ConfigValue::Bool
);
setter!(
    set_apply_all_parent_limits,
    "applyAllParentLimits",
    bool,
    ConfigValue::Bool
);
setter!(
    set_load_balancer_enabled,
    "loadBalancerEnabled",
    bool,
    ConfigValue::Bool
);
setter!(set_fsrs, "fsrs", bool, ConfigValue::Bool);
setter!(set_last_fsrs_optimize, "lastFsrsOptimize", i32, |v| {
    ConfigValue::Integer(i128::from(v))
});
setter!(set_due_reviewer, "setDueReviewer", &str, |v: &str| ConfigValue::String(
    v.into()
));
setter!(set_due_browser, "setDueBrowser", &str, |v: &str| ConfigValue::String(
    v.into()
));
setter!(
    set_restore_position_reviewer,
    "restorePositionReviewer",
    bool,
    ConfigValue::Bool
);
setter!(
    set_restore_position_browser,
    "restorePositionBrowser",
    bool,
    ConfigValue::Bool
);
setter!(
    set_reset_counts_reviewer,
    "resetCountsReviewer",
    bool,
    ConfigValue::Bool
);
setter!(set_reset_counts_browser, "resetCountsBrowser", bool, ConfigValue::Bool);
setter!(set_sched2021, "sched2021", bool, ConfigValue::Bool);
// nextPos and the day markers retain their single existing engine writers.
