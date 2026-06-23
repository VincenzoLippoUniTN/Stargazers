//! Keeps the rendered world in step with the data.
//!
//! [`ingest_feed`] pulls the latest snapshot off the feed and writes it onto the
//! [`Planet`]/[`Explorer`] components; the `sync_*` systems then derive each
//! entity's appearance (cell glow, rocket and planet visibility) from that state.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::domain::components::{Cell, Explorer, OfPlanet, Planet, Rocket};
use crate::domain::layout::GalaxyLayout;
use crate::domain::state::GameState;
use crate::feed::GalaxyFeed;
use crate::VisualizerSet;

pub struct SyncPlugin;

impl Plugin for SyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, ingest_feed.in_set(VisualizerSet::Ingest))
            .add_systems(
                Update,
                (sync_cell_visuals, sync_rockets, sync_static_visuals)
                    .in_set(VisualizerSet::React),
            );
    }
}

/// Drains the feed (if any) and applies the latest snapshot. Before the scene is
/// built, the first snapshot becomes the pending layout for `build_galaxy`.
fn ingest_feed(
    feed: Option<Res<GalaxyFeed>>,
    mut state: ResMut<GameState>,
    mut planets: Query<&mut Planet>,
    mut explorers: Query<&mut Explorer>,
) {
    let Some(feed) = feed else {
        return;
    };
    let Some(snapshot) = feed.latest() else {
        return;
    };

    if !state.built {
        if state.pending.is_none() {
            state.pending = Some(GalaxyLayout::from_snapshot(&snapshot));
        }
        return;
    }

    for mut planet in &mut planets {
        let ext_id = state.planet_ids[planet.id];
        if let Some(p) = snapshot.planets.iter().find(|p| p.id == ext_id) {
            let cell_count = planet.planet_type.cell_count();
            planet.cells_charged = p.cells.clone();
            planet.cells_charged.resize(cell_count, false);
            planet.has_rocket = p.has_rocket;
            planet.alive = p.alive;
        }
    }

    for reported in &snapshot.explorers {
        let Some(target) = state.index_of(reported.at_planet) else {
            continue;
        };
        for mut ex in &mut explorers {
            if ex.id == reported.id && ex.at != target && ex.target.is_none() {
                ex.target = Some(target);
            }
        }
    }
}

fn sync_cell_visuals(
    planets: Query<&Planet>,
    mut cells: Query<(&mut Cell, &Handle<StandardMaterial>, &mut Visibility)>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let by_index: HashMap<usize, &Planet> = planets.iter().map(|p| (p.id, p)).collect();

    for (mut cell, handle, mut vis) in &mut cells {
        let planet = by_index.get(&cell.planet);
        let alive = planet.is_some_and(|p| p.alive);

        set_visibility(&mut vis, if alive { Visibility::Inherited } else { Visibility::Hidden });

        let lit = alive
            && planet
                .and_then(|p| p.cells_charged.get(cell.index))
                .copied()
                .unwrap_or(false);
        if cell.lit != lit {
            cell.lit = lit;
            if let Some(mat) = mats.get_mut(handle) {
                if lit {
                    mat.base_color = Color::srgb(1.0, 0.95, 0.5);
                    mat.emissive = LinearRgba::new(1.5, 1.3, 0.4, 1.0);
                } else {
                    mat.base_color = Color::srgb(0.2, 0.2, 0.25);
                    mat.emissive = LinearRgba::NONE;
                }
            }
        }
    }
}

fn sync_rockets(planets: Query<&Planet>, mut rockets: Query<(&Rocket, &mut Visibility)>) {
    let by_index: HashMap<usize, &Planet> = planets.iter().map(|p| (p.id, p)).collect();
    for (rocket, mut vis) in &mut rockets {
        let show = by_index.get(&rocket.planet).is_some_and(|p| p.alive && p.has_rocket);
        set_visibility(&mut vis, if show { Visibility::Visible } else { Visibility::Hidden });
    }
}

fn sync_static_visuals(planets: Query<&Planet>, mut q: Query<(&OfPlanet, &mut Visibility)>) {
    let alive: HashMap<usize, bool> = planets.iter().map(|p| (p.id, p.alive)).collect();
    for (of, mut vis) in &mut q {
        let visible = alive.get(&of.0).copied().unwrap_or(true);
        set_visibility(&mut vis, if visible { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Writes a visibility only when it changes, to avoid needless change detection.
fn set_visibility(vis: &mut Visibility, desired: Visibility) {
    if *vis != desired {
        *vis = desired;
    }
}
