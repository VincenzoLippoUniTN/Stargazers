//! Player input: keyboard shortcuts, the mouse wheel, and the on-screen buttons.
//!
//! Actions split in two:
//! - **View controls** (mode, focus, zoom, pause the animation) are purely
//!   client-side and always act locally.
//! - **Game operations** (sun ray, move explorer) represent real game actions. In
//!   feed mode they're emitted as [`GalaxyCommand`]s for the simulation to carry
//!   out; in demo mode (no [`CommandSink`]) they fall back to a local simulation.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use rand::prelude::*;

use crate::command::{CommandSink, GalaxyCommand};
use crate::domain::components::{Action, Btn, Explorer, Planet};
use crate::domain::state::GameState;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (handle_keyboard, handle_mouse, handle_buttons, style_buttons),
        );
    }
}

fn handle_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    mut explorers: Query<&mut Explorer>,
    mut planets: Query<&mut Planet>,
    commands: Option<Res<CommandSink>>,
) {
    if keys.just_pressed(KeyCode::KeyP) {
        state.toggle_mode();
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        state.next();
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        state.prev();
    }
    if keys.just_pressed(KeyCode::KeyE) {
        request_move_explorer(&state, &mut explorers, &commands);
    }
    if keys.just_pressed(KeyCode::KeyS) {
        request_sunray(&state, &mut planets, &commands);
    }
    if keys.just_pressed(KeyCode::Space) {
        state.paused = !state.paused;
    }
}

fn handle_mouse(mut scroll: EventReader<MouseWheel>, mut state: ResMut<GameState>) {
    for ev in scroll.read() {
        state.zoom(-ev.y * 2.5);
    }
}

#[allow(clippy::type_complexity)]
fn handle_buttons(
    interaction: Query<(&Interaction, &Btn), (Changed<Interaction>, With<Button>)>,
    mut state: ResMut<GameState>,
    mut explorers: Query<&mut Explorer>,
    mut planets: Query<&mut Planet>,
    commands: Option<Res<CommandSink>>,
) {
    for (inter, btn) in &interaction {
        if *inter != Interaction::Pressed {
            continue;
        }
        match btn.0 {
            Action::Mode => state.toggle_mode(),
            Action::Prev => state.prev(),
            Action::Next => state.next(),
            Action::Move => request_move_explorer(&state, &mut explorers, &commands),
            Action::Sunray => request_sunray(&state, &mut planets, &commands),
            Action::Pause => state.paused = !state.paused,
            Action::ZoomIn => state.zoom(-5.0),
            Action::ZoomOut => state.zoom(5.0),
        }
    }
}

fn style_buttons(mut q: Query<(&Interaction, &mut BackgroundColor), With<Button>>) {
    for (inter, mut bg) in &mut q {
        bg.0 = match inter {
            Interaction::Pressed => Color::srgba(0.35, 0.35, 0.5, 1.0),
            Interaction::Hovered => Color::srgba(0.25, 0.25, 0.35, 0.95),
            Interaction::None => Color::srgba(0.15, 0.15, 0.2, 0.9),
        };
    }
}

/// Charge a cell on the focused planet: emit a command in feed mode, simulate
/// locally in demo mode.
fn request_sunray(
    state: &GameState,
    planets: &mut Query<&mut Planet>,
    commands: &Option<Res<CommandSink>>,
) {
    if let Some(sink) = commands {
        if let Some(&planet_id) = state.planet_ids.get(state.focus) {
            sink.send(GalaxyCommand::Sunray { planet_id });
        }
    } else {
        charge_first_empty_cell(state.focus, planets);
    }
}

/// Send an idle explorer to a neighbouring planet: emit a command in feed mode,
/// simulate locally in demo mode.
fn request_move_explorer(
    state: &GameState,
    explorers: &mut Query<&mut Explorer>,
    commands: &Option<Res<CommandSink>>,
) {
    let mut rng = rand::thread_rng();

    match commands {
        Some(sink) => {
            for ex in explorers.iter() {
                if ex.target.is_some() {
                    continue;
                }
                let neighbors = state.neighbors(ex.at);
                if neighbors.is_empty() {
                    continue;
                }
                let target = neighbors[rng.gen_range(0..neighbors.len())];
                if let Some(&to_planet) = state.planet_ids.get(target) {
                    sink.send(GalaxyCommand::MoveExplorer {
                        explorer_id: ex.id,
                        to_planet,
                    });
                }
                break;
            }
        }
        None => {
            for mut ex in explorers.iter_mut() {
                if ex.target.is_some() {
                    continue;
                }
                let neighbors = state.neighbors(ex.at);
                if neighbors.is_empty() {
                    continue;
                }
                ex.target = Some(neighbors[rng.gen_range(0..neighbors.len())]);
                break;
            }
        }
    }
}

fn charge_first_empty_cell(focus: usize, planets: &mut Query<&mut Planet>) {
    for mut p in planets.iter_mut() {
        if p.id != focus {
            continue;
        }
        if let Some(i) = p.cells_charged.iter().position(|&charged| !charged) {
            p.cells_charged[i] = true;
        }
        break;
    }
}
