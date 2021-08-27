// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::display::DisplayPtr;
use crate::fork::ForkPtr;
use crate::geom::Rect;
use crate::window::WindowPtr;
use ghost_cell::{GhostCell, GhostToken};
use std::fmt::{self, Debug};
use std::rc::Rc;

/// A virtual workspace, which may be assigned to a display, and may have a focused window.
#[derive(Clone, Deref, DerefMut)]
pub(crate) struct WorkspacePtr<'g>(Rc<GhostCell<'g, Workspace<'g>>>);

impl<'g> WorkspacePtr<'g> {
    pub fn new(id: u32, parent: DisplayPtr<'g>) -> Self {
        Self(Rc::new(GhostCell::new(Workspace {
            id,
            focus: None,
            fork: None,
            parent,
        })))
    }

    pub fn area(&self, t: &GhostToken<'g>) -> Rect {
        self.borrow(t).parent.area(t)
    }

    pub fn fork(&self, t: &GhostToken<'g>) -> Option<ForkPtr<'g>> {
        self.borrow(t).fork.clone()
    }

    pub fn id(&self, t: &GhostToken<'g>) -> u32 {
        self.borrow(t).id
    }
}

/// A virtual workspace, which may be assigned to a display, and may have a focused window.
pub(crate) struct Workspace<'g> {
    pub id: u32,
    pub focus: Option<WindowPtr<'g>>,
    pub fork: Option<ForkPtr<'g>>,
    pub parent: DisplayPtr<'g>,
}

impl<'g> Workspace<'g> {
    pub(crate) fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> WorkspaceDebug<'a, 'g> {
        WorkspaceDebug::new(self, t)
    }
}

pub(crate) struct WorkspaceDebug<'a, 'g> {
    info: &'a Workspace<'g>,
    t: &'a GhostToken<'g>,
}

impl<'a, 'g> WorkspaceDebug<'a, 'g> {
    pub fn new(info: &'a Workspace<'g>, t: &'a GhostToken<'g>) -> Self {
        Self { info, t }
    }
}

impl<'a, 'g> Debug for WorkspaceDebug<'a, 'g> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let focus = self.info.focus.as_ref().map(|win| win.id(self.t));
        fmt.debug_struct("Workspace")
            .field("focus", &focus)
            .field("fork", &self.info.fork.as_ref().map(|f| f.debug(self.t)))
            .finish()
    }
}
