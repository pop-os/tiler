// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

#[macro_use]
extern crate derive_more;

mod components;
mod rect;
mod tiler;

pub use self::components::{WindowID, WindowPtr};
pub use self::rect::Rect;
pub use self::tiler::Tiler;

/// Instructs where to place a tiling component entity.
#[derive(Debug)]
pub struct Placement {
    pub area: Rect,
    pub workspace: u32,
}

/// An event for the window manager to act upon.
#[derive(Debug)]
pub enum Event {
    /// Move focus to this window.
    Focus(WindowID),

    /// Place window or stack in this location.
    Place(Entity, Placement),

    /// Change visibility of this window or stack.
    Show(Entity, bool),
}

/// The object being acted upon.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Entity {
    Window(WindowID),
    Stack(usize),
}
