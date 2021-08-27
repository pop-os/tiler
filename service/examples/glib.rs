// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: LGPL-3.0-only

use pop_tiler_service::TilerThread;

fn main() {
    glib::MainContext::default().spawn(async move {
        let tiler = TilerThread::default();
        let request = pop_tiler_service::Request::FocusLeft;
        eprintln!("Request: {:?}", request);
        let result = tiler.handle(request).await;
        eprintln!("Result: {:?}", result);
    });
}
