//! Godot client (GDExtension). Exposes the `SpaceGame` node.

mod game;
pub mod net;
pub mod state;
mod visuals;

use godot::prelude::*;

struct SpaceGameExtension;

#[gdextension]
unsafe impl ExtensionLibrary for SpaceGameExtension {}
