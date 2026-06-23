//! The data layer: planet types, ECS components, world state and galaxy layout.
//!
//! Nothing here runs systems or touches the renderer; it only describes *what* a
//! galaxy is. The feature plugins (`scene`, `sync`, `motion`, `interaction`,
//! `view`) build on top of these types.

pub mod components;
pub mod layout;
pub mod planet;
pub mod state;
