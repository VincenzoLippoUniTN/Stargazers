//! Bridge between the Stargazers orchestrator and the 3D galaxy visualizer.
//!
//! The orchestrator already polls each planet with
//! [`InternalStateRequest`](common_game::protocols::orchestrator_planet::OrchestratorToPlanet::InternalStateRequest)
//! and receives a [`DummyPlanetState`]. This module turns that state into the
//! visualizer's neutral [`GalaxySnapshot`] and pushes it through a channel.
//!
//! State flows *out* to the visualizer as snapshots. Manual operations flow back
//! *in* as [`GalaxyCommand`]s: the visualizer emits an intent, the orchestrator
//! drains it and runs it through the same code its AI uses, so a button press and
//! an automatic action are one and the same. See `CONNECTING_THE_VISUALIZER.md`.
//!
//! Typical use:
//!
//! ```ignore
//! use crate::visualizer::{kind_of, VizBridge};
//! use galaxy_visualizer_stargazers as viz;
//!
//! // 1. Create the channel and the bridge.
//! let (sender, feed) = viz::galaxy_channel();
//! let mut bridge = VizBridge::new(sender);
//!
//! // 2. Register each planet once with the type you created it as.
//! bridge.register_planet(1, kind_of(common_game::components::planet::PlanetType::B));
//! // ...register the rest...
//!
//! // 3. On every poll, feed the state you already receive and publish.
//! bridge.update_planet(planet_id, &planet_state); // from InternalStateResponse
//! bridge.publish();
//!
//! // 4. Hand the feed to the visualizer (must own the main thread).
//! viz::run_with_feed(feed);
//! ```
//!
//! See `CONNECTING_THE_VISUALIZER.md` for the full wiring walkthrough.

use std::collections::BTreeMap;

use common_game::components::planet::{DummyPlanetState, PlanetType};
use common_game::utils::ID;

use galaxy_visualizer_stargazers as viz;
use viz::{ExplorerSnapshot, GalaxySender, GalaxySnapshot, PlanetKind, PlanetSnapshot};

// Re-exported so the orchestrator can pull the whole visualizer surface from here.
pub use viz::{command_channel, galaxy_channel, run_with_io, CommandSource, GalaxyCommand};

/// Maps a `common-game` planet type to the visualizer's neutral kind.
pub fn kind_of(planet_type: PlanetType) -> PlanetKind {
    match planet_type {
        PlanetType::A => PlanetKind::A,
        PlanetType::B => PlanetKind::B,
        PlanetType::C => PlanetKind::C,
        PlanetType::D => PlanetKind::D,
    }
}

/// Accumulates the current galaxy state and publishes it to the visualizer.
///
/// Register each planet once with its kind, then feed it the [`DummyPlanetState`]
/// values you poll. Call [`publish`](Self::publish) whenever you want the view to
/// refresh; only the most recent snapshot matters, so publishing often is cheap.
pub struct VizBridge {
    sender: GalaxySender,
    planets: BTreeMap<ID, PlanetView>,
    explorers: BTreeMap<ID, ID>,
    edges: Vec<(ID, ID)>,
}

struct PlanetView {
    kind: PlanetKind,
    cells: Vec<bool>,
    has_rocket: bool,
    alive: bool,
}

impl VizBridge {
    pub fn new(sender: GalaxySender) -> Self {
        Self {
            sender,
            planets: BTreeMap::new(),
            explorers: BTreeMap::new(),
            edges: Vec::new(),
        }
    }

    /// Registers a planet with its kind. Call this once per planet before sending
    /// state; calling it again just updates the kind.
    pub fn register_planet(&mut self, id: ID, kind: PlanetKind) {
        self.planets
            .entry(id)
            .or_insert_with(|| PlanetView::new(kind))
            .kind = kind;
    }

    /// Updates a planet from the state returned by an `InternalStateRequest`.
    ///
    /// If the planet was never registered, its kind is guessed from the cell count
    /// (see [`PlanetKind::from_cell_count`]); prefer registering it explicitly.
    pub fn update_planet(&mut self, id: ID, state: &DummyPlanetState) {
        let view = self
            .planets
            .entry(id)
            .or_insert_with(|| PlanetView::new(PlanetKind::from_cell_count(state.energy_cells.len())));
        view.cells = state.energy_cells.clone();
        view.has_rocket = state.has_rocket;
    }

    /// Marks a planet alive or destroyed (e.g. after it fails to deflect an asteroid).
    pub fn set_alive(&mut self, id: ID, alive: bool) {
        if let Some(view) = self.planets.get_mut(&id) {
            view.alive = alive;
        }
    }

    /// Records that an explorer is currently visiting a planet.
    pub fn set_explorer(&mut self, explorer_id: ID, at_planet: ID) {
        self.explorers.insert(explorer_id, at_planet);
    }

    /// Sets the connections drawn between planets, as pairs of planet ids.
    /// Optional — leave unset to let the visualizer lay out a default ring.
    pub fn set_edges(&mut self, edges: Vec<(ID, ID)>) {
        self.edges = edges;
    }

    /// Builds a snapshot from the accumulated state and sends it to the visualizer.
    /// Returns `false` once the visualizer window has closed.
    #[must_use]
    pub fn publish(&self) -> bool {
        let planets = self
            .planets
            .iter()
            .map(|(&id, view)| PlanetSnapshot {
                id,
                kind: view.kind,
                cells: view.cells.clone(),
                has_rocket: view.has_rocket,
                alive: view.alive,
            })
            .collect();

        let explorers = self
            .explorers
            .iter()
            .map(|(&id, &at_planet)| ExplorerSnapshot { id, at_planet })
            .collect();

        let snapshot = GalaxySnapshot {
            planets,
            explorers,
            edges: self.edges.clone(),
        };

        self.sender.send(snapshot).is_ok()
    }
}

impl PlanetView {
    fn new(kind: PlanetKind) -> Self {
        Self {
            kind,
            cells: Vec::new(),
            has_rocket: false,
            alive: true,
        }
    }
}
