use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};
use pop_tiler_service::{Client, Server};

fn main() {
    let app = Application::builder()
        .application_id("com.system76.TilerServiceExample")
        .build();

    let (client_send, server_recv) = async_channel::unbounded();
    let (server_send, client_recv) = async_channel::unbounded();

    let mut client = Client::new(client_send, client_recv);

    std::thread::spawn(move || {
        ghost_cell::GhostToken::new(|t| {
            if let Err(why) = async_io::block_on(Server::new(server_recv, server_send, t).run()) {
                eprintln!("pop-tiler service exited with error: {}", why);
            }
        })
    });

    let context = glib::MainContext::new();

    context.spawn(async move {
        let request = pop_tiler_service::Request::FocusLeft;
        eprintln!("Request: {:?}", request);
        let result = client.handle(request).await;
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
}
