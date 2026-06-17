use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::StorageError;

const DB_META: TableDefinition<&str, &str> = TableDefinition::new("idb_meta");
const DB_STORES: TableDefinition<&str, &str> = TableDefinition::new("idb_stores");
const DB_INDEXES: TableDefinition<&str, &str> = TableDefinition::new("idb_indexes");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdbDatabase {
    pub name: String,
    pub version: u32,
    pub store_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdbObjectStore {
    pub name: String,
    pub key_path: Option<String>,
    pub auto_increment: bool,
    pub index_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdbIndex {
    pub name: String,
    pub key_path: String,
    pub unique: bool,
    pub multi_entry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdbRecord {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdbIndexEntry {
    pub index_value: String,
    pub primary_key: String,
}

#[derive(Clone)]
pub struct IndexedDbStore {
    db: Arc<Database>,
    cache: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl IndexedDbStore {
    pub fn new(database: Arc<Database>) -> Result<Self, StorageError> {
        let write_txn = database.begin_write()?;
        {
            let _ = write_txn.open_table(DB_META)?;
            let _ = write_txn.open_table(DB_STORES)?;
            let _ = write_txn.open_table(DB_INDEXES)?;
        }
        write_txn.commit()?;
        debug!("IndexedDB tables opened");
        Ok(Self { db: database, cache: Arc::new(Mutex::new(HashMap::new())) })
    }

    pub fn list_databases(&self) -> Result<Vec<String>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DB_META)?;
        let mut names = Vec::new();
        for result in table.iter()? {
            let (k, _) = result?;
            names.push(k.value().to_owned());
        }
        Ok(names)
    }

    pub fn get_database(&self, name: &str) -> Result<Option<IdbDatabase>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DB_META)?;
        match table.get(name)? {
            Some(val) => {
                let db: IdbDatabase = serde_json::from_str(val.value())
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(db))
            }
            None => Ok(None),
        }
    }

    pub fn put_database(&self, database: &IdbDatabase) -> Result<(), StorageError> {
        let json = serde_json::to_string(database)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DB_META)?;
            table.insert(database.name.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn delete_database(&self, name: &str) -> Result<(), StorageError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut meta = write_txn.open_table(DB_META)?;
            meta.remove(name)?;
            let mut stores = write_txn.open_table(DB_STORES)?;
            let prefix = format!("{}:", name);
            let to_remove: Vec<String> = stores
                .iter()?
                .filter_map(|r| r.ok())
                .filter(|(k, _)| k.value().starts_with(&prefix))
                .map(|(k, _)| k.value().to_owned())
                .collect();
            for key in to_remove {
                stores.remove(key.as_str())?;
            }
            let mut indexes = write_txn.open_table(DB_INDEXES)?;
            let idx_prefix = format!("{}:", name);
            let idx_to_remove: Vec<String> = indexes
                .iter()?
                .filter_map(|r| r.ok())
                .filter(|(k, _)| k.value().starts_with(&idx_prefix))
                .map(|(k, _)| k.value().to_owned())
                .collect();
            for key in idx_to_remove {
                indexes.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    fn store_key(db_name: &str, store_name: &str, key: &str) -> String {
        format!("{}:{}:{}", db_name, store_name, key)
    }

    fn index_key(db_name: &str, store_name: &str, index_name: &str) -> String {
        format!("idx:{}:{}:{}", db_name, store_name, index_name)
    }

    fn store_prefix(db_name: &str, store_name: &str) -> String {
        format!("{}:{}:", db_name, store_name)
    }

    pub fn put_record(
        &self,
        db_name: &str,
        store_name: &str,
        key: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        let sk = Self::store_key(db_name, store_name, key);
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DB_STORES)?;
            table.insert(sk.as_str(), value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn get_record(
        &self,
        db_name: &str,
        store_name: &str,
        key: &str,
    ) -> Result<Option<String>, StorageError> {
        let sk = Self::store_key(db_name, store_name, key);
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DB_STORES)?;
        Ok(table.get(sk.as_str())?.map(|v| v.value().to_owned()))
    }

    pub fn delete_record(
        &self,
        db_name: &str,
        store_name: &str,
        key: &str,
    ) -> Result<(), StorageError> {
        let sk = Self::store_key(db_name, store_name, key);
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DB_STORES)?;
            table.remove(sk.as_str())?;
        }
        write_txn.commit()?;
        self.clear_index_entries(db_name, store_name, key)?;
        Ok(())
    }

    pub fn clear_store(&self, db_name: &str, store_name: &str) -> Result<(), StorageError> {
        let prefix = Self::store_prefix(db_name, store_name);
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DB_STORES)?;
            let to_remove: Vec<String> = table
                .iter()?
                .filter_map(|r| r.ok())
                .filter(|(k, _)| k.value().starts_with(&prefix))
                .map(|(k, _)| k.value().to_owned())
                .collect();
            for key in to_remove {
                table.remove(key.as_str())?;
            }
            let mut indexes = write_txn.open_table(DB_INDEXES)?;
            let idx_prefix = format!("{}:{}:", db_name, store_name);
            let idx_to_remove: Vec<String> = indexes
                .iter()?
                .filter_map(|r| r.ok())
                .filter(|(k, _)| k.value().starts_with(&idx_prefix))
                .map(|(k, _)| k.value().to_owned())
                .collect();
            for key in idx_to_remove {
                indexes.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn get_all_records(
        &self,
        db_name: &str,
        store_name: &str,
    ) -> Result<Vec<IdbRecord>, StorageError> {
        let prefix = Self::store_prefix(db_name, store_name);
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DB_STORES)?;
        let mut records = Vec::new();
        for result in table.iter()? {
            let (k, v) = result?;
            let key_str = k.value();
            if key_str.starts_with(&prefix) {
                let record_key = key_str[prefix.len()..].to_owned();
                records.push(IdbRecord {
                    key: record_key,
                    value: v.value().to_owned(),
                });
            }
        }
        Ok(records)
    }

    pub fn put_index_entry(
        &self,
        db_name: &str,
        store_name: &str,
        index_name: &str,
        index_value: &str,
        primary_key: &str,
    ) -> Result<(), StorageError> {
        let ik = format!(
            "{}:{}",
            Self::index_key(db_name, store_name, index_name),
            index_value
        );
        let entry = IdbIndexEntry {
            index_value: index_value.to_owned(),
            primary_key: primary_key.to_owned(),
        };
        let json = serde_json::to_string(&entry)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DB_INDEXES)?;
            table.insert(ik.as_str(), json.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn query_index(
        &self,
        db_name: &str,
        store_name: &str,
        index_name: &str,
        index_value: &str,
    ) -> Result<Vec<String>, StorageError> {
        let prefix = format!(
            "{}:{}",
            Self::index_key(db_name, store_name, index_name),
            index_value
        );
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DB_INDEXES)?;
        let mut keys = Vec::new();
        for result in table.iter()? {
            let (k, v) = result?;
            if k.value().starts_with(&prefix) {
                let entry: IdbIndexEntry = serde_json::from_str(v.value())
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                keys.push(entry.primary_key);
            }
        }
        Ok(keys)
    }

    fn clear_index_entries(
        &self,
        db_name: &str,
        store_name: &str,
        primary_key: &str,
    ) -> Result<(), StorageError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DB_INDEXES)?;
            let idx_prefix = format!("{}:{}:", db_name, store_name);
            let to_remove: Vec<String> = table
                .iter()?
                .filter_map(|r| r.ok())
                .filter(|(k, v)| {
                    k.value().starts_with(&idx_prefix)
                        && v.value().contains(primary_key)
                })
                .map(|(k, _)| k.value().to_owned())
                .collect();
            for key in to_remove {
                table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn flush(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}