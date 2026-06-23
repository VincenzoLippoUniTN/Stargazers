//! The data contract between a simulation and the visualizer.
//!
//! A producer (for example the Stargazers orchestrator) sends [`GalaxySnapshot`]
//! values through a [`GalaxySender`]; the visualizer drains them from the matching
//! [`GalaxyFeed`] and reconciles the scene with whatever the latest snapshot says.
//!
//! These types deliberately avoid any Bevy types so a producer can build snapshots
//! without pulling in the renderer.

use bevy::prelude::Resource;
use crossbeam_channel::{Receiver, Sender, TryRecvError};

/// The four planet families from the Advanced Programming game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanetKind {
    A,
    B,
    C,
    D,
}

impl PlanetKind {
    /// Best-effort guess of the kind from the number of energy cells.
    ///
    /// Types A and D have five cells, B and C have one. This cannot tell A from D
    /// or B from C, so prefer passing the real kind when you know it.
    pub fn from_cell_count(cells: usize) -> Self {
        if cells >= 5 {
            PlanetKind::A
        } else {
            PlanetKind::B
        }
    }
}

/// State of a single planet at one instant.
#[derive(Clone, Debug)]
pub struct PlanetSnapshot {
    pub id: u32,
    pub kind: PlanetKind,
    /// One entry per energy cell, `true` when charged.
    pub cells: Vec<bool>,
    pub has_rocket: bool,
    /// `false` once the planet has been destroyed.
    pub alive: bool,
}

/// Where an explorer currently is.
#[derive(Clone, Debug)]
pub struct ExplorerSnapshot {
    pub id: u32,
    /// Id of the planet the explorer is visiting.
    pub at_planet: u32,
}

/// A full picture of the galaxy at one instant.
#[derive(Clone, Debug, Default)]
pub struct GalaxySnapshot {
    pub planets: Vec<PlanetSnapshot>,
    pub explorers: Vec<ExplorerSnapshot>,
    /// Connections drawn between planets, as pairs of planet ids. Leave empty to
    /// let the visualizer lay out a default ring.
    pub edges: Vec<(u32, u32)>,
}

/// Creates a connected sender/feed pair. Hand the [`GalaxyFeed`] to
/// [`crate::run_with_feed`] and keep the [`GalaxySender`] on the producer side.
pub fn galaxy_channel() -> (GalaxySender, GalaxyFeed) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (GalaxySender(tx), GalaxyFeed(rx))
}

/// Producer handle for pushing snapshots to the visualizer. Cheap to clone.
#[derive(Clone)]
pub struct GalaxySender(Sender<GalaxySnapshot>);

impl GalaxySender {
    /// Sends a snapshot. Returns the snapshot back as `Err` if the visualizer has
    /// already shut down.
    pub fn send(&self, snapshot: GalaxySnapshot) -> Result<(), GalaxySnapshot> {
        self.0.send(snapshot).map_err(|e| e.0)
    }
}

/// Consumer handle held by the visualizer.
#[derive(Resource)]
pub struct GalaxyFeed(Receiver<GalaxySnapshot>);

impl GalaxyFeed {
    /// Drains every pending snapshot and returns the most recent one, so the
    /// visualizer never lags behind a fast producer.
    pub(crate) fn latest(&self) -> Option<GalaxySnapshot> {
        let mut newest = None;
        loop {
            match self.0.try_recv() {
                Ok(snapshot) => newest = Some(snapshot),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return newest,
            }
        }
    }
}
