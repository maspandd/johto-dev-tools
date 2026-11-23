//! In-memory record collections for CRUD resources.
//!
//! The runtime loads collections from persistent storage before serving
//! and persists the [`RecordChange`](workbench_domain::RecordChange)s that
//! CRUD requests emit. One `DataStore` outlives engine rebuilds, so a
//! reload never loses data.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::Value;
use workbench_core::ResourceId;
use workbench_domain::{RecordChange, StoredRecord};

/// Records of one resource, kept in display order.
#[derive(Debug, Default, Clone)]
pub(crate) struct Collection {
    order: BTreeMap<i64, String>,
    records: HashMap<String, (i64, Value)>,
}

impl Collection {
    fn from_stored(records: Vec<StoredRecord>) -> Self {
        let mut c = Self::default();
        for r in records {
            c.upsert(r.key, r.position, r.data);
        }
        c
    }

    /// `(key, record)` pairs in display order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.order
            .values()
            .filter_map(|k| self.records.get(k).map(|(_, v)| (k.as_str(), v)))
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.records.get(key).map(|(_, v)| v)
    }

    pub(crate) fn position(&self, key: &str) -> Option<i64> {
        self.records.get(key).map(|(p, _)| *p)
    }

    pub(crate) fn next_position(&self) -> i64 {
        self.order.keys().next_back().map_or(0, |p| p + 1)
    }

    pub(crate) fn upsert(&mut self, key: String, position: i64, value: Value) {
        if let Some((old, _)) = self.records.get(&key) {
            self.order.remove(old);
        }
        self.order.insert(position, key.clone());
        self.records.insert(key, (position, value));
    }

    pub(crate) fn remove(&mut self, key: &str) {
        if let Some((pos, _)) = self.records.remove(key) {
            self.order.remove(&pos);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    fn to_stored(&self) -> Vec<StoredRecord> {
        self.order
            .iter()
            .filter_map(|(pos, key)| {
                self.records.get(key).map(|(_, v)| StoredRecord {
                    key: key.clone(),
                    position: *pos,
                    data: v.clone(),
                })
            })
            .collect()
    }
}

/// Called with every record change while the data lock is held, so
/// listeners observe changes in exactly the order they were applied.
pub type ChangeListener = Box<dyn Fn(ResourceId, &RecordChange) + Send + Sync>;

/// Every resource's records, shared by the engine and the runtime.
#[derive(Default)]
pub struct DataStore {
    collections: Mutex<HashMap<ResourceId, Collection>>,
    listener: Option<ChangeListener>,
}

impl std::fmt::Debug for DataStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataStore")
            .field("resources", &self.lock().len())
            .field("listener", &self.listener.is_some())
            .finish()
    }
}

impl DataStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A store that reports every change (for persistence).
    #[must_use]
    pub fn with_listener(listener: ChangeListener) -> Self {
        Self {
            collections: Mutex::default(),
            listener: Some(listener),
        }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, HashMap<ResourceId, Collection>> {
        self.collections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Reports changes; call while holding the guard from [`Self::lock`].
    pub(crate) fn notify(&self, changes: &[(ResourceId, RecordChange)]) {
        if let Some(listener) = &self.listener {
            for (id, change) in changes {
                listener(*id, change);
            }
        }
    }

    /// Whether the resource's records have been loaded.
    #[must_use]
    pub fn contains(&self, id: ResourceId) -> bool {
        self.lock().contains_key(&id)
    }

    /// Sets a resource's records from persistent storage, without
    /// reporting a change.
    pub fn load(&self, id: ResourceId, records: Vec<StoredRecord>) {
        self.lock().insert(id, Collection::from_stored(records));
    }

    /// Replaces a resource's records (seeding or reset) and reports a
    /// [`RecordChange::Replace`].
    pub fn replace(&self, id: ResourceId, records: Vec<StoredRecord>) {
        let mut guard = self.lock();
        guard.insert(id, Collection::from_stored(records.clone()));
        self.notify(&[(id, RecordChange::Replace { records })]);
        drop(guard);
    }

    /// The resource's records in display order, or `None` if not loaded.
    #[must_use]
    pub fn records(&self, id: ResourceId) -> Option<Vec<Value>> {
        self.lock()
            .get(&id)
            .map(|c| c.iter().map(|(_, v)| v.clone()).collect())
    }

    /// The resource's records with keys and positions.
    #[must_use]
    pub fn stored(&self, id: ResourceId) -> Option<Vec<StoredRecord>> {
        self.lock().get(&id).map(Collection::to_stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_display_order_across_updates() {
        let mut c = Collection::default();
        c.upsert("a".into(), 0, json!(1));
        c.upsert("b".into(), 1, json!(2));
        c.upsert("a".into(), 0, json!(3));
        c.upsert("c".into(), c.next_position(), json!(4));
        assert_eq!(
            c.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert_eq!(c.get("a"), Some(&json!(3)));
        c.remove("b");
        assert_eq!(c.len(), 2);
        assert_eq!(c.next_position(), 3);
        assert_eq!(c.position("c"), Some(2));
    }
}
