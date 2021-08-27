// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

use pop_tiler::*;

fn main() {
    ghost_cell::GhostToken::new(|mut t| {
        let t = &mut t;

        let mut tiler = Tiler::default();

        // // Instruct about available displays.
        // tiler.update_display(0, Rect::new(1, 1, 2560, 1440), t);
        // tiler.update_display(1, Rect::new(1, 1, 1920, 1080), t);
        // tiler.update_display(2, Rect::new(1, 1, 1920, 1080), t);

        // // Assign workspaces to displays.
        // tiler.assign_workspace(0, 0, t);
        // tiler.assign_workspace(1, 1, t);
        // tiler.assign_workspace(2, 2, t);

        // Create some windows to assign.
        let win_a = &tiler.window((0, 0));
        let win_b = &tiler.window((0, 1));
        let win_c = &tiler.window((0, 2));
        let win_d = &tiler.window((0, 3));

        // Attach windows to workspaces.
        tiler.attach(win_a, 0, t);
        tiler.attach(win_b, 0, t);
        tiler.attach(win_c, 0, t);

        // Detach one.
        tiler.detach(win_b, t);

        // Focus and attach another.
        tiler.focus(win_c, t);
        tiler.attach(win_d, 0, t);

        // Make one stacked.
        tiler.stack_toggle(win_c, t);

        println!("Tree: {:#?}", tiler.debug(t));

        for window in tiler.windows.values() {
            println!("Window: {:?}", window.borrow(t).debug(t))
        }

        for event in tiler.events(t) {
            println!("Event: {:?}", event);
        }
    });
}
