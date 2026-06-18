use bevy::prelude::*;

use super::layout::GalaxyLayout;

/// Camera framing, toggled with `P`.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Galaxy,
    Planet,
}

/// Where the galaxy state comes from.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// Self-contained random galaxy.
    #[default]
    Demo,
    /// Driven by snapshots arriving on a [`crate::GalaxyFeed`].
    Feed,
}

/// The single shared resource describing the running visualization.
#[derive(Resource, Default)]
pub struct GameState {
    pub positions: Vec<Vec3>,
    /// Maps a planet's scene index to its external (simulation) id.
    pub planet_ids: Vec<u32>,
    pub edges: Vec<(usize, usize)>,
    pub focus: usize,
    pub mode: Mode,
    pub zoom: f32,
    pub paused: bool,
    pub source: Source,
    /// Layout waiting to be turned into entities, set once.
    pub pending: Option<GalaxyLayout>,
    /// `true` once the scene entities exist.
    pub built: bool,
}

impl GameState {
    pub fn planet_count(&self) -> usize {
        self.positions.len()
    }

    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Galaxy => Mode::Planet,
            Mode::Planet => Mode::Galaxy,
        };
    }

    pub fn next(&mut self) {
        let n = self.planet_count();
        if n > 0 {
            self.focus = (self.focus + 1) % n;
        }
    }

    pub fn prev(&mut self) {
        let n = self.planet_count();
        if n > 0 {
            self.focus = (self.focus + n - 1) % n;
        }
    }

    pub fn zoom(&mut self, delta: f32) {
        self.zoom = (self.zoom + delta).clamp(20.0, 90.0);
    }

    /// Scene index of the planet with the given external id.
    pub fn index_of(&self, planet_id: u32) -> Option<usize> {
        self.planet_ids.iter().position(|&id| id == planet_id)
    }

    pub fn focus_position(&self) -> Option<Vec3> {
        self.positions.get(self.focus).copied()
    }

    pub fn neighbors(&self, id: usize) -> Vec<usize> {
        self.edges
            .iter()
            .filter_map(|&(a, b)| {
                if a == id {
                    Some(b)
                } else if b == id {
                    Some(a)
                } else {
                    None
                }
            })
            .collect()
    }
}
