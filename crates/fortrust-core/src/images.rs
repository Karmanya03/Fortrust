//! Decoded image registry shared between the layout, paint, and chrome stages.
//!
//! During the load phase, the engine downloads and decodes every `<img>` on the
//! page. The decoded RGBA pixels are stored in a per-page `ImageRegistry` and
//! referenced from layout boxes / paint commands by integer id (cheap to clone,
//! cheap to compare, immune to lifetime issues).

use std::collections::HashMap;

/// A single decoded image: RGBA8 pixels, owned buffer, and source URL.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Maps the source URL of an `<img>` to its decoded data and assigns a stable
/// integer id to every entry. Layout boxes reference images by id, paint
/// commands render by id, and the chrome shell looks up the texture by id.
#[derive(Debug, Default, Clone)]
pub struct ImageRegistry {
    entries: Vec<DecodedImage>,
    url_to_id: HashMap<String, u32>,
}

impl ImageRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a decoded image. Returns the assigned id. Re-inserting the same
    /// URL returns the existing id (idempotent).
    pub fn insert(&mut self, image: DecodedImage) -> u32 {
        if let Some(&existing) = self.url_to_id.get(&image.url) {
            return existing;
        }
        let id = self.entries.len() as u32;
        self.url_to_id.insert(image.url.clone(), id);
        self.entries.push(image);
        id
    }

    pub fn get(&self, id: u32) -> Option<&DecodedImage> {
        self.entries.get(id as usize)
    }

    pub fn find_by_url(&self, url: &str) -> Option<u32> {
        self.url_to_id.get(url).copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &DecodedImage)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i as u32, e))
    }
}
