// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

use crate::components::{ForkPtr, Orientation, StackPtr};
use crate::tiler::EventQueue;
use crate::{Entity, Placement, Rect};
use ghost_cell::{GhostCell, GhostToken};
use std::fmt::{self, Debug};
use std::rc::Rc;

/// An ID assigned to a window by a window manager.
#[derive(Copy, Clone, From, Into, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct WindowID(pub u32, pub u32);

/// Pointer to reference-counted window managed by a `GhostCell`.
#[derive(Clone, Deref, DerefMut)]
pub struct WindowPtr<'g>(pub(crate) Rc<GhostCell<'g, Window<'g>>>);

impl<'g> WindowPtr<'g> {
    /// The ID assigned to the window by the window manager.
    pub fn id(&self, t: &GhostToken<'g>) -> WindowID {
        self.borrow(t).id
    }

    /// Get a pointer to the parent fork assocation.
    pub(crate) fn fork(&self, t: &GhostToken<'g>) -> Option<ForkPtr<'g>> {
        self.borrow(t).fork.clone()
    }

    /// Remove the parent fork association and return it.
    pub(crate) fn fork_take(&self, t: &mut GhostToken<'g>) -> Option<ForkPtr<'g>> {
        self.borrow_mut(t).fork.take()
    }

    /// Set the parent fork association for this window.
    pub(crate) fn fork_set(&self, fork: ForkPtr<'g>, t: &mut GhostToken<'g>) {
        self.borrow_mut(t).workspace = fork.borrow(t).workspace;
        self.borrow_mut(t).fork = Some(fork);
    }

    /// Toggle the orientation of the fork that owns this window.
    pub(crate) fn orientation_toggle(&self, t: &mut GhostToken<'g>) {
        if let Some(fork) = self.fork(t) {
            let this = fork.borrow_mut(t);

            this.orientation = match this.orientation {
                Orientation::Horizontal => Orientation::Vertical,
                Orientation::Vertical => Orientation::Horizontal,
            };

            fork.work_area_refresh(t);
        }
    }

    /// Get the pointer to the stack it may be associated with.
    pub(crate) fn stack(&self, t: &GhostToken<'g>) -> Option<StackPtr<'g>> {
        self.borrow(t).stack.clone()
    }

    /// Swaps the tree location of this window with another.
    pub(crate) fn swap_position_with(&self, other: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        if let Some(stack) = self.stack(t) {
            stack.swap(self, other, t);
            stack.work_area_refresh(t);
        } else if let Some(fork) = self.fork(t) {
            fork.swap(self, other, t);
            fork.work_area_refresh(t);
        }

        if let Some(stack) = other.stack(t) {
            stack.swap(other, self, t);
            stack.work_area_refresh(t)
        } else if let Some(fork) = other.fork(t) {
            fork.swap(other, self, t);
            fork.work_area_refresh(t);
        }
    }

    /// Update the position and dimensions of this window.
    pub(crate) fn work_area_update(&self, area: Rect, t: &mut GhostToken<'g>) {
        let this = self.borrow_mut(t);
        if this.rect != area {
            this.rect = area;
        }

        let id = this.id;
        let workspace = this.workspace;

        this.event_queue
            .clone()
            .borrow_mut(t)
            .placements
            .insert(Entity::Window(id), Placement { area, workspace });
    }
}

pub struct Window<'g> {
    pub(crate) fork: Option<ForkPtr<'g>>,
    pub(crate) id: WindowID,
    pub(crate) rect: Rect,
    pub(crate) stack: Option<StackPtr<'g>>,
    pub(crate) workspace: u32,
    pub(crate) visible: bool,

    event_queue: Rc<GhostCell<'g, EventQueue>>,
}

impl<'g> Window<'g> {
    pub(crate) fn new<I: Into<WindowID>>(
        id: I,
        event_queue: Rc<GhostCell<'g, EventQueue>>,
    ) -> Self {
        Self {
            event_queue,
            fork: None::<ForkPtr<'g>>,
            id: id.into(),
            rect: Rect::new(1, 1, 1, 1),
            stack: None,
            workspace: 0,
            visible: true,
        }
    }

    pub fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> WindowDebug<'a, 'g> {
        WindowDebug::new(self, t)
    }
}

pub struct WindowDebug<'a, 'g> {
    window: &'a Window<'g>,
    _t: &'a GhostToken<'g>,
}

impl<'a, 'g> WindowDebug<'a, 'g> {
    fn new(window: &'a Window<'g>, t: &'a GhostToken<'g>) -> Self {
        Self { window, _t: t }
    }
}

impl<'a, 'g> Debug for WindowDebug<'a, 'g> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Window")
            .field("id", &self.window.id)
            .field("fork", &self.window.fork.as_ref().map(|p| p.as_ptr()))
            .field("stack", &self.window.stack.as_ref().map(|p| p.as_ptr()))
            .field("workspace", &self.window.workspace)
            .field("rect", &self.window.rect)
            .finish()
    }
}

impl Debug for WindowID {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "WindowID({}, {})", self.0, self.1)
    }
}
