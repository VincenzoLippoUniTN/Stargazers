//! Player input: keyboard shortcuts, the mouse wheel, and the on-screen buttons.
//!
//! Actions split in two:
//! - **View controls** (mode, focus, zoom, pause the animation) are purely
//!   client-side and always act locally.
//! - **Game operations** (sun ray, asteroid, kill, move/kill/reset explorer,
//!   generate/combine, and the read-only queries) represent real game actions.
//!   In feed mode they're emitted as [`GalaxyCommand`]s for the simulation to
//!   carry out; in demo mode (no [`CommandSink`]) the ones that make sense fall
//!   back to a local simulation and the rest are no-ops.
//!
//! Every [`Action`] is routed through [`dispatch`], so a keypress and a button
//! click are guaranteed to do the exact same thing — there's one code path per
//! operation, and the on-screen bar can never drift from the shortcuts.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use rand::prelude::*;

use crate::command::{CommandSink, GalaxyCommand, BASIC_CHOICES, COMPLEX_CHOICES};
use crate::domain::components::{Action, Btn, Explorer, Planet};
use crate::domain::state::GameState;
use crate::report::ReportFeed;
use crate::theme;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                ingest_reports,
                handle_keyboard,
                handle_mouse,
                handle_buttons,
                style_buttons,
            ),
        );
    }
}

/// Pulls any query answers off the report feed and stores them for the HUD.
fn ingest_reports(reports: Option<Res<ReportFeed>>, mut state: ResMut<GameState>) {
    let Some(reports) = reports else {
        return;
    };
    let incoming = reports.drain();
    if incoming.is_empty() {
        return;
    }
    // Show the newest report; each report may render as several lines.
    if let Some(latest) = incoming.last() {
        let lines = latest.describe();
        state.set_report(lines);
    }
}

/// Maps every physical key to the [`Action`] it triggers, so the mapping lives in
/// exactly one place.
fn key_to_action(key: KeyCode) -> Option<Action> {
    Some(match key {
        KeyCode::KeyP => Action::Mode,
        KeyCode::ArrowLeft => Action::Prev,
        KeyCode::ArrowRight => Action::Next,
        KeyCode::KeyS => Action::Sunray,
        KeyCode::Space => Action::Pause,
        KeyCode::KeyA => Action::Asteroid,
        KeyCode::KeyK => Action::KillPlanet,
        KeyCode::KeyI => Action::ToggleAi,
        KeyCode::KeyE => Action::Move,
        KeyCode::KeyX => Action::SelExplorer,
        KeyCode::KeyJ => Action::KillExplorer,
        KeyCode::KeyR => Action::ResetExplorer,
        KeyCode::KeyB => Action::Bag,
        KeyCode::Digit1 => Action::Resources,
        KeyCode::Digit2 => Action::Combinations,
        KeyCode::KeyN => Action::CycleBasic,
        KeyCode::KeyG => Action::Generate,
        KeyCode::KeyM => Action::CycleComplex,
        KeyCode::KeyC => Action::Combine,
        _ => return None,
    })
}

fn handle_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    mut explorers: Query<&mut Explorer>,
    mut planets: Query<&mut Planet>,
    commands: Option<Res<CommandSink>>,
) {
    for key in keys.get_just_pressed() {
        if let Some(action) = key_to_action(*key) {
            dispatch(action, &mut state, &mut explorers, &mut planets, &commands);
        }
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
        dispatch(btn.0, &mut state, &mut explorers, &mut planets, &commands);
    }
}

/// The single place that turns an [`Action`] into an effect. View controls mutate
/// [`GameState`]; game operations emit a [`GalaxyCommand`] in feed mode or fall
/// back to local behaviour in demo mode.
fn dispatch(
    action: Action,
    state: &mut GameState,
    explorers: &mut Query<&mut Explorer>,
    planets: &mut Query<&mut Planet>,
    commands: &Option<Res<CommandSink>>,
) {
    match action {
        // ---- View controls (always local) ----
        Action::Mode => state.toggle_mode(),
        Action::Prev => state.prev(),
        Action::Next => state.next(),
        Action::Pause => state.paused = !state.paused,
        Action::ZoomIn => state.zoom(-5.0),
        Action::ZoomOut => state.zoom(5.0),

        // ---- Selection / choice cycling (local; drives later ops) ----
        Action::SelExplorer => cycle_selected_explorer(state, explorers),
        Action::CycleBasic => {
            state.basic_choice = (state.basic_choice + 1) % BASIC_CHOICES.len();
            let name = format!("{:?}", BASIC_CHOICES[state.basic_choice]);
            state.set_report(vec![format!("Next Generate: {name}")]);
        }
        Action::CycleComplex => {
            state.complex_choice = (state.complex_choice + 1) % COMPLEX_CHOICES.len();
            let name = format!("{:?}", COMPLEX_CHOICES[state.complex_choice]);
            state.set_report(vec![format!("Next Combine: {name}")]);
        }

        // ---- Planet operations (act on the focused planet) ----
        Action::Sunray => request_sunray(state, planets, commands),
        Action::Asteroid => {
            if let Some(planet_id) = focused_planet_id(state) {
                emit(commands, GalaxyCommand::Asteroid { planet_id });
            }
        }
        Action::KillPlanet => {
            if let Some(planet_id) = focused_planet_id(state) {
                emit(commands, GalaxyCommand::Kill { planet_id });
            }
        }
        Action::ToggleAi => {
            state.ai_paused = !state.ai_paused;
            let running = !state.ai_paused;
            // SetAi is global in the orchestrator; planet_id is carried for
            // symmetry but the focused one is a sensible default.
            let planet_id = focused_planet_id(state).unwrap_or(0);
            emit(commands, GalaxyCommand::SetAi { planet_id, running });
            state.set_report(vec![format!(
                "AI {}",
                if running { "running" } else { "paused" }
            )]);
        }

        // ---- Explorer operations (act on the selected explorer) ----
        Action::Move => request_move_explorer(state, explorers, commands),
        Action::KillExplorer => {
            if let Some(explorer_id) = selected_explorer(state, explorers) {
                emit(commands, GalaxyCommand::KillExplorer { explorer_id });
            }
        }
        Action::ResetExplorer => {
            if let Some(explorer_id) = selected_explorer(state, explorers) {
                emit(commands, GalaxyCommand::ResetExplorer { explorer_id });
            }
        }
        Action::Bag => {
            if let Some(explorer_id) = selected_explorer(state, explorers) {
                emit(commands, GalaxyCommand::BagContent { explorer_id });
            }
        }
        Action::Resources => {
            if let Some(explorer_id) = selected_explorer(state, explorers) {
                emit(commands, GalaxyCommand::SupportedResources { explorer_id });
            }
        }
        Action::Combinations => {
            if let Some(explorer_id) = selected_explorer(state, explorers) {
                emit(
                    commands,
                    GalaxyCommand::SupportedCombinations { explorer_id },
                );
            }
        }
        Action::Generate => {
            if let Some(explorer_id) = selected_explorer(state, explorers) {
                let resource = BASIC_CHOICES[state.basic_choice];
                emit(
                    commands,
                    GalaxyCommand::Generate {
                        explorer_id,
                        resource,
                    },
                );
            }
        }
        Action::Combine => {
            if let Some(explorer_id) = selected_explorer(state, explorers) {
                let resource = COMPLEX_CHOICES[state.complex_choice];
                emit(
                    commands,
                    GalaxyCommand::Combine {
                        explorer_id,
                        resource,
                    },
                );
            }
        }
    }
}

/// External id of the focused planet, if any.
fn focused_planet_id(state: &GameState) -> Option<u32> {
    state.planet_ids.get(state.focus).copied()
}

/// Returns the currently selected explorer's external id, auto-selecting the
/// first live explorer if none is chosen yet. Returns `None` only when there are
/// no explorers at all.
fn selected_explorer(state: &mut GameState, explorers: &Query<&mut Explorer>) -> Option<u32> {
    let ids: Vec<u32> = explorers.iter().map(|e| e.id).collect();
    if ids.is_empty() {
        return None;
    }
    match state.selected_explorer {
        Some(id) if ids.contains(&id) => Some(id),
        _ => {
            let first = ids.into_iter().min();
            state.selected_explorer = first;
            first
        }
    }
}

/// Advances the selection to the next explorer id (wrapping), so a user can pick
/// which explorer the explorer-ops target.
fn cycle_selected_explorer(state: &mut GameState, explorers: &Query<&mut Explorer>) {
    let mut ids: Vec<u32> = explorers.iter().map(|e| e.id).collect();
    if ids.is_empty() {
        state.selected_explorer = None;
        state.set_report(vec!["No explorers to select".to_string()]);
        return;
    }
    ids.sort_unstable();
    let next = match state.selected_explorer {
        Some(cur) => match ids.iter().position(|&id| id == cur) {
            Some(pos) => ids[(pos + 1) % ids.len()],
            None => ids[0],
        },
        None => ids[0],
    };
    state.selected_explorer = Some(next);
    state.set_report(vec![format!("Selected explorer {next}")]);
}

/// Emits a command if a sink is present (feed mode); a no-op in demo mode.
fn emit(commands: &Option<Res<CommandSink>>, command: GalaxyCommand) {
    if let Some(sink) = commands {
        sink.send(command);
    }
}

fn style_buttons(
    mut q: Query<(&Interaction, &mut BackgroundColor, &mut BorderColor), With<Button>>,
) {
    for (inter, mut bg, mut border) in &mut q {
        let (fill, line) = match inter {
            Interaction::Pressed => (theme::BTN_PRESSED, theme::SELECTION),
            Interaction::Hovered => (theme::BTN_HOVER, theme::SELECTION),
            Interaction::None => (theme::BTN_IDLE, theme::BORDER),
        };
        bg.0 = fill;
        border.0 = line;
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

/// Send an explorer to a neighbouring planet. In feed mode this moves the
/// *selected* explorer (so the user controls which one) and lets the simulation
/// route it; in demo mode it moves a random idle explorer locally.
fn request_move_explorer(
    state: &mut GameState,
    explorers: &mut Query<&mut Explorer>,
    commands: &Option<Res<CommandSink>>,
) {
    let mut rng = rand::thread_rng();

    match commands {
        Some(sink) => {
            let Some(explorer_id) = selected_explorer(state, explorers) else {
                state.set_report(vec!["No explorer to move".to_string()]);
                return;
            };
            // Find where the selected explorer currently sits.
            let Some(at) = explorers.iter().find(|e| e.id == explorer_id).map(|e| e.at) else {
                return;
            };
            let neighbors = state.neighbors(at);
            if neighbors.is_empty() {
                state.set_report(vec![format!(
                    "Explorer {explorer_id} has no reachable neighbour"
                )]);
                return;
            }
            let target = neighbors[rng.gen_range(0..neighbors.len())];
            if let Some(&to_planet) = state.planet_ids.get(target) {
                sink.send(GalaxyCommand::MoveExplorer {
                    explorer_id,
                    to_planet,
                });
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
