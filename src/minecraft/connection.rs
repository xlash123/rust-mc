use super::{
    proto::{self, HandshakeNextState},
    Packet, RawPacket,
};
use anyhow::Result;
use mcproto_rs::protocol::{PacketDirection, State};
use mctokio::{Bridge, TcpConnection, TcpReadBridge, TcpWriteBridge};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex};
use std::sync::Arc;

/// Represents a connection to a Mineceraft server.
pub struct MinecraftConnection {
    /// Read channel of the Server socket.
    reader: Arc<Mutex<TcpReadBridge>>,
    /// Write channel of the Server socket.
    writer: Arc<Mutex<TcpWriteBridge>>,
    /// Where the packets are sent.
    packet_direction: PacketDirection,
    /// Alerts other thread whenever packets are received. Use Arc<Packet> to avoid cloning entire packets
    packet_read_sender: broadcast::Sender<Arc<Packet>>,
    /// Alerts other thread whenever packets are sent. Use Arc<Packet> to avoid cloning entire packets
    packet_write_sender: broadcast::Sender<Arc<Packet>>,
    /// IP address and port of connection
    address: SocketAddr,
}

impl MinecraftConnection {
    /// Returns a MinecraftConnection based on the given TcpConnection and packet direction..
    /// It is highly recommended to use `connect` or `connect_async` instead if you are trying to connect to a server as a client and `from_tcp_stream` if you are accepting client connections as a server.
    ///
    /// # Arguments
    ///
    /// * `connection` TcpConnection to the target (client or server).
    /// * `packet_direction` Where packets are being sent (ServerBound if target is a server, or ClientBount if target is a client).
    ///
    /// # Examples
    ///
    /// This example creates a new MinecraftConnection to a server based on an existing TcpConnection, it requires a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::MinecraftConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use mcproto_rs::protocol::PacketDirection;
    /// use futures::executor::block_on;
    /// use mctokio::TcpConnection;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let connect = async {
    ///     let connection = TcpConnection::connect_to_server(address).await;
    ///
    ///     let mut server_connection = ServerConnection::new(connection, PacketDirection::ServerBound);
    /// };
    ///
    /// block_on(connect);
    /// ```
    ///
    /// This example creates a new MinecraftConnection to a client based on an existing TcpConnection.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::MinecraftConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use mcproto_rs::protocol::PacketDirection;
    /// use futures::executor::block_on;
    /// use mctokio::TcpConnection;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let connect = async {
    ///     let mut listener = TcpListener::bind(address).await;
    ///     if let Ok(listener) = &mut listener {
    ///         loop {
    ///             if let Ok((socket, address)) = listener.accept().await {
    ///                let mut client_connection = MinecraftConnection::new(TcpConnection::from_client_connection(socket), PacketDirection::ClientBound);
    ///            }
    ///         }
    ///     }
    /// };
    ///
    /// block_on(connect);
    /// ```
    pub fn new(connection: TcpConnection, packet_direction: PacketDirection, address: SocketAddr) -> Self {
        let (packet_read_sender, _) = broadcast::channel(1 << 21);
        let (packet_write_sender, _) = broadcast::channel(1 << 21);
        return Self {
            reader: Arc::new(Mutex::new(connection.reader)),
            writer: Arc::new(Mutex::new(connection.writer)),
            packet_direction,
            packet_read_sender,
            packet_write_sender,
            address,
        };
    }

    /// Returns a MinecraftConnection to a client based on the given TcpStream.
    /// This method is only for servers to use when a client connects.
    ///
    /// # Arguments
    ///
    /// * `connection` TcpStream from the client's connection.
    ///
    /// # Examples
    ///
    /// This example creates a new MinecraftConnection to a client based on an existing TcpStream.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::MinecraftConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    /// use mctokio::TcpConnection;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let connect = async {
    ///     let mut listener = TcpListener::bind(address).await;
    ///     if let Ok(listener) = &mut listener {
    ///         loop {
    ///             if let Ok((socket, address)) = listener.accept().await {
    ///                let mut client_connection = MinecraftConnection::from_tcp_stream(socket);
    ///            }
    ///         }
    ///     }
    /// };
    ///
    /// block_on(connect);
    /// ```
    pub fn from_tcp_stream(connection: TcpStream) -> Self {
        let address = connection.local_addr().unwrap();
        Self::new(
            TcpConnection::from_client_connection(connection),
            PacketDirection::ClientBound,
            address,
        )
    }

    /// Returns a MinecraftConnection to a server based on the given read/write channels.
    ///
    /// # Arguments
    ///
    /// * `reader` Read channel of a connected Socket.
    /// * `writer` Write channel of a connected Socket.
    ///
    /// # Examples
    ///
    /// This example requires a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let connect_async = async {
    ///     if let Ok(connection) = ServerConnection::connect_async(address).await {
    ///         // Do stuff with connection here.
    ///     }
    /// };
    ///
    /// block_on(connect_async);
    /// ```
    pub async fn connect_async(address: SocketAddr) -> Result<Self, std::io::Error> {
        let connection = TcpConnection::connect_to_server(address).await;
        if let Ok(connected) = connection {
            Ok(Self::new(connected, PacketDirection::ServerBound, address))
        } else {
            Err(connection.err().unwrap())
        }
    }

    /// Connects to a server socket and returns a MinecraftConnection based on that connection.
    ///
    /// # Arguments
    ///
    /// * `address` Address of the server to connect to.
    ///
    /// # Examples
    ///
    /// This example requires a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// if let Ok(connection) = ServerConnection::connect(address) {
    ///     // Do stuff with connection here.
    /// };
    /// ```
    pub fn connect(address: SocketAddr) -> Result<Self, std::io::Error> {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(Self::connect_async(address))
    }

    /// Completes the Handshaking sequence with the Minecraft server.
    /// Returns the state that was set or receieved by the client.
    ///
    /// # Arguments
    ///
    /// * `next_state` The state to enter after Handshake, should be None when called by a server.
    /// * `name` The name of the player connecting, can be `None` if `next_state` is Status or if this is called by a server.
    ///
    /// # Examples
    ///
    /// This example is for handling incoming connections to a server.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::MinecraftConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    /// use mctokio::TcpConnection;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let connect = async {
    ///     let mut listener = TcpListener::bind(address).await;
    ///     if let Ok(listener) = &mut listener {
    ///         loop {
    ///             if let Ok((socket, address)) = listener.accept().await {
    ///                 let mut client_connection = MinecraftConnection::from_tcp_stream(socket);
    ///                 client_connection.handshake(None, Some(256)).await;
    ///            }
    ///         }
    ///     }
    /// };
    ///
    /// block_on(connect);
    /// ```
    ///
    /// These examples are for clients and require a Minecraft server to be running on localhost:25565.
    ///
    /// When you are trying to get the status of a server:
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use rust_mc::minecraft::proto::HandshakeNextState::Status;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let status_handshake = async {
    ///     let mut connection = ServerConnection::connect_async(address).await;
    ///
    ///     if let Ok(server) = &mut connection {
    ///         let handshake = server.handshake(Status, None).await; // Note the usage of `None` here as this is a "status" handshake.
    ///         if let Ok(_) = &handshake {
    ///             // Do stuff on Handshake success here
    ///         };
    ///     };
    /// };
    ///s
    /// block_on(status_handshake);
    /// ```
    ///
    /// When you are trying to login to a server:
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use rust_mc::minecraft::proto::HandshakeNextState::Login;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let login_handshake = async {
    ///     let mut connection = ServerConnection::connect_async(address).await;
    ///
    ///     if let Ok(server) = &mut connection {
    ///         let handshake = server.handshake(Login, Some("test_player".to_string())).await; // Note the string with a username here as this is a "login" handshake.
    ///         if let Ok(_) = &handshake {
    ///             // Do stuff on Handshake success here
    ///         };
    ///     };
    /// };
    ///
    /// block_on(login_handshake);
    /// ```
    pub async fn handshake(
        &mut self,
        next_state: Option<proto::HandshakeNextState>,
        name: Option<String>,
    ) -> anyhow::Result<State> {
        if self.packet_direction == PacketDirection::ClientBound {
            let first = self.read_next_packet().await;
            if let Ok(first) = first {
                if let Some(p) = first {
                    if let Packet::Handshake(body) = &*p {
                        match body.next_state {
                            HandshakeNextState::Status => {
                                debug!("[Server] Received request for status");
                                self.set_state(State::Status).await;
                                return Ok(State::Status);
                            }
                            HandshakeNextState::Login => {
                                self.set_state(State::Login).await;
                                return Ok(State::Login);
                            }
                        }
                    } else {
                        return Err(anyhow::anyhow!("Did not receive handshake packet."));
                    }
                } else {
                    return Err(anyhow::anyhow!("Did not receive handshake packet."));
                }
            } else {
                return Err(first.err().unwrap());
            }
        } else {
            if let Some(next_state) = next_state {
                let handshake = proto::HandshakeSpec {
                    version: mcproto_rs::types::VarInt::from(753),
                    server_address: self.address.ip().to_string(),
                    server_port: self.address.port(),
                    next_state: next_state.clone(),
                };
                debug!("[Client] Starting handshake");
                if let Err(error) = self.write_packet(Packet::Handshake(handshake)).await {
                    return Err(error);
                } else {
                    if next_state == proto::HandshakeNextState::Status {
                        self.set_state(State::Status).await;
                        if let Err(error) = self
                            .write_packet(Packet::StatusRequest(proto::StatusRequestSpec {}))
                            .await
                        {
                            return Err(error);
                        }
                        return Ok(State::Status);
                    } else {
                        self.set_state(State::Login).await;
                        if let Some(name) = name {
                            if let Err(error) = self
                                .write_packet(Packet::LoginStart(proto::LoginStartSpec {
                                    name: name.clone(),
                                }))
                                .await
                            {
                                return Err(error);
                            }
                            debug!("[Client] Entered Login state");
                            return Ok(State::Login);
                        } else {
                            return Err(anyhow::anyhow!(
                                "Username cannot be empty when next_state is Login."
                            ));
                        }
                    }
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Cannot handshake as a client without specifying what the next state is."
                ));
            }
        }
    }

    /// Sends a packet to the target.
    ///
    /// # Arguments
    ///
    /// * `packet` Packet to send to the server.
    ///
    /// # Examples
    ///
    /// This example require a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::{Packet::PlayClientChatMessage, proto::PlayClientChatMessageSpec};
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let send_packet = async {
    ///     let mut connection = ServerConnection::connect_async(address).await;
    ///
    ///     if let Ok(server) = &mut connection {
    ///         server.write_packet().await;
    ///         let spec = PlayClientChatMessageSpec {
    ///             message: "Hello from rust-mc!",
    ///         };
    ///         self.send_packet(PlayClientChatMessage(spec)).await;
    ///     };
    /// };
    ///
    /// block_on(send_packet);
    /// ```
    pub async fn write_packet_arc(&self, packet: Arc<Packet>) -> Result<()> {
        {
            self.writer.lock().await.write_packet(packet.as_ref()).await?;
        }
        if self.packet_write_sender.receiver_count() > 0 {
            self.packet_write_sender.send(packet).unwrap_or_else(|e| {
                debug!("Failed to send written packet: {:?}", e.0);
                debug!("# of Writes: {}", self.packet_write_sender.receiver_count());
                0
            });
        }
        Ok(())
    }

    pub async fn write_packet(&self, packet: Packet) -> Result<()> {
        self.write_packet_arc(Arc::new(packet)).await
    }

    /// Reads the next packet from the buffer of packets received from the target.
    ///
    /// # Examples
    ///
    /// This example require a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let set_compression_threshold = async {
    ///     let mut connection = ServerConnection::connect_async(address).await;
    ///
    ///     if let Ok(server) = &mut connection {
    ///         let packet = server.read_next_packet().await;
    ///         if let Ok(packet) = packet {
    ///             // Do stuff with packet here.
    ///         }
    ///     };
    /// };
    ///
    /// block_on(set_compression_threshold);
    /// ```
    pub async fn read_next_packet(&self) -> Result<Option<Arc<Packet>>> {
        if let Some(raw) = {
            self.reader.lock().await.read_packet::<RawPacket>().await?
        } {
            let packet = mcproto_rs::protocol::RawPacket::deserialize(&raw);
            match packet {
                Ok(packet) => {
                    let packet = Arc::new(packet);
                    if self.packet_read_sender.receiver_count() > 0 {
                        self.packet_read_sender.send(packet.clone()).unwrap_or_else(|e| {
                            debug!("Failed to send read packet: {:?}", e.0);
                            debug!("# of Reads: {}", self.packet_read_sender.receiver_count());
                            0
                        });
                    }
                    Ok(Some(packet))
                },
                Err(e) => {
                    error!("Bad packet: {:?}", raw);
                    return Err(anyhow::anyhow!("{}", e));
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Sets the state of the connection.
    ///
    /// # Arguments
    ///
    /// * `state` Desired state.
    ///
    /// # Examples
    ///
    /// This example require a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use mcproto_rs::protocol::State::Handshaking;
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let set_state = async {
    ///     let mut connection = ServerConnection::connect_async(address).await;
    ///
    ///     if let Ok(server) = &mut connection {
    ///         server.set_state(Handshaking);
    ///     };
    /// };
    ///
    /// block_on(set_state);
    /// ```
    pub async fn set_state(&self, state: State) {
        self.reader.lock().await.set_state(state.clone());
        self.writer.lock().await.set_state(state);
    }

    /// Enables encryption for this connection
    /// Not actually sure on how to use this properly. Examples still WIP.
    ///
    /// # Arguments
    ///
    /// * `key` the shared secret key created by the connected client.
    /// * `iv` initialization vector, usually just the shared key.
    ///
    /// # Examples
    ///
    /// This example require a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let set_state = async {
    ///     let mut connection = ServerConnection::connect_async(address).await;
    ///
    ///     if let Ok(server) = &mut connection {
    ///         server.enable_encryption(key, iv); // todo!("Finish example.")
    ///     };
    /// };
    ///
    /// block_on(set_state);
    /// ```
    pub async fn enable_encryption(&self, key: &[u8], iv: &[u8]) -> Result<()> {
        self.reader.lock().await.enable_encryption(key, iv)?;
        self.writer.lock().await.enable_encryption(key, iv)?;
        Ok(())
    }

    /// Sets the size packets can reach before being compressed
    ///
    /// # Arguments
    ///
    /// * `threshold` Maximum size in bytes before compressions is enforced.
    ///
    /// # Examples
    ///
    /// This example require a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::net::connection::ServerConnection;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
    ///
    /// let set_compression_threshold = async {
    ///     let mut connection = ServerConnection::connect_async(address).await;
    ///
    ///     if let Ok(server) = &mut connection {
    ///         server.set_compression_threshold(512);
    ///     };
    /// };
    ///
    /// block_on(set_compression_threshold);
    /// ```
    pub async fn set_compression_threshold(&self, threshold: i32) {
        self.reader.lock().await.set_compression_threshold(Some(threshold));
        self.writer.lock().await.set_compression_threshold(Some(threshold));
    }

    pub fn subscribe_read_packets(&self) -> broadcast::Receiver<Arc<Packet>> {
        self.packet_read_sender.subscribe()
    }

    pub fn subscribe_write_packets(&self) -> broadcast::Receiver<Arc<Packet>> {
        self.packet_write_sender.subscribe()
    }
}
