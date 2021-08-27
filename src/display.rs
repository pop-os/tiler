// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::workspace::WorkspacePtr;
use crate::{Rect, Tiler};
use ghost_cell::{GhostCell, GhostToken};
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::rc::Rc;

/// A physical display, which has physical dimensions, and may have multiple workspaces associated with it.
#[derive(Clone, Deref, DerefMut)]
pub(crate) struct DisplayPtr<'g>(Rc<GhostCell<'g, Display<'g>>>);

/// A physical display, which has physical dimensions, and may have multiple workspaces associated with it.
#[derive(Default)]
pub(crate) struct Display<'g> {
    pub area: Rect,
    pub active: Option<u32>,
    pub workspaces: HashMap<u32, WorkspacePtr<'g>>,
}

impl<'g> DisplayPtr<'g> {
    pub fn new(area: Rect) -> Self {
        Self(Rc::new(GhostCell::new(Display {
            area,
            active: None,
            workspaces: HashMap::new(),
        })))
    }

    pub fn area(&self, t: &GhostToken<'g>) -> Rect {
        self.borrow(t).area
    }

    /// Assign a workspace to this display, removing the previous parent association of
    /// that workspace.
    pub fn assign_workspace(&self, workspace: WorkspacePtr<'g>, t: &mut GhostToken<'g>) {
        // Assign workspace as a child of this display.
        {
            let id = workspace.borrow(t).id;
            let this = self.borrow_mut(t);

            for ours in this.workspaces.values() {
                if Rc::ptr_eq(ours, &workspace) {
                    return;
                }
            }

            this.workspaces.insert(id, workspace.clone());
        }

        let previous_parent;

        // Define a new parent association for the workspace.
        {
            let workspace = workspace.borrow_mut(t);

            if Rc::ptr_eq(&workspace.parent, self) {
                return;
            }

            previous_parent = workspace.parent.clone();
            workspace.parent = self.clone();
        }

        // Remove the child association of the previous parent.
        previous_parent.remove_association(workspace, t);
    }

    /// Create a new workspace on this display.
    pub fn create_workspace(&self, id: u32, t: &mut GhostToken<'g>) -> WorkspacePtr<'g> {
        // Create new workspace associated with this display.
        let workspace = WorkspacePtr::new(id, self.clone());

        // Assign the workspace pointer to the display.
        let this = self.borrow_mut(t);
        this.workspaces.insert(id, workspace.clone());

        // Set it as the active if one is not already set.
        if this.active.is_none() {
            this.active = Some(id);
        }

        workspace
    }

    pub fn remove_association(&self, workspace: WorkspacePtr<'g>, t: &mut GhostToken<'g>) {
        let this = self.borrow_mut(t);

        if let Some(id) = this
            .workspaces
            .iter()
            .find(|(_, w)| Rc::ptr_eq(w, &workspace))
            .map(|(id, _)| *id)
        {
            this.workspaces.remove(&id);
        }
    }

    /// Updates the work area of every workspace attached to this display.
    pub fn work_area_update(&self, tiler: &mut Tiler<'g>, area: Rect, t: &mut GhostToken<'g>) {
        // Update the area of this display.
        self.borrow_mut(t).area = area;

        // Take ownership of this display's workspaces.
        let mut workspaces = HashMap::new();
        std::mem::swap(&mut workspaces, &mut self.borrow_mut(t).workspaces);

        // Apply the update to all forks in each workspace.
        for workspace in workspaces.values() {
            if let Some(ref fork) = workspace.fork(t) {
                fork.work_area_update(tiler, area, t);
            }
        }

        // Give it back to the display.
        std::mem::swap(&mut workspaces, &mut self.borrow_mut(t).workspaces);
    }

    pub(crate) fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> DisplayDebug<'a, 'g> {
        DisplayDebug::new(self, t)
    }
}

pub(crate) struct DisplayDebug<'a, 'g> {
    info: &'a DisplayPtr<'g>,
    t: &'a GhostToken<'g>,
}

impl<'a, 'g> DisplayDebug<'a, 'g> {
    pub fn new(info: &'a DisplayPtr<'g>, t: &'a GhostToken<'g>) -> Self {
        Self { info, t }
    }
}

impl<'a, 'g> Debug for DisplayDebug<'a, 'g> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let &Self { info, t } = self;
        let info = info.borrow(t);

        let workspaces: Vec<_> = info
            .workspaces
            .iter()
            .map(|(_, w)| w.borrow(t).debug(t))
            .collect();
        fmt.debug_struct("Display")
            .field("area", &info.area)
            .field("active", &info.active)
            .field("workspaces", &workspaces)
            .finish()
    }
}
