use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use tracing::debug;

use crate::StorageError;

const TABLE: TableDefinition<&str, &str> = TableDefinition::new("local_storage");

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

    pub fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TABLE)?;
        Ok(table.get(key)?.map(|v| v.value().to_owned()))
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
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
}
