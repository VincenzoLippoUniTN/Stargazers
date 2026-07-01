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
    /// External id of the explorer manual explorer-ops act on. `None` until one
    /// is selected (or auto-picked from the first snapshot).
    pub selected_explorer: Option<u32>,
    /// Index into [`crate::command::BASIC_CHOICES`] for the next `Generate`.
    pub basic_choice: usize,
    /// Index into [`crate::command::COMPLEX_CHOICES`] for the next `Combine`.
    pub complex_choice: usize,
    /// The most recent query answers, newest last, shown in the HUD.
    pub reports: Vec<String>,
    /// Whether the simulation AI is (believed to be) running. Toggled by the
    /// "Toggle AI" op. Defaults to running, which is how the orchestrator boots.
    /// Stored inverted so `#[derive(Default)]` (false) means "running".
    pub ai_paused: bool,
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

    /// Replaces the report panel with `lines`, keeping only the newest entries so
    /// the HUD can't grow without bound.
    pub fn set_report(&mut self, lines: Vec<String>) {
        const MAX_REPORT_LINES: usize = 8;
        self.reports = lines;
        if self.reports.len() > MAX_REPORT_LINES {
            let start = self.reports.len() - MAX_REPORT_LINES;
            self.reports.drain(0..start);
        }
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
