//! One flat, shared colour palette for the whole visualizer.
//!
//! Keeping the palette in one place makes the scene feel intentionally designed
//! instead of tuned by scattered magic numbers: matte planet bodies, soft additive
//! glow and quiet UI cards all pull from the same small set of tones.

use bevy::prelude::*;

// --- Backdrop ---------------------------------------------------------------

/// Deep space: a calm, slightly blue near-black the camera clears to.
pub const SPACE: Color = Color::srgb(0.035, 0.04, 0.07);

// --- Planet decor -----------------------------------------------------------

/// A discharged energy cell (matte dark metal-grey).
pub const CELL_OFF: Color = Color::srgb(0.18, 0.2, 0.28);
/// A charged energy cell (warm, glowing).
pub const CELL_ON: Color = Color::srgb(1.0, 0.9, 0.5);
/// Hovering rocket beacon.
pub const ROCKET: Color = Color::srgb(1.0, 0.42, 0.32);
/// Drifting explorer capsule.
pub const EXPLORER: Color = Color::srgb(0.36, 0.95, 0.62);
/// Faint links drawn between connected planets.
pub const CONNECTION: Color = Color::srgb(0.42, 0.6, 0.95);
/// Thin orbit guide-lines.
pub const ORBIT: Color = Color::srgb(0.5, 0.65, 1.0);
/// Flat planetary rings.
pub const RING: Color = Color::srgba(0.9, 0.88, 0.82, 0.22);

// --- Selection --------------------------------------------------------------

/// The halo colour that marks the currently focused planet.
pub const SELECTION: Color = Color::srgb(0.45, 0.85, 1.0);

// --- HUD / UI ---------------------------------------------------------------

/// Frosted card behind the HUD text.
pub const PANEL_BG: Color = Color::srgba(0.07, 0.08, 0.13, 0.82);
/// Hairline border shared by panels and buttons.
pub const BORDER: Color = Color::srgba(0.45, 0.55, 0.85, 0.35);

/// Primary heading text.
pub const TEXT_TITLE: Color = Color::srgb(0.94, 0.95, 1.0);
/// Body / detail text.
pub const TEXT_BODY: Color = Color::srgb(0.74, 0.8, 0.93);
/// Button captions.
pub const TEXT_BTN: Color = Color::srgb(0.9, 0.92, 0.98);

/// Button faces in their three interaction states.
pub const BTN_IDLE: Color = Color::srgba(0.12, 0.14, 0.22, 0.92);
pub const BTN_HOVER: Color = Color::srgba(0.2, 0.24, 0.36, 0.96);
pub const BTN_PRESSED: Color = Color::srgba(0.32, 0.42, 0.68, 1.0);

/// Scales a colour's RGB (leaving alpha visually opaque for emissive use).
pub fn glow(color: Color, intensity: f32) -> LinearRgba {
    let c = color.to_linear();
    LinearRgba::new(
        c.red * intensity,
        c.green * intensity,
        c.blue * intensity,
        1.0,
    )
}
