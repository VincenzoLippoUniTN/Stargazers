//! Galaxy graph: ring topology with random shortcuts.
//!
//! Planets 1-7 are arranged in a ring (1-2-3-4-5-6-7-1).
//! On top of that, random shortcut edges are added at startup based on a
//! per-planet shortcut budget:
//!
//! - 4 out of 7 planets get at most 1 shortcut  (common: often 0)
//! - 2 out of 7 planets get at most 2 shortcuts
//! - 1 out of 7 planets gets  at most 3 shortcuts  (rare: usually fewer)
//!
//! The actual number of shortcuts per planet is drawn with weights so that
//! 0 is the most likely outcome for every planet.
//!
//! Call [`build_galaxy`] once at orchestrator startup. It returns a
//! [`GalaxyLayout`] that the orchestrator uses for `neighbors_of` queries
//! and for the edge list it sends in the visualizer snapshot.

use std::collections::{HashMap, HashSet};

use common_game::utils::ID;

// =========================================================================
// PUBLIC TYPES
// =========================================================================

/// The complete adjacency graph and edge list for the galaxy.
pub struct GalaxyLayout {
    /// Adjacency list: planet id → sorted list of reachable planet ids.
    adjacency: HashMap<ID, Vec<ID>>,
    /// All edges as (lo, hi) pairs. Drive the visualizer's edge list.
    pub edges: Vec<(ID, ID)>,
}

impl GalaxyLayout {
    /// Returns the neighbours of `planet_id` (only alive planets are filtered
    /// later by the orchestrator — the layout itself is static and complete).
    pub fn neighbors_of(&self, planet_id: ID) -> Vec<ID> {
        self.adjacency.get(&planet_id).cloned().unwrap_or_default()
    }
}

// =========================================================================
// BUILDER
// =========================================================================

/// Builds the galaxy layout for `n` planets (normally 7).
///
/// The ring always connects planet `i` to planet `i+1` (wrapping), so planet
/// 1 and planet `n` are also connected.
///
/// Shortcuts are generated from the RNG seed you pass in. Use the same seed
/// as the orchestrator's `rng` field so the graph is stable for a full run.
pub fn build_galaxy(n: u32, seed: u64) -> GalaxyLayout {
    let mut rng = Rng::new(seed);

    // ---- Ring edges --------------------------------------------------------
    let mut edge_set: HashSet<(ID, ID)> = HashSet::new();
    for i in 1..=n {
        let a = i;
        let b = if i == n { 1 } else { i + 1 };
        edge_set.insert((a.min(b), a.max(b)));
    }

    // ---- Per-planet shortcut budget ----------------------------------------
    // 4 planets → budget 1, 2 planets → budget 2, 1 planet → budget 3.
    let mut budgets: Vec<u32> = vec![1, 1, 1, 1, 2, 2, 3];
    rng.shuffle(&mut budgets);

    // ---- Draw shortcuts ----------------------------------------------------
    let mut shortcuts: HashSet<(ID, ID)> = HashSet::new();

    for planet in 1..=n {
        let budget = budgets[(planet - 1) as usize];

        // Weights for 0..=budget shortcuts: 0 is most likely.
        // weights: [50, 30, 15, 5] truncated to budget+1 entries.
        let all_weights: [u32; 4] = [50, 30, 15, 5];
        let weights = &all_weights[..=(budget as usize)];
        let count = rng.weighted_choice(weights);

        // Candidates: not already in ring or shortcuts, not self.
        let candidates: Vec<ID> = (1..=n)
            .filter(|&p| {
                p != planet
                    && !edge_set.contains(&(p.min(planet), p.max(planet)))
                    && !shortcuts.contains(&(p.min(planet), p.max(planet)))
            })
            .collect();

        let actual = count.min(candidates.len() as u32) as usize;
        let chosen = rng.sample(&candidates, actual);
        for dest in chosen {
            shortcuts.insert((dest.min(planet), dest.max(planet)));
        }
    }

    // ---- Merge and build adjacency -----------------------------------------
    let all_edges: HashSet<(ID, ID)> = edge_set.union(&shortcuts).copied().collect();

    let mut adjacency: HashMap<ID, Vec<ID>> = HashMap::new();
    for i in 1..=n {
        adjacency.entry(i).or_default();
    }
    for &(a, b) in &all_edges {
        adjacency.entry(a).or_default().push(b);
        adjacency.entry(b).or_default().push(a);
    }
    for v in adjacency.values_mut() {
        v.sort_unstable();
    }

    let mut edges: Vec<(ID, ID)> = all_edges.into_iter().collect();
    edges.sort_unstable();

    GalaxyLayout { adjacency, edges }
}

// =========================================================================
// MINIMAL DEPENDENCY-FREE RNG  (xorshift64 + helpers)
// Mirrors the one in orchestrator.rs so no extra crate is needed.
// =========================================================================

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1) // never zero
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform float in [0, 1).
    fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Picks an index from `weights` proportionally.
    fn weighted_choice(&mut self, weights: &[u32]) -> u32 {
        let total: u32 = weights.iter().sum();
        let mut r = (self.f64() * total as f64) as u32;
        for (i, &w) in weights.iter().enumerate() {
            if r < w {
                return i as u32;
            }
            r -= w;
        }
        (weights.len() - 1) as u32
    }

    /// Fisher-Yates shuffle in place.
    fn shuffle<T>(&mut self, v: &mut Vec<T>) {
        let n = v.len();
        for i in (1..n).rev() {
            let j = (self.next() as usize) % (i + 1);
            v.swap(i, j);
        }
    }

    /// Reservoir sample `k` elements from `slice` without replacement.
    fn sample<T: Copy>(&mut self, slice: &[T], k: usize) -> Vec<T> {
        if k == 0 || slice.is_empty() {
            return vec![];
        }
        let mut result: Vec<T> = slice[..k.min(slice.len())].to_vec();
        for i in k..slice.len() {
            let j = (self.next() as usize) % (i + 1);
            if j < k {
                result[j] = slice[i];
            }
        }
        result
    }
}

// =========================================================================
// TESTS
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_always_present() {
        let layout = build_galaxy(7, 42);
        // Every planet must be reachable from its ring neighbour.
        for i in 1u32..=7 {
            let next = if i == 7 { 1 } else { i + 1 };
            assert!(
                layout.neighbors_of(i).contains(&next),
                "planet {i} should connect to {next} via the ring"
            );
        }
    }

    #[test]
    fn no_self_loops() {
        let layout = build_galaxy(7, 99);
        for i in 1u32..=7 {
            assert!(
                !layout.neighbors_of(i).contains(&i),
                "planet {i} must not be its own neighbour"
            );
        }
    }

    #[test]
    fn graph_is_symmetric() {
        let layout = build_galaxy(7, 777);
        for i in 1u32..=7 {
            for &j in layout.neighbors_of(i).iter() {
                assert!(
                    layout.neighbors_of(j).contains(&i),
                    "edge ({i},{j}) is not symmetric"
                );
            }
        }
    }

    #[test]
    fn all_planets_have_at_least_two_neighbours() {
        // Ring guarantees degree >= 2 for every planet.
        for seed in [1, 42, 99, 777, 12345] {
            let layout = build_galaxy(7, seed);
            for i in 1u32..=7 {
                assert!(
                    layout.neighbors_of(i).len() >= 2,
                    "planet {i} has fewer than 2 neighbours (seed {seed})"
                );
            }
        }
    }
}
