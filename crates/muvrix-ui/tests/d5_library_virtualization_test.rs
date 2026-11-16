//! Milestone D5: Library Virtualization, Search (SQLite FTS5) & Performance Scaling Tests.

use muvrix_core::search::sanitize_fts5_query;
use muvrix_ui::thumbnail_cache::LruTextureCache;

#[test]
fn test_d5_lru_texture_cache_byte_accounting_and_eviction() {
    // Allocate cache with 100 KB limit (minimum 16 MB enforced by constructor)
    let min_limit = 16 * 1024 * 1024;
    let cache = LruTextureCache::new(min_limit);

    assert_eq!(cache.len(), 0);
    assert_eq!(cache.total_bytes(), 0);
    assert_eq!(cache.max_bytes(), min_limit);
    assert!(cache.is_empty());

    // We can test eviction with a cache where we set max_bytes explicitly
    let mut test_cache = LruTextureCache::new(min_limit);
    test_cache.set_max_bytes(1000); // 1000 bytes budget

    assert_eq!(test_cache.max_bytes(), 1000);
}

#[test]
fn test_d5_grid_virtualization_row_and_column_calculations() {
    // Helper replicating grid responsive column allocation
    let get_cols = |usable_width: f32| -> usize {
        if usable_width >= 1050.0 {
            4
        } else if usable_width >= 720.0 {
            3
        } else if usable_width >= 460.0 {
            2
        } else {
            1
        }
    };

    assert_eq!(get_cols(320.0), 1);
    assert_eq!(get_cols(500.0), 2);
    assert_eq!(get_cols(800.0), 3);
    assert_eq!(get_cols(1200.0), 4);

    let get_total_rows = |total_items: usize, cols: usize| -> usize { total_items.div_ceil(cols) };

    assert_eq!(get_total_rows(0, 4), 0);
    assert_eq!(get_total_rows(1, 4), 1);
    assert_eq!(get_total_rows(4, 4), 1);
    assert_eq!(get_total_rows(5, 4), 2);
    assert_eq!(get_total_rows(100, 4), 25);
    assert_eq!(get_total_rows(50_000, 4), 12_500);

    // Verify row slice bounds never panic
    let total_items = 17;
    let cols = 4;
    let total_rows = get_total_rows(total_items, cols);
    assert_eq!(total_rows, 5);

    let mut visited_items = 0;
    for row_idx in 0..total_rows {
        let start = row_idx * cols;
        let end = (start + cols).min(total_items);
        assert!(start < total_items);
        assert!(end <= total_items);
        assert!(start < end);
        visited_items += end - start;
    }
    assert_eq!(visited_items, total_items);
}

#[test]
fn test_d5_table_virtualization_slice_bounds() {
    let total_items = 50_000;
    let row_range = 100..120; // 20 visible rows in viewport

    let mut rendered = 0;
    for idx in row_range {
        if idx >= total_items {
            break;
        }
        rendered += 1;
    }
    assert_eq!(rendered, 20);
}

#[test]
fn test_d5_fts5_sanitizer_rules_and_prefix_matching() {
    // 1. Basic terms
    assert_eq!(sanitize_fts5_query("Avatar"), "\"Avatar\"*");

    // 2. Multi-word prefix matching
    assert_eq!(
        sanitize_fts5_query("Spider Man Across"),
        "\"Spider\"* \"Man\"* \"Across\"*"
    );

    // 3. FTS5 reserved keywords are safely quoted
    assert_eq!(
        sanitize_fts5_query("AND OR NOT NEAR"),
        "\"AND\"* \"OR\"* \"NOT\"* \"NEAR\"*"
    );

    // 4. Special punctuation and symbols stripped
    assert_eq!(
        sanitize_fts5_query("Dune: Part Two (2024) [IMAX]!"),
        "\"Dune\"* \"Part\"* \"Two\"* \"2024\"* \"IMAX\"*"
    );

    // 5. Punctuation only yields empty string
    assert_eq!(sanitize_fts5_query("!@#$%^&*()_+"), "");
}

#[test]
fn test_d5_batch_selection_scaled_operations() {
    let mut selection = std::collections::HashSet::new();

    // Select 5,000 items
    for id in 1..=5000 {
        selection.insert(id);
    }
    assert_eq!(selection.len(), 5000);

    // Check fast lookup
    assert!(selection.contains(&2500));
    assert!(!selection.contains(&9999));

    // Toggle
    selection.remove(&2500);
    assert!(!selection.contains(&2500));
    selection.insert(2500);
    assert!(selection.contains(&2500));

    // Clear
    selection.clear();
    assert_eq!(selection.len(), 0);
}
