use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};

fn main() {
    ghost_cell::GhostToken::new(|t| {
        let app = Application::builder()
            .application_id("com.system76.TilerServiceExample")
            .build();

        let context = glib::MainContext::new();
        let (mut tile_client, tile_server) = context.block_on(async {
            pop_tiler_service::create_client_and_server(t).await
        });

        //TODO: Can't do this due to lifetimes
        // context.spawn(async move {
        //     tile_server.run();
        // });
        //

        context.spawn(async move {
            let request = pop_tiler_service::Request::FocusLeft;
            eprintln!("Request: {:?}", request);
            let result = tile_client.handle(request).await;
            eprintln!("Result: {:?}", result);
        });

        app.connect_activate(move |app| {
            let win = ApplicationWindow::builder()
                .application(app)
                .title("TilerServiceExample")
                .build();
            win.show_all();
        });

        app.run();
    });
}
