// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

pub(crate) use debug::ForkDebug;

use super::branch::{Branch, BranchRef};
use super::window::WindowPtr;
use crate::Rect;
use ghost_cell::{GhostCell, GhostToken};
use std::rc::Rc;

/// The orientation of a fork.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Clone, Deref, DerefMut, From)]
pub(crate) struct ForkPtr<'g>(Rc<GhostCell<'g, Fork<'g>>>);

impl<'g> ForkPtr<'g> {
    pub fn new(fork: Fork<'g>) -> Self {
        Self(Rc::new(GhostCell::new(fork)))
    }

    /// Locates the largest window in the fork, walking all of its branches.
    pub fn largest_window(&self, t: &GhostToken<'g>) -> Option<WindowPtr<'g>> {
        let mut largest_area = 0;
        let mut largest_window = None;

        let mut compare_window = |window: &WindowPtr<'g>| {
            let area = window.borrow(t).rect.area();

            if area > largest_area {
                largest_area = area;
                largest_window = Some(window.clone());
            }
        };

        for window in self.windows(t) {
            compare_window(&window);
        }

        largest_window
    }

    pub fn reset_orientation(&self, t: &mut GhostToken<'g>) {
        self.borrow_mut(t).orientation = preferred_orientation(self.borrow(t).area);
    }

    pub fn swap(&self, our: &WindowPtr<'g>, their: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        let this = self.borrow_mut(t);
        if let Branch::Window(ref mut window) = this.left {
            if Rc::ptr_eq(window, our) {
                this.left = Branch::Window(their.clone());
                return;
            }
        }

        if let Some(Branch::Window(ref mut window)) = this.right {
            if Rc::ptr_eq(window, our) {
                this.right = Some(Branch::Window(their.clone()));
            }
        }
    }

    pub fn windows<'a>(&self, t: &'a GhostToken<'g>) -> impl Iterator<Item = WindowPtr<'g>> + 'a {
        let mut forks: Vec<ForkPtr> = vec![self.clone()];
        let mut branches: Vec<Branch> = Vec::new();
        let mut windows: Vec<WindowPtr<'g>> = Vec::new();

        std::iter::from_fn(move || {
            loop {
                if let Some(window) = windows.pop() {
                    return Some(window);
                }

                if let Some(branch) = branches.pop() {
                    match branch {
                        Branch::Window(window) => {
                            return Some(window);
                        }

                        Branch::Fork(fork) => {
                            forks.push(fork.clone());
                        }

                        Branch::Stack(stack) => {
                            windows.extend_from_slice(&stack.borrow(t).windows);
                            return windows.pop();
                        }
                    }
                }

                if let Some(fork) = forks.pop() {
                    branches.push(fork.borrow(t).left.clone());

                    if let Some(right) = fork.borrow(t).right.clone() {
                        branches.push(right);
                    }

                    continue;
                }

                break;
            }

            None
        })
    }

    /// Recalculate the work areas of the fork's branches.
    pub fn work_area_refresh(&self, t: &mut GhostToken<'g>) {
        self.work_area_update(self.borrow(t).area, t)
    }

    /// Update the work area of the fork and its branches.
    #[allow(clippy::many_single_char_names)]
    pub fn work_area_update(&self, area: Rect, t: &mut GhostToken<'g>) {
        let mut left_rect = area;
        let left_branch: Branch<'g>;
        let mut right_branch: Option<(Branch<'g>, Rect)> = None;

        {
            let this = self.borrow_mut(t);

            left_branch = this.left.clone();

            if let Some(right) = this.right.clone() {
                let x = area.x;
                let y = area.y;
                let w = area.width;
                let h = area.height;

                match this.orientation {
                    Orientation::Vertical => {
                        let r = h * (this.ratio as u32) / 100;
                        left_rect = Rect::new(x, y, w, r);
                        right_branch = Some((right, Rect::new(x, y + r, w, h - r)));
                    }

                    Orientation::Horizontal => {
                        let r = w * (this.ratio as u32) / 100;
                        left_rect = Rect::new(x, y, r, h);
                        right_branch = Some((right, Rect::new(x + r, y, w - r, h)));
                    }
                }
            }

            this.area = area;
        };

        left_branch.work_area_update(left_rect, t);

        if let Some((branch, rect)) = right_branch {
            branch.work_area_update(rect, t);
        }
    }

    pub fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> ForkDebug<'a, 'g> {
        ForkDebug::new(self, t)
    }
}

/// Splits a tile into two branching paths.
///
/// A branch may contain a window, a stack, or another fork. The dimensions of a fork are
/// split between the two branches vertically or horizontally.
pub(crate) struct Fork<'g> {
    /// The position and dimensions of this fork and its children.
    pub area: Rect,

    /// The left branch, which is only permitted to be a Fork if the right branch is also allocated.
    pub left: Branch<'g>,

    /// Right branch, which may be empty.
    pub right: Option<Branch<'g>>,

    /// The ID of the workspace that the fork is attached to.
    pub workspace: u32,

    /// How branches in this fork are aligned.
    pub orientation: Orientation,

    /// Pointer to the parent of this fork.
    pub parent: Option<ForkPtr<'g>>,

    /// Value between 1 and 100 for dividing space between the two branches
    pub ratio: u8,
}

impl<'g> Fork<'g> {
    pub fn new(area: Rect, left: Branch<'g>, workspace: u32) -> Self {
        Self {
            area,
            left,
            right: None,
            workspace,
            orientation: preferred_orientation(area),
            parent: None,
            ratio: 50,
        }
    }

    pub fn branch_of(&mut self, branch: BranchRef<'_, 'g>) -> Option<&mut Branch<'g>> {
        if self.left_is(branch) {
            return Some(&mut self.left);
        }

        if let Some(right) = &mut self.right {
            if right.ref_eq(branch) {
                return Some(right);
            }
        }

        None
    }

    pub fn left_is(&self, branch: BranchRef<'_, 'g>) -> bool {
        self.left.ref_eq(branch)
    }

    pub fn right_is(&self, branch: BranchRef<'_, 'g>) -> bool {
        self.right.as_ref().map_or(false, |r| r.ref_eq(branch))
    }
}

fn preferred_orientation(rect: Rect) -> Orientation {
    if rect.height > rect.width {
        Orientation::Vertical
    } else {
        Orientation::Horizontal
    }
}

mod debug {
    use super::{Branch, ForkPtr};
    use ghost_cell::GhostToken;
    use std::fmt::{self, Debug};

    pub(crate) struct ForkDebug<'a, 'g> {
        pub fork: &'a ForkPtr<'g>,
        pub t: &'a GhostToken<'g>,
    }

    impl<'a, 'g> ForkDebug<'a, 'g> {
        pub fn new(fork: &'a ForkPtr<'g>, t: &'a GhostToken<'g>) -> Self {
            Self { fork, t }
        }
    }

    impl<'a, 'g> Debug for ForkDebug<'a, 'g> {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            fn as_debug<'a, 'g>(
                branch: &'a Branch<'g>,
                t: &'a GhostToken<'g>,
            ) -> Box<dyn Debug + 'a> {
                match branch {
                    Branch::Window(window) => Box::new(window.id(t)),
                    Branch::Stack(stack) => Box::new(stack.borrow(t).debug(t)),
                    Branch::Fork(fork) => Box::new(fork.debug(t)),
                }
            }

            let fork = self.fork.borrow(self.t);

            let left = self.fork.borrow(self.t).parent.as_ref().map(|p| p.as_ptr());

            let right = fork.right.as_ref().map(|branch| as_debug(branch, self.t));

            fmt.debug_struct("Fork")
                .field("ptr", &self.fork.as_ptr())
                .field("parent", &left)
                .field("orientation", &fork.orientation)
                .field("left", &as_debug(&fork.left, self.t))
                .field("right", &right)
                .finish()
        }
    }
}
