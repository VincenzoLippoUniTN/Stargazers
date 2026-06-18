//! Player input: keyboard shortcuts, the mouse wheel, and the on-screen buttons.
//! Both the keyboard and the buttons funnel into the same handful of actions.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use rand::prelude::*;

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
        move_explorer(&state, &mut explorers);
    }
    if keys.just_pressed(KeyCode::KeyS) {
        send_sunray(state.focus, &mut planets);
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
) {
    for (inter, btn) in &interaction {
        if *inter != Interaction::Pressed {
            continue;
        }
        match btn.0 {
            Action::Mode => state.toggle_mode(),
            Action::Prev => state.prev(),
            Action::Next => state.next(),
            Action::Move => move_explorer(&state, &mut explorers),
            Action::Sunray => send_sunray(state.focus, &mut planets),
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

/// Sends the first idle explorer toward a random neighbouring planet.
fn move_explorer(state: &GameState, explorers: &mut Query<&mut Explorer>) {
    let mut rng = rand::thread_rng();
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

/// Charges the first empty cell of the focused planet.
fn send_sunray(focus: usize, planets: &mut Query<&mut Planet>) {
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
