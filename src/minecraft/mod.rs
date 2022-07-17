/// Client for Minecraft servers.
pub mod client;
/// mctokio Minecraft networking wrapper.
pub mod connection;
/// Minecraft server implementation
pub mod server;
/// Minecraft server status checking.
pub mod status;
/// Data types.
pub mod types;
/// MITM proxy for forwarding clients packets to another server. Endpoint server must be offline
pub mod proxy;

use mcproto_rs::v1_16_3 as proto;
use proto::{Packet753 as Packet, RawPacket753 as RawPacket};
