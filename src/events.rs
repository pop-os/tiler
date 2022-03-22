// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::fork::ForkPtr;
use crate::stack::{StackMovement, StackPtr};
use crate::window::WindowPtr;
use crate::{Orientation, Rect, WindowID};
use qcell::TCellOwner;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// Instructs where to place a tiling component entity.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct Placement {
    pub area: Rect,
    pub workspace: u32,
}

/// An event for the window manager to act upon.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub enum Event {
    /// Focus this window.
    Focus(WindowID),

    /// Focus this workspace ID.
    FocusWorkspace(u32),

    /// Where to place a resize handle, in what orientation, and with what range limits.
    Fork(usize, ForkUpdate),

    // Destroy the fork associated with this ID.
    ForkDestroy(usize),

    /// A window was assigned to a stack
    StackAssign(usize, WindowID),

    /// A window was detached from a stack
    StackDetach(usize, WindowID),

    /// Destroy the stack associated with this ID.
    StackDestroy(usize),

    /// Alter the dimensions of an existing stack.
    StackPlace(usize, Placement),

    /// Raise this window of a stack to the top.
    /// Other windows in this stack should be hidden.
    StackRaise(usize, WindowID),

    /// Swap the position of these windows in a stack
    StackMovement(usize, StackMovement),

    // Change the visibility of a stack.
    StackVisibility(usize, bool),

    /// Alter the dimensions of a window actor.
    WindowPlace(WindowID, Placement),

    /// Change the visibility of a window.
    WindowVisibility(WindowID, bool),

    // Assign workspace to diplsay
    WorkspaceAssign {
        workspace: u32,
        display: u32,
    },
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct ForkUpdate {
    /// On what workspace the fork resides.
    pub workspace: u32,
    /// The orientation of this fork.
    pub orientation: Orientation,
    /// The region this fork occupies.
    pub rect: Rect,
    /// Where to place the resize handle in that region.
    pub handle: u32,
}

#[derive(Default)]
pub struct ForkEvents {
    pub destroy: bool,
    pub update: Option<ForkUpdate>,
}

#[derive(Default)]
pub struct WindowEvents {
    pub place: Option<Placement>,
    pub visibility: Option<bool>,
}

#[derive(Default)]
pub struct StackEvents {
    pub destroy: bool,
    pub assignments: BTreeMap<WindowID, bool>,
    pub place: Option<Placement>,
    pub visibility: Option<bool>,
    pub raise: Option<WindowID>,
}

pub(crate) struct EventQueue<T: 'static> {
    pub(crate) forks: BTreeMap<usize, ForkEvents>,
    pub(crate) windows: HashMap<WindowID, WindowEvents>,
    pub(crate) stacks: BTreeMap<usize, StackEvents>,
    pub(crate) events: Vec<Event>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: 'static> Default for EventQueue<T> {
    fn default() -> EventQueue<T> {
        EventQueue {
            forks: BTreeMap::new(),
            windows: HashMap::new(),
            stacks: BTreeMap::new(),
            events: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: 'static> EventQueue<T> {
    pub fn consume_events(&mut self) -> impl Iterator<Item = Event> + '_ {
        let fork_events = {
            let mut forks = BTreeMap::new();
            std::mem::swap(&mut self.forks, &mut forks);

            forks.into_iter().filter_map(|(id, event)| {
                if event.destroy {
                    Some(Event::ForkDestroy(id))
                } else {
                    event.update.map(|update| Event::Fork(id, update))
                }
            })
        };

        let stack_events = {
            let mut stacks = BTreeMap::new();
            std::mem::swap(&mut self.stacks, &mut stacks);

            stacks.into_iter().flat_map(|(a, events)| {
                let attachments = events.assignments.into_iter().map(move |(id, attached)| {
                    if attached {
                        Event::StackAssign(a, id)
                    } else {
                        Event::StackDetach(a, id)
                    }
                });

                let iterator: Box<dyn Iterator<Item = Event>> = if events.destroy {
                    Box::new(std::iter::once(Event::StackDestroy(a)))
                } else {
                    let placement = events
                        .place
                        .into_iter()
                        .map(move |p| Event::StackPlace(a, p));

                    let visibility = events
                        .visibility
                        .into_iter()
                        .map(move |v| Event::StackVisibility(a, v));

                    Box::new(placement.chain(visibility))
                };

                iterator.chain(attachments)
            })
        };

        let window_events = {
            let mut windows = HashMap::new();
            std::mem::swap(&mut self.windows, &mut windows);

            windows.into_iter().flat_map(|(a, events)| {
                let placement = events
                    .place
                    .into_iter()
                    .map(move |p| Event::WindowPlace(a, p));

                let visibility = events
                    .visibility
                    .into_iter()
                    .map(move |v| Event::WindowVisibility(a, v));

                placement.chain(visibility)
            })
        };

        fork_events
            .chain(stack_events)
            .chain(window_events)
            .chain(self.events.drain(..))
    }

    /// Instruct the window manager that this fork was destroyed.
    pub fn fork_destroy(&mut self, fork: &ForkPtr<T>) {
        tracing::debug!("destroying Fork({:?})", Rc::as_ptr(fork));
        self.forks
            .entry(Rc::as_ptr(fork) as usize)
            .or_default()
            .destroy = true;
    }

    /// Instruct the window manager about this fork's dimensions and split handle.
    pub fn fork_update(&mut self, fork: &ForkPtr<T>, t: &TCellOwner<T>) {
        self.forks
            .entry(Rc::as_ptr(fork) as usize)
            .or_default()
            .update = Some({
            let fork = fork.ro(t);
            ForkUpdate {
                workspace: fork.workspace,
                orientation: fork.orientation,
                rect: fork.area,
                handle: fork.split_handle,
            }
        });
    }

    /// Instruct the window manager that a window was assigned to a stack.
    pub fn stack_assign(&mut self, stack: &StackPtr<T>, window: &WindowPtr<T>, t: &TCellOwner<T>) {
        *self
            .stacks
            .entry(Rc::as_ptr(stack) as usize)
            .or_default()
            .assignments
            .entry(window.id(t))
            .or_default() = true;
    }

    /// Instruct the window manager that a window was detached from a stack.
    pub fn stack_detach(&mut self, stack: &StackPtr<T>, window: &WindowPtr<T>, t: &TCellOwner<T>) {
        *self
            .stacks
            .entry(Rc::as_ptr(stack) as usize)
            .or_default()
            .assignments
            .entry(window.id(t))
            .or_default() = false;
    }

    /// Instruct the window manager that this stack was destroyed.
    pub fn stack_destroy(&mut self, stack: &StackPtr<T>) {
        self.stacks
            .entry(Rc::as_ptr(stack) as usize)
            .or_default()
            .destroy = true;
    }

    /// Instruct the window manager to ensure that this window should be the visible one in the stack.
    pub fn stack_raise_window(
        &mut self,
        stack: &StackPtr<T>,
        window: &WindowPtr<T>,
        t: &TCellOwner<T>,
    ) {
        self.stacks
            .entry(Rc::as_ptr(stack) as usize)
            .or_default()
            .raise = Some(window.id(t))
    }

    pub fn stack_movement(&mut self, stack: &StackPtr<T>, movement: StackMovement) {
        self.events
            .push(Event::StackMovement(Rc::as_ptr(stack) as usize, movement));
    }

    /// Instruct the window manager about a placement of a stack.
    pub fn stack_update(&mut self, stack: &StackPtr<T>, t: &TCellOwner<T>) {
        let stack_ = stack.ro(t);
        self.stacks
            .entry(Rc::as_ptr(stack) as usize)
            .or_default()
            .place = Some(Placement {
            area: stack_.area,
            workspace: stack_.workspace,
        });
    }
}
