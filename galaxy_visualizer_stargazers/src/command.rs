//! The input contract: manual operations requested from the visualizer UI.
//!
//! This is the mirror image of [`crate::feed`]. Where snapshots flow *from* the
//! simulation *to* the visualizer, commands flow the other way: the visualizer
//! only ever *emits* an intent, and the simulation decides whether and how to
//! carry it out. The resulting state change comes back as the next snapshot, so
//! the simulation stays the single source of truth.

use bevy::prelude::Resource;
use common_game::components::resource::{BasicResourceType, ComplexResourceType};
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
    /// Kill an explorer immediately.
    KillExplorer { explorer_id: u32 },
    /// Reset an explorer's AI (wipes its learned knowledge / restarts it).
    ResetExplorer { explorer_id: u32 },
    /// Ask what basic resources the explorer's current planet supports.
    SupportedResources { explorer_id: u32 },
    /// Ask what combination recipes the explorer's current planet supports.
    SupportedCombinations { explorer_id: u32 },
    /// Ask the explorer's current planet to generate a basic resource.
    Generate {
        explorer_id: u32,
        resource: BasicResourceType,
    },
    /// Ask the explorer's current planet to combine a complex resource.
    Combine {
        explorer_id: u32,
        resource: ComplexResourceType,
    },
    /// Ask the explorer to report its bag contents.
    BagContent { explorer_id: u32 },
}

/// The basic resources a user can pick for a [`GalaxyCommand::Generate`], in the
/// order the UI cycles through them. Kept here so the enum and the UI agree.
pub const BASIC_CHOICES: [BasicResourceType; 4] = [
    BasicResourceType::Oxygen,
    BasicResourceType::Hydrogen,
    BasicResourceType::Carbon,
    BasicResourceType::Silicon,
];

/// The complex resources a user can pick for a [`GalaxyCommand::Combine`], in the
/// order the UI cycles through them.
pub const COMPLEX_CHOICES: [ComplexResourceType; 6] = [
    ComplexResourceType::Water,
    ComplexResourceType::Diamond,
    ComplexResourceType::Life,
    ComplexResourceType::Robot,
    ComplexResourceType::Dolphin,
    ComplexResourceType::AIPartner,
];

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

    /// Consumes this handle and returns the raw crossbeam receiver.
    /// The orchestrator uses this to wait on commands inside `select_biased!`.
    pub fn into_receiver(self) -> Receiver<GalaxyCommand> {
        self.0
    }
}
