//! CloudKit identity moves (K14.2, I5). No graves: this is an identity move,
//! and all reference changes belong to the caller's one journalled Local op.
use crate::{Cell, ConfigValue, ReadRows, Record, RowKey, StorageError, Table, WriteScope};
use meguri_model::{ids::NotetypeId, proto::deck::DeckKind};

impl WriteScope<'_> {
    /// Smallest physically free id strictly above `after`, within I5's cap.
    /// Tracking reservations are checked by the binding owner, without bodies.
    pub fn cloud_rekey_candidate(
        &self,
        table: Table,
        after: i64,
        cancelled: impl FnMut() -> bool,
    ) -> Result<i64, StorageError> {
        crate::ids::rekey_candidate(self.conn(), table, after, self.now().0, cancelled)
    }

    /// Move one allocated identity and its references. The caller checks
    /// cancellation between pages, using a local token (never a provider).
    pub fn cloud_rekey_id(
        &mut self,
        table: Table,
        old: i64,
        new: i64,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<(), StorageError> {
        if self.mode() != crate::StampMode::Local
            || !table.allocates_ids()
            || new <= old
            || matches!(table, Table::Decks | Table::DeckConfig) && old == 1
        {
            return Err(StorageError::Mode {
                detail: "invalid CloudKit identity move".into(),
            });
        }
        let cap = self
            .now()
            .0
            .checked_add(crate::ids::MAX_AHEAD_MS)
            .ok_or(StorageError::IdsExhausted { table: table.name() })?;
        if new > cap || self.row_image(table, &RowKey::Id(new))?.is_some() {
            return Err(StorageError::IdsExhausted { table: table.name() });
        }
        let mut row = self.row_image(table, &RowKey::Id(old))?.ok_or(StorageError::Mode {
            detail: "missing CloudKit identity".into(),
        })?;
        let old_deck_name = if table == Table::Decks {
            Some(meguri_model::deck::Deck::from_cells(&row)?.name)
        } else {
            None
        };
        let review_card = if table == Table::Revlog {
            let Cell::Integer(cid) = row[1] else {
                return Err(bad_reference());
            };
            (self.max_revlog_id(meguri_model::ids::CardId(cid))? == old).then_some(cid)
        } else {
            None
        };
        self.remove_key(table, &RowKey::Id(old))?;
        row[0] = Cell::Integer(new);
        self.put_cells(table, row, false)?;
        if let Some(cid) = review_card {
            let card = self.card(meguri_model::ids::CardId(cid))?.ok_or_else(bad_reference)?;
            self.put_card_forced(&card)?;
        }
        if table == Table::Notetypes {
            for part in [Table::Fields, Table::Templates] {
                let mut after = None;
                loop {
                    check_cancel(&mut cancelled)?;
                    let page = if part == Table::Fields {
                        self.notetype_fields_page(NotetypeId(old), after)?
                    } else {
                        self.notetype_templates_page(NotetypeId(old), after)?
                    };
                    if page.is_empty() {
                        break;
                    }
                    for mut row in page {
                        let Cell::Integer(ord) = row[1] else {
                            return Err(bad_reference());
                        };
                        after = Some(ord);
                        self.remove_key(part, &RowKey::Pair(old, ord))?;
                        row[0] = Cell::Integer(new);
                        self.put_cells(part, row, false)?;
                    }
                }
            }
        }
        let refs: &[(Table, &[usize])] = match table {
            Table::Notes => &[(Table::Cards, &[1])],
            Table::Cards => &[(Table::Revlog, &[1])],
            Table::Decks => &[(Table::Cards, &[2, 15])],
            Table::Notetypes => &[(Table::Notes, &[2])],
            _ => &[],
        };
        for &(reference, columns) in refs {
            let mut after = None;
            loop {
                check_cancel(&mut cancelled)?;
                let page = identity_page(self, reference, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                for mut row in page {
                    after = Some(RowKey::of(reference, &row));
                    let mut changed = false;
                    for &column in columns {
                        if row[column] == Cell::Integer(old) {
                            row[column] = Cell::Integer(new);
                            changed = true;
                        }
                    }
                    if changed {
                        self.put_cells(reference, row, false)?;
                    }
                }
            }
        }
        if table == Table::DeckConfig {
            let mut after = None;
            loop {
                check_cancel(&mut cancelled)?;
                let page = identity_page(self, Table::Decks, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                for row in page {
                    after = Some(RowKey::of(Table::Decks, &row));
                    let mut deck = meguri_model::deck::Deck::from_cells(&row)?;
                    if let DeckKind::Normal(normal) = &mut deck.kind.kind
                        && normal.config_id == old
                    {
                        normal.config_id = new;
                        self.put(&deck)?;
                    }
                }
            }
        }
        if let Some(parent_name) = old_deck_name {
            // Parent ids in cloud deck payloads are derived from native paths.
            // Stamp direct children too so that their changed parent identity
            // is a real journal target and a pending reference, not a clean
            // stale payload held forever under the previous parent id.
            let mut after = None;
            loop {
                check_cancel(&mut cancelled)?;
                let page = identity_page(self, Table::Decks, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                for row in page {
                    after = Some(RowKey::of(Table::Decks, &row));
                    let deck = meguri_model::deck::Deck::from_cells(&row)?;
                    if deck
                        .name
                        .rsplit_once('\u{1f}')
                        .is_some_and(|(parent, _)| meguri_text::fold::compare_folded(parent, &parent_name).is_eq())
                    {
                        self.put_deck_forced(&deck)?;
                    }
                }
            }
        }
        if table == Table::Decks {
            let mut after = None;
            loop {
                check_cancel(&mut cancelled)?;
                let page = identity_page(self, Table::Notetypes, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                for row in page {
                    after = Some(RowKey::of(Table::Notetypes, &row));
                    let Cell::Integer(id) = row[0] else {
                        return Err(bad_reference());
                    };
                    let mut notetype = self.notetype(NotetypeId(id))?.ok_or_else(bad_reference)?;
                    let mut changed = false;
                    for template in &mut notetype.templates {
                        if template.config.target_deck_id == old {
                            template.config.target_deck_id = new;
                            changed = true;
                        }
                    }
                    if changed {
                        self.put_notetype(&notetype)?;
                    }
                }
            }
        }
        if matches!(table, Table::Decks | Table::Notetypes) {
            self.cloud_rekey_config(table, old, new, &mut cancelled)?;
        }
        check_cancel(&mut cancelled)
    }

    fn cloud_rekey_config(
        &mut self,
        table: Table,
        old: i64,
        new: i64,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<(), StorageError> {
        let deck = table == Table::Decks;
        let prefix = format!("_{}_{old}_", if deck { "deck" } else { "nt" });
        let replacement = format!("_{}_{new}_", if deck { "deck" } else { "nt" });
        let mut after = None;
        loop {
            check_cancel(cancelled)?;
            let bound = match &after {
                Some(RowKey::Text(key)) => Cell::Text(key.clone()),
                None => Cell::Null,
                _ => return Err(bad_reference()),
            };
            let page = crate::rows::select_where(
                self.conn(),
                Table::Config,
                "KEY IN (SELECT KEY FROM config WHERE (?1 IS NULL OR KEY > ?1) ORDER BY KEY LIMIT 1)",
                &[bound],
            )?;
            if page.is_empty() {
                break;
            }
            for row in page {
                after = Some(RowKey::of(Table::Config, &row));
                let entry = meguri_model::config::ConfigEntry::from_cells(&row)?;
                let key = &entry.key;
                let renamed = key.strip_prefix(&prefix).map(|tail| format!("{replacement}{tail}"));
                let single = if deck {
                    key == "curDeck" || numeric_family(key, "_nt_", "_lastDeck")
                } else {
                    key == "curModel" || numeric_family(key, "_deck_", "_lastNotetype")
                };
                let mut value = crate::rows::config::stored_value(&row[3]);
                let mut changed = false;
                if single && value.as_ref().and_then(ConfigValue::as_i64) == Some(old) {
                    value = Some(ConfigValue::from(new));
                    changed = true;
                } else if deck
                    && key == "activeDecks"
                    && let Some(ConfigValue::Array(items)) = &mut value
                {
                    for item in items {
                        if item.as_i64() == Some(old) {
                            *item = ConfigValue::from(new);
                            changed = true;
                        }
                    }
                }
                if let Some(renamed) = renamed {
                    if self.row_image(Table::Config, &RowKey::text(&renamed))?.is_some() {
                        return Err(bad_reference());
                    }
                    // Keep opaque values byte-for-byte when just moving a key.
                    let mut moved = row;
                    moved[0] = Cell::text(&renamed);
                    if changed {
                        moved[3] = Cell::Blob(value.ok_or_else(bad_reference)?.compact().into_bytes());
                    }
                    self.remove_config(key)?;
                    self.put_cells(Table::Config, moved, false)?;
                } else if changed {
                    self.set_config(key, &value.ok_or_else(bad_reference)?)?;
                }
            }
        }
        Ok(())
    }
}
fn numeric_family(key: &str, prefix: &str, suffix: &str) -> bool {
    key.strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(suffix))
        .is_some_and(|s| s.parse::<i64>().is_ok())
}
fn bad_reference() -> StorageError {
    StorageError::Mode {
        detail: "invalid CloudKit reference move".into(),
    }
}
fn check_cancel(cancelled: &mut impl FnMut() -> bool) -> Result<(), StorageError> {
    if cancelled() {
        Err(StorageError::Mode {
            detail: "CloudKit rekey cancelled".into(),
        })
    } else {
        Ok(())
    }
}

// Identity scans keep one body resident; journal images spill independently.
fn identity_page(
    scope: &WriteScope<'_>,
    table: Table,
    after: Option<&RowKey>,
) -> Result<Vec<crate::RowImage>, StorageError> {
    let bound = match after {
        Some(RowKey::Id(id)) => Cell::Integer(*id),
        None => Cell::Null,
        _ => return Err(bad_reference()),
    };
    crate::rows::select_where(
        scope.conn(),
        table,
        &format!(
            "id IN (SELECT id FROM {} WHERE (?1 IS NULL OR id > ?1) ORDER BY id LIMIT 1)",
            table.name()
        ),
        &[bound],
    )
}
