// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

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
#[derive(Debug)]
pub enum Request {
    Attach { window: WindowID, display: u32 },
    Detach(WindowID),
    Focus(WindowID),
    FocusAbove,
    FocusBelow,
    FocusLeft,
    FocusRight,
    FocusDisplayAbove,
    FocusDisplayBelow,
    FocusDisplayLeft,
    FocusDisplayRight,
    ToggleOrientation(WindowID),
    ToggleStack(WindowID),
    Swap(WindowID, WindowID),
    WorkspaceSwitch(u32),
}

/// Handle for sending and receiving instructions to and from the pop-tiler.
pub struct Client {
    send: Sender<Request>,
    recv: Receiver<Response>,
}

impl Client {
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
pub struct Server<'g> {
    recv: Receiver<Request>,
    send: Sender<Response>,
    tiler: Tiler<'g>,
    t: GhostToken<'g>,
}

impl<'g> Server<'g> {
    pub fn new(recv: Receiver<Request>, send: Sender<Response>, t: GhostToken<'g>) -> Self {
        Self {
            recv,
            send,
            t,
            tiler: Tiler::default(),
        }
    }

    fn handle(&mut self, input: Request) -> Response {
        let &mut Self {
            ref mut tiler,
            ref mut t,
            ..
        } = self;

        let window_from_id = |window: WindowID| tiler.windows.get(&window).cloned();

        match input {
            Request::Attach { window, display } => {
                if let Some(window) = window_from_id(window) {
                    tiler.attach(&window, display, t)
                }
            }

            Request::Detach(window) => {
                if let Some(window) = window_from_id(window) {
                    tiler.detach(&window, t);
                }
            }

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

            Request::Swap(a, b) => {
                if let Some((a, b)) = window_from_id(a).zip(window_from_id(b)) {
                    tiler.swap(&a, &b, t);
                }
            }

            Request::ToggleOrientation(window) => {
                if let Some(window) = window_from_id(window) {
                    tiler.toggle_orientation(&window, t)
                }
            }

            Request::ToggleStack(window) => {
                if let Some(window) = window_from_id(window) {
                    tiler.toggle_orientation(&window, t)
                }
            }

            Request::WorkspaceSwitch(workspace) => {
                tiler.workspace_switch(workspace, t);
            }
        }

        self.tiler.events(&mut self.t).collect()
    }

    /// Starts an async event loop which will begin listening for instructions.
    pub async fn run(&mut self) -> Result<(), Error> {
        loop {
            let input = self.recv.recv().await.map_err(Error::ServerRequest)?;

            let output = self.handle(input);

            self.send
                .send(output)
                .await
                .map_err(Error::ServerResponse)?;
        }
    }
}

/// Manages a thread running the pop-tiler service on it, and all communication to it.
///
/// On drop of a value of this type, the background thread will be stopped.
pub struct TilerThread {
    client: Client,

    // On drop, a signal will be sent here to stop the background thread.
    drop_tx: async_oneshot::Sender<()>,
}

impl Default for TilerThread {
    fn default() -> Self {
        let (client_send, server_recv) = async_channel::unbounded();
        let (server_send, client_recv) = async_channel::unbounded();
        let (drop_tx, drop_rx) = async_oneshot::oneshot();

        let client = Client::new(client_send, client_recv);

        thread::spawn(move || {
            ghost_cell::GhostToken::new(|t| {
                // Tiling service as a future.
                let service = async move {
                    if let Err(why) = Server::new(server_recv, server_send, t).run().await {
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
