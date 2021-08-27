// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

#[cfg(feature = "ipc")]
#[macro_use]
extern crate serde;

use async_channel::{Receiver, RecvError, SendError, Sender};
use ghost_cell::GhostToken;
use pop_tiler::*;
use std::thread;
use thiserror::Error as ThisError;

pub type Response = Vec<Event>;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("pop-tiler server-side request error")]
    ServerRequest(#[source] RecvError),

    #[error("pop-tiler client-side response error")]
    ClientResponse(#[source] RecvError),

    #[error("pop-tiler client-side request error")]
    ClientRequest(#[source] SendError<Request>),

    #[error("pop-tiler server-side response error")]
    ServerResponse(#[source] SendError<Response>),
}

/// An instruction to send to the pop-tiling service
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub enum Request {
    /// Attach a window to the tiler.
    Attach(WindowID),
    /// Detach a window from the tiler.
    Detach(WindowID),
    /// Insert or update the dimensions of a display.
    DisplayUpdate { display: u32, dimensions: Rect },
    /// Remove a display from the tree.
    DisplayDetach(u32),
    /// Make this window the actively-focused window.
    Focus(WindowID),
    /// Focus the window above the active window.
    FocusAbove,
    /// Focus the window below the active window.
    FocusBelow,
    /// Focus the display above the active one.
    FocusDisplayAbove,
    /// Focus the display below the active one.
    FocusDisplayBelow,
    /// Focus the display to the left of the active one.
    FocusDisplayLeft,
    /// Focus the display to the right of the active one.
    FocusDisplayRight,
    /// Focus the window to the left of the active one.
    FocusLeft,
    /// Focus the window to the right of the active one.
    FocusRight,
    /// Move the active window above.
    MoveAbove,
    /// Move the active window below.
    MoveBelow,
    /// Move the active window to the left.
    MoveLeft,
    /// Move the active window to the right.
    MoveRight,
    /// Toggle the orientation of the fork the window is attached to.
    ToggleOrientation,
    /// Toggle the stackability of the window.
    ToggleStack,
    /// Resize a fork with an updated split.
    Resize(usize, u32),
    /// Swap the positions of two windows.
    Swap(WindowID, WindowID),
    /// Switch to a different workspace.
    WorkspaceSwitch(u32),
    /// Associate a workspace with a display.
    WorkspaceUpdate { workspace: u32, display: u32 },
}

pub struct Service<'g> {
    tiler: Tiler<'g>,
}

impl<'g> Default for Service<'g> {
    fn default() -> Self {
        Self {
            tiler: Tiler::default(),
        }
    }
}

impl<'g> Service<'g> {
    pub fn handle<'a>(
        &'a mut self,
        input: Request,
        t: &'a mut GhostToken<'g>,
    ) -> impl Iterator<Item = Event> + 'a {
        let tiler = &mut self.tiler;

        let window_from_id = |window: WindowID| tiler.windows.get(&window).cloned();

        match input {
            Request::Attach(window) => {
                if let Some(window) = window_from_id(window) {
                    tiler.attach(&window, t)
                }
            }

            Request::Detach(window) => {
                if let Some(window) = window_from_id(window) {
                    tiler.detach(&window, t);
                }
            }

            Request::DisplayUpdate {
                display,
                dimensions,
            } => {
                tiler.display_update(display, dimensions, t);
            }

            Request::DisplayDetach(display_id) => tiler.display_detach(display_id, t),

            Request::Focus(window) => {
                if let Some(window) = window_from_id(window) {
                    tiler.focus(&window, t);
                }
            }

            Request::FocusAbove => tiler.focus_above(t),
            Request::FocusBelow => tiler.focus_below(t),
            Request::FocusLeft => tiler.focus_left(t),
            Request::FocusRight => tiler.focus_right(t),
            Request::FocusDisplayAbove => tiler.focus_display_above(t),
            Request::FocusDisplayBelow => tiler.focus_display_below(t),
            Request::FocusDisplayLeft => tiler.focus_display_left(t),
            Request::FocusDisplayRight => tiler.focus_display_right(t),
            Request::MoveAbove => tiler.move_above(t),
            Request::MoveBelow => tiler.move_below(t),
            Request::MoveLeft => tiler.move_left(t),
            Request::MoveRight => tiler.move_right(t),

            Request::Resize(fork, handle) => tiler.fork_resize(fork, handle, t),

            Request::Swap(a, b) => {
                if let Some((a, b)) = window_from_id(a).zip(window_from_id(b)) {
                    tiler.swap(&a, &b, t);
                }
            }

            Request::ToggleOrientation => tiler.toggle_orientation(t),

            Request::ToggleStack => tiler.stack_toggle(t),

            Request::WorkspaceSwitch(workspace) => {
                tiler.workspace_switch(workspace, t);
            }

            Request::WorkspaceUpdate { display, workspace } => {
                tiler.workspace_update(workspace, display, t);
            }
        }

        self.tiler.events(t)
    }
}

/// Handle for sending and receiving instructions to and from the pop-tiler.
struct ClientThread {
    send: Sender<Request>,
    recv: Receiver<Response>,
}

impl ClientThread {
    pub fn new(send: Sender<Request>, recv: Receiver<Response>) -> Self {
        Self { send, recv }
    }
    /// Sends an instruction to pop-tiler, then waits for the response.
    pub async fn handle(&self, input: Request) -> Result<Response, Error> {
        self.send.send(input).await.map_err(Error::ClientRequest)?;

        self.recv.recv().await.map_err(Error::ClientResponse)
    }
}

/// The pop-tiling service, which you can spawn in a separate thread / local async task
struct ServiceThread<'g> {
    recv: Receiver<Request>,
    send: Sender<Response>,
    service: Service<'g>,
    t: GhostToken<'g>,
}

impl<'g> ServiceThread<'g> {
    pub fn new(recv: Receiver<Request>, send: Sender<Response>, t: GhostToken<'g>) -> Self {
        Self {
            recv,
            send,
            t,
            service: Service::default(),
        }
    }

    /// Starts an async event loop which will begin listening for instructions.
    pub async fn run(&mut self) -> Result<(), Error> {
        loop {
            let input = self.recv.recv().await.map_err(Error::ServerRequest)?;

            let output = self.service.handle(input, &mut self.t);

            self.send
                .send(output.collect())
                .await
                .map_err(Error::ServerResponse)?;
        }
    }
}

/// Manages a thread running the pop-tiler service on it, and all communication to it.
///
/// On drop of a value of this type, the background thread will be stopped.
pub struct TilerThread {
    client: ClientThread,

    // On drop, a signal will be sent here to stop the background thread.
    drop_tx: async_oneshot::Sender<()>,
}

impl Default for TilerThread {
    fn default() -> Self {
        let (client_send, server_recv) = async_channel::unbounded();
        let (server_send, client_recv) = async_channel::unbounded();
        let (drop_tx, drop_rx) = async_oneshot::oneshot();

        let client = ClientThread::new(client_send, client_recv);

        thread::spawn(move || {
            ghost_cell::GhostToken::new(|t| {
                // Tiling service as a future.
                let service = async move {
                    if let Err(why) = ServiceThread::new(server_recv, server_send, t).run().await {
                        eprintln!("pop-tiler service exited with error: {}", why);
                    }
                };

                // If the type is dropped, a message will be received that stops the service.
                let drop = async move {
                    let _ = drop_rx.await;
                };

                async_io::block_on(futures_lite::future::or(drop, service));
            })
        });

        Self { client, drop_tx }
    }
}

impl TilerThread {
    /// Submits a request to the pop-tiling service managed by this type.
    pub async fn handle(&self, request: Request) -> Result<Response, Error> {
        self.client.handle(request).await
    }
}

impl Drop for TilerThread {
    fn drop(&mut self) {
        let _ = self.drop_tx.send(());
    }
}
