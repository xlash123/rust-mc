use rust_mc::minecraft::{proxy::MinecraftProxy, server::MinecraftServer};
use tokio;
use std::env;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    let endpoint = &args[1];

    debug!("Starting proxy server example");
    // Create underlying Minecraft server
    let (server, _tx) = MinecraftServer::new(
        "0.0.0.0:25565",
        "Rust test MC server",
        5,
        true,
    );
    // Give the server to the proxy
    let proxy = MinecraftProxy::new(server, endpoint.parse().unwrap());
    // Start the proxy
    let handle = proxy.start().await;

    // Yield
    handle.await.unwrap();
}
