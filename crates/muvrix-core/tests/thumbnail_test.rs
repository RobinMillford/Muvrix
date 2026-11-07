//! Integration tests for thumbnail caching and atomic disk persistence in muvrix-core.

use muvrix_core::thumbnails::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_cache_key_determinism() {
    let key1 = ThumbnailCache::compute_cache_key("/path/to/movie.mp4", 1024, 1700000000, 320, 180);
    let key2 = ThumbnailCache::compute_cache_key("/path/to/movie.mp4", 1024, 1700000000, 320, 180);
    let key3 = ThumbnailCache::compute_cache_key("/path/to/movie.mp4", 1025, 1700000000, 320, 180);

    assert_eq!(key1, key2);
    assert_ne!(key1, key3);
    assert_eq!(key1.len(), 64); // SHA-256 hex string length
}

#[test]
fn test_thumbnail_atomic_storage_and_retrieval() {
    let temp = TempDir::new().unwrap();
    let cache = ThumbnailCache::new(temp.path().to_path_buf(), 10 * 1024 * 1024);

    let key = "test_key_12345";
    assert!(cache.get_cached_thumbnail(key).is_none());

    let stored_path = cache
        .store_thumbnail_atomic(key, MINIMAL_PNG_BYTES)
        .expect("Store thumbnail");
    assert!(stored_path.is_file());

    let hit = cache.get_cached_thumbnail(key).expect("Cache hit");
    assert_eq!(hit, stored_path);

    // Invalidate
    cache.invalidate(key).expect("Invalidate");
    assert!(cache.get_cached_thumbnail(key).is_none());
}

#[test]
fn test_thumbnail_cache_lru_pruning() {
    let temp = TempDir::new().unwrap();
    // Cache size limit: 300 bytes (each minimal PNG is ~70 bytes)
    let cache = ThumbnailCache::new(temp.path().to_path_buf(), 200);

    for i in 0..5 {
        let key = format!("thumb_{}", i);
        cache
            .store_thumbnail_atomic(&key, MINIMAL_PNG_BYTES)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // After storing 5 thumbnails (~350 bytes), pruning should have freed oldest entries to stay under 200 bytes
    let total_bytes: u64 = fs::read_dir(temp.path())
        .unwrap()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();

    assert!(total_bytes <= 200);
}

#[test]
fn test_seek_seconds_boundaries() {
    assert_eq!(calculate_thumbnail_seek_seconds(60_000), 3.0);
    assert_eq!(calculate_thumbnail_seek_seconds(20_000), 3.0);
    assert_eq!(calculate_thumbnail_seek_seconds(500), 0.0);
}

#[test]
fn test_fallback_thumbnail_extractor() {
    let temp = TempDir::new().unwrap();
    let out = temp.path().join("thumb.png");
    let ext = FallbackThumbnailExtractor;

    ext.extract_thumbnail(std::path::Path::new("/dummy/file.mp4"), 3000, &out)
        .unwrap();
    assert!(out.is_file());
    assert_eq!(fs::read(&out).unwrap(), MINIMAL_PNG_BYTES);
}
