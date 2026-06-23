//! Builds the 3D world: the sun, camera, lights and HUD once at startup, then the
//! planets, cells, rockets and explorers from a [`GalaxyLayout`] when one is ready.

mod mesh;

use bevy::prelude::*;
use rand::prelude::*;
use std::f32::consts::TAU;

use crate::domain::components::*;
use crate::domain::layout::{GalaxyLayout, PlanetInit};
use crate::domain::state::{GameState, Source};
use crate::VisualizerSet;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene)
            .add_systems(Update, build_galaxy.in_set(VisualizerSet::Build));
    }
}

/// Spawns everything that doesn't depend on the layout: sun, camera, lights, UI.
/// In demo mode it also queues a random layout to build.
fn setup_scene(
    mut cmd: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut state: ResMut<GameState>,
) {
    state.zoom = 55.0;

    cmd.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(3.0).mesh().ico(5).unwrap()),
            material: mats.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.98, 0.9),
                emissive: LinearRgba::new(4.0, 3.5, 2.0, 1.0),
                ..default()
            }),
            ..default()
        },
        Sun,
    ));

    for layer in 1..=4 {
        cmd.spawn((
            PbrBundle {
                mesh: meshes.add(
                    Sphere::new(3.0 * (1.0 + layer as f32 * 0.25))
                        .mesh()
                        .ico(3)
                        .unwrap(),
                ),
                material: mats.add(StandardMaterial {
                    base_color: Color::srgba(1.0, 0.9, 0.6, 0.25 / layer as f32),
                    emissive: LinearRgba::new(0.8, 0.5, 0.2, 1.0),
                    alpha_mode: AlphaMode::Add,
                    ..default()
                }),
                ..default()
            },
            Corona(layer),
        ));
    }

    cmd.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(0.0, 45.0, 65.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        MainCamera,
    ));

    cmd.spawn(PointLightBundle {
        point_light: PointLight {
            intensity: 12_000_000.0,
            range: 250.0,
            color: Color::srgb(1.0, 0.97, 0.92),
            shadows_enabled: true,
            ..default()
        },
        ..default()
    });

    cmd.insert_resource(AmbientLight {
        color: Color::srgb(0.4, 0.45, 0.6),
        brightness: 60.0,
    });

    spawn_ui(&mut cmd);

    if state.source == Source::Demo {
        state.pending = Some(GalaxyLayout::demo());
    }
}

/// Turns a pending [`GalaxyLayout`] into entities. Runs once, when a layout is
/// available (immediately in demo mode, on the first snapshot in feed mode).
fn build_galaxy(
    mut cmd: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut state: ResMut<GameState>,
) {
    if state.built {
        return;
    }
    let Some(layout) = state.pending.take() else {
        return;
    };
    let mut rng = rand::thread_rng();

    for (index, p) in layout.planets.iter().enumerate() {
        spawn_planet(&mut cmd, &mut meshes, &mut mats, &mut rng, index, p);
    }

    let connection_mat = mats.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.6, 0.9, 0.08),
        emissive: LinearRgba::new(0.05, 0.08, 0.15, 1.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });

    for &(a, b) in &layout.edges {
        let p1 = layout.planets[a].position;
        let p2 = layout.planets[b].position;
        let dir = p2 - p1;
        cmd.spawn(PbrBundle {
            mesh: meshes.add(Cylinder::new(0.04, dir.length())),
            material: connection_mat.clone(),
            transform: Transform::from_translation((p1 + p2) * 0.5)
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, dir.normalize())),
            ..default()
        });
    }

    for e in &layout.explorers {
        cmd.spawn((
            PbrBundle {
                mesh: meshes.add(Capsule3d::new(0.22, 0.4)),
                material: mats.add(StandardMaterial {
                    base_color: Color::srgb(0.3, 1.0, 0.6),
                    emissive: LinearRgba::new(0.2, 0.8, 0.4, 1.0),
                    metallic: 0.6,
                    ..default()
                }),
                transform: Transform::from_translation(layout.planets[e.at].position),
                ..default()
            },
            Explorer {
                id: e.id,
                at: e.at,
                target: None,
                progress: 0.0,
                angle: rng.gen_range(0.0..TAU),
            },
        ));
    }

    state.positions = layout.planets.iter().map(|p| p.position).collect();
    state.planet_ids = layout.planets.iter().map(|p| p.id).collect();
    state.edges = layout.edges;
    state.built = true;
}

fn spawn_planet(
    cmd: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    rng: &mut ThreadRng,
    index: usize,
    p: &PlanetInit,
) {
    let kind = p.kind;
    let pos = p.position;

    cmd.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(kind.radius()).mesh().ico(5).unwrap()),
            material: mats.add(StandardMaterial {
                base_color: kind.color(),
                metallic: 0.05,
                perceptual_roughness: 0.75,
                ..default()
            }),
            transform: Transform::from_translation(pos),
            ..default()
        },
        Planet {
            id: index,
            planet_type: kind,
            cells_charged: p.cells.clone(),
            has_rocket: p.has_rocket,
            element: p.element,
            spin: rng.gen_range(0.08..0.2),
            alive: p.alive,
        },
        OfPlanet(index),
    ));

    let c = kind.color().to_srgba();
    cmd.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(kind.radius() * 1.12).mesh().ico(3).unwrap()),
            material: mats.add(StandardMaterial {
                base_color: Color::srgba(c.red * 1.2, c.green * 1.2, c.blue * 1.2, 0.08),
                alpha_mode: AlphaMode::Add,
                ..default()
            }),
            transform: Transform::from_translation(pos),
            ..default()
        },
        OfPlanet(index),
    ));

    if index.is_multiple_of(2) {
        let tilt = Quat::from_euler(
            EulerRot::XYZ,
            rng.gen_range(0.3..0.6),
            0.0,
            rng.gen_range(-0.15..0.15),
        );
        cmd.spawn((
            PbrBundle {
                mesh: meshes.add(mesh::ring(kind.radius() * 1.5, kind.radius() * 2.2)),
                material: mats.add(StandardMaterial {
                    base_color: Color::srgba(0.9, 0.85, 0.8, 0.25),
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                }),
                transform: Transform::from_translation(pos).with_rotation(tilt),
                ..default()
            },
            OfPlanet(index),
        ));
    }

    let orbit_tilt = Quat::from_euler(EulerRot::XYZ, rng.gen_range(0.4..0.7), 0.0, 0.0);
    cmd.spawn((
        PbrBundle {
            mesh: meshes.add(mesh::orbit(kind.radius() + 1.5, 0.8, orbit_tilt)),
            material: mats.add(StandardMaterial {
                base_color: Color::srgba(0.6, 0.75, 1.0, 0.1),
                emissive: LinearRgba::new(0.15, 0.2, 0.35, 1.0),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            }),
            transform: Transform::from_translation(pos),
            ..default()
        },
        OfPlanet(index),
    ));

    let cell_count = kind.cell_count();
    for ci in 0..cell_count {
        cmd.spawn((
            PbrBundle {
                mesh: meshes.add(Sphere::new(0.18).mesh().ico(2).unwrap()),
                material: mats.add(StandardMaterial {
                    base_color: Color::srgb(0.2, 0.2, 0.25),
                    metallic: 0.8,
                    ..default()
                }),
                transform: Transform::from_translation(pos),
                ..default()
            },
            Cell {
                planet: index,
                index: ci,
                angle: (ci as f32 / cell_count as f32) * TAU,
                speed: rng.gen_range(0.35..0.55),
                lit: false,
            },
        ));
    }

    if kind.can_have_rocket() {
        cmd.spawn((
            PbrBundle {
                mesh: meshes.add(Capsule3d::new(0.15, 0.6)),
                material: mats.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.35, 0.25),
                    emissive: LinearRgba::new(0.5, 0.15, 0.1, 1.0),
                    metallic: 0.9,
                    ..default()
                }),
                transform: Transform::from_translation(pos + Vec3::Y * (kind.radius() + 1.2)),
                visibility: if p.has_rocket {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                },
                ..default()
            },
            Rocket {
                planet: index,
                phase: rng.gen_range(0.0..TAU),
            },
        ));
    }
}

fn spawn_ui(cmd: &mut Commands) {
    cmd.spawn(NodeBundle {
        style: Style {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        ..default()
    })
    .with_children(|p| {
        p.spawn((
            TextBundle::from_sections([
                TextSection::new(
                    "Galaxy Overview\n",
                    TextStyle {
                        font_size: 24.0,
                        color: Color::srgb(0.85, 0.85, 0.9),
                        ..default()
                    },
                ),
                TextSection::new(
                    "",
                    TextStyle {
                        font_size: 16.0,
                        color: Color::srgb(0.7, 0.65, 0.5),
                        ..default()
                    },
                ),
            ])
            .with_style(Style {
                margin: UiRect::all(Val::Px(15.0)),
                ..default()
            }),
            Hud,
        ));

        p.spawn(NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(10.0)),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            background_color: Color::srgba(0.0, 0.0, 0.0, 0.5).into(),
            ..default()
        })
        .with_children(|p| {
            for action in Action::BAR {
                p.spawn((
                    ButtonBundle {
                        style: Style {
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            ..default()
                        },
                        background_color: Color::srgba(0.15, 0.15, 0.2, 0.9).into(),
                        ..default()
                    },
                    Btn(action),
                ))
                .with_children(|p| {
                    p.spawn(TextBundle::from_section(
                        action.label(),
                        TextStyle {
                            font_size: 13.0,
                            color: Color::srgb(0.85, 0.85, 0.9),
                            ..default()
                        },
                    ));
                });
            }
        });
    });
}
