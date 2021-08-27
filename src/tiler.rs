// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

use crate::components::{Branch, BranchRef, Fork, ForkPtr, StackPtr, Window, WindowID, WindowPtr};
use crate::{Entity, Event, Placement, Rect};
use ghost_cell::{GhostCell, GhostToken};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Debug};
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct EventQueue {
    pub(crate) placements: HashMap<Entity, Placement>,
    pub(crate) visibility: HashMap<Entity, bool>,
}

/// A tiling window manager
pub struct Tiler<'g> {
    event_queue: Rc<GhostCell<'g, EventQueue>>,
    active_changed: bool,
    active: Option<WindowPtr<'g>>,
    active_display: u32,
    active_workspace: u32,
    pub windows: BTreeMap<WindowID, WindowPtr<'g>>,
    displays: BTreeMap<u32, Rect>,
    workspaces: BTreeMap<u32, WorkspaceInfo<'g>>,
}

impl<'g> Default for Tiler<'g> {
    fn default() -> Self {
        Self {
            event_queue: Rc::new(GhostCell::new(EventQueue::default())),
            active_changed: false,
            active: None,
            active_workspace: 0,
            active_display: 0,
            windows: BTreeMap::new(),
            displays: BTreeMap::new(),
            workspaces: BTreeMap::new(),
        }
    }
}

impl<'g> Tiler<'g> {
    /// Attach a window to the focused window in the tiler, and associate it with the tiler.
    pub fn attach(&mut self, window: &WindowPtr<'g>, workspace: u32, t: &mut GhostToken<'g>) {
        // Attach the window to the tiler in case it was not.
        self.windows.insert(window.id(t), window.clone());

        if let Some(focus) = self.active.clone() {
            eprintln!("attaching to window");
            self.attach_to_window(window, &focus, t)
        } else {
            eprintln!("attaching to workspace");
            self.active = Some(window.clone());
            self.active_changed = true;
            self.attach_to_workspace(window, workspace, t)
        }
    }

    /// Attach a window to an existing window
    fn attach_to_window(
        &mut self,
        new_window: &WindowPtr<'g>,
        attach_to: &WindowPtr<'g>,
        t: &mut GhostToken<'g>,
    ) {
        // If window is attached to stack, then attach new window to the same stack
        if let Some((stack, fork)) = attach_to.stack(t).zip(attach_to.fork(t)) {
            new_window.fork_set(fork.clone(), t);
            stack.attach(new_window, t);
            stack.work_area_update(stack.borrow(t).area, t);
            return;
        }

        // If window is attached to a fork, attach to the same fork
        if let Some(fork) = attach_to.fork(t) {
            self.attach_to_window_in_fork(new_window, attach_to, &fork, t);
            return;
        }

        tracing::error!("attempted attach to window that's not attached to anything");
    }

    /// Attach the `window` to the `attaching` window in the `fork`.
    fn attach_to_window_in_fork(
        &mut self,
        window: &WindowPtr<'g>,
        attaching: &WindowPtr<'g>,
        fork: &ForkPtr<'g>,
        t: &mut GhostToken<'g>,
    ) {
        let workspace: u32;

        // If the right branch is empty, assign our new window to it.
        {
            let fork_ = fork.borrow_mut(t);

            workspace = fork_.workspace;

            if fork_.right.is_none() {
                fork_.right = Some(Branch::Window(window.clone()));
                window.fork_set(fork.clone(), t);
                return;
            }
        };

        // Create a new fork branch and assign both windows to it.
        let new_fork = ForkPtr::new({
            let area = Rect::new(1, 1, 1, 1);
            let branch = Branch::Window(attaching.clone());
            let mut fork = Fork::new(area, branch, workspace);
            fork.right = Some(Branch::Window(window.clone()));
            fork
        });

        attaching.fork_set(new_fork.clone(), t);
        window.fork_set(new_fork.clone(), t);

        let new_branch = Branch::Fork(new_fork.clone());

        // Then assign the new branch to the fork where the window was.
        {
            let fork_ = fork.borrow_mut(t);
            match fork_.branch_of(BranchRef::Window(attaching)) {
                Some(branch) => *branch = new_branch,
                None => tracing::error!("invalid parent fork association in window"),
            }
        }

        // Refresh work areas
        fork.work_area_refresh(t);

        // Reassign fork orientation and refresh again. TODO: Avoid redoing refresh
        new_fork.reset_orientation(t);
        new_fork.work_area_refresh(t);
    }

    /// Attach a window a tree on a workspace.
    fn attach_to_workspace(
        &mut self,
        window: &WindowPtr<'g>,
        workspace: u32,
        t: &mut GhostToken<'g>,
    ) {
        let display = self.workspace(workspace).display;

        let display = ward::ward!(self.displays.get(&display).cloned(), else {
            tracing::error!("attached to workspace that isn't associated with a display");
            return;
        });

        let info = self.workspace(workspace);

        // Assign window to an existing fork on the workspace.
        if let Some(fork) = info.fork.clone() {
            if let Some(attach_to) = fork.largest_window(t) {
                self.attach_to_window_in_fork(window, &attach_to, &fork, t);
                fork.work_area_refresh(t);
                return;
            }
        }

        // Create a new fork and assign that, otherwise.
        let branch = Branch::Window(window.clone());
        let fork = ForkPtr::new(Fork::new(display, branch, workspace));

        window.fork_set(fork.clone(), t);

        info.focus = Some(window.clone());
        info.fork = Some(fork.clone());

        fork.work_area_refresh(t);
    }

    /// Detach a window from its tree, and removes its association with this tiler.
    pub fn detach(&mut self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        // Remove the window from management of the tiler.
        self.windows.remove(&window.id(t));

        if let Some(stack) = window.stack(t) {
            stack.detach(window, t);
            window.fork_take(t);
            return;
        }

        if let Some(fork) = window.fork_take(t) {
            self.detach_from_fork(fork, window.clone(), t);
        }
    }

    /// Detach a window from a fork.
    fn detach_fork(&mut self, fork: ForkPtr<'g>, t: &mut GhostToken<'g>) {
        let mut detaching = Some(fork);

        while let Some(fork) = detaching.take() {
            if let Some(parent) = fork.borrow_mut(t).parent.take() {
                let parent_ = parent.borrow_mut(t);
                if let Branch::Fork(ref compare) = &parent_.left {
                    if Rc::ptr_eq(compare, &fork) {
                        if let Some(right) = parent_.right.take() {
                            parent_.left = right;
                        } else {
                            detaching = Some(compare.clone());
                        }
                    }
                } else {
                    parent_.right = None;
                }
            } else {
                for (id, workspace) in self.workspaces.iter() {
                    if let Some(ref root) = workspace.fork {
                        if Rc::ptr_eq(root, &fork) {
                            let remove = *id;
                            self.workspaces.remove(&remove);
                            return;
                        }
                    }
                }

                tracing::error!("attempted to detach a root fork that didn't exist");
            }
        }
    }

    /// Detach a window from a fork
    fn detach_from_fork(
        &mut self,
        fork: ForkPtr<'g>,
        window: WindowPtr<'g>,
        t: &mut GhostToken<'g>,
    ) {
        eprintln!("detaching {:?} from fork {:?}", window.id(t), fork.as_ptr());

        // After removing a window, it's possible to have a fork in a fork with an empty
        // branch on one side. When this happens, we will discard the parent fork and
        // place the grandchild in its place. Then the child association of the
        // grandparent fork is updated to point to the grandchild who is now a direct
        // child.
        let grandparent;
        let grandchild;

        {
            let fork_ = fork.borrow_mut(t);
            if fork_.left_is(BranchRef::Window(&window)) {
                // If the right branch exists, the right branch becomes the left branch.
                if let Some(right) = fork_.right.take() {
                    fork_.left = right;

                    // Handle possible double-fork scenario.
                    if let Branch::Fork(ref fork_to_compress) = fork_.left {
                        grandparent = fork_.parent.clone();
                        grandchild = fork_to_compress.clone();
                    } else {
                        return;
                    }
                } else {
                    // If the fork has nothing on either branch, we are free to discard it.
                    self.detach_fork(fork, t);
                    return;
                }
            } else if fork_.right_is(BranchRef::Window(&window)) {
                fork_.right = None;

                // Handle possible double-fork scenario.
                if let Branch::Fork(ref fork_to_compress) = fork_.left {
                    grandparent = fork_.parent.clone();
                    grandchild = fork_to_compress.clone();
                } else {
                    return;
                }
            } else {
                return;
            }
        }

        match grandparent {
            Some(grandparent) => {
                // Update child association of the grandparent fork
                let grandparent_ = grandparent.borrow_mut(t);
                match grandparent_.branch_of(BranchRef::Fork(&fork)) {
                    Some(branch) => *branch = Branch::Fork(grandchild.clone()),
                    None => tracing::error!("fork contained parent that doesn't own it"),
                }

                grandparent.work_area_refresh(t);

                // Update the parent association of the grandchild now a child
                grandchild.borrow_mut(t).parent = Some(grandparent);
            }
            None => {
                for info in self.workspaces.values_mut() {
                    if let Some(ref workspace_fork) = info.fork {
                        if !Rc::ptr_eq(workspace_fork, &fork) {
                            continue;
                        }

                        let display = ward::ward!(self.displays.get(&info.display).cloned(), else {
                            tracing::error!("workspace is not assigned to display");
                            return;
                        });

                        grandchild.borrow_mut(t).parent = None;
                        grandchild.work_area_update(display, t);
                        info.fork = Some(grandchild);
                        return;
                    }
                }
            }
        }
    }

    /// Retrieves the latest set of instructions for the window manager to carry out.
    pub fn events<'a>(&'a mut self, t: &'a mut GhostToken<'g>) -> impl Iterator<Item = Event> + 'a {
        let focus: Option<Event> = if self.active_changed {
            self.active.as_ref().map(|a| Event::Focus(a.id(t)))
        } else {
            None
        };

        self.active_changed = false;

        let queue = self.event_queue.borrow_mut(t);

        let mut visibility = HashMap::new();
        std::mem::swap(&mut queue.visibility, &mut visibility);
        let visibility = visibility.into_iter().map(|(a, v)| Event::Show(a, v));

        let mut placements = HashMap::new();
        std::mem::swap(&mut queue.placements, &mut placements);
        let placements = placements.into_iter().map(|(a, r)| Event::Place(a, r));

        placements.chain(visibility).chain(focus.into_iter())
    }

    /// Focus this window in the tree.
    pub fn focus(&mut self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        if let Some(focus) = self.active.as_ref() {
            if Rc::ptr_eq(focus, window) {
                return;
            }
        }

        if let Some(stack) = window.stack(t) {
            let mut visibility = Vec::new();
            for this in stack.borrow(t).windows.iter() {
                visibility.push((this.id(t), Rc::ptr_eq(window, this)));
            }

            let events = self.event_queue.borrow_mut(t);

            for (id, show) in visibility {
                events.visibility.insert(Entity::Window(id), show);
            }
        }

        self.active_changed = true;
        self.active = Some(window.clone());
    }

    /// Move focus to the window left of the active one.
    pub fn focus_left(&mut self, t: &mut GhostToken<'g>) {
        let active = ward::ward!(self.active.clone(), else { return });

        // If window is in a stack, select the window to the left of the stack.
        if let Some(stack) = active.stack(t) {
            if let Some(left) = stack.select_left(t) {
                stack.borrow_mut(t).active = left.clone();
                self.active = Some(left);
                self.active_changed = true;
                return;
            }
        }

        // If it was the left-most window in a stack, or was not in one.
        let fork = ward::ward!(active.fork(t), else { return });
    }

    /// Move focus to the window right of the active one.
    pub fn focus_right(&mut self, t: &mut GhostToken<'g>) {}

    /// Move focus to the window above the active one.
    pub fn focus_above(&mut self, t: &mut GhostToken<'g>) {}

    /// Move focus to the window below the active one.
    pub fn focus_below(&mut self, t: &mut GhostToken<'g>) {}

    /// Move focus to the workspace on the display to the left of the active one.
    pub fn focus_display_left(&mut self, t: &mut GhostToken<'g>) {}

    /// Move focus to the workspace on the display to the right of the active one.
    pub fn focus_display_right(&mut self, t: &mut GhostToken<'g>) {}

    /// Move focus to the workspace on the display above the active one.
    pub fn focus_display_above(&mut self, t: &mut GhostToken<'g>) {}

    /// Move focus to the workspace on the display below the active one.
    pub fn focus_display_below(&mut self, t: &mut GhostToken<'g>) {}

    /// Toggle the orientation of the fork that owns this window.
    pub fn toggle_orientation(&self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        window.orientation_toggle(t);
    }

    /// If a window is stacked, unstack it. If it is not stacked, stack it.
    pub fn stack_toggle(&mut self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        if let Some(stack) = window.stack(t) {
            stack.detach(window, t);

            if stack.borrow(t).windows.is_empty() {
                let fork = ward::ward!(window.fork(t), else {
                    tracing::error!("window does not have a parent fork");
                    return;
                });

                let fork_ = fork.borrow_mut(t);

                let branch = ward::ward!(fork_.branch_of(BranchRef::Stack(&stack)), else {
                    tracing::error!("parent fork of window did not have a stack assocation for this window");
                    return;
                });

                *branch = Branch::Window(window.clone())
            }

            return;
        }

        let stack = StackPtr::new(window, t);

        let fork = ward::ward!(window.fork(t), else {
            tracing::error!("window does not have a parent fork");
            stack.detach(window, t);
            return;
        });

        let fork_ = fork.borrow_mut(t);

        let branch = ward::ward!(fork_.branch_of(BranchRef::Window(window)), else {
            tracing::error!("parent fork of window did not contain this window");
            stack.detach(window, t);
            return;
        });

        *branch = Branch::Stack(stack);
    }

    /// Swaps the tree location of this window with another.
    pub fn swap(&self, a: &WindowPtr<'g>, b: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        a.swap_position_with(b, t);
    }

    // /// Updates the work area of a display, or otherwise assigns one.
    // pub fn update_display(&mut self, display: u32, area: Rect, t: &mut GhostToken<'g>) {
    //     if let Some(display)
    //     self.workspace(workspace).work_area_update(area, t);
    // }

    /// Create a new pointer to a window managed by this tiler.
    pub fn window<I: Into<WindowID>>(&mut self, id: I) -> WindowPtr<'g> {
        let id = id.into();

        if let Some(window) = self.windows.get(&id) {
            return window.clone();
        }

        let window = WindowPtr(Rc::new(GhostCell::new(Window::new(
            id,
            self.event_queue.clone(),
        ))));

        self.windows.insert(id, window.clone());

        window
    }

    /// Fetches information about a workspace.
    ///
    /// If the workspace does not exist, a new entry will be added for it.
    pub(crate) fn workspace(&mut self, workspace: u32) -> &mut WorkspaceInfo<'g> {
        self.workspaces
            .entry(workspace)
            .or_insert_with(|| WorkspaceInfo {
                focus: None,
                fork: None,
                display: 0,
            })
    }

    /// Switching the workspace will hide all windows on other workspaces, show all
    /// visible windows for the given workspace, and focus the active window on the
    /// given workspace.
    pub fn workspace_switch(&mut self, workspace: u32, t: &mut GhostToken<'g>) {
        let mut events = Vec::new();

        for (id, window) in self.windows.iter() {
            let this = window.borrow_mut(t);

            let is_visible = this.visible;

            // If window's workspace is not the same as what is being switched to.
            if this.workspace != workspace {
                if is_visible {
                    this.visible = false;
                    events.push((Entity::Window(*id), this.visible));
                }

                continue;
            }

            // If window on switched workspace is the active window in a stack
            if let Some(stack) = this.stack.clone() {
                if Rc::ptr_eq(&stack.borrow(t).active, window) {
                    if !is_visible {
                        window.borrow_mut(t).visible = true;
                        events.push((Entity::Window(*id), true));
                    }
                } else if is_visible {
                    window.borrow_mut(t).visible = false;
                    events.push((Entity::Window(*id), false))
                }

                continue;
            }

            // All other windows not in a stack
            if !is_visible {
                window.borrow_mut(t).visible = true;
                events.push((Entity::Window(*id), true));
            }
        }

        let queue = self.event_queue.borrow_mut(t);

        for (entity, event) in events {
            queue.visibility.insert(entity, event);
        }

        if let Some(focus) = self.workspace(workspace).focus.clone() {
            self.active = Some(focus);
        }
    }

    pub fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> TilerDisplay<'a, 'g> {
        TilerDisplay::new(self, t)
    }
}

pub struct TilerDisplay<'a, 'g> {
    pub tiler: &'a Tiler<'g>,
    pub t: &'a GhostToken<'g>,
}

impl<'a, 'g> TilerDisplay<'a, 'g> {
    fn new(tiler: &'a Tiler<'g>, t: &'a GhostToken<'g>) -> Self {
        Self { tiler, t }
    }
}

impl<'a, 'g> Debug for TilerDisplay<'a, 'g> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let active = self.tiler.active.as_ref().map(|window| window.id(self.t));

        let workspaces = self
            .tiler
            .workspaces
            .iter()
            .map(|(key, ws)| (key, ws.debug(self.t)))
            .collect::<Vec<_>>();

        fmt.debug_struct("Tiler")
            .field("active", &active)
            .field("workspaces", &workspaces)
            .finish()
    }
}

pub(crate) struct WorkspaceInfo<'g> {
    pub focus: Option<WindowPtr<'g>>,
    pub display: u32,
    pub fork: Option<ForkPtr<'g>>,
}

impl<'g> WorkspaceInfo<'g> {
    // pub fn work_area_update(&mut self, area: Rect, t: &mut GhostToken<'g>) {
    //     self.area = area;

    //     if let Some(ref fork) = self.fork {
    //         fork.work_area_update(area, t)
    //     }
    // }

    pub(crate) fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> WorkspaceInfoDebug<'a, 'g> {
        WorkspaceInfoDebug::new(self, t)
    }
}

pub(crate) struct WorkspaceInfoDebug<'a, 'g> {
    info: &'a WorkspaceInfo<'g>,
    t: &'a GhostToken<'g>,
}

impl<'a, 'g> WorkspaceInfoDebug<'a, 'g> {
    pub fn new(info: &'a WorkspaceInfo<'g>, t: &'a GhostToken<'g>) -> Self {
        Self { info, t }
    }
}

impl<'a, 'g> Debug for WorkspaceInfoDebug<'a, 'g> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let focus = self.info.focus.as_ref().map(|win| win.id(self.t));
        fmt.debug_struct("WorkspaceInfo")
            .field("focus", &focus)
            .field("fork", &self.info.fork.as_ref().map(|f| f.debug(self.t)))
            .finish()
    }
}
