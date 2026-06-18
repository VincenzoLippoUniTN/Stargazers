//! Idle motion: planet spin, orbiting cells and rockets, drifting explorers and
//! the pulsing sun. All of it pauses with [`GameState::paused`].

use bevy::prelude::*;
use std::f32::consts::PI;

use crate::domain::components::{Cell, Corona, Explorer, Planet, Rocket};
use crate::domain::state::GameState;

pub struct MotionPlugin;

impl Plugin for MotionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                animate_planets,
                animate_cells,
                animate_rockets,
                animate_explorers,
                animate_sun,
            ),
        );
    }
}

fn animate_planets(time: Res<Time>, state: Res<GameState>, mut q: Query<(&mut Transform, &Planet)>) {
    if state.paused {
        return;
    }
    let dt = time.delta_seconds();
    for (mut t, p) in &mut q {
        t.rotate_y(p.spin * dt);
    }
}

fn animate_cells(
    time: Res<Time>,
    state: Res<GameState>,
    planets: Query<(&Planet, &Transform), Without<Cell>>,
    mut cells: Query<(&mut Transform, &Cell), Without<Planet>>,
) {
    if state.paused {
        return;
    }
    let elapsed = time.elapsed_seconds();
    let tilt = Quat::from_euler(EulerRot::XYZ, 0.5, 0.0, 0.0);

    for (mut cell_tf, cell) in &mut cells {
        let Some((planet, planet_tf)) = planets.iter().find(|(p, _)| p.id == cell.planet) else {
            continue;
        };
        let r = planet.planet_type.radius() + 1.5;
        let a = cell.angle + elapsed * cell.speed;
        let offset = tilt * Vec3::new(r * a.cos(), 0.0, r * 0.8 * a.sin());

        cell_tf.translation = planet_tf.translation + offset;
        cell_tf.scale = Vec3::splat(1.0 + 0.1 * (elapsed * 3.0 + cell.index as f32).sin());
    }
}

fn animate_rockets(
    time: Res<Time>,
    state: Res<GameState>,
    planets: Query<(&Planet, &Transform), Without<Rocket>>,
    mut rockets: Query<(&mut Transform, &Rocket), Without<Planet>>,
) {
    if state.paused {
        return;
    }
    let t = time.elapsed_seconds();

    for (mut rocket_tf, rocket) in &mut rockets {
        let Some((planet, planet_tf)) = planets.iter().find(|(p, _)| p.id == rocket.planet) else {
            continue;
        };
        let hover = (t * 2.0 + rocket.phase).sin() * 0.2;
        let orbit = t * 0.5;
        let offset = Vec3::new(
            0.6 * orbit.cos(),
            planet.planet_type.radius() + 1.0 + hover,
            0.6 * orbit.sin(),
        );

        rocket_tf.translation = planet_tf.translation + offset;
        rocket_tf.rotation = Quat::from_euler(EulerRot::XYZ, 0.0, -orbit, 0.15);
    }
}

fn animate_explorers(time: Res<Time>, state: Res<GameState>, mut q: Query<(&mut Transform, &mut Explorer)>) {
    if state.paused {
        return;
    }
    let dt = time.delta_seconds();
    let t = time.elapsed_seconds();

    for (mut tf, mut ex) in &mut q {
        match ex.target {
            Some(target) => {
                ex.progress += dt * 0.35;
                let (Some(from), Some(to)) = (state.positions.get(ex.at), state.positions.get(target))
                else {
                    continue;
                };
                if ex.progress >= 1.0 {
                    ex.at = target;
                    ex.target = None;
                    ex.progress = 0.0;
                } else {
                    // Quadratic bezier arc that lifts off the galactic plane.
                    let mid = (*from + *to) * 0.5 + Vec3::Y * 8.0;
                    let s = ex.progress;
                    let inv = 1.0 - s;
                    tf.translation = inv * inv * *from + 2.0 * inv * s * mid + s * s * *to;
                    let dir = (*to - *from).normalize_or_zero();
                    if dir != Vec3::ZERO {
                        tf.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
                    }
                }
            }
            None => {
                let Some(home) = state.positions.get(ex.at) else {
                    continue;
                };
                ex.angle += dt * 0.5;
                tf.translation = *home
                    + Vec3::new(4.0 * ex.angle.cos(), 0.5 * (t * 0.8).sin(), 4.0 * ex.angle.sin());
                tf.rotation = Quat::from_rotation_y(-ex.angle + PI * 0.5);
            }
        }
    }
}

fn animate_sun(time: Res<Time>, mut q: Query<(&mut Transform, &Corona)>) {
    let t = time.elapsed_seconds();
    for (mut tf, corona) in &mut q {
        let pulse = 1.0 + 0.03 * (t * (1.5 - corona.0 as f32 * 0.2)).sin();
        tf.scale = Vec3::splat(pulse);
        tf.rotate_y(0.01 * (corona.0 as f32 + 1.0));
    }
}
