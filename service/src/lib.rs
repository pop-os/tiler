// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

use async_channel::{Receiver, RecvError, SendError, Sender};
use ghost_cell::GhostToken;
use pop_tiler::*;
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
    client_send: Sender<Request>,
    client_recv: Receiver<Response>,
}

impl Client {
    /// Sends an instruction to pop-tiler, then waits for the response.
    pub async fn handle(&mut self, input: Request) -> Result<Response, Error> {
        self.client_send
            .send(input)
            .await
            .map_err(Error::ClientRequest)?;

        self.client_recv.recv().await.map_err(Error::ClientResponse)
    }
}

/// The pop-tiling service, which you can spawn in a separate thread / local async task
pub struct Server<'g> {
    server_recv: Receiver<Request>,
    server_send: Sender<Response>,
    tiler: Tiler<'g>,
    t: GhostToken<'g>,
}

impl<'g> Server<'g> {
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
            let input = self
                .server_recv
                .recv()
                .await
                .map_err(Error::ServerRequest)?;

            let output = self.handle(input);

            self.server_send
                .send(output)
                .await
                .map_err(Error::ServerResponse)?;
        }
    }
}

pub async fn create_client_and_server(t: GhostToken<'_>) -> (Client, Server<'_>) {
    let (client_send, server_recv) = async_channel::unbounded();
    let (server_send, client_recv) = async_channel::unbounded();
    (
        Client {
            client_send,
            client_recv,
        },
        Server {
            server_recv,
            server_send,
            tiler: Tiler::default(),
            t,
        },
    )
}
