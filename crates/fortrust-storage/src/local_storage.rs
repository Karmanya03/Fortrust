use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use tracing::debug;

use crate::StorageError;

const TABLE: TableDefinition<&str, &str> = TableDefinition::new("local_storage");

/// Maximum total bytes for localStorage per database (5 MB).
const MAX_STORAGE_BYTES: usize = 5 * 1024 * 1024;

/// Error returned when a localStorage write would exceed the quota.
#[derive(Debug, Clone)]
pub struct QuotaExceededError;

impl std::fmt::Display for QuotaExceededError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "localStorage quota exceeded (max {} bytes)", MAX_STORAGE_BYTES)
    }
}

#[derive(Clone)]
pub struct LocalStorageStore {
    db: Arc<Database>,
}

impl LocalStorageStore {
    pub fn new(db: Arc<Database>) -> Result<Self, StorageError> {
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(TABLE)?;
        }
        write_txn.commit()?;
        debug!("LocalStorage table opened");
        Ok(Self { db })
    }

    /// Returns the total byte usage of all keys and values.
    fn total_bytes(&self) -> Result<usize, StorageError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TABLE)?;
        let mut total = 0usize;
        for result in table.iter()? {
            let (k, v) = result?;
            total += k.value().len() + v.value().len();
        }
        Ok(total)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TABLE)?;
        Ok(table.get(key)?.map(|v| v.value().to_owned()))
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        // Check quota before writing
        let old_value_len = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(TABLE)?;
            table.get(key).ok().flatten().map(|v| v.value().len()).unwrap_or(0)
        };
        let current_total = self.total_bytes()?;
        let new_bytes = key.len() + value.len();
        let old_bytes = key.len() + old_value_len;
        let total_after = current_total + new_bytes - old_bytes;
        if total_after > MAX_STORAGE_BYTES {
            return Err(StorageError::QuotaExceeded(format!(
                "localStorage write would exceed {} byte limit", MAX_STORAGE_BYTES
            )));
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn remove(&self, key: &str) -> Result<(), StorageError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE)?;
            table.remove(key)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), StorageError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE)?;
            table.retain(|_, _| false)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn len(&self) -> Result<usize, StorageError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TABLE)?;
        Ok(table.iter()?.count())
    }

    pub fn is_empty(&self) -> Result<bool, StorageError> {
        self.len().map(|l| l == 0)
    }

    pub fn key_at(&self, index: usize) -> Result<Option<String>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TABLE)?;
        Ok(table
            .iter()?
            .nth(index)
            .map(|r| r.map(|(k, _)| k.value().to_owned()))
            .transpose()?)
    }

    /// Returns the current total byte usage for quota monitoring.
    pub fn usage(&self) -> Result<usize, StorageError> {
        self.total_bytes()
    }

    /// Returns the maximum allowed bytes for quota monitoring.
    pub fn max_quota(&self) -> usize {
        MAX_STORAGE_BYTES
    }
}
