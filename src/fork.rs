// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

pub(crate) use debug::ForkDebug;

use super::branch::{Branch, BranchRef};
use super::window::WindowPtr;
use crate::{Rect, Tiler};
use either::Either;
use qcell::{TCell, TCellOwner};
use std::rc::Rc;

/// The orientation of a fork.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Deref, DerefMut, From)]
pub(crate) struct ForkPtr<T: 'static>(Rc<TCell<T, Fork<T>>>);
impl<T: 'static> Clone for ForkPtr<T> {
    fn clone(&self) -> ForkPtr<T> {
        ForkPtr(self.0.clone())
    }
}

impl<T: 'static> ForkPtr<T> {
    pub fn new(fork: Fork<T>) -> Self {
        Self(Rc::new(TCell::new(fork)))
    }

    /// Locates the largest window in the fork, walking all of its branches.
    pub fn largest_window(&self, t: &TCellOwner<T>) -> Option<WindowPtr<T>> {
        let mut largest_area = 0;
        let mut largest_window = None;

        let mut compare_window = |window: &WindowPtr<T>| {
            let area = window.ro(t).rect.area();

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
        tiler: &mut Tiler<T>,
        orientation: Orientation,
        t: &mut TCellOwner<T>,
    ) {
        if self.ro(t).orientation == orientation {
            return;
        }

        self.toggle_orientation(tiler, t);
    }

    /// Resets the orientation and split handle of this fork.
    pub fn reset_orientation(&self, tiler: &mut Tiler<T>, t: &mut TCellOwner<T>) {
        let this = self.rw(t);

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
    pub fn resize(&self, tiler: &mut Tiler<T>, split: u32, t: &mut TCellOwner<T>) {
        let this = self.rw(t);

        let area = this.area;
        this.split_handle = match this.orientation {
            Orientation::Horizontal => split.min(area.width),
            Orientation::Vertical => split.min(area.height),
        };

        self.work_area_refresh(tiler, t);
    }

    /// Toggle the orientation of the fork
    pub fn toggle_orientation(&self, tiler: &mut Tiler<T>, t: &mut TCellOwner<T>) {
        let this = self.rw(t);

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
    pub fn swap(&self, our: &WindowPtr<T>, their: &WindowPtr<T>, t: &mut TCellOwner<T>) {
        let this = self.rw(t);
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
    pub fn windows<'a>(&self, t: &'a TCellOwner<T>) -> impl Iterator<Item = WindowPtr<T>> + 'a {
        let mut forks: Vec<ForkPtr<T>> = vec![self.clone()];
        let mut branches: Vec<Branch<T>> = Vec::new();
        let mut windows: Vec<WindowPtr<T>> = Vec::new();

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
                            windows.extend_from_slice(&stack.ro(t).windows);
                            return windows.pop();
                        }
                    }
                }

                if let Some(fork) = forks.pop() {
                    branches.push(fork.ro(t).left.clone());

                    if let Some(right) = fork.ro(t).right.clone() {
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
    pub fn work_area_refresh(&self, tiler: &mut Tiler<T>, t: &mut TCellOwner<T>) {
        self.work_area_update(tiler, self.ro(t).area, t)
    }

    /// Update the work area of the fork and its branches.
    #[allow(clippy::many_single_char_names)]
    pub fn work_area_update(&self, tiler: &mut Tiler<T>, area: Rect, t: &mut TCellOwner<T>) {
        tracing::debug!("assigning fork to {:?}", area);
        let mut left_rect = area;
        let left_branch: Branch<T>;
        let mut right_branch: Option<(Branch<T>, Rect)> = None;

        {
            let this = self.rw(t);

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

    pub fn debug<'a>(&'a self, t: &'a TCellOwner<T>) -> ForkDebug<'a, T> {
        ForkDebug::new(self, t)
    }
}

/// Splits a tile into two branching paths.
///
/// A branch may contain a window, a stack, or another fork. The dimensions of a fork are
/// split between the two branches vertically or horizontally.
pub(crate) struct Fork<T: 'static> {
    /// The position and dimensions of this fork and its children.
    pub area: Rect,

    /// Pointer to the parent of this fork.
    pub parent: Option<ForkPtr<T>>,

    /// The left branch, which is only permitted to be a Fork if the right branch is also allocated.
    pub left: Branch<T>,

    /// Right branch, which may be empty.
    pub right: Option<Branch<T>>,

    /// The ID of the workspace that the fork is attached to.
    pub workspace: u32,

    /// How branches in this fork are aligned.
    pub orientation: Orientation,

    /// Location of the split in this fork.
    pub split_handle: u32,

    /// Tracks when we should flip branches.
    pub orientation_toggled: bool,
}

impl<T: 'static> Fork<T> {
    pub fn new(area: Rect, left: Branch<T>, workspace: u32) -> Self {
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
        branch: BranchRef<'_, T>,
    ) -> Option<Either<&mut Branch<T>, &mut Branch<T>>> {
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

    pub fn left_is(&self, branch: BranchRef<'_, T>) -> bool {
        self.left.ref_eq(branch)
    }

    pub fn right_is(&self, branch: BranchRef<'_, T>) -> bool {
        self.right.as_ref().map_or(false, |r| r.ref_eq(branch))
    }
}

impl<T: 'static> Drop for Fork<T> {
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
    use qcell::TCellOwner;
    use std::fmt::{self, Debug};
    use std::rc::Rc;

    pub(crate) struct ForkDebug<'a, T: 'static> {
        pub fork: &'a ForkPtr<T>,
        pub t: &'a TCellOwner<T>,
    }

    impl<'a, T> ForkDebug<'a, T> {
        pub fn new(fork: &'a ForkPtr<T>, t: &'a TCellOwner<T>) -> Self {
            Self { fork, t }
        }
    }

    impl<'a, T> Debug for ForkDebug<'a, T> {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            fn as_debug<'a, T>(branch: &'a Branch<T>, t: &'a TCellOwner<T>) -> Box<dyn Debug + 'a> {
                match branch {
                    Branch::Window(window) => Box::new(window.id(t)),
                    Branch::Stack(stack) => Box::new(stack.ro(t).debug(t)),
                    Branch::Fork(fork) => Box::new(fork.debug(t)),
                }
            }

            let fork = self.fork.ro(self.t);

            let left = self.fork.ro(self.t).parent.as_ref().map(|p| Rc::as_ptr(&p));

            let right = fork.right.as_ref().map(|branch| as_debug(branch, self.t));

            fmt.debug_struct("Fork")
                .field("ptr", &Rc::as_ptr(self.fork))
                .field("parent", &left)
                .field("orientation", &fork.orientation)
                .field("left", &as_debug(&fork.left, self.t))
                .field("right", &right)
                .finish()
        }
    }
}
