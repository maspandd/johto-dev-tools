//! Durable storage for CRUD resource records.
//!
//! The runtime loads each resource's records once at start and then applies
//! every [`RecordChange`] in order from a background writer, so responses
//! never wait on disk or database I/O. Hosts plug in their own store (for
//! example the SQLite/PostgreSQL store) by implementing [`RecordStore`].

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

use workbench_core::ResourceId;
use workbench_domain::{RecordChange, StoredRecord};

pub type RecordStoreError = Box<dyn std::error::Error + Send + Sync>;
pub type RecordFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RecordStoreError>> + Send + 'a>>;

pub trait RecordStore: Send + Sync + 'static {
    /// The resource's records in display order, or `None` if the resource
    /// has never been initialized (it will then be seeded).
    fn load(&self, resource: ResourceId) -> RecordFuture<'_, Option<Vec<StoredRecord>>>;

    /// Applies one change. Changes arrive in the order they happened.
    fn apply(&self, resource: ResourceId, change: RecordChange) -> RecordFuture<'_, ()>;
}

type Records = BTreeMap<String, StoredRecord>;

fn apply_to(records: &mut Records, change: RecordChange) {
    match change {
        RecordChange::Upsert { record } => {
            records.insert(record.key.clone(), record);
        }
        RecordChange::Delete { key } => {
            records.remove(&key);
        }
        RecordChange::Replace { records: all } => {
            *records = all.into_iter().map(|r| (r.key.clone(), r)).collect();
        }
    }
}

fn ordered(records: &Records) -> Vec<StoredRecord> {
    let mut list: Vec<StoredRecord> = records.values().cloned().collect();
    list.sort_by_key(|r| r.position);
    list
}

/// Keeps records in memory. Data survives server restarts as long as the
/// same store instance is reused, but not process restarts.
#[derive(Debug, Default)]
pub struct MemoryRecordStore {
    data: Mutex<HashMap<ResourceId, Records>>,
}

impl MemoryRecordStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl RecordStore for MemoryRecordStore {
    fn load(&self, resource: ResourceId) -> RecordFuture<'_, Option<Vec<StoredRecord>>> {
        let result = self
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&resource)
            .map(ordered);
        Box::pin(async move { Ok(result) })
    }

    fn apply(&self, resource: ResourceId, change: RecordChange) -> RecordFuture<'_, ()> {
        let mut data = self.data.lock().unwrap_or_else(PoisonError::into_inner);
        apply_to(data.entry(resource).or_default(), change);
        Box::pin(async { Ok(()) })
    }
}

/// One JSON file per resource (`<resource-id>.json`) in a directory.
/// Files are replaced atomically on every change. Suited to the CLI and
/// small local projects.
#[derive(Debug)]
pub struct JsonFileRecordStore {
    dir: PathBuf,
    cache: Mutex<HashMap<ResourceId, Records>>,
}

impl JsonFileRecordStore {
    /// Creates the directory if needed.
    pub fn open(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            cache: Mutex::default(),
        })
    }

    #[must_use]
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn path(&self, resource: ResourceId) -> PathBuf {
        self.dir.join(format!("{resource}.json"))
    }

    fn read(&self, resource: ResourceId) -> Result<Option<Records>, RecordStoreError> {
        let path = self.path(resource);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let list: Vec<StoredRecord> = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                Ok(Some(list.into_iter().map(|r| (r.key.clone(), r)).collect()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display()).into()),
        }
    }

    fn write(&self, resource: ResourceId, records: &Records) -> Result<(), RecordStoreError> {
        let path = self.path(resource);
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&ordered(records))?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn load_sync(
        &self,
        resource: ResourceId,
    ) -> Result<Option<Vec<StoredRecord>>, RecordStoreError> {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(records) = cache.get(&resource) {
            return Ok(Some(ordered(records)));
        }
        let loaded = self.read(resource)?;
        let result = loaded.as_ref().map(ordered);
        if let Some(records) = loaded {
            cache.insert(resource, records);
        }
        Ok(result)
    }

    fn apply_sync(
        &self,
        resource: ResourceId,
        change: RecordChange,
    ) -> Result<(), RecordStoreError> {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        let records = match cache.entry(resource) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(self.read(resource)?.unwrap_or_default())
            }
        };
        apply_to(records, change);
        self.write(resource, records)
    }
}

impl RecordStore for JsonFileRecordStore {
    fn load(&self, resource: ResourceId) -> RecordFuture<'_, Option<Vec<StoredRecord>>> {
        let result = self.load_sync(resource);
        Box::pin(async move { result })
    }

    fn apply(&self, resource: ResourceId, change: RecordChange) -> RecordFuture<'_, ()> {
        let result = self.apply_sync(resource, change);
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rec(key: &str, position: i64) -> StoredRecord {
        StoredRecord {
            key: key.into(),
            position,
            data: json!({ "id": key }),
        }
    }

    async fn exercise(store: &dyn RecordStore) {
        let id = ResourceId::new();
        assert_eq!(store.load(id).await.unwrap(), None, "never initialized");
        store
            .apply(
                id,
                RecordChange::Replace {
                    records: vec![rec("b", 1), rec("a", 0)],
                },
            )
            .await
            .unwrap();
        store
            .apply(
                id,
                RecordChange::Upsert {
                    record: rec("c", 2),
                },
            )
            .await
            .unwrap();
        store
            .apply(id, RecordChange::Delete { key: "a".into() })
            .await
            .unwrap();
        let keys: Vec<String> = store
            .load(id)
            .await
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|r| r.key)
            .collect();
        assert_eq!(keys, ["b", "c"]);

        let empty = ResourceId::new();
        store
            .apply(empty, RecordChange::Replace { records: vec![] })
            .await
            .unwrap();
        assert_eq!(
            store.load(empty).await.unwrap(),
            Some(vec![]),
            "initialized but empty"
        );
    }

    #[tokio::test]
    async fn memory_store() {
        exercise(&MemoryRecordStore::new()).await;
    }

    #[tokio::test]
    async fn json_file_store_persists_across_instances() {
        let dir = std::env::temp_dir().join(format!("wb-records-{}", uuid::Uuid::new_v4()));
        exercise(&JsonFileRecordStore::open(&dir).unwrap()).await;

        let id = ResourceId::new();
        let first = JsonFileRecordStore::open(&dir).unwrap();
        first
            .apply(
                id,
                RecordChange::Replace {
                    records: vec![rec("x", 0)],
                },
            )
            .await
            .unwrap();
        drop(first);
        let second = JsonFileRecordStore::open(&dir).unwrap();
        assert_eq!(second.load(id).await.unwrap(), Some(vec![rec("x", 0)]));
        std::fs::remove_dir_all(&dir).ok();
    }
}
