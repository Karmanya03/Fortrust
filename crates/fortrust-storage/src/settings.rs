use dashmap::DashMap;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

use crate::StorageError;

const SETTINGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("settings");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Json(serde_json::Value),
}

impl SettingValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v.as_str()),
            _ => None,
        }
    }
}

impl From<bool> for SettingValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for SettingValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for SettingValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<i64> for SettingValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    values: DashMap<String, SettingValue>,
}

impl SettingsStore {
    pub fn new() -> Self {
        Self {
            values: DashMap::new(),
        }
    }

    pub fn set(&self, key: impl Into<String>, value: SettingValue) {
        self.values.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<SettingValue> {
        self.values.get(key).map(|r| r.clone())
    }

    pub fn remove(&self, key: &str) {
        self.values.remove(key);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn all(&self) -> Vec<(String, SettingValue)> {
        self.values
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    pub fn clear(&self) {
        self.values.clear();
    }
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SettingsDatabase {
    db: Option<Arc<Database>>,
    cache: DashMap<String, SettingValue>,
}

impl SettingsDatabase {
    pub fn new(db: Arc<Database>) -> Result<Self, StorageError> {
        let write_txn = db.begin_write()?;
        write_txn.open_table(SETTINGS_TABLE)?;
        write_txn.commit()?;

        let cache = DashMap::new();

        if let Ok(read_txn) = db.begin_read()
            && let Ok(table) = read_txn.open_table(SETTINGS_TABLE)
        {
            for result in table.iter()? {
                let Ok((key, value)) = result else {
                    continue;
                };
                if let Ok((setting, _)) = bincode::serde::decode_from_slice::<SettingValue, _>(
                    value.value(),
                    bincode::config::standard(),
                ) {
                    cache.insert(key.value().to_owned(), setting);
                }
            }
        }

        Ok(Self {
            db: Some(db),
            cache,
        })
    }

    pub fn empty() -> Self {
        Self {
            db: None,
            cache: DashMap::new(),
        }
    }

    pub fn store(&self, key: &str, value: &SettingValue) -> Result<(), StorageError> {
        let data = bincode::serde::encode_to_vec(value, bincode::config::standard())?;
        self.cache.insert(key.to_owned(), value.clone());

        if let Some(db) = &self.db {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(SETTINGS_TABLE)?;
                table.insert(key, data.as_slice())?;
            }
            write_txn.commit()?;
        }
        debug!("Stored setting: {key}");
        Ok(())
    }

    pub fn load(&self, key: &str) -> Option<SettingValue> {
        self.cache.get(key).map(|r| r.clone())
    }

    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.cache.remove(key);
        if let Some(db) = &self.db {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(SETTINGS_TABLE)?;
                table.remove(key)?;
            }
            write_txn.commit()?;
        }
        Ok(())
    }

    pub fn all(&self) -> Vec<(String, SettingValue)> {
        self.cache
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    pub fn count(&self) -> usize {
        self.cache.len()
    }
}
