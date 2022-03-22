// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

#[macro_use]
extern crate derive_more;

#[cfg(feature = "ipc")]
#[macro_use]
extern crate serde;

mod branch;
mod display;
mod events;
mod fork;
mod geom;
mod stack;
mod tiler;
mod window;
mod workspace;

pub use self::events::{Event, ForkUpdate, Placement};
pub use self::fork::Orientation;
pub use self::geom::{Point, Rect};
pub use self::stack::StackMovement;
pub use self::tiler::Tiler;
pub use self::window::{WindowID, WindowPtr};

pub use qcell::TCellOwner;
