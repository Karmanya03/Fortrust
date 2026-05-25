use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

use crate::StorageError;

const HISTORY_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("history");
const HISTORY_INDEX_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("history_by_time");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visit_time: DateTime<Utc>,
    pub visit_count: u32,
    pub typed_count: u32,
    pub is_bookmarked: bool,
}

#[derive(Debug, Clone)]
pub struct HistoryQuery {
    pub query: String,
    pub limit: usize,
    pub offset: usize,
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
}

impl HistoryQuery {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: 100,
            offset: 0,
            from_date: None,
            to_date: None,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_date_range(mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        self.from_date = Some(from);
        self.to_date = Some(to);
        self
    }
}

#[derive(Debug, Clone)]
pub struct HistoryStore {
    entries: Vec<HistoryEntry>,
}

impl HistoryStore {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add_visit(&mut self, url: String, title: String) {
        let now = Utc::now();
        if let Some(existing) = self.entries.iter_mut().find(|entry| entry.url == url) {
            existing.visit_count = existing.visit_count.saturating_add(1);
            existing.visit_time = now;
            if !title.is_empty() {
                existing.title = title;
            }
        } else {
            self.entries.push(HistoryEntry {
                url,
                title,
                visit_time: now,
                visit_count: 1,
                typed_count: 0,
                is_bookmarked: false,
            });
        }
    }

    pub fn search(&self, query: &HistoryQuery) -> Vec<HistoryEntry> {
        let query_lower = query.query.to_ascii_lowercase();
        let mut results: Vec<HistoryEntry> = self
            .entries
            .iter()
            .filter(|entry| {
                if !query_lower.is_empty() {
                    entry.url.to_ascii_lowercase().contains(&query_lower)
                        || entry.title.to_ascii_lowercase().contains(&query_lower)
                } else {
                    true
                }
            })
            .filter(|entry| {
                if let Some(from) = query.from_date
                    && entry.visit_time < from
                {
                    return false;
                }
                if let Some(to) = query.to_date
                    && entry.visit_time > to
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| b.visit_time.cmp(&a.visit_time));
        let start = query.offset.min(results.len());
        let end = (query.offset + query.limit).min(results.len());
        results[start..end].to_vec()
    }
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HistoryDatabase {
    db: Option<Arc<Database>>,
}

impl HistoryDatabase {
    pub fn new(db: Arc<Database>) -> Result<Self, StorageError> {
        let write_txn = db.begin_write()?;
        write_txn.open_table(HISTORY_TABLE)?;
        write_txn.open_table(HISTORY_INDEX_TABLE)?;
        write_txn.commit()?;
        Ok(Self { db: Some(db) })
    }

    pub fn empty() -> Self {
        Self { db: None }
    }

    pub fn store(&self, entry: &HistoryEntry) -> Result<(), StorageError> {
        let Some(db) = &self.db else {
            return Ok(());
        };

        let data = bincode::serde::encode_to_vec(entry, bincode::config::standard())?;
        let key = entry.url.as_bytes();
        let timestamp_key = entry.visit_time.timestamp() as u64;

        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;
            table.insert(key, data.as_slice())?;

            let mut index = write_txn.open_table(HISTORY_INDEX_TABLE)?;
            index.insert(timestamp_key, key)?;
        }
        write_txn.commit()?;
        debug!("Stored history entry: {}", entry.url);
        Ok(())
    }

    pub fn search(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>, StorageError> {
        let Some(db) = &self.db else {
            return Ok(Vec::new());
        };

        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(HISTORY_TABLE)?;
        let query_lower = query.query.to_ascii_lowercase();

        let mut results = Vec::new();
        for result in table.iter()? {
            let Ok((key, value)) = result else {
                continue;
            };
            let url = String::from_utf8_lossy(key.value()).to_string();

            if !query_lower.is_empty() && !url.to_ascii_lowercase().contains(&query_lower) {
                continue;
            }

            if let Ok((entry, _)) = bincode::serde::decode_from_slice::<HistoryEntry, _>(
                value.value(),
                bincode::config::standard(),
            ) && query.from_date.is_none_or(|d| entry.visit_time >= d)
                && query.to_date.is_none_or(|d| entry.visit_time <= d)
            {
                results.push(entry);
            }
        }

        results.sort_by(|a, b| b.visit_time.cmp(&a.visit_time));
        let start = query.offset.min(results.len());
        let end = (query.offset + query.limit).min(results.len());
        results.truncate(end);
        if start > 0 {
            results.drain(0..start);
        }
        Ok(results)
    }

    pub fn recently_visited(&self, limit: usize) -> Result<Vec<HistoryEntry>, StorageError> {
        self.search(&HistoryQuery::new("").with_limit(limit))
    }

    pub fn count(&self) -> usize {
        let Some(db) = &self.db else {
            return 0;
        };
        let Ok(read_txn) = db.begin_read() else {
            return 0;
        };
        let Ok(table) = read_txn.open_table(HISTORY_TABLE) else {
            return 0;
        };
        let Ok(range) = table.iter() else {
            return 0;
        };
        range.count()
    }

    pub fn clear(&self) -> Result<(), StorageError> {
        let Some(db) = &self.db else {
            return Ok(());
        };
        let write_txn = db.begin_write()?;
        write_txn.delete_table(HISTORY_TABLE)?;
        write_txn.delete_table(HISTORY_INDEX_TABLE)?;
        write_txn.commit()?;
        debug!("History cleared");
        Ok(())
    }

    pub fn delete_entry(&self, url: &str) -> Result<(), StorageError> {
        let Some(db) = &self.db else {
            return Ok(());
        };
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;
            table.remove(url.as_bytes())?;
        }
        write_txn.commit()?;
        Ok(())
    }
}
