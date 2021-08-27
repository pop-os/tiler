// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

pub(crate) use debug::ForkDebug;

use super::branch::{Branch, BranchRef};
use super::window::WindowPtr;
use crate::{Rect, Tiler};
use either::Either;
use ghost_cell::{GhostCell, GhostToken};
use std::rc::Rc;

/// The orientation of a fork.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Orientation {
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

    /// Change the orientation of the fork, if it differs.
    pub fn orientation_set(
        &self,
        tiler: &mut Tiler<'g>,
        orientation: Orientation,
        t: &mut GhostToken<'g>,
    ) {
        if self.borrow(t).orientation == orientation {
            return;
        }

        self.toggle_orientation(tiler, t);
    }

    /// Resets the orientation and split handle of this fork.
    pub fn reset_orientation(&self, tiler: &mut Tiler<'g>, t: &mut GhostToken<'g>) {
        let this = self.borrow_mut(t);

        this.split_handle = match this.orientation {
            Orientation::Horizontal => this.area.width / 2,
            Orientation::Vertical => this.area.height / 2,
        };

        let preferred = preferred_orientation(this.area);

        if this.orientation != preferred {
            self.toggle_orientation(tiler, t)
        }
    }

    /// Resize a fork with a new split
    pub fn resize(&self, tiler: &mut Tiler<'g>, split: u32, t: &mut GhostToken<'g>) {
        let this = self.borrow_mut(t);

        let area = this.area;
        this.split_handle = match this.orientation {
            Orientation::Horizontal => split.min(area.width),
            Orientation::Vertical => split.min(area.height),
        };

        self.work_area_refresh(tiler, t);
    }

    /// Toggle the orientation of the fork
    pub fn toggle_orientation(&self, tiler: &mut Tiler<'g>, t: &mut GhostToken<'g>) {
        let this = self.borrow_mut(t);

        this.split_handle = match this.orientation {
            Orientation::Horizontal => {
                this.orientation = Orientation::Vertical;
                let ratio = (this.split_handle * 100) / this.area.width;
                (this.area.height * ratio) / 100
            }

            Orientation::Vertical => {
                this.orientation = Orientation::Horizontal;
                let ratio = (this.split_handle * 100) / this.area.height;
                (this.area.width * ratio) / 100
            }
        };

        // Swap branches if a fork has had its orientation toggled twice.
        if this.orientation_toggled {
            if let Some(right) = this.right.as_mut() {
                std::mem::swap(&mut this.left, right);
            }
        }

        this.orientation_toggled = !this.orientation_toggled;

        self.work_area_refresh(tiler, t);
    }

    /// Swaps a window owned by this fork with a different window.
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

    /// Generator which locates all windows in this fork, but does allocate.
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
    pub fn work_area_refresh(&self, tiler: &mut Tiler<'g>, t: &mut GhostToken<'g>) {
        self.work_area_update(tiler, self.borrow(t).area, t)
    }

    /// Update the work area of the fork and its branches.
    #[allow(clippy::many_single_char_names)]
    pub fn work_area_update(&self, tiler: &mut Tiler<'g>, area: Rect, t: &mut GhostToken<'g>) {
        tracing::debug!("assigning fork to {:?}", area);
        let mut left_rect = area;
        let left_branch: Branch<'g>;
        let mut right_branch: Option<(Branch<'g>, Rect)> = None;

        {
            let this = self.borrow_mut(t);

            // Update the location of the split in the fork
            this.split_handle = match this.orientation {
                Orientation::Horizontal => {
                    let ratio = this.split_handle * 100 / this.area.width;
                    area.width * ratio / 100
                }

                Orientation::Vertical => {
                    let ratio = this.split_handle * 100 / this.area.height;
                    area.height * ratio / 100
                }
            };

            left_branch = this.left.clone();

            if let Some(right) = this.right.clone() {
                let x = area.x;
                let y = area.y;
                let w = area.width;
                let h = area.height;
                let r = this.split_handle;

                match this.orientation {
                    Orientation::Vertical => {
                        left_rect = Rect::new(x, y, w, r);
                        right_branch = Some((right, Rect::new(x, y + r, w, h - r)));
                    }

                    Orientation::Horizontal => {
                        left_rect = Rect::new(x, y, r, h);
                        right_branch = Some((right, Rect::new(x + r, y, w - r, h)));
                    }
                }
            }

            this.area = area;
        };

        // tracing::debug!("left branch = {:?}; right branch = {:?}", left_rect, right_branch.as_ref().map(|x| x.1));

        left_branch.work_area_update(tiler, left_rect, t);

        if let Some((branch, rect)) = right_branch {
            branch.work_area_update(tiler, rect, t);
        }

        tiler.event_queue.fork_update(self, t);
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

    /// Pointer to the parent of this fork.
    pub parent: Option<ForkPtr<'g>>,

    /// The left branch, which is only permitted to be a Fork if the right branch is also allocated.
    pub left: Branch<'g>,

    /// Right branch, which may be empty.
    pub right: Option<Branch<'g>>,

    /// The ID of the workspace that the fork is attached to.
    pub workspace: u32,

    /// How branches in this fork are aligned.
    pub orientation: Orientation,

    /// Location of the split in this fork.
    pub split_handle: u32,

    /// Tracks when we should flip branches.
    pub orientation_toggled: bool,
}

impl<'g> Fork<'g> {
    pub fn new(area: Rect, left: Branch<'g>, workspace: u32) -> Self {
        let orientation = preferred_orientation(area);

        let split_handle = match orientation {
            Orientation::Horizontal => area.x_center() - 1,
            Orientation::Vertical => area.y_center() - 1,
        };

        Self {
            area,
            left,
            right: None,
            workspace,
            orientation,
            parent: None,
            split_handle,
            orientation_toggled: false,
        }
    }

    pub fn branch(
        &mut self,
        branch: BranchRef<'_, 'g>,
    ) -> Option<Either<&mut Branch<'g>, &mut Branch<'g>>> {
        if self.left_is(branch) {
            return Some(Either::Left(&mut self.left));
        }

        if let Some(right) = &mut self.right {
            if right.ref_eq(branch) {
                return Some(Either::Right(right));
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

impl<'g> Drop for Fork<'g> {
    fn drop(&mut self) {
        tracing::debug!("dropped fork");
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
