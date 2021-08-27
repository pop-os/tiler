// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

use super::{ForkPtr, StackPtr, WindowPtr};
use crate::Rect;
use ghost_cell::GhostToken;
use std::rc::Rc;

#[derive(Clone)]
pub(crate) enum Branch<'g> {
    Window(WindowPtr<'g>),
    Fork(ForkPtr<'g>),
    Stack(StackPtr<'g>),
}

#[derive(Copy, Clone)]
pub(crate) enum BranchRef<'a, 'g> {
    Window(&'a WindowPtr<'g>),
    Fork(&'a ForkPtr<'g>),
    Stack(&'a StackPtr<'g>),
}

impl<'g> Branch<'g> {
    pub fn work_area_update(&self, area: Rect, t: &mut GhostToken<'g>) {
        match self {
            Branch::Fork(ptr) => ptr.work_area_update(area, t),
            Branch::Stack(ptr) => ptr.work_area_update(area, t),
            Branch::Window(ptr) => ptr.work_area_update(area, t),
        }
    }

    pub fn ref_eq<'a>(&self, other: BranchRef<'a, 'g>) -> bool {
        match (self, other) {
            (Branch::Window(a), BranchRef::Window(b)) => Rc::ptr_eq(a, b),
            (Branch::Fork(a), BranchRef::Fork(b)) => Rc::ptr_eq(a, b),
            (Branch::Stack(a), BranchRef::Stack(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl<'g> PartialEq for Branch<'g> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Branch::Window(a), Branch::Window(b)) => Rc::ptr_eq(a, b),
            (Branch::Fork(a), Branch::Fork(b)) => Rc::ptr_eq(a, b),
            (Branch::Stack(a), Branch::Stack(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}
