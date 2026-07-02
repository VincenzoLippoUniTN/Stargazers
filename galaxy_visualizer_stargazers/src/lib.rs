//! 3D galaxy visualizer for the Stargazers Advanced Programming project.
//!
//! The crate is organised as a small set of Bevy plugins layered on a plain data
//! core:
//!
//! - [`mod@feed`] - the data contract *out* ([`GalaxySnapshot`] and friends).
//! - [`mod@command`] - the contract *in* ([`GalaxyCommand`]) for manual operations.
//! - [`mod@report`] - the contract *out* ([`GalaxyReport`]) for query answers.
//! - `domain` - planet types, ECS components, world state and galaxy layout.
//! - `scene` - builds the 3D world from a layout.
//! - `sync` - reconciles the world with incoming snapshots.
//! - `motion` - idle animation.
//! - `interaction` - keyboard, mouse and buttons.
//! - `view` - camera and HUD.
//!
//! Use [`run`] for a self-contained random demo, [`run_with_feed`] to drive the
//! scene from live snapshots, [`run_with_io`] to also let the UI send manual
//! [`GalaxyCommand`]s back to the producer, or [`run_with_reports`] to *also*
//! receive [`GalaxyReport`] answers (bag contents, recipe lists) to show them.

mod command;
mod domain;
mod feed;
mod interaction;
mod motion;
mod report;
mod scene;
mod sync;
mod theme;
mod view;

use bevy::prelude::*;

use domain::state::{GameState, Source};

pub use command::{command_channel, CommandSink, CommandSource, GalaxyCommand};
pub use feed::{
    galaxy_channel, ExplorerSnapshot, GalaxyFeed, GalaxySender, GalaxySnapshot, PlanetKind,
    PlanetSnapshot,
};
pub use report::{report_channel, GalaxyReport, ReportFeed, ReportSender};

/// Ordering of the per-frame data pipeline: pull the latest snapshot, build the
/// scene if needed, then map the resulting state onto the entities' appearance.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum VisualizerSet {
    Ingest,
    Build,
    React,
}

/// The plugin that wires the whole visualizer together. Install it on an [`App`]
/// that already has [`DefaultPlugins`]; insert a [`GalaxyFeed`] beforehand to run
/// in live mode, or leave it out for the random demo.
pub struct GalaxyVisualizerPlugin;

impl Plugin for GalaxyVisualizerPlugin {
    fn build(&self, app: &mut App) {
        let source = if app.world().contains_resource::<GalaxyFeed>() {
            Source::Feed
        } else {
            Source::Demo
        };

        app.insert_resource(ClearColor(theme::SPACE))
            .insert_resource(GameState {
                source,
                ..default()
            })
            .configure_sets(
                Update,
                (
                    VisualizerSet::Ingest,
                    VisualizerSet::Build,
                    VisualizerSet::React,
                )
                    .chain(),
            )
            .add_plugins((
                scene::ScenePlugin,
                sync::SyncPlugin,
                motion::MotionPlugin,
                interaction::InteractionPlugin,
                view::ViewPlugin,
            ));
    }
}

/// Runs the visualizer as a self-contained demo with a random galaxy.
pub fn run() {
    build_app(None, None, None).run();
}

/// Runs the visualizer driven by snapshots arriving on `feed`.
///
/// Must be called from the main thread (a windowing requirement), so launch any
/// data producer on a background thread first.
pub fn run_with_feed(feed: GalaxyFeed) {
    build_app(Some(feed), None, None).run();
}

/// Like [`run_with_feed`], but the UI also emits manual [`GalaxyCommand`]s through
/// `commands` for the producer to act on.
pub fn run_with_io(feed: GalaxyFeed, commands: CommandSink) {
    build_app(Some(feed), Some(commands), None).run();
}

/// Like [`run_with_io`], but also drains [`GalaxyReport`]s off `reports` and shows
/// the latest one in the HUD - this is what lets a user *see* an explorer's bag
/// or a planet's recipe list after asking for it.
pub fn run_with_reports(feed: GalaxyFeed, commands: CommandSink, reports: ReportFeed) {
    build_app(Some(feed), Some(commands), Some(reports)).run();
}

fn build_app(
    feed: Option<GalaxyFeed>,
    commands: Option<CommandSink>,
    reports: Option<ReportFeed>,
) -> App {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "galaxy visualizer by Stargazers".into(),
            resolution: (1400.0, 850.0).into(),
            ..default()
        }),
        ..default()
    }));

    // Insert the feed before the plugin so it can detect feed vs demo mode.
    if let Some(feed) = feed {
        app.insert_resource(feed);
    }

    app.add_plugins(GalaxyVisualizerPlugin);

    if let Some(commands) = commands {
        app.insert_resource(commands);
    }

    if let Some(reports) = reports {
        app.insert_resource(reports);
    }

    app
}
