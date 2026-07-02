//! The viewer's window onto the galaxy: the easing camera and the text HUD.

use bevy::prelude::*;

use crate::command::{BASIC_CHOICES, COMPLEX_CHOICES};
use crate::domain::components::{Explorer, Hud, MainCamera, Planet, SelectionGlow};
use crate::domain::state::{GameState, Mode, Source};

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (update_camera, update_selection_glow, update_hud));
    }
}

fn update_camera(
    time: Res<Time>,
    state: Res<GameState>,
    mut q: Query<&mut Transform, With<MainCamera>>,
) {
    let Ok(mut tf) = q.get_single_mut() else {
        return;
    };
    if state.positions.is_empty() {
        return;
    }

    let (target, look) = match state.mode {
        Mode::Galaxy => {
            let a = time.elapsed_seconds() * 0.025;
            (
                Vec3::new(
                    state.zoom * 0.5 * a.sin(),
                    state.zoom * 0.55,
                    state.zoom * 0.7 * a.cos(),
                ),
                Vec3::ZERO,
            )
        }
        Mode::Planet => {
            let pos = state.focus_position().unwrap_or(Vec3::ZERO);
            (
                pos + Vec3::new(0.0, state.zoom * 0.25, state.zoom * 0.4),
                pos,
            )
        }
    };

    tf.translation = tf.translation.lerp(target, 0.035);
    tf.look_at(look, Vec3::Y);
}

fn update_selection_glow(
    time: Res<Time>,
    state: Res<GameState>,
    planets: Query<&Planet>,
    mut glow: Query<(&mut Transform, &mut Visibility), With<SelectionGlow>>,
) {
    let Ok((mut transform, mut visibility)) = glow.get_single_mut() else {
        return;
    };

    let Some(position) = state.focus_position() else {
        *visibility = Visibility::Hidden;
        return;
    };

    let Some(planet) = planets.iter().find(|p| p.id == state.focus && p.alive) else {
        *visibility = Visibility::Hidden;
        return;
    };

    let pulse = 1.0 + 0.08 * (time.elapsed_seconds() * 3.4).sin();
    transform.translation = position;
    transform.scale = Vec3::splat(planet.planet_type.radius() * 1.52 * pulse);
    *visibility = Visibility::Visible;
}

fn update_hud(
    state: Res<GameState>,
    planets: Query<&Planet>,
    explorers: Query<&Explorer>,
    mut text: Query<&mut Text, With<Hud>>,
) {
    let Ok(mut t) = text.get_single_mut() else {
        return;
    };

    if !state.built {
        t.sections[0].value = "Waiting for galaxy data\n".into();
        t.sections[1].value = "The map will appear as soon as the first snapshot arrives.".into();
        return;
    }

    let live = matches!(state.source, Source::Feed);

    t.sections[0].value = match state.mode {
        Mode::Galaxy => {
            if live {
                "Stargazers Galaxy  -  live\n".into()
            } else {
                "Stargazers Galaxy  -  demo\n".into()
            }
        }
        Mode::Planet => {
            let kind = planets
                .iter()
                .find(|p| p.id == state.focus)
                .map(|p| format!("{:?}", p.planet_type))
                .unwrap_or_default();
            format!("Selected planet {}  -  Type {}\n", state.focus, kind)
        }
    };

    if let Some(p) = planets.iter().find(|p| p.id == state.focus) {
        let charged = p.cells_charged.iter().filter(|&&c| c).count();
        let mut here: Vec<u32> = explorers
            .iter()
            .filter(|e| e.at == p.id && e.target.is_none())
            .map(|e| e.id)
            .collect();
        here.sort_unstable();
        let here = if here.is_empty() {
            "none".to_string()
        } else {
            here.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Which explorer manual explorer-ops target, and what Generate/Combine
        // will craft, so the user can see the current selection before acting.
        let selected = match state.selected_explorer {
            Some(id) => id.to_string(),
            None => "none".to_string(),
        };
        let next_basic = format!(
            "{:?}",
            BASIC_CHOICES[state.basic_choice % BASIC_CHOICES.len()]
        );
        let next_complex = format!(
            "{:?}",
            COMPLEX_CHOICES[state.complex_choice % COMPLEX_CHOICES.len()]
        );

        let status = match (p.alive, state.paused) {
            (false, true) => "\nStatus  destroyed - animation paused",
            (false, false) => "\nStatus  destroyed",
            (true, true) => "\nStatus  animation paused",
            (true, false) => "",
        };

        let mut body = format!(
            "{}\n\nEnergy   {}/{}\nRocket   {}\nElement  {}\nVisitors {}\n\nExplorer {}\nAI       {}\nGenerate > {}\nCombine  > {}{}",
            p.planet_type.info(),
            charged,
            p.cells_charged.len(),
            yes_no(p.has_rocket),
            p.element.symbol(),
            here,
            selected,
            if state.ai_paused { "paused" } else { "running" },
            next_basic,
            next_complex,
            status,
        );

        // The report panel: answers to the user's last query (bag, recipes, ...).
        if !state.reports.is_empty() {
            body.push_str("\n\nReport\n");
            body.push_str(&state.reports.join("\n"));
        }

        t.sections[1].value = body;
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
