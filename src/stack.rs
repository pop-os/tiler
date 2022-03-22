// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::branch::BranchRef;
use crate::fork::ForkPtr;
use crate::tiler::Tiler;
use crate::window::{WindowID, WindowPtr};
use crate::Rect;
use qcell::{TCell, TCellOwner};
use std::fmt::{self, Debug};
use std::rc::Rc;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug)]
pub enum StackMovement {
    Left(WindowID),
    Right(WindowID),
}

#[derive(Deref, DerefMut)]
pub(crate) struct StackPtr<T: 'static>(Rc<TCell<T, Stack<T>>>);
impl<T: 'static> Clone for StackPtr<T> {
    fn clone(&self) -> StackPtr<T> {
        StackPtr(self.0.clone())
    }
}

impl<T: 'static> StackPtr<T> {
    pub fn new(window: &WindowPtr<T>, parent: ForkPtr<T>, t: &mut TCellOwner<T>) -> Self {
        let workspace = window.ro(t).workspace;
        let ptr = StackPtr(Rc::new(TCell::new(Stack {
            area: window.ro(t).rect,
            active: window.clone(),
            parent,
            windows: vec![window.clone()],
            workspace,
        })));

        window.rw(t).stack = Some(ptr.clone());

        ptr
    }

    pub fn attach(&self, window: &WindowPtr<T>, t: &mut TCellOwner<T>) {
        window.rw(t).stack = Some(self.clone());
        self.rw(t).windows.push(window.clone());
    }

    pub fn detach(&self, tiler: &mut Tiler<T>, window: &WindowPtr<T>, t: &mut TCellOwner<T>) {
        window.rw(t).stack = None;
        tiler.event_queue.stack_detach(self, window, t);

        let this = self.rw(t);

        if let Some(pos) = this.windows.iter().position(|w| Rc::ptr_eq(w, window)) {
            this.windows.remove(pos);

            // Set the focus window if the detached window was the active window.
            if Rc::ptr_eq(window, &this.active) {
                if let Some(to_focus) = this
                    .windows
                    .get(pos)
                    .or_else(|| this.windows.get(pos - 1))
                    .cloned()
                {
                    this.active = to_focus.clone();
                    tiler.event_queue.stack_raise_window(self, &to_focus, t);
                }
            }

            let this = self.rw(t);

            if this.windows.is_empty() {
                let parent = self.ro(t).parent.clone();
                tiler.detach_branch(parent, BranchRef::Stack(self), t);
                tiler.event_queue.stack_destroy(self);
            }
        }
    }

    pub fn move_left(&self, t: &mut TCellOwner<T>) -> Option<StackMovement> {
        let this = self.rw(t);

        if let Some(pos) = this.active_window_position() {
            if pos != 0 {
                this.windows.swap(pos, pos - 1);
                return Some(StackMovement::Left(self.ro(t).active.id(t)));
            }
        }

        None
    }

    pub fn move_right(&self, t: &mut TCellOwner<T>) -> Option<StackMovement> {
        let this = self.rw(t);

        if let Some(pos) = this.active_window_position() {
            if pos != this.windows.len() - 1 {
                this.windows.swap(pos, pos + 1);
                return Some(StackMovement::Right(self.ro(t).active.id(t)));
            }
        }

        None
    }

    pub fn select_left(&self, t: &mut TCellOwner<T>) -> Option<WindowPtr<T>> {
        let this = self.ro(t);

        let mut prev = None;

        for window in this.windows.iter() {
            if Rc::ptr_eq(&this.active, window) {
                return prev;
            }

            prev = Some(window.clone());
        }

        None
    }

    pub fn select_right(&self, t: &mut TCellOwner<T>) -> Option<WindowPtr<T>> {
        let this = self.ro(t);

        let mut windows = this.windows.iter();

        while let Some(window) = windows.next() {
            if Rc::ptr_eq(window, &this.active) {
                return windows.next().cloned();
            }
        }

        None
    }

    pub fn swap(&self, our: &WindowPtr<T>, their: &WindowPtr<T>, t: &mut TCellOwner<T>) {
        for window in self.rw(t).windows.iter_mut() {
            if Rc::ptr_eq(window, our) {
                std::mem::swap(window, &mut their.clone());
            }
        }
    }

    pub fn work_area_refresh(&self, tiler: &mut Tiler<T>, t: &mut TCellOwner<T>) {
        self.work_area_update(tiler, self.ro(t).area, t);
    }

    pub fn work_area_update(&self, tiler: &mut Tiler<T>, area: Rect, t: &mut TCellOwner<T>) {
        self.rw(t).area = area;
        for window in self.ro(t).windows.clone() {
            window.work_area_update(tiler, area, t);
        }

        tiler.event_queue.stack_update(self, t);
    }
}

pub(crate) struct Stack<T: 'static> {
    pub area: Rect,
    pub active: WindowPtr<T>,
    pub parent: ForkPtr<T>,
    pub windows: Vec<WindowPtr<T>>,
    pub workspace: u32,
}

impl<T: 'static> Stack<T> {
    fn active_window_position(&self) -> Option<usize> {
        self.windows
            .iter()
            .position(|win| Rc::ptr_eq(&self.active, win))
    }

    pub fn debug<'a>(&'a self, t: &'a TCellOwner<T>) -> StackDebug<'a, T> {
        StackDebug::new(self, t)
    }
}

impl<T: 'static> Drop for Stack<T> {
    fn drop(&mut self) {
        tracing::debug!("Dropped stack");
    }
}

pub struct StackDebug<'a, T: 'static> {
    stack: &'a Stack<T>,
    t: &'a TCellOwner<T>,
}

impl<'a, T: 'static> StackDebug<'a, T> {
    fn new(stack: &'a Stack<T>, t: &'a TCellOwner<T>) -> Self {
        Self { stack, t }
    }
}

impl<'a, T> Debug for StackDebug<'a, T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let windows: Vec<_> = self
            .stack
            .windows
            .iter()
            .map(|window| window.id(self.t))
            .collect();

        fmt.debug_struct("Stack")
            .field("windows", &windows)
            .finish()
    }
}
