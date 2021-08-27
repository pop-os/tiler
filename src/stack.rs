// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::branch::BranchRef;
use crate::fork::ForkPtr;
use crate::tiler::Tiler;
use crate::window::{WindowID, WindowPtr};
use crate::Rect;
use ghost_cell::{GhostCell, GhostToken};
use std::fmt::{self, Debug};
use std::rc::Rc;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug)]
pub enum StackMovement {
    Left(WindowID),
    Right(WindowID),
}

#[derive(Clone, Deref, DerefMut)]
pub(crate) struct StackPtr<'g>(Rc<GhostCell<'g, Stack<'g>>>);

impl<'g> StackPtr<'g> {
    pub fn new(window: &WindowPtr<'g>, parent: ForkPtr<'g>, t: &mut GhostToken<'g>) -> Self {
        let workspace = window.borrow(t).workspace;
        let ptr = StackPtr(Rc::new(GhostCell::new(Stack {
            area: window.borrow(t).rect,
            active: window.clone(),
            parent,
            windows: vec![window.clone()],
            workspace,
        })));

        window.borrow_mut(t).stack = Some(ptr.clone());

        ptr
    }

    pub fn attach(&self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        window.borrow_mut(t).stack = Some(self.clone());
        self.borrow_mut(t).windows.push(window.clone());
    }

    pub fn detach(&self, tiler: &mut Tiler<'g>, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        window.borrow_mut(t).stack = None;
        tiler.event_queue.stack_detach(self, window, t);

        let this = self.borrow_mut(t);

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

            let this = self.borrow_mut(t);

            if this.windows.is_empty() {
                let parent = self.borrow(t).parent.clone();
                tiler.detach_branch(parent, BranchRef::Stack(self), t);
                tiler.event_queue.stack_destroy(self);
            }
        }
    }

    pub fn move_left(&self, t: &mut GhostToken<'g>) -> Option<StackMovement> {
        let this = self.borrow_mut(t);

        if let Some(pos) = this.active_window_position() {
            if pos != 0 {
                this.windows.swap(pos, pos - 1);
                return Some(StackMovement::Left(self.borrow(t).active.id(t)));
            }
        }

        None
    }

    pub fn move_right(&self, t: &mut GhostToken<'g>) -> Option<StackMovement> {
        let this = self.borrow_mut(t);

        if let Some(pos) = this.active_window_position() {
            if pos != this.windows.len() - 1 {
                this.windows.swap(pos, pos + 1);
                return Some(StackMovement::Right(self.borrow(t).active.id(t)));
            }
        }

        None
    }

    pub fn select_left(&self, t: &mut GhostToken<'g>) -> Option<WindowPtr<'g>> {
        let this = self.borrow(t);

        let mut prev = None;

        for window in this.windows.iter() {
            if Rc::ptr_eq(&this.active, window) {
                return prev;
            }

            prev = Some(window.clone());
        }

        None
    }

    pub fn select_right(&self, t: &mut GhostToken<'g>) -> Option<WindowPtr<'g>> {
        let this = self.borrow(t);

        let mut windows = this.windows.iter();

        while let Some(window) = windows.next() {
            if Rc::ptr_eq(window, &this.active) {
                return windows.next().cloned();
            }
        }

        None
    }

    pub fn swap(&self, our: &WindowPtr<'g>, their: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        for window in self.borrow_mut(t).windows.iter_mut() {
            if Rc::ptr_eq(window, our) {
                std::mem::swap(window, &mut their.clone());
            }
        }
    }

    pub fn work_area_refresh(&self, tiler: &mut Tiler<'g>, t: &mut GhostToken<'g>) {
        self.work_area_update(tiler, self.borrow(t).area, t);
    }

    pub fn work_area_update(&self, tiler: &mut Tiler<'g>, area: Rect, t: &mut GhostToken<'g>) {
        self.borrow_mut(t).area = area;
        for window in self.borrow(t).windows.clone() {
            window.work_area_update(tiler, area, t);
        }

        tiler.event_queue.stack_update(self, t);
    }
}

pub(crate) struct Stack<'g> {
    pub area: Rect,
    pub active: WindowPtr<'g>,
    pub parent: ForkPtr<'g>,
    pub windows: Vec<WindowPtr<'g>>,
    pub workspace: u32,
}

impl<'g> Stack<'g> {
    fn active_window_position(&self) -> Option<usize> {
        self.windows
            .iter()
            .position(|win| Rc::ptr_eq(&self.active, win))
    }

    pub fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> StackDebug<'a, 'g> {
        StackDebug::new(self, t)
    }
}

impl<'g> Drop for Stack<'g> {
    fn drop(&mut self) {
        tracing::debug!("Dropped stack");
    }
}

pub struct StackDebug<'a, 'g> {
    stack: &'a Stack<'g>,
    t: &'a GhostToken<'g>,
}

impl<'a, 'g> StackDebug<'a, 'g> {
    fn new(stack: &'a Stack<'g>, t: &'a GhostToken<'g>) -> Self {
        Self { stack, t }
    }
}

impl<'a, 'g> Debug for StackDebug<'a, 'g> {
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
