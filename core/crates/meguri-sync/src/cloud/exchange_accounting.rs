//! Tracking facts used at the original exchange completion.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

impl TrackingStore {
    pub(super) fn blocked_exchange_names(
        &self,
        binding: BindingId,
        selected: &BTreeSet<String>,
    ) -> Result<BTreeSet<String>, Error> {
        if selected.is_empty() {
            return Ok(BTreeSet::new());
        }
        let mut blocked: BTreeSet<_> = self
            .inbox(binding)?
            .into_iter()
            .filter(|entry| entry.entry.reason == InboxReason::Format)
            .map(|entry| entry.entry.record_name)
            .collect();
        blocked.extend(
            self.refused_entries(binding)?
                .into_iter()
                .map(|entry| entry.record_name),
        );
        let media = self.media_ledger(binding)?;
        let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut visited = BTreeSet::new();
        let mut pending: VecDeque<_> = selected.iter().cloned().collect();
        while let Some(name) = pending.pop_front() {
            if !visited.insert(name.clone()) || blocked.contains(&name) {
                continue;
            }
            if self.row_failed(binding, &name)? { blocked.insert(name); continue; }
            let Some(header) = self.entity_header(binding, &name)? else {
                continue;
            };
            if header.facts.kind == entity_codec::Kind::MediaName {
                let key = std::str::from_utf8(&header.facts.key).map_err(|_| Error::InvalidData)?;
                if media
                    .names
                    .get(key)
                    .is_some_and(|flags| flags.unsyncable || flags.removal_held || flags.file_missing)
                {
                    blocked.insert(name);
                }
                continue;
            }
            if header.facts.server_erev > 0 && header.facts.mark.is_none() || header.facts.tombstone_at.is_some() {
                continue;
            }
            let payload = header.local_payload(&self.connection, binding)?;
            for reference in self.collection_references(binding, &payload)? {
                parents.entry(reference.clone()).or_default().push(name.clone());
                pending.push_back(reference);
            }
        }
        let mut pending: VecDeque<_> = blocked.iter().cloned().collect();
        while let Some(name) = pending.pop_front() {
            for parent in parents.get(&name).into_iter().flatten() {
                if blocked.insert(parent.clone()) {
                    pending.push_back(parent.clone());
                }
            }
        }
        blocked.retain(|name| selected.contains(name));
        Ok(blocked)
    }
}
