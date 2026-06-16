use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct LocalSearchIndex {
    index: Arc<Index>,
    schema: Arc<Schema>,
    reader: IndexReader,
    writer: Arc<std::sync::Mutex<IndexWriter>>,
}

#[derive(Debug, Clone)]
pub struct IndexedDocument {
    pub url: String,
    pub title: String,
    pub content_snippet: String,
    pub visit_time: DateTime<Utc>,
    pub score: f32,
}

impl LocalSearchIndex {
    pub fn open_or_create(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut schema_builder = SchemaBuilder::new();
        schema_builder.add_text_field("url", STRING | STORED);
        schema_builder.add_text_field("title", TEXT | STORED);
        schema_builder.add_text_field("content", TEXT);
        let visit_time_options = DateOptions::default().set_stored();
        schema_builder.add_date_field("visit_time", visit_time_options);
        let schema = Arc::new(schema_builder.build());

        let index = if path.exists() {
            match Index::open_in_dir(&path) {
                Ok(idx) => idx,
                Err(e) => {
                    warn!("Failed to open existing search index at {}: {e}, creating new", path.display());
                    Index::create_in_dir(&path, (*schema).clone()).unwrap_or_else(|e| {
                        error!("Failed to create search index: {e}");
                        Index::create_from_tempdir((*schema).clone()).expect("Failed to create temp index")
                    })
                }
            }
        } else {
            std::fs::create_dir_all(&path).ok();
            Index::create_in_dir(&path, (*schema).clone()).unwrap_or_else(|e| {
                error!("Failed to create search index at {}: {e}, using temp dir", path.display());
                Index::create_from_tempdir((*schema).clone()).expect("Failed to create temp index")
            })
        };

        let writer = Arc::new(std::sync::Mutex::new(
            index.writer(64_000_000).expect("Failed to create index writer"),
        ));

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .expect("Failed to create index reader");

        info!("Local search index ready at {}", path.display());
        Self {
            index: Arc::new(index),
            schema,
            reader,
            writer,
        }
    }

    pub fn open_temp() -> Self {
        let mut schema_builder = SchemaBuilder::new();
        schema_builder.add_text_field("url", STRING | STORED);
        schema_builder.add_text_field("title", TEXT | STORED);
        schema_builder.add_text_field("content", TEXT);
        let visit_time_options = DateOptions::default().set_stored();
        schema_builder.add_date_field("visit_time", visit_time_options);
        let schema = Arc::new(schema_builder.build());

        let index = Index::create_from_tempdir((*schema).clone()).expect("Failed to create temp index");
        let writer = Arc::new(std::sync::Mutex::new(
            index.writer(64_000_000).expect("Failed to create index writer"),
        ));
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .expect("Failed to create index reader");

        Self { index: Arc::new(index), schema, reader, writer }
    }

    pub fn index_page(&self, url: &str, title: &str, content: &str, visit_time: DateTime<Utc>) {
        let url_field = self.schema.get_field("url").unwrap();
        let title_field = self.schema.get_field("title").unwrap();
        let content_field = self.schema.get_field("content").unwrap();
        let visit_time_field = self.schema.get_field("visit_time").unwrap();

        let ts = tantivy::DateTime::from_timestamp_secs(visit_time.timestamp());

        let content_trimmed = content.chars().take(100_000).collect::<String>();

        let mut writer = match self.writer.lock() {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to lock index writer: {e}");
                return;
            }
        };

        // Delete existing document with this URL before re-indexing
        let term = tantivy::Term::from_field_text(url_field, url);
        let _ = writer.delete_term(term);

        let result = writer.add_document(doc!(
            url_field => url,
            title_field => title,
            content_field => content_trimmed,
            visit_time_field => ts,
        ));

        if let Err(e) = result {
            error!("Failed to index page {url}: {e}");
        }

        if let Err(e) = writer.commit() {
            error!("Failed to commit index: {e}");
        }

        debug!("Indexed: {url}");
    }

    pub fn delete_page(&self, url: &str) {
        let url_field = self.schema.get_field("url").unwrap();
        let mut writer = match self.writer.lock() {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to lock index writer: {e}");
                return;
            }
        };
        let term = tantivy::Term::from_field_text(url_field, url);
        let _ = writer.delete_term(term);
        if let Err(e) = writer.commit() {
            error!("Failed to commit after delete: {e}");
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<IndexedDocument> {
        if query.trim().is_empty() {
            return Vec::new();
        }

        let searcher = self.reader.searcher();
        let title_field = self.schema.get_field("title").unwrap();
        let content_field = self.schema.get_field("content").unwrap();
        let url_field = self.schema.get_field("url").unwrap();
        let visit_time_field = self.schema.get_field("visit_time").unwrap();

        let query_parser = QueryParser::for_index(&self.index, vec![title_field, content_field, url_field]);

        let parsed = match query_parser.parse_query(query) {
            Ok(q) => q,
            Err(e) => {
                debug!("Failed to parse query '{query}': {e}");
                return Vec::new();
            }
        };

        let top_docs = match searcher.search(&parsed, &TopDocs::with_limit(limit)) {
            Ok(docs) => docs,
            Err(e) => {
                error!("Search failed: {e}");
                return Vec::new();
            }
        };

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_addr) in top_docs {
            let retrieved = match searcher.doc::<TantivyDocument>(doc_addr) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let url = retrieved
                .get_first(url_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let title = retrieved
                .get_first(title_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let ts = retrieved
                .get_first(visit_time_field)
                .and_then(|v| v.as_datetime())
                .map(|d| {
                    let secs = d.into_timestamp_secs();
                    DateTime::from_timestamp(secs, 0).unwrap_or_default()
                })
                .unwrap_or_default();
            let content_snippet = retrieved
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(200).collect())
                .unwrap_or_default();

            results.push(IndexedDocument {
                url,
                title,
                content_snippet,
                visit_time: ts,
                score,
            });
        }

        results
    }

    pub fn clear(&self) {
        let mut writer = match self.writer.lock() {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to lock index writer: {e}");
                return;
            }
        };
        if let Err(e) = writer.delete_all_documents() {
            error!("Failed to clear index: {e}");
        }
        if let Err(e) = writer.commit() {
            error!("Failed to commit after clear: {e}");
        }
        info!("Local search index cleared");
    }

    pub fn num_docs(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }
}
