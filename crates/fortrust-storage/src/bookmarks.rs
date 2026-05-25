use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::StorageError;

const BOOKMARKS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("bookmarks");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub title: String,
    pub folder_id: Option<String>,
    pub added_at: DateTime<Utc>,
    pub last_visited: Option<DateTime<Utc>>,
    pub visit_count: u32,
    pub icon_data: Option<Vec<u8>>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub position: u32,
}

#[derive(Debug, Clone)]
pub struct BookmarkStore {
    bookmarks: DashMap<String, Bookmark>,
    #[allow(dead_code)]
    folders: DashMap<String, BookmarkFolder>,
}

impl BookmarkStore {
    pub fn new() -> Self {
        Self {
            bookmarks: DashMap::new(),
            folders: DashMap::new(),
        }
    }

    pub fn add(&mut self, bookmark: Bookmark) {
        self.bookmarks.insert(bookmark.id.clone(), bookmark);
    }

    pub fn remove(&mut self, id: &str) -> Option<Bookmark> {
        self.bookmarks.remove(id).map(|(_, b)| b)
    }

    pub fn get(&self, id: &str) -> Option<Bookmark> {
        self.bookmarks.get(id).map(|r| r.clone())
    }

    pub fn search(&self, query: &str) -> Vec<Bookmark> {
        let query_lower = query.to_ascii_lowercase();
        self.bookmarks
            .iter()
            .filter(|entry| {
                query_lower.is_empty()
                    || entry.url.to_ascii_lowercase().contains(&query_lower)
                    || entry.title.to_ascii_lowercase().contains(&query_lower)
            })
            .map(|r| r.clone())
            .collect()
    }

    pub fn all(&self) -> Vec<Bookmark> {
        self.bookmarks.iter().map(|r| r.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.bookmarks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bookmarks.is_empty()
    }
}

impl Default for BookmarkStore {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BookmarkDatabase {
    db: Option<Arc<Database>>,
    cache: DashMap<String, Bookmark>,
}

impl BookmarkDatabase {
    pub fn new(db: &Database) -> Result<Self, StorageError> {
        let write_txn = db.begin_write()?;
        write_txn.open_table(BOOKMARKS_TABLE)?;
        write_txn.commit()?;

        let arc_db = Arc::new(Database::create("")
            .map_err(|e| StorageError::Database(e.to_string()))?);
        let cache = DashMap::new();

        if let Ok(read_txn) = arc_db.begin_read()
            && let Ok(table) = read_txn.open_table(BOOKMARKS_TABLE)
        {
            for result in table.iter()? {
                let Ok((key, value)) = result else {
                    continue;
                };
                if let Ok((bookmark, _)) = bincode::serde::decode_from_slice::<Bookmark, _>(
                    value.value(),
                    bincode::config::standard(),
                ) {
                    cache.insert(String::from_utf8_lossy(key.value()).to_string(), bookmark);
                }
            }
        }

        Ok(Self {
            db: Some(arc_db),
            cache,
        })
    }

    pub fn empty() -> Self {
        Self {
            db: None,
            cache: DashMap::new(),
        }
    }

    pub fn store(&self, bookmark: &Bookmark) -> Result<(), StorageError> {
        let data = bincode::serde::encode_to_vec(bookmark, bincode::config::standard())?;
        self.cache.insert(bookmark.id.clone(), bookmark.clone());

        if let Some(db) = &self.db {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(BOOKMARKS_TABLE)?;
                table.insert(bookmark.url.as_bytes(), data.as_slice())?;
            }
            write_txn.commit()?;
        }
        debug!("Stored bookmark: {}", bookmark.url);
        Ok(())
    }

    pub fn delete(&self, url: &str) -> Result<(), StorageError> {
        self.cache.retain(|_, b| b.url != url);
        if let Some(db) = &self.db {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(BOOKMARKS_TABLE)?;
                table.remove(url.as_bytes())?;
            }
            write_txn.commit()?;
        }
        Ok(())
    }

    pub fn get_by_url(&self, url: &str) -> Option<Bookmark> {
        self.cache.iter().find(|r| r.url == url).map(|r| r.clone())
    }

    pub fn search(&self, query: &str) -> Vec<Bookmark> {
        let query_lower = query.to_ascii_lowercase();
        self.cache
            .iter()
            .filter(|entry| {
                query_lower.is_empty()
                    || entry.url.to_ascii_lowercase().contains(&query_lower)
                    || entry.title.to_ascii_lowercase().contains(&query_lower)
            })
            .map(|r| r.clone())
            .collect()
    }

    pub fn all(&self) -> Vec<Bookmark> {
        self.cache.iter().map(|r| r.clone()).collect()
    }

    pub fn count(&self) -> usize {
        self.cache.len()
    }

    pub fn clear(&self) -> Result<(), StorageError> {
        self.cache.clear();
        if let Some(db) = &self.db {
            let write_txn = db.begin_write()?;
            write_txn.delete_table(BOOKMARKS_TABLE)?;
            write_txn.commit()?;
        }
        Ok(())
    }
}
