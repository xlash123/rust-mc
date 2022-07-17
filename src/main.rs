use rust_mc::minecraft::{proxy::MinecraftProxy, server::MinecraftServer};
use tokio;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    env_logger::init();

    debug!("Starting proxy server example");
    // Create underlying Minecraft server
    let (server, _tx) = MinecraftServer::new(
        "127.0.0.1:25565",
        "Rust test MC server",
        5,
        true,
    );
    // Give the server to the proxy
    let proxy = MinecraftProxy::new(server);
    // Start the proxy
    let handle = proxy.start().await;
    // Yield
    handle.await.unwrap();
}
