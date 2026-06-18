use bevy::prelude::Color;

use crate::feed::PlanetKind;

/// The four planet families, with the render facets the visualizer cares about:
/// size, colour, and which capabilities to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanetType {
    A,
    B,
    C,
    D,
}

impl PlanetType {
    pub fn cell_count(self) -> usize {
        match self {
            PlanetType::A | PlanetType::D => 5,
            PlanetType::B | PlanetType::C => 1,
        }
    }

    pub fn can_have_rocket(self) -> bool {
        matches!(self, PlanetType::A | PlanetType::C)
    }

    pub fn radius(self) -> f32 {
        match self {
            PlanetType::A => 1.6,
            PlanetType::B => 1.2,
            PlanetType::C => 1.4,
            PlanetType::D => 1.8,
        }
    }

    pub fn color(self) -> Color {
        match self {
            PlanetType::A => Color::srgb(0.9, 0.5, 0.3),
            PlanetType::B => Color::srgb(0.3, 0.6, 0.85),
            PlanetType::C => Color::srgb(0.7, 0.4, 0.8),
            PlanetType::D => Color::srgb(0.35, 0.75, 0.5),
        }
    }

    pub fn info(self) -> &'static str {
        match self {
            PlanetType::A => "5 cells | 1 recipe | rockets | no combine",
            PlanetType::B => "1 cell | unlimited | no rockets | 1 combine",
            PlanetType::C => "1 cell | 1 recipe | rockets | 6 combines",
            PlanetType::D => "5 cells | unlimited | no rockets | no combine",
        }
    }
}

impl From<PlanetKind> for PlanetType {
    fn from(kind: PlanetKind) -> Self {
        match kind {
            PlanetKind::A => PlanetType::A,
            PlanetKind::B => PlanetType::B,
            PlanetKind::C => PlanetType::C,
            PlanetKind::D => PlanetType::D,
        }
    }
}

/// A cosmetic element badge shown for each planet in the HUD. Snapshots don't
/// carry this, so it's assigned for flavour only.
#[derive(Debug, Clone, Copy)]
pub enum Element {
    Hydrogen,
    Oxygen,
    Carbon,
    Silicon,
}

impl Element {
    pub const ALL: [Element; 4] = [
        Element::Hydrogen,
        Element::Oxygen,
        Element::Carbon,
        Element::Silicon,
    ];

    pub fn symbol(self) -> &'static str {
        match self {
            Element::Hydrogen => "H",
            Element::Oxygen => "O",
            Element::Carbon => "C",
            Element::Silicon => "Si",
        }
    }
}
