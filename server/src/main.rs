use crate::server::Server;

mod code;
mod error;
mod peer;
mod room;
mod server;
mod session;
mod signal;
mod state;
mod track;

#[tokio::main]
async fn main() {
    let server = Server::new("127.0.0.1:3000");
    server.run().await;
}
