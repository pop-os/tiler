// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::fork::ForkPtr;
use crate::stack::StackPtr;
use crate::window::WindowPtr;
use crate::Rect;
use crate::Tiler;
use qcell::TCellOwner;
use std::rc::Rc;

pub(crate) enum Branch<T: 'static> {
    Window(WindowPtr<T>),
    Fork(ForkPtr<T>),
    Stack(StackPtr<T>),
}
impl<T: 'static> Clone for Branch<T> {
    fn clone(&self) -> Branch<T> {
        match self {
            Branch::Window(w) => Branch::Window(w.clone()),
            Branch::Fork(f) => Branch::Fork(f.clone()),
            Branch::Stack(s) => Branch::Stack(s.clone()),
        }
    }
}

pub(crate) enum BranchRef<'a, T: 'static> {
    Window(&'a WindowPtr<T>),
    Fork(&'a ForkPtr<T>),
    Stack(&'a StackPtr<T>),
}
impl<'a, T: 'static> Clone for BranchRef<'a, T> {
    fn clone(&self) -> BranchRef<'a, T> {
        match self {
            BranchRef::Window(w) => BranchRef::Window(w),
            BranchRef::Fork(f) => BranchRef::Fork(f),
            BranchRef::Stack(s) => BranchRef::Stack(s),
        }
    }
}
impl<'a, T: 'static> Copy for BranchRef<'a, T> {}

impl<T: 'static> Branch<T> {
    pub fn work_area_update(&self, tiler: &mut Tiler<T>, area: Rect, t: &mut TCellOwner<T>) {
        match self {
            Branch::Fork(ptr) => ptr.work_area_update(tiler, area, t),
            Branch::Stack(ptr) => ptr.work_area_update(tiler, area, t),
            Branch::Window(ptr) => ptr.work_area_update(tiler, area, t),
        }
    }

    pub fn ref_eq<'a>(&self, other: BranchRef<'a, T>) -> bool {
        match (self, other) {
            (Branch::Window(a), BranchRef::Window(b)) => Rc::ptr_eq(a, b),
            (Branch::Fork(a), BranchRef::Fork(b)) => Rc::ptr_eq(a, b),
            (Branch::Stack(a), BranchRef::Stack(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl<T: 'static> PartialEq for Branch<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Branch::Window(a), Branch::Window(b)) => Rc::ptr_eq(a, b),
            (Branch::Fork(a), Branch::Fork(b)) => Rc::ptr_eq(a, b),
            (Branch::Stack(a), Branch::Stack(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}
