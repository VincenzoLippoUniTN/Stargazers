//! The input contract: manual operations requested from the visualizer UI.
//!
//! This is the mirror image of [`crate::feed`]. Where snapshots flow *from* the
//! simulation *to* the visualizer, commands flow the other way: the visualizer
//! only ever *emits* an intent, and the simulation decides whether and how to
//! carry it out. The resulting state change comes back as the next snapshot, so
//! the simulation stays the single source of truth.

use bevy::prelude::Resource;
use crossbeam_channel::{Receiver, Sender};

/// A manual operation the viewer asked for. Ids refer to the same planet/explorer
/// ids carried in [`crate::GalaxySnapshot`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GalaxyCommand {
    /// Charge a cell on this planet (the manual equivalent of an automatic sun ray).
    Sunray { planet_id: u32 },
    /// Throw an asteroid at this planet.
    Asteroid { planet_id: u32 },
    /// Start (`true`) or stop (`false`) this planet's AI.
    SetAi { planet_id: u32, running: bool },
    /// Destroy this planet.
    Kill { planet_id: u32 },
    /// Send an explorer to a planet.
    MoveExplorer { explorer_id: u32, to_planet: u32 },
}

/// Creates a connected sink/source pair. Hand the [`CommandSink`] to
/// [`crate::run_with_io`] and keep the [`CommandSource`] on the producer side.
pub fn command_channel() -> (CommandSink, CommandSource) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (CommandSink(tx), CommandSource(rx))
}

/// Visualizer side: emits commands. Inserted into the app as a resource; cheap to clone.
#[derive(Resource, Clone)]
pub struct CommandSink(Sender<GalaxyCommand>);

impl CommandSink {
    /// Emits a command, best-effort. Silently does nothing if the producer has gone.
    pub fn send(&self, command: GalaxyCommand) {
        let _ = self.0.send(command);
    }
}

/// Producer side: drains the commands the visualizer has emitted.
pub struct CommandSource(Receiver<GalaxyCommand>);

impl CommandSource {
    /// Returns the next pending command, or `None` if none are waiting.
    pub fn try_recv(&self) -> Option<GalaxyCommand> {
        self.0.try_recv().ok()
    }

    /// Drains every pending command.
    pub fn drain(&self) -> impl Iterator<Item = GalaxyCommand> + '_ {
        std::iter::from_fn(|| self.try_recv())
    }
}
