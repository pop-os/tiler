// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use pop_tiler::*;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut owner = TCellOwner::<()>::new();
    let t = &mut owner;

    let mut tiler = Tiler::default();

    // Instruct about available displays.
    tiler.display_update(0, Rect::new(1, 1, 2560, 1440), t); // 2560x1440 display with ID 0

    // Assign workspaces to displays.
    tiler.workspace_update(0, 0, t); // Assign workspace 0 to display 0

    // Create some windows to assign.
    let win_a = tiler.window((0, 0));
    let win_b = tiler.window((0, 1));
    let win_c = tiler.window((0, 2));
    let win_d = tiler.window((0, 3));
    let win_e = tiler.window((0, 4));
    let win_f = tiler.window((0, 5));

    // Focus first workspace.
    tiler.workspace_switch(0, t);

    // Attach windows to active workspace.
    tiler.attach(&win_a, t);
    tiler.focus(&win_a, t);
    tiler.attach(&win_b, t);
    tiler.focus(&win_b, t);
    tiler.attach(&win_c, t);
    tiler.focus(&win_c, t);
    tiler.attach(&win_d, t);
    tiler.focus(&win_d, t);
    tiler.attach(&win_e, t);
    tiler.focus(&win_e, t);
    tiler.attach(&win_f, t);

    let mut first_fork = None;

    for event in tiler.events(t) {
        if first_fork.is_none() {
            if let Event::Fork(ref id, ForkUpdate { ref handle, .. }) = event {
                first_fork = Some((*id, *handle));
            }
        }
        println!("Event: {:?}", event);
    }

    eprintln!("perform resize");

    if let Some((fork, handle)) = first_fork {
        tiler.fork_resize(fork, handle / 2, t);
    }

    for event in tiler.events(t) {
        println!("Event: {:?}", event);
    }

    tiler.detach(&win_a, t);

    for event in tiler.events(t) {
        println!("Event: {:?}", event);
    }
}
