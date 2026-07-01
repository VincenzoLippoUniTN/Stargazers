use bevy::prelude::*;

use super::planet::{Element, PlanetType};

/// A planet's live state. The visual entities (body, cells, rocket) read from
/// this every frame, so updating these fields is all it takes to change the view.
#[derive(Component)]
pub struct Planet {
    pub id: usize,
    pub planet_type: PlanetType,
    pub cells_charged: Vec<bool>,
    pub has_rocket: bool,
    pub element: Element,
    pub spin: f32,
    pub alive: bool,
}

/// Tags the static visuals owned by a planet (body, glow, ring, orbit) by index,
/// so they can be hidden together when the planet dies.
#[derive(Component)]
pub struct OfPlanet(pub usize);

#[derive(Component)]
pub struct Sun;

/// One layer of the sun's corona; the field is the layer number.
#[derive(Component)]
pub struct Corona(pub usize);

#[derive(Component)]
pub struct Cell {
    pub planet: usize,
    pub index: usize,
    pub angle: f32,
    pub speed: f32,
    /// Last charge state pushed to the material, so we only repaint on change.
    pub lit: bool,
}

#[derive(Component)]
pub struct Rocket {
    pub planet: usize,
    pub phase: f32,
}

#[derive(Component)]
pub struct Explorer {
    pub id: u32,
    pub at: usize,
    pub target: Option<usize>,
    pub progress: f32,
    pub angle: f32,
}

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct Hud;

#[derive(Component)]
pub struct Btn(pub Action);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // --- View controls (client-side only) ---
    Mode,
    Prev,
    Next,
    Sunray,
    Pause,
    ZoomIn,
    ZoomOut,
    // --- Planet operations (act on the focused planet) ---
    Asteroid,
    KillPlanet,
    ToggleAi,
    // --- Explorer operations (act on the selected explorer) ---
    Move,
    SelExplorer,
    KillExplorer,
    ResetExplorer,
    Bag,
    Resources,
    Combinations,
    CycleBasic,
    Generate,
    CycleComplex,
    Combine,
}

impl Action {
    /// The buttons shown in the bottom bar, in order. Kept in one place so the
    /// bar, the keyboard shortcuts and the click handler can't drift apart.
    pub const BAR: [Action; 21] = [
        Action::Mode,
        Action::Prev,
        Action::Next,
        Action::Sunray,
        Action::Pause,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::Asteroid,
        Action::KillPlanet,
        Action::ToggleAi,
        Action::Move,
        Action::SelExplorer,
        Action::KillExplorer,
        Action::ResetExplorer,
        Action::Bag,
        Action::Resources,
        Action::Combinations,
        Action::CycleBasic,
        Action::Generate,
        Action::CycleComplex,
        Action::Combine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::Mode => "Mode [P]",
            Action::Prev => "Previous [<-]",
            Action::Next => "Next [->]",
            Action::Sunray => "Sun ray [S]",
            Action::Pause => "Pause [Space]",
            Action::ZoomIn => "Zoom in",
            Action::ZoomOut => "Zoom out",
            Action::Asteroid => "Asteroid [A]",
            Action::KillPlanet => "Kill planet [K]",
            Action::ToggleAi => "Toggle AI [I]",
            Action::Move => "Move explorer [E]",
            Action::SelExplorer => "Sel explorer [X]",
            Action::KillExplorer => "Kill explorer [J]",
            Action::ResetExplorer => "Reset explorer [R]",
            Action::Bag => "Bag [B]",
            Action::Resources => "Resources [1]",
            Action::Combinations => "Combines [2]",
            Action::CycleBasic => "Basic+ [N]",
            Action::Generate => "Generate [G]",
            Action::CycleComplex => "Complex+ [M]",
            Action::Combine => "Combine [C]",
        }
    }
}
