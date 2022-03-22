// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::display::DisplayPtr;
use crate::fork::ForkPtr;
use crate::geom::Rect;
use crate::window::WindowPtr;
use qcell::{TCell, TCellOwner};
use std::fmt::{self, Debug};
use std::rc::Rc;

/// A virtual workspace, which may be assigned to a display, and may have a focused window.
#[derive(Deref, DerefMut)]
pub(crate) struct WorkspacePtr<T: 'static>(Rc<TCell<T, Workspace<T>>>);
impl<T: 'static> Clone for WorkspacePtr<T> {
    fn clone(&self) -> WorkspacePtr<T> {
        WorkspacePtr(self.0.clone())
    }
}

impl<T: 'static> WorkspacePtr<T> {
    pub fn new(id: u32, parent: DisplayPtr<T>) -> Self {
        Self(Rc::new(TCell::new(Workspace {
            id,
            focus: None,
            fork: None,
            parent,
        })))
    }

    pub fn area(&self, t: &TCellOwner<T>) -> Rect {
        self.ro(t).parent.area(t)
    }

    pub fn fork(&self, t: &TCellOwner<T>) -> Option<ForkPtr<T>> {
        self.ro(t).fork.clone()
    }

    pub fn id(&self, t: &TCellOwner<T>) -> u32 {
        self.ro(t).id
    }
}

/// A virtual workspace, which may be assigned to a display, and may have a focused window.
pub(crate) struct Workspace<T: 'static> {
    pub id: u32,
    pub focus: Option<WindowPtr<T>>,
    pub fork: Option<ForkPtr<T>>,
    pub parent: DisplayPtr<T>,
}

impl<T: 'static> Workspace<T> {
    pub(crate) fn debug<'a>(&'a self, t: &'a TCellOwner<T>) -> WorkspaceDebug<'a, T> {
        WorkspaceDebug::new(self, t)
    }
}

pub(crate) struct WorkspaceDebug<'a, T: 'static> {
    info: &'a Workspace<T>,
    t: &'a TCellOwner<T>,
}

impl<'a, T> WorkspaceDebug<'a, T> {
    pub fn new(info: &'a Workspace<T>, t: &'a TCellOwner<T>) -> Self {
        Self { info, t }
    }
}

impl<'a, T> Debug for WorkspaceDebug<'a, T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let focus = self.info.focus.as_ref().map(|win| win.id(self.t));
        fmt.debug_struct("Workspace")
            .field("focus", &focus)
            .field("fork", &self.info.fork.as_ref().map(|f| f.debug(self.t)))
            .finish()
    }
}
