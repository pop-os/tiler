// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

use crate::components::WindowPtr;
use crate::Rect;
use ghost_cell::{GhostCell, GhostToken};
use std::fmt::{self, Debug};
use std::rc::Rc;

#[derive(Clone, Deref, DerefMut)]
pub struct StackPtr<'g>(Rc<GhostCell<'g, Stack<'g>>>);

impl<'g> StackPtr<'g> {
    pub fn new(window: &WindowPtr<'g>, t: &mut GhostToken<'g>) -> Self {
        let ptr = StackPtr(Rc::new(GhostCell::new(Stack {
            area: window.borrow(t).rect,
            active: window.clone(),
            windows: vec![window.clone()],
        })));

        window.borrow_mut(t).stack = Some(ptr.clone());

        ptr
    }

    pub fn attach(&self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        window.borrow_mut(t).stack = Some(self.clone());
        self.borrow_mut(t).windows.push(window.clone());
    }

    pub fn detach(&self, window: &WindowPtr<'g>, t: &mut GhostToken<'g>) {
        if let Some(pos) = self
            .borrow(t)
            .windows
            .iter()
            .position(|w| Rc::ptr_eq(w, window))
        {
            let this = self.borrow_mut(t);
            this.windows.remove(pos);

            // Set the focus window if the detached window was the active window.
            if Rc::ptr_eq(window, &this.active) {
                if let Some(to_focus) = this
                    .windows
                    .get(pos)
                    .or_else(|| this.windows.get(pos - 1))
                    .cloned()
                {
                    this.active = to_focus;
                    // TODO: Mark that the stack's active window should switch.
                }
            }

            window.borrow_mut(t).stack = None;
        }
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

    pub fn work_area_refresh(&self, t: &mut GhostToken<'g>) {
        self.work_area_update(self.borrow(t).area, t);
    }

    pub fn work_area_update(&self, area: Rect, t: &mut GhostToken<'g>) {
        self.borrow_mut(t).area = area;
        for window in self.borrow(t).windows.clone() {
            window.work_area_update(area, t);
        }
    }
}

pub struct Stack<'g> {
    pub area: Rect,
    pub active: WindowPtr<'g>,
    pub windows: Vec<WindowPtr<'g>>,
}

impl<'g> Stack<'g> {
    pub fn debug<'a>(&'a self, t: &'a GhostToken<'g>) -> StackDebug<'a, 'g> {
        StackDebug::new(self, t)
    }
}

pub struct StackDebug<'a, 'g> {
    pub stack: &'a Stack<'g>,
    pub t: &'a GhostToken<'g>,
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
