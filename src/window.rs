// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::branch::{Branch, BranchRef};
use crate::fork::ForkPtr;
use crate::stack::StackPtr;
use crate::tiler::Tiler;
use crate::{Placement, Rect};
use either::Either;
use ghost_cell::{GhostCell, GhostToken};
use std::fmt::{self, Debug};
use std::rc::Rc;

/// An ID assigned to a window by a window manager.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
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

    /// Focus this window in the tree.
    pub(crate) fn focus(&self, tiler: &mut Tiler<'g>, t: &mut GhostToken<'g>) {
        if let Some(focus) = tiler.active_window() {
            if Rc::ptr_eq(focus, self) {
                return;
            }
        }

        if let Some(stack) = self.stack(t) {
            let mut visibility = Vec::new();
            for this in stack.borrow(t).windows.iter() {
                visibility.push((this.id(t), Rc::ptr_eq(self, this)));
            }

            for (id, show) in visibility {
                tiler.event_queue.windows.entry(id).or_default().visibility = Some(show);
            }
        }

        tiler.set_active_window(self, t)
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

    /// Get the pointer to the stack it may be associated with.
    pub(crate) fn stack(&self, t: &GhostToken<'g>) -> Option<StackPtr<'g>> {
        self.borrow(t).stack.clone()
    }

    /// If a window is stacked, unstack it. If it is not stacked, stack it.
    pub(crate) fn stack_toggle(&self, tiler: &mut Tiler<'g>, t: &mut GhostToken<'g>) {
        if let Some(stack) = self.stack(t) {
            stack.detach(tiler, self, t);

            if stack.borrow(t).windows.is_empty() {
                let fork = ward::ward!(self.fork(t), else {
                    tracing::error!("window does not have a parent fork");
                    return;
                });

                let fork_ = fork.borrow_mut(t);

                let branch = ward::ward!(fork_.branch(BranchRef::Stack(&stack)), else {
                    tracing::error!("parent fork of window did not have a stack assocation for this window");
                    return;
                });

                let (Either::Left(branch) | Either::Right(branch)) = branch;
                *branch = Branch::Window(self.clone());
                tiler.event_queue.stack_destroy(&stack);
            }

            return;
        }

        let fork = ward::ward!(self.fork(t), else {
            tracing::error!("cannot stack because window does not have a parent fork");
            return;
        });

        let stack = StackPtr::new(self, fork.clone(), t);

        let branch = ward::ward!(fork.borrow_mut(t).branch(BranchRef::Window(self)), else {
            tracing::error!("cannot stack because window has invalid parent fork");
            stack.detach(tiler, self, t);
            return;
        });

        let (Either::Left(branch) | Either::Right(branch)) = branch;
        *branch = Branch::Stack(stack.clone());

        tiler.event_queue.stack_update(&stack, t);
        tiler.event_queue.stack_assign(&stack, self, t);
    }

    /// Swaps the tree location of this window with another.
    pub(crate) fn swap_position_with(
        &self,
        tiler: &mut Tiler<'g>,
        other: &WindowPtr<'g>,
        t: &mut GhostToken<'g>,
    ) {
        if let Some(stack) = self.stack(t) {
            stack.swap(self, other, t);
            stack.work_area_refresh(tiler, t);
        } else if let Some(fork) = self.fork(t) {
            fork.swap(self, other, t);
            fork.work_area_refresh(tiler, t);
        }

        if let Some(stack) = other.stack(t) {
            stack.swap(other, self, t);
            stack.work_area_refresh(tiler, t)
        } else if let Some(fork) = other.fork(t) {
            fork.swap(other, self, t);
            fork.work_area_refresh(tiler, t);
        }
    }

    /// Update the position and dimensions of this window.
    pub(crate) fn work_area_update(
        &self,
        tiler: &mut Tiler<'g>,
        area: Rect,
        t: &mut GhostToken<'g>,
    ) {
        let this = self.borrow_mut(t);
        if this.rect != area {
            this.rect = area;
        }

        let id = this.id;
        let workspace = this.workspace;

        tiler.event_queue.windows.entry(id).or_default().place = Some(Placement { area, workspace })
    }
}

pub struct Window<'g> {
    pub(crate) fork: Option<ForkPtr<'g>>,
    pub(crate) id: WindowID,
    pub(crate) rect: Rect,
    pub(crate) stack: Option<StackPtr<'g>>,
    pub(crate) workspace: u32,
    pub(crate) visible: bool,
}

impl<'g> Window<'g> {
    pub(crate) fn new<I: Into<WindowID>>(id: I) -> Self {
        Self {
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

impl<'g> Drop for Window<'g> {
    fn drop(&mut self) {
        tracing::debug!("dropped {:?}", self.id);
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
