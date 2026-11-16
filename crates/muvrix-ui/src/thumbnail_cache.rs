//! Memory-bounded LRU texture cache for media thumbnails.
//!
//! Enforces strict byte budget constraints on GPU texture allocations. When the
//! cache exceeds `max_bytes`, least recently accessed textures are evicted, dropping
//! their `egui::TextureHandle` so egui deallocates the OpenGL texture from VRAM.

use std::collections::HashMap;
use std::sync::Mutex;

pub struct LruTextureCache {
    textures: HashMap<String, egui::TextureHandle>,
    access_order: Mutex<Vec<String>>,
    byte_sizes: HashMap<String, usize>,
    total_bytes: usize,
    max_bytes: usize,
}

impl std::fmt::Debug for LruTextureCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LruTextureCache")
            .field("cached_textures_count", &self.textures.len())
            .field("total_bytes", &self.total_bytes)
            .field("max_bytes", &self.max_bytes)
            .finish()
    }
}

impl Default for LruTextureCache {
    fn default() -> Self {
        // Default to 256 MB VRAM budget for thumbnail textures
        Self::new(256 * 1024 * 1024)
    }
}

impl LruTextureCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            textures: HashMap::new(),
            access_order: Mutex::new(Vec::new()),
            byte_sizes: HashMap::new(),
            total_bytes: 0,
            max_bytes: max_bytes.max(16 * 1024 * 1024), // Minimum 16 MB
        }
    }

    pub fn get(&self, key: &str) -> Option<&egui::TextureHandle> {
        if let Some(tex) = self.textures.get(key) {
            if let Ok(mut order) = self.access_order.lock() {
                if let Some(pos) = order.iter().position(|k| k == key) {
                    let k = order.remove(pos);
                    order.push(k);
                }
            }
            Some(tex)
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: String, texture: egui::TextureHandle, byte_size: usize) {
        if let Some(old_size) = self.byte_sizes.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(old_size);
        }
        if let Ok(mut order) = self.access_order.lock() {
            if let Some(pos) = order.iter().position(|k| k == &key) {
                order.remove(pos);
            }
            order.push(key.clone());

            self.byte_sizes.insert(key.clone(), byte_size);
            self.total_bytes += byte_size;
            self.textures.insert(key, texture);

            // Enforce max_bytes budget: evict oldest
            while self.total_bytes > self.max_bytes && !order.is_empty() {
                let oldest_key = order.remove(0);
                if let Some(size) = self.byte_sizes.remove(&oldest_key) {
                    self.total_bytes = self.total_bytes.saturating_sub(size);
                }
                self.textures.remove(&oldest_key);
            }
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.textures.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.textures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub fn set_max_bytes(&mut self, max: usize) {
        self.max_bytes = max;
        if let Ok(mut order) = self.access_order.lock() {
            while self.total_bytes > self.max_bytes && !order.is_empty() {
                let oldest_key = order.remove(0);
                if let Some(size) = self.byte_sizes.remove(&oldest_key) {
                    self.total_bytes = self.total_bytes.saturating_sub(size);
                }
                self.textures.remove(&oldest_key);
            }
        }
    }

    pub fn clear(&mut self) {
        self.textures.clear();
        if let Ok(mut order) = self.access_order.lock() {
            order.clear();
        }
        self.byte_sizes.clear();
        self.total_bytes = 0;
    }
}
