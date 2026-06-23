use bevy::math::Vec3;
use rand::prelude::*;
use std::f32::consts::{PI, TAU};

use super::planet::{Element, PlanetType};
use crate::feed::GalaxySnapshot;

const DEMO_PLANETS: usize = 5;

/// One planet's starting description, before it becomes entities.
pub struct PlanetInit {
    pub id: u32,
    pub kind: PlanetType,
    pub position: Vec3,
    pub element: Element,
    pub cells: Vec<bool>,
    pub has_rocket: bool,
    pub alive: bool,
}

pub struct ExplorerInit {
    pub id: u32,
    pub at: usize,
}

/// A complete galaxy ready to spawn, produced either randomly (demo) or from the
/// first [`GalaxySnapshot`] (feed).
pub struct GalaxyLayout {
    pub planets: Vec<PlanetInit>,
    pub edges: Vec<(usize, usize)>,
    pub explorers: Vec<ExplorerInit>,
}

impl GalaxyLayout {
    /// A random self-contained galaxy, used when no feed is connected.
    pub fn demo() -> Self {
        let mut rng = rand::thread_rng();
        let kinds = [
            PlanetType::A,
            PlanetType::B,
            PlanetType::C,
            PlanetType::D,
            PlanetType::A,
        ];

        let planets = (0..DEMO_PLANETS)
            .map(|i| {
                let kind = kinds[i % kinds.len()];
                PlanetInit {
                    id: i as u32,
                    kind,
                    position: ring_position(i, DEMO_PLANETS),
                    element: Element::ALL[rng.gen_range(0..Element::ALL.len())],
                    cells: (0..kind.cell_count()).map(|_| rng.gen_bool(0.5)).collect(),
                    has_rocket: kind.can_have_rocket() && rng.gen_bool(0.7),
                    alive: true,
                }
            })
            .collect();

        let mut edges: Vec<(usize, usize)> =
            (0..DEMO_PLANETS).map(|i| (i, (i + 1) % DEMO_PLANETS)).collect();
        edges.push((0, 2));
        edges.push((1, 3));

        let explorers = (0..2)
            .map(|id| ExplorerInit {
                id,
                at: rng.gen_range(0..DEMO_PLANETS),
            })
            .collect();

        GalaxyLayout {
            planets,
            edges,
            explorers,
        }
    }

    /// Builds a layout from the first snapshot received over a feed.
    pub fn from_snapshot(snapshot: &GalaxySnapshot) -> Self {
        let count = snapshot.planets.len();

        let planets: Vec<PlanetInit> = snapshot
            .planets
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let kind = PlanetType::from(p.kind);
                let mut cells = p.cells.clone();
                cells.resize(kind.cell_count(), false);
                PlanetInit {
                    id: p.id,
                    kind,
                    position: ring_position(i, count),
                    element: Element::ALL[(p.id as usize) % Element::ALL.len()],
                    cells,
                    has_rocket: p.has_rocket,
                    alive: p.alive,
                }
            })
            .collect();

        let index_of: std::collections::HashMap<u32, usize> =
            planets.iter().enumerate().map(|(i, p)| (p.id, i)).collect();

        let edges = if snapshot.edges.is_empty() {
            (0..count).map(|i| (i, (i + 1) % count.max(1))).collect()
        } else {
            snapshot
                .edges
                .iter()
                .filter_map(|&(a, b)| Some((*index_of.get(&a)?, *index_of.get(&b)?)))
                .collect()
        };

        let explorers = snapshot
            .explorers
            .iter()
            .filter_map(|e| {
                Some(ExplorerInit {
                    id: e.id,
                    at: *index_of.get(&e.at_planet)?,
                })
            })
            .collect();

        GalaxyLayout {
            planets,
            edges,
            explorers,
        }
    }
}

/// Spreads `count` planets along a tilted ring around the sun.
pub fn ring_position(i: usize, count: usize) -> Vec3 {
    let angle = (i as f32 / count.max(1) as f32) * TAU - PI / 2.0;
    let distance = 18.0 + i as f32 * 4.0;
    Vec3::new(
        distance * angle.cos(),
        (i as f32 - 2.0) * 2.0,
        distance * angle.sin(),
    )
}
