// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::branch::{Branch, BranchRef};
use crate::display::DisplayPtr;
use crate::events::EventQueue;
use crate::fork::{Fork, ForkPtr, Orientation};
use crate::stack::{StackMovement, StackPtr};
use crate::window::{Window, WindowID, WindowPtr};
use crate::workspace::WorkspacePtr;
use crate::{Event, Rect};
use either::Either;
use ghost_cell::{GhostCell, GhostToken};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Debug};
use std::rc::Rc;

type DistanceFn = fn(&Rect, &Rect) -> f64;
type DirectionalConditionFn = fn(&Rect, &Rect) -> bool;

pub enum Direction {
    Above,
    Below,
    Left,
    Right,
}

/// A tiling window manager
pub struct Tiler<'g> {
    pub(crate) event_queue: EventQueue,
    active_changed: bool,
    active: Option<WindowPtr<'g>>,
    active_workspace: u32,
    active_workspace_changed: bool,

    pub windows: BTreeMap<WindowID, WindowPtr<'g>>,
    forks: BTreeMap<usize, ForkPtr<'g>>,
    displays: BTreeMap<u32, DisplayPtr<'g>>,
    workspaces: BTreeMap<u32, WorkspacePtr<'g>>,
}

impl<'g> Default for Tiler<'g> {
    fn default() -> Self {
        Self {
            event_queue: EventQueue::default(),
            active_changed: false,
            active: None,
            active_workspace: 0,
            active_workspace_changed: false,
            forks: BTreeMap::new(),
            windows: BTreeMap::new(),
            displays: BTreeMap::new(),
            workspaces: BTreeMap::new(),
        }
    }
}

impl<'g> Tiler<'g> {
    pub fn active_window(&self) -> Option<&WindowPtr<'g>> {
        self.active.as_ref()
    }

    /// Attach a window to the focused window in the tiler, and associate it with the tiler.
    pub fn attach(&mut self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        // Attach the window to the tiler in case it was not.
        self.windows.insert(window.id(t), window.clone());

        if let Some(focus) = self.active_window().cloned() {
            tracing::debug!("attaching to focus window");
            self.attach_to_window(window, &focus, t);
            return;
        }

        tracing::debug!("no active window: attaching to display instead");

        self.set_active_window(window, t);

        let workspace = self
            .workspaces
            .get(&self.active_workspace)
            .expect("no workspace found to attach to")
            .clone();

        self.attach_to_workspace(window, &workspace, t);
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
            stack.work_area_update(self, stack.borrow(t).area, t);
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

        self.fork_register(new_fork.clone(), t);

        attaching.fork_set(new_fork.clone(), t);
        window.fork_set(new_fork.clone(), t);

        let new_branch = Branch::Fork(new_fork.clone());

        // Then assign the new branch to the fork where the window was.
        {
            let fork_ = fork.borrow_mut(t);
            match fork_.branch(BranchRef::Window(attaching)) {
                Some(Either::Left(branch)) | Some(Either::Right(branch)) => *branch = new_branch,
                None => tracing::error!("invalid parent fork association in window"),
            }
        }

        // Refresh work areas
        fork.work_area_refresh(self, t);

        // Reassign fork orientation and refresh again. TODO: Avoid redoing refresh
        new_fork.reset_orientation(self, t);
        new_fork.work_area_refresh(self, t);
    }

    /// Attach a window a tree on a display.
    fn attach_to_workspace(
        &mut self,
        window: &WindowPtr<'g>,
        workspace: &WorkspacePtr<'g>,
        t: &mut GhostToken<'g>,
    ) {
        let area = workspace.area(t);

        // Assign window to an existing fork on the workspace.
        if let Some(fork) = workspace.fork(t) {
            if let Some(attach_to) = fork.largest_window(t) {
                self.attach_to_window_in_fork(window, &attach_to, &fork, t);
                fork.work_area_refresh(self, t);
                return;
            }
        }

        // Create a new fork and assign that, otherwise.
        let branch = Branch::Window(window.clone());
        let fork = ForkPtr::new(Fork::new(area, branch, workspace.id(t)));
        self.fork_register(fork.clone(), t);

        window.fork_set(fork.clone(), t);

        let workspace = workspace.borrow_mut(t);
        workspace.focus = Some(window.clone());
        workspace.fork = Some(fork.clone());

        fork.work_area_refresh(self, t);
    }

    /// Detach a window from its tree, and removes its association with this tiler.
    pub fn detach(&mut self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        // Remove the window from management of the tiler.
        self.windows.remove(&window.id(t));

        if let Some(stack) = window.stack(t) {
            window.fork_take(t);
            stack.detach(self, window, t);
            return;
        }

        if let Some(fork) = window.fork_take(t) {
            self.detach_branch(fork, BranchRef::Window(window), t);
        }

        // If window being detached is the active window, remove focus
        if let Some(active) = self.active.as_ref() {
            if Rc::ptr_eq(window, active) {
                self.active = None;
                self.active_changed = false;
            }
        }
    }

    /// Detach a window from a fork.
    fn detach_fork(&mut self, fork: ForkPtr<'g>, t: &mut GhostToken<'g>) {
        eprintln!("requested to detach fork");
        let mut detaching = Some(fork);

        while let Some(fork) = detaching.take() {
            self.event_queue.fork_destroy(&fork);
            self.forks.remove(&(fork.as_ptr() as usize));

            if let Some(parent) = fork.borrow_mut(t).parent.take() {
                let parent_ = parent.borrow_mut(t);
                if parent_.left_is(BranchRef::Fork(&fork)) {
                    if let Some(right) = parent_.right.take() {
                        parent_.left = right;
                    }
                } else if parent_.right_is(BranchRef::Fork(&fork)) {
                    parent_.right = None;
                }
            } else {
                for workspace in self.workspaces.values() {
                    if let Some(ref root) = workspace.fork(t) {
                        if Rc::ptr_eq(root, &fork) {
                            workspace.borrow_mut(t).fork = None;
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Detach a window or stack from a fork
    pub(crate) fn detach_branch(
        &mut self,
        fork: ForkPtr<'g>,
        reference: BranchRef<'_, 'g>,
        t: &mut GhostToken<'g>,
    ) {
        tracing::debug!("detaching branch from Fork({})", fork.as_ptr() as usize);

        // After removing a window, it's possible to have a fork in a fork with an empty
        // branch on one side. When this happens, we will discard the parent fork and
        // place the grandchild in its place. Then the child association of the
        // grandparent fork is updated to point to the grandchild who is now a direct
        // child.
        fn reparent<'g>(
            tiler: &mut Tiler<'g>,
            grandparent: Option<ForkPtr<'g>>,
            parent: ForkPtr<'g>,
            grandchild: ForkPtr<'g>,
            t: &mut GhostToken<'g>,
        ) {
            match grandparent {
                Some(grandparent) => {
                    // Update child association of the grandparent fork
                    let grandparent_ = grandparent.borrow_mut(t);
                    match grandparent_.branch(BranchRef::Fork(&parent)) {
                        Some(Either::Left(branch)) | Some(Either::Right(branch)) => {
                            *branch = Branch::Fork(grandchild.clone())
                        }
                        None => tracing::error!("fork contained parent that doesn't own it"),
                    }

                    grandparent.work_area_refresh(tiler, t);

                    // Update the parent association of the grandchild now a child
                    grandchild.borrow_mut(t).parent = Some(grandparent);
                }
                None => {
                    let mut display = None;

                    for info in tiler.workspaces.values() {
                        if let Some(ref workspace_fork) = info.borrow(t).fork {
                            if !Rc::ptr_eq(workspace_fork, &parent) {
                                continue;
                            }

                            display = Some(info.borrow(t).parent.borrow(t).area);

                            grandchild.borrow_mut(t).parent = None;
                            info.borrow_mut(t).fork = Some(grandchild.clone());
                            break;
                        }
                    }

                    if let Some(display) = display {
                        grandchild.work_area_update(tiler, display, t);
                    }
                }
            }

            // Remove the orphaned fork
            parent.borrow_mut(t).parent.take();
            tiler.detach_fork(parent, t);
        }

        let fork_ = fork.borrow_mut(t);

        if fork_.left_is(reference) {
            tracing::debug!("detaching left branch of fork");
            if let Some(right) = fork_.right.take() {
                tracing::debug!("right branch of fork was assigned to left branch");
                fork_.left = right;

                if let Branch::Fork(ref grandchild) = fork_.left {
                    tracing::debug!("reparenting left branch of fork into parent");
                    reparent(
                        self,
                        fork_.parent.clone(),
                        fork.clone(),
                        grandchild.clone(),
                        t,
                    );
                }
            } else {
                tracing::debug!("fork is now childless");
                self.detach_fork(fork, t);
            }
        } else if fork_.right_is(reference) {
            tracing::debug!("detaching right branch of fork");
            fork_.right = None;

            if let Branch::Fork(ref grandchild) = fork_.left.clone() {
                tracing::debug!("reparenting left branch into parent fork");
                reparent(
                    self,
                    fork_.parent.clone(),
                    fork.clone(),
                    grandchild.clone(),
                    t,
                );
            }
        }
    }

    /// Removes a display from the tree, and migrates its workspaces to another display
    pub fn display_detach(&mut self, display_id: u32, t: &mut GhostToken<'g>) {
        // Get the active display to assign to.
        let active = self
            .workspaces
            .get(&self.active_workspace)
            .expect("active workspace doesn't exist")
            .borrow(t)
            .parent
            .clone();

        // Remove the display from the tiler.
        let display_ptr = ward::ward!(self.displays.remove(&display_id), else {
            tracing::error!("detach of non-existent display: {}", display_id);
            return;
        });

        // Take ownership of its workspaces.
        let mut workspaces = HashMap::new();
        std::mem::swap(&mut workspaces, &mut display_ptr.borrow_mut(t).workspaces);

        // Migrate workspaces.
        for workspace in workspaces.into_values() {
            active.assign_workspace(workspace, t);
        }
    }

    /// Creates or updates a display associated with the tree.
    pub fn display_update(&mut self, display: u32, area: Rect, t: &mut GhostToken<'g>) {
        let display = self
            .displays
            .entry(display)
            .or_insert_with(|| DisplayPtr::new(area))
            .clone();

        display.work_area_update(self, area, t);
    }

    /// Retrieves the latest set of instructions for the window manager to carry out.
    pub fn events<'a>(&'a mut self, t: &'a mut GhostToken<'g>) -> impl Iterator<Item = Event> + 'a {
        let focus: Option<Event> = if self.active_changed {
            self.active_window().map(|a| Event::Focus(a.id(t)))
        } else {
            None
        };

        self.active_changed = false;

        let workspace_switch = if self.active_workspace_changed {
            Some(Event::FocusWorkspace(self.active_workspace))
        } else {
            None
        };

        self.event_queue
            .consume_events()
            .chain(workspace_switch.into_iter())
            .chain(focus.into_iter())
    }

    /// Focus this window in the tree.
    pub fn focus(&mut self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        window.focus(self, t);
    }

    /// Move focus to the window above the active one.
    pub fn focus_above(&mut self, t: &mut GhostToken<'g>) {
        match self.window_in_direction(Rect::distance_upward, Rect::is_below, t) {
            Some(active) => self.set_active_window(&active, t),
            None => self.focus_display_above(t),
        }
    }

    /// Move focus to the window below the active one.
    pub fn focus_below(&mut self, t: &mut GhostToken<'g>) {
        match self.window_in_direction(Rect::distance_downward, Rect::is_above, t) {
            Some(active) => self.set_active_window(&active, t),
            None => self.focus_display_below(t),
        }
    }

    /// Move focus to the window left of the active one.
    pub fn focus_left(&mut self, t: &mut GhostToken<'g>) {
        self.focus_with_stack(StackPtr::select_left, Self::focus_left_absolute, t);
    }

    /// Move focus to the left window, even if in a stack.
    pub fn focus_left_absolute(&mut self, t: &mut GhostToken<'g>) {
        match self.window_in_direction(Rect::distance_westward, Rect::is_right, t) {
            Some(active) => self.set_active_window(&active, t),
            None => self.focus_display_left(t),
        }
    }

    /// Move focus to the window right of the active one.
    pub fn focus_right(&mut self, t: &mut GhostToken<'g>) {
        self.focus_with_stack(StackPtr::select_right, Self::focus_right_absolute, t);
    }

    /// Move focus to the right window, even if in a stack.
    pub fn focus_right_absolute(&mut self, t: &mut GhostToken<'g>) {
        match self.window_in_direction(Rect::distance_eastward, Rect::is_left, t) {
            Some(active) => self.set_active_window(&active, t),
            None => self.focus_display_right(t),
        }
    }

    /// Focus the active window on this display.
    fn focus_display(&mut self, display: DisplayPtr<'g>, t: &mut GhostToken<'g>) {
        let display = display.borrow(t);

        if let Some(active) = display.active {
            if let Some(workspace) = display.workspaces.get(&active) {
                if let Some(active) = workspace.borrow(t).focus.clone() {
                    self.set_active_window(&active, t);
                }
            }
        }
    }

    /// Move focus to the workspace on the display to the left of the active one.
    pub fn focus_display_left(&mut self, t: &mut GhostToken<'g>) {
        if let Some(display) = self.display_in_direction(Rect::distance_westward, Rect::is_right, t)
        {
            self.focus_display(display, t);
        }
    }

    /// Move focus to the workspace on the display to the right of the active one.
    pub fn focus_display_right(&mut self, t: &mut GhostToken<'g>) {
        if let Some(display) = self.display_in_direction(Rect::distance_eastward, Rect::is_left, t)
        {
            self.focus_display(display, t);
        }
    }

    /// Move focus to the workspace on the display above the active one.
    pub fn focus_display_above(&mut self, t: &mut GhostToken<'g>) {
        if let Some(display) = self.display_in_direction(Rect::distance_upward, Rect::is_below, t) {
            self.focus_display(display, t);
        }
    }

    /// Move focus to the workspace on the display below the active one.
    pub fn focus_display_below(&mut self, t: &mut GhostToken<'g>) {
        if let Some(display) = self.display_in_direction(Rect::distance_downward, Rect::is_above, t)
        {
            self.focus_display(display, t);
        }
    }

    /// Manage focus movements with consideration for in-stack movements.
    fn focus_with_stack(
        &mut self,
        stack_func: fn(&StackPtr<'g>, &mut GhostToken<'g>) -> Option<WindowPtr<'g>>,
        focus_func: fn(&mut Self, &mut GhostToken<'g>),
        t: &mut GhostToken<'g>,
    ) {
        let active = ward::ward!(self.active_window(), else { return });

        // If window is in a stack, select the window to the left of the stack.
        if let Some(stack) = active.stack(t) {
            if let Some(left) = stack_func(&stack, t) {
                stack.borrow_mut(t).active = left.clone();
                self.set_active_window(&left, t);
                return;
            }
        }

        focus_func(self, t);
    }

    /// Keep track of this fork directly in the tiler.
    fn fork_register(&mut self, fork: ForkPtr<'g>, t: &GhostToken<'g>) {
        self.event_queue.fork_update(&fork, t);
        self.forks.insert(fork.as_ptr() as usize, fork);
    }

    /// Resize a fork with a new split
    pub fn fork_resize(&mut self, fork: usize, split: u32, t: &mut GhostToken<'g>) {
        if let Some(fork) = self.forks.get(&fork).cloned() {
            fork.resize(self, split, t);
        }
    }

    /// When moving vertically or horizontally, move active window out of the stack.
    fn move_from_stack(
        &mut self,
        active: &WindowPtr<'g>,
        fork: &ForkPtr<'g>,
        stack: &StackPtr<'g>,
        direction: Direction,
        t: &mut GhostToken<'g>,
    ) {
        let (orientation, stack_on_left) = match direction {
            Direction::Above => (Orientation::Vertical, false),
            Direction::Below => (Orientation::Vertical, true),
            Direction::Left => (Orientation::Horizontal, false),
            Direction::Right => (Orientation::Horizontal, true),
        };

        let area = stack.borrow(t).area;
        let workspace = fork.borrow(t).workspace;
        let windows = stack.borrow(t).windows.len();

        let branch = ward::ward!(fork.borrow_mut(t).branch(BranchRef::Stack(stack)), else {
            tracing::error!("invalid parent fork association of stacked window");
            return;
        });

        match branch {
            Either::Left(prev_branch) | Either::Right(prev_branch) => {
                if windows == 1 {
                    *prev_branch = Branch::Window(active.clone());
                    self.event_queue.stack_destroy(stack);
                } else {
                    let stack_branch = Branch::Stack(stack.clone());
                    let window_branch = Branch::Window(active.clone());

                    let (left, right) = if stack_on_left {
                        (stack_branch, window_branch)
                    } else {
                        (window_branch, stack_branch)
                    };

                    let new_fork = ForkPtr::new({
                        let mut fork = Fork::new(area, left, workspace);
                        fork.right = Some(right);
                        fork
                    });

                    *prev_branch = Branch::Fork(new_fork.clone());

                    new_fork.borrow_mut(t).parent = Some(fork.clone());
                    self.fork_register(new_fork.clone(), t);
                    new_fork.orientation_set(self, orientation, t);
                }
            }
        }

        fork.work_area_refresh(self, t);
    }

    /// When moving horizontally, check if a window is stacked and can be moved within the stack.
    fn move_horizontally(
        &mut self,
        stack_func: fn(&StackPtr<'g>, &mut GhostToken<'g>) -> Option<StackMovement>,
        else_func: fn(&mut Self, &mut GhostToken<'g>),
        t: &mut GhostToken<'g>,
    ) {
        let active = ward::ward!(self.active_window(), else { return });

        // If window is in a stack, move the tab positioning in the stack
        if let Some(stack) = active.stack(t) {
            if let Some(movement) = stack_func(&stack, t) {
                self.event_queue.stack_movement(&stack, movement);
                return;
            }
        }

        else_func(self, t);
    }

    /// Move the active window up in the tree.
    pub fn move_left(&mut self, t: &mut GhostToken<'g>) {
        self.move_horizontally(StackPtr::move_left, Self::move_left_absolute, t);
    }

    fn move_in_direction(&mut self, direction: Direction, t: &mut GhostToken<'g>) {
        let active = ward::ward!(self.active_window().cloned(), else { return });
        let fork = ward::ward!(active.fork(t), else { return });

        // If in a stack, create a fork and make the window adjacent to the stack.
        if let Some(stack) = active.stack(t) {
            self.move_from_stack(&active, &fork, &stack, direction, t);
            return;
        }

        // Fetch nearest window in direction
        let (distance, filter): (DistanceFn, DirectionalConditionFn) = match direction {
            Direction::Above => (Rect::distance_upward, Rect::is_below),
            Direction::Below => (Rect::distance_downward, Rect::is_above),
            Direction::Left => (Rect::distance_westward, Rect::is_right),
            Direction::Right => (Rect::distance_eastward, Rect::is_left),
        };

        if let Some(window) = self.window_in_direction(distance, filter, t) {
            let matched_fork = ward::ward!(window.fork(t), else {
                tracing::error!("cannot move into window that is forkless");
                return;
            });

            // If the window being attached is in the same fork, swap positions.
            if Rc::ptr_eq(&fork, &matched_fork) {
                let fork_ = fork.borrow_mut(t);
                if let Some(right) = fork_.right.as_mut() {
                    std::mem::swap(right, &mut fork_.left);
                    fork.work_area_refresh(self, t);
                    return;
                }
            }

            // Detach and create a fork in new window.
            self.detach(&active, t);
            self.attach_to_window_in_fork(&active, &window, &matched_fork, t);
            self.set_active_window(&active, t);
        }

        // TODO: Move across displays if not found
    }

    /// Move the active window to the left, even if it is stacked.
    pub fn move_left_absolute(&mut self, t: &mut GhostToken<'g>) {
        self.move_in_direction(Direction::Left, t);
    }

    /// Move the active window to the right in the tree.
    pub fn move_right(&mut self, t: &mut GhostToken<'g>) {
        self.move_horizontally(StackPtr::move_right, Self::move_right_absolute, t);
    }

    /// Move the active window to the right, even if it is stacked.
    pub fn move_right_absolute(&mut self, t: &mut GhostToken<'g>) {
        self.move_in_direction(Direction::Right, t);
    }

    /// Move the active window above in the tree.
    pub fn move_above(&mut self, t: &mut GhostToken<'g>) {
        self.move_in_direction(Direction::Above, t)
    }

    /// Move the active window below in the tree.
    pub fn move_below(&mut self, t: &mut GhostToken<'g>) {
        self.move_in_direction(Direction::Below, t);
    }

    /// Toggle the orientation of the active window.
    pub fn toggle_orientation(&mut self, t: &mut GhostToken<'g>) {
        if let Some(active) = self.active_window() {
            if let Some(fork) = active.fork(t) {
                fork.toggle_orientation(self, t);
            }
        }
    }

    /// Set a new active window, and mark that we should notify the window manager.
    pub(crate) fn set_active_window(&mut self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        self.active = Some(window.clone());
        self.active_changed = true;

        let workspace = window.borrow(t).workspace;

        if self.active_workspace != workspace {
            self.workspace_switch(workspace, t);
        }
    }

    /// If a window is stacked, unstack it. If it is not stacked, stack it.
    pub fn stack_toggle(&mut self, t: &mut GhostToken<'g>) {
        if let Some(active) = self.active_window().cloned() {
            active.stack_toggle(self, t);
        }
    }

    /// Swaps the tree location of this window with another.
    pub fn swap(&mut self, from: &WindowPtr<'g>, with: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        from.swap_position_with(self, with, t);
    }

    /// Create a new pointer to a window managed by this tiler.
    pub fn window<I: Into<WindowID>>(&mut self, id: I) -> WindowPtr<'g> {
        let id = id.into();

        if let Some(window) = self.windows.get(&id) {
            return window.clone();
        }

        let window = WindowPtr(Rc::new(GhostCell::new(Window::new(id))));

        self.windows.insert(id, window.clone());

        window
    }

    /// Locates the display adjacent to the active display.
    fn display_in_direction(
        &self,
        distance: DistanceFn,
        filter: DirectionalConditionFn,
        t: &mut GhostToken<'g>,
    ) -> Option<DisplayPtr<'g>> {
        let active = ward::ward!(self.workspaces.get(&self.active_workspace), else { return None });

        let active = &active.borrow(t).parent;
        let active_rect = &active.borrow(t).area;

        let mut least_distance = f64::MAX;
        let mut candidate = None;

        for display in self.displays.values() {
            if Rc::ptr_eq(display, active) {
                continue;
            }

            let this_rect = &display.borrow(t).area;

            if filter(active_rect, this_rect) {
                continue;
            }

            let distance = distance(active_rect, this_rect);
            if distance < least_distance {
                least_distance = distance;
                candidate = Some(display.clone());
            }
        }

        candidate
    }

    /// Locates the window adjacent to the active window in the active workspace that has
    /// the lowest distance for a given distance function. Ignores windows windows in the
    /// same stack.
    fn window_in_direction(
        &self,
        distance: DistanceFn,
        filter: DirectionalConditionFn,
        t: &GhostToken<'g>,
    ) -> Option<WindowPtr<'g>> {
        let active = ward::ward!(self.active_window(), else { return None });

        let active_ = active.borrow(t);
        let stack = active_.stack.as_ref();
        let rect = active_.rect;
        let workspace = active_.workspace;

        let mut lowest_distance = f64::MAX;
        let mut candidate = None;

        for window in self.windows.values() {
            // Ignores windows from a different workspace.
            if window.borrow(t).workspace != workspace {
                continue;
            }

            // Ignores same window.
            if Rc::ptr_eq(active, window) {
                continue;
            }

            // Ignores windows in the same stack.
            if let Some((active, this)) = stack.zip(window.borrow(t).stack.as_ref()) {
                if Rc::ptr_eq(active, this) {
                    continue;
                }
            }

            let this_rect = &window.borrow(t).rect;

            // Avoid considering windows that meet this criteria.
            if filter(&rect, this_rect) {
                continue;
            }

            // The window with the least distance wins.
            let distance = distance(&rect, this_rect);
            if distance < lowest_distance {
                candidate = Some(window.clone());
                lowest_distance = distance;
            }
        }

        candidate
    }

    /// Detaches a workspace from the tree.
    fn workspace_detach(&mut self, workspace: u32, t: &mut GhostToken<'g>) {
        let workspace = ward::ward!(self.workspaces.remove(&workspace), else {
            tracing::error!("detached a workspace that didn't exist");
            return;
        });

        workspace
            .borrow_mut(t)
            .parent
            .clone()
            .remove_association(workspace, t);
    }

    /// Switching the workspace will hide all windows on other workspaces, show all
    /// visible windows for the given workspace, and focus the active window on the
    /// given workspace.
    pub fn workspace_switch(&mut self, workspace: u32, t: &mut GhostToken<'g>) {
        if self.active_workspace == workspace {
            return;
        }

        self.active_workspace = workspace;
        self.active_workspace_changed = true;

        let mut window_events = HashMap::new();

        std::mem::swap(&mut self.event_queue.windows, &mut window_events);

        for (id, window) in self.windows.iter() {
            let this = window.borrow_mut(t);

            let is_visible = this.visible;

            // If window's workspace is not the same as what is being switched to.
            if this.workspace != workspace {
                if is_visible {
                    this.visible = false;
                    window_events.entry(*id).or_default().visibility = Some(false);
                }

                continue;
            }

            // If window on switched workspace is the active window in a stack
            if let Some(stack) = this.stack.clone() {
                if Rc::ptr_eq(&stack.borrow(t).active, window) {
                    if !is_visible {
                        window.borrow_mut(t).visible = true;
                        window_events.entry(*id).or_default().visibility = Some(true);
                    }
                } else if is_visible {
                    window.borrow_mut(t).visible = false;
                    window_events.entry(*id).or_default().visibility = Some(false);
                }

                continue;
            }

            // All other windows not in a stack
            if !is_visible {
                window.borrow_mut(t).visible = true;
                window_events.entry(*id).or_default().visibility = Some(true);
            }
        }

        std::mem::swap(&mut self.event_queue.windows, &mut window_events);

        let workspace = self
            .workspaces
            .get_mut(&workspace)
            .expect("no workspace assigned")
            .borrow_mut(t);

        if let Some(active) = workspace.focus.clone() {
            self.set_active_window(&active, t);
        }
    }

    /// Associate a workspace with a display, and creates the workspace if it didn't exist.
    pub fn workspace_update(&mut self, workspace: u32, display: u32, t: &mut GhostToken<'g>) {
        let display_ = ward::ward!(self.displays.get(&display).cloned(), else {
            tracing::error!("cannot attach workspace to non-existent display");
            return;
        });

        match self.workspaces.get(&workspace).cloned() {
            Some(workspace) => {
                display_.assign_workspace(workspace, t);
            }
            None => {
                self.workspaces
                    .insert(workspace, display_.create_workspace(workspace, t));
            }
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
        let active = self.tiler.active_window().map(|window| window.id(self.t));

        let displays = self
            .tiler
            .displays
            .iter()
            .map(|(key, display)| (key, display.debug(self.t)))
            .collect::<Vec<_>>();

        fmt.debug_struct("Tiler")
            .field("active", &active)
            .field("displays", &displays)
            .finish()
    }
}
