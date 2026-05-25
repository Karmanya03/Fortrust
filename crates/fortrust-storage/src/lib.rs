pub mod bookmarks;
pub mod cookies;
pub mod history;
pub mod settings;

pub use bookmarks::{Bookmark, BookmarkDatabase, BookmarkFolder, BookmarkStore};
pub use cookies::{CookieDatabase, CookieJar, CookieKey, CookiePolicy, CookieValue};
pub use history::{HistoryDatabase, HistoryEntry, HistoryQuery, HistoryStore};
pub use settings::{SettingValue, SettingsDatabase, SettingsStore};

use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Entry not found: {0}")]
    NotFound(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

impl From<redb::Error> for StorageError {
    fn from(error: redb::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<redb::TableError> for StorageError {
    fn from(error: redb::TableError) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<redb::TransactionError> for StorageError {
    fn from(error: redb::TransactionError) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<redb::CommitError> for StorageError {
    fn from(error: redb::CommitError) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<redb::DatabaseError> for StorageError {
    fn from(error: redb::DatabaseError) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<redb::StorageError> for StorageError {
    fn from(error: redb::StorageError) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<redb::CompactionError> for StorageError {
    fn from(error: redb::CompactionError) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<bincode::error::EncodeError> for StorageError {
    fn from(error: bincode::error::EncodeError) -> Self {
        Self::Serialization(error.to_string())
    }
}

impl From<bincode::error::DecodeError> for StorageError {
    fn from(error: bincode::error::DecodeError) -> Self {
        Self::Serialization(error.to_string())
    }
}

const SCHEMA_VERSION: u32 = 1;
const SCHEMA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("__schema__");

pub struct StorageDatabase {
    #[allow(dead_code)]
    db: Arc<Database>,
    pub history: HistoryDatabase,
    pub bookmarks: BookmarkDatabase,
    pub cookies: CookieDatabase,
    pub settings: SettingsDatabase,
}

impl StorageDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let db_path = path.as_ref().to_path_buf();
        info!("Opening storage database at: {}", db_path.display());

        let db = Database::create(&db_path)?;
        let db = Arc::new(db);

        Self::init_schema(&db)?;

        let history = HistoryDatabase::new(Arc::clone(&db))?;
        let bookmarks = BookmarkDatabase::new(Arc::clone(&db))?;
        let cookies = CookieDatabase::new(Arc::clone(&db))?;
        let settings = SettingsDatabase::new(Arc::clone(&db))?;

        info!("Storage database opened successfully");
        Ok(Self {
            db,
            history,
            bookmarks,
            cookies,
            settings,
        })
    }

    pub fn open_or_default(path: impl AsRef<Path>) -> Self {
        Self::open(path).unwrap_or_else(|error| {
            warn!("Failed to open storage database: {error}, using in-memory fallback");
            let db = Arc::new(
                Database::create("")
                    .unwrap_or_else(|_| panic!("Failed to create in-memory database")),
            );
            Self {
                history: HistoryDatabase::empty(),
                bookmarks: BookmarkDatabase::empty(),
                cookies: CookieDatabase::empty(),
                settings: SettingsDatabase::empty(),
                db,
            }
        })
    }

    fn init_schema(db: &Database) -> Result<(), StorageError> {
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(SCHEMA_TABLE)?;
            if table.get("version").is_err() || table.get("version").ok().flatten().is_none() {
                table.insert("version", SCHEMA_VERSION.to_le_bytes().as_slice())?;
                table.insert(
                    "created_at",
                    chrono::Utc::now().to_rfc3339().into_bytes().as_slice(),
                )?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn compact(&self) -> Result<(), StorageError> {
        debug!("Storage database compact skipped (shared database)");
        Ok(())
    }

    pub fn stats(&self) -> StorageStats {
        StorageStats {
            history_count: self.history.count(),
            bookmark_count: self.bookmarks.count(),
            cookie_count: self.cookies.count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageStats {
    pub history_count: usize,
    pub bookmark_count: usize,
    pub cookie_count: usize,
}
