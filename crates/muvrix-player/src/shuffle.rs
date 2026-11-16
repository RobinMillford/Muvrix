//! Smart Shuffle Engine.
//!
//! Provides a mathematically fair pseudo-random shuffle that enforces separation
//! penalties for consecutive tracks from the same artist, album, or genre.
//! Prevents clustering while ensuring all N tracks appear in the shuffled queue
//! exactly once.

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShuffleTrackInfo {
    pub index: usize,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SmartShuffleEngine {
    pub artist_penalty: f32,
    pub album_penalty: f32,
    pub genre_penalty: f32,
    pub lookback_window: usize,
}

impl Default for SmartShuffleEngine {
    fn default() -> Self {
        Self {
            artist_penalty: 100.0,
            album_penalty: 50.0,
            genre_penalty: 25.0,
            lookback_window: 5,
        }
    }
}

impl SmartShuffleEngine {
    pub fn new(artist_penalty: f32, album_penalty: f32, genre_penalty: f32) -> Self {
        Self {
            artist_penalty,
            album_penalty,
            genre_penalty,
            lookback_window: 5,
        }
    }

    /// Computes an optimal permutation of `items` that separates tracks with
    /// the same artist, album, or genre while guaranteeing all items appear exactly once.
    ///
    /// `seed`: seed for deterministic pseudo-random tie-breaking and variation.
    pub fn calculate_order(&self, items: &[ShuffleTrackInfo], seed: u64) -> Vec<usize> {
        let n = items.len();
        if n <= 2 {
            return (0..n).collect();
        }

        let mut rng = SimpleRng::new(seed);
        let mut remaining: Vec<usize> = (0..n).collect();
        let mut ordered: Vec<usize> = Vec::with_capacity(n);

        // Pick initial track randomly from pool
        let start_pos = rng.next_range(remaining.len());
        let initial_idx = remaining.swap_remove(start_pos);
        ordered.push(initial_idx);

        while !remaining.is_empty() {
            let mut best_score = f32::MAX;
            let mut candidates: Vec<usize> = Vec::new();

            for (rem_pos, &cand_idx) in remaining.iter().enumerate() {
                let cand_item = &items[cand_idx];
                let mut penalty = 0.0_f32;

                // Evaluate penalties against the last `lookback_window` tracks placed in `ordered`
                let window_start = ordered.len().saturating_sub(self.lookback_window);
                for (pos_in_window, &prev_idx) in ordered[window_start..].iter().enumerate() {
                    let prev_item = &items[prev_idx];
                    let distance = (ordered.len() - (window_start + pos_in_window)) as f32; // 1, 2, ...
                    let dist_weight = 1.0 / distance;

                    if let (Some(ref a1), Some(ref a2)) = (&cand_item.artist, &prev_item.artist) {
                        if !a1.is_empty() && a1.eq_ignore_ascii_case(a2) {
                            penalty += self.artist_penalty * dist_weight;
                        }
                    }

                    if let (Some(ref b1), Some(ref b2)) = (&cand_item.album, &prev_item.album) {
                        if !b1.is_empty() && b1.eq_ignore_ascii_case(b2) {
                            penalty += self.album_penalty * dist_weight;
                        }
                    }

                    if let (Some(ref g1), Some(ref g2)) = (&cand_item.genre, &prev_item.genre) {
                        if !g1.is_empty() && g1.eq_ignore_ascii_case(g2) {
                            penalty += self.genre_penalty * dist_weight;
                        }
                    }
                }

                // Collect candidates with lowest penalty
                if (penalty - best_score).abs() < 0.001 {
                    candidates.push(rem_pos);
                } else if penalty < best_score {
                    best_score = penalty;
                    candidates.clear();
                    candidates.push(rem_pos);
                }
            }

            // Pick among best candidates
            let chosen_pick = rng.next_range(candidates.len());
            let chosen_rem_pos = candidates[chosen_pick];
            let item_idx = remaining.swap_remove(chosen_rem_pos);
            ordered.push(item_idx);
        }

        // Verification: ensure permutation validity (all 0..n present, no duplicates)
        debug_assert_eq!(ordered.len(), n);
        let unique: HashSet<usize> = ordered.iter().copied().collect();
        debug_assert_eq!(unique.len(), n);

        ordered
    }
}

/// Lightweight, deterministic linear congruential generator for shuffle tie-breaking.
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x853c49e6748fea9b } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_range(&mut self, max: usize) -> usize {
        if max <= 1 {
            0
        } else {
            (self.next_u64() % (max as u64)) as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_shuffle_permutation_integrity() {
        let engine = SmartShuffleEngine::default();
        let items: Vec<ShuffleTrackInfo> = (0..20)
            .map(|i| ShuffleTrackInfo {
                index: i,
                title: Some(format!("Track {i}")),
                artist: Some(format!("Artist {}", i % 4)),
                album: Some(format!("Album {}", i % 5)),
                genre: Some("Rock".to_string()),
            })
            .collect();

        let order = engine.calculate_order(&items, 12345);
        assert_eq!(order.len(), 20);

        let unique: HashSet<usize> = order.iter().copied().collect();
        assert_eq!(unique.len(), 20);
        for i in 0..20 {
            assert!(unique.contains(&i));
        }
    }

    #[test]
    fn test_smart_shuffle_artist_separation() {
        let engine = SmartShuffleEngine::default();
        // 4 tracks from Artist A, 4 tracks from Artist B
        let items = vec![
            ShuffleTrackInfo {
                index: 0,
                title: Some("Song A1".to_string()),
                artist: Some("Band Alpha".to_string()),
                album: None,
                genre: None,
            },
            ShuffleTrackInfo {
                index: 1,
                title: Some("Song A2".to_string()),
                artist: Some("Band Alpha".to_string()),
                album: None,
                genre: None,
            },
            ShuffleTrackInfo {
                index: 2,
                title: Some("Song A3".to_string()),
                artist: Some("Band Alpha".to_string()),
                album: None,
                genre: None,
            },
            ShuffleTrackInfo {
                index: 3,
                title: Some("Song A4".to_string()),
                artist: Some("Band Alpha".to_string()),
                album: None,
                genre: None,
            },
            ShuffleTrackInfo {
                index: 4,
                title: Some("Song B1".to_string()),
                artist: Some("Band Beta".to_string()),
                album: None,
                genre: None,
            },
            ShuffleTrackInfo {
                index: 5,
                title: Some("Song B2".to_string()),
                artist: Some("Band Beta".to_string()),
                album: None,
                genre: None,
            },
            ShuffleTrackInfo {
                index: 6,
                title: Some("Song B3".to_string()),
                artist: Some("Band Beta".to_string()),
                album: None,
                genre: None,
            },
            ShuffleTrackInfo {
                index: 7,
                title: Some("Song B4".to_string()),
                artist: Some("Band Beta".to_string()),
                album: None,
                genre: None,
            },
        ];

        let order = engine.calculate_order(&items, 42);
        assert_eq!(order.len(), 8);

        // Verify no two consecutive tracks have the exact same artist!
        for i in 0..order.len() - 1 {
            let a1 = &items[order[i]].artist;
            let a2 = &items[order[i + 1]].artist;
            assert_ne!(
                a1,
                a2,
                "Consecutive tracks at pos {} and {} have the same artist: {:?}",
                i,
                i + 1,
                a1
            );
        }
    }
}
