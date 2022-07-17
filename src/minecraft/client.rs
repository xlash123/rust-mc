use super::{connection::MinecraftConnection, proto, Packet};
use anyhow::Result;
use mcproto_rs::{protocol::State, types::CountedArray};
use openssl::rsa::{Rsa, Padding};
use std::{net::SocketAddr};

use std::sync::Arc;
use tokio::{sync::{
    mpsc::{self, error::TryRecvError, Receiver, Sender},
    broadcast, RwLock,
}, task::JoinHandle};

use super::server::NameUUID;

/// Error text when the client is not connected to a server.
const SERVER_NONE_ERROR: &str = "Not connected to server.";
/// Error when the client recieves a Packet it was not expecting.
const WRONG_PACKET_ERROR: &str = "Recieved an unexpected packet.";

/// A Minecraft client, a wrapper for the utilities required to connect to a Minecraft server and perform various activities in said server.
pub struct MinecraftClient {
    /// The socket address of the server the client will connect to.
    address: SocketAddr,
    /// Running instance of the client
    runner: Arc<RwLock<ClientRunner>>,
}

struct ClientRunner {
    /// The game profile that will be used to authenticate with the server.
    profile: crate::auth::Profile,
    /// Whether or not the client is currently connected to the server.
    connected: bool,
    /// When the client is connected, this holds the actual connection to the server.
    server: Option<MinecraftConnection>,
}

impl MinecraftClient {
    /// Returns a Minecraft client that will connect to the given server with the given profile.
    ///
    /// # Arguments
    ///
    /// * `address` Socket address of the server the client should connect to.
    /// * `profile` Minecraft account/profile that the client should use.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rust_mc::minecraft::client::MinecraftClient;
    /// use rust_mc::mojang::auth::Profile;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// let mut client = MinecraftClient::new(
    ///     "127.0.0.1:25565".parse().unwrap(),
    ///     Profile::new("rust_mc", "", true),
    /// );
    /// ```
    pub fn new(address: SocketAddr, profile: crate::auth::Profile) -> MinecraftClient {
        let runner = Arc::new(RwLock::new(
            ClientRunner {
                profile,
                server: None,
                connected: false,
            }
        ));
        Self {
            address,
            runner,
        }
    }

    /// Tells the client to connect to the Minecraft server at the socket address in it's `address` field.
    ///
    /// # Examples
    ///
    /// This example requires a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::client::MinecraftClient;
    /// use rust_mc::mojang::auth::Profile;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use std::sync::Arc;
    /// use tokio::sync::Mutex;
    /// use futures::executor::block_on;
    ///
    /// let mut client = MinecraftClient::new(
    ///     "127.0.0.1:25565".parse().unwrap(),
    ///     Profile::new("rust_mc", "", true),
    /// );
    ///
    /// block_on(client.connect()); // Connect the client.
    /// ```
    pub async fn connect(&mut self) -> Result<(JoinHandle<()>, Sender<()>)> {
        let runner_mut = self.runner.clone();

        // Start the TCP connection and wait for completion
        {
            let mut runner = runner_mut.write().await;
            // Authenticate profile
            let auth = runner.profile.authenticate().await;
    
            // Check if the auth succeeded
            if let Err(err) = auth {
                return Err(err);
            }    
            // Create a connection that can be used to connect to the server
            let connection = MinecraftConnection::connect_async(self.address).await?;
    
            // Save our connection
            runner.server = Some(connection);
            
            let profile = runner.profile.game_profile.name.clone();

            // Perform a full handshake with the server
            let server = runner.server.as_mut().unwrap();
            server.handshake(
                Some(proto::HandshakeNextState::Login),
                Some(profile),
            ).await?;
            // Do the login procedure
            runner.login().await?;
        }

        // Create channels for communicating to the client
        let (tx, rx) = mpsc::channel(20);

        let loop_runner_mut = self.runner.clone();

        // Start the client loop in a new thread
        let handle = tokio::spawn(async move {
            ClientRunner::start_loop(loop_runner_mut, rx).await;
        });
        
        Ok((handle, tx))
    }

    /// Tells the client to send a chat message
    ///
    /// # Arguments
    ///
    /// * `message` Message the client should send to the server.
    ///
    /// # Examples
    ///
    /// This example requires a Minecraft server to be running on localhost:25565.
    ///
    /// ```rust
    /// use rust_mc::minecraft::client::MinecraftClient;
    /// use rust_mc::mojang::auth::Profile;
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// use futures::executor::block_on;
    ///
    /// let mut client = MinecraftClient::new(
    ///     "127.0.0.1:25565".parse().unwrap(),
    ///     Profile::new("rust_mc", "", true),
    /// );
    ///
    /// block_on(client.connect()); // Connect the client before sending the message.
    ///
    /// block_on(client.send_chat_message("Hello!")); // Send the chat message. Assumes success of connection
    /// ```
    pub async fn send_chat_message(&self, msg: &str) {
        let runner = self.runner.read().await;
        runner.send_chat_message(msg).await.unwrap();
    }

    pub async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.runner.read().await.send_packet(packet).await
    }

    pub async fn send_packet_arc(&self, packet: Arc<Packet>) -> Result<()> {
        self.runner.read().await.send_packet_arc(packet).await
    }

    /// Receive packets that the client will send to the server
    pub async fn subscribe_to_serverbound_packets(&self) -> broadcast::Receiver<Arc<Packet>> {
        self.runner.read().await.subscribe_to_serverbound_packets()
    }

    /// Receive packets that the server has sent to the client
    pub async fn subscribe_to_clientbound_packets(&self) -> broadcast::Receiver<Arc<Packet>> {
        self.runner.read().await.subscribe_to_clientbound_packets()
    }

    pub async fn get_name_uuid(&self) -> NameUUID {
        let profile = &self.runner.read().await.profile.game_profile;
        (
            profile.name.clone(),
            profile.id,
        )
    }
}

impl ClientRunner {
    /// Starts the client loop and listens for incoming server packets
    async fn start_loop(client: Arc<RwLock<Self>>, mut receiver: Receiver<()>) {
        let client_arc = client.clone();
        debug!("[Client] Starting client loop");
        loop {
            if let Err(err) = receiver.try_recv() {
                if err == TryRecvError::Disconnected {
                    println!("[Client] Closed receiver connection");
                    break;
                }
                let client_lock = client_arc.read().await;
                if !client_lock.connected {
                    println!("[Client] Disconnected from server");
                    break;
                }
                let packet_read = {
                    match client_lock.read_packet().await {
                        Ok(packet) => packet,
                        Err(e) => {
                            error!("[Client] {:?}", e);
                            continue;
                        },
                    }
                };
                // Process a packet if deserialized correctly
                client_lock.handle_packet(&packet_read).await
            }
        }
    }

    async fn send_chat_message(&self, message: &str) -> Result<()> {
        let spec = proto::PlayClientChatMessageSpec {
            message: message.to_string(),
        };
        self.send_packet(Packet::PlayClientChatMessage(spec)).await
    }

    /// Reads a packet from the server when connected.
    async fn read_packet(&self) -> Result<Arc<Packet>> {
        if let Some(server) = &self.server {
            let possible_packet = server.read_next_packet().await?;
            if let Some(packet) = possible_packet {
                Ok(packet)
            } else {
                Err(anyhow::anyhow!("Empty packet"))
            }
        } else {
            Err(anyhow::anyhow!(SERVER_NONE_ERROR))
        }
    }

    /// Sends a packet to the server when connected.
    async fn send_packet_arc(&self, packet: Arc<Packet>) -> Result<()> {
        if let Some(server) = &self.server {
            debug!("[Client] Writing packet: {:?}", packet);
            server.write_packet_arc(packet).await
        } else {
            Err(anyhow::anyhow!(SERVER_NONE_ERROR))
        }
    }

    async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.send_packet_arc(Arc::new(packet)).await
    }

    /// Sets the client's current state (Handshake, Login, Play, Status), should match server.
    async fn set_state(&mut self, new_state: State) -> Result<()> {
        if let Some(server) = &mut self.server {
            server.set_state(new_state).await;
            Ok(())
        } else {
            Err(anyhow::anyhow!(SERVER_NONE_ERROR))
        }
    }

    /// Sets the size threshold at which packets are compressed before being sent.
    async fn set_compression_threshold(&mut self, threshold: i32) -> Result<()> {
        if self.connected {
            Err(anyhow::anyhow!("Already connected."))
        } else {
            if let Some(server) = &self.server {
                server.set_compression_threshold(threshold).await;
                debug!("[Client] Enabled compression");
                let read = self.read_packet().await;
                if let Ok(packet) = read {
                    match &*packet {
                        Packet::LoginSuccess(spec) => self.login_success(&spec).await,
                        _ => Err(anyhow::anyhow!(WRONG_PACKET_ERROR)),
                    }
                } else {
                    Err(read.err().unwrap())
                }
            } else {
                Err(anyhow::anyhow!(SERVER_NONE_ERROR))
            }
        }
    }

    /// Enables encryption on the client, requires authentication with mojang.
    /// Still WIP, does not actually work.
    async fn enable_encryption(&mut self, spec: &proto::LoginEncryptionRequestSpec) -> Result<()> {
        if self.connected {
            return Err(anyhow::anyhow!("Already connected."));
        }
        if self.profile.offline {
            return Err(anyhow::anyhow!(
                "Cannot use encryption with offline account."
            ));
        }
        if let Some(server) = &self.server {
            debug!("[Client] Establishing an encrypted connection...");
            // Public key from server
            let public_key = &spec.public_key.as_slice();
            // Parse the public key
            let public_key_rsa = Rsa::public_key_from_der(public_key).unwrap();
            // Generate a random shared secret to share with the server. Must encrpyt with public key
            let shared_secret: &mut [u8] = &mut [0; 16];
            for mut _i in shared_secret.iter() {
                _i = &rand::random::<u8>();
            }
            // Buffer to store encrypted secret into
            let mut shared_secret_enc: Vec<u8> = vec![0; public_key_rsa.size() as usize];
            // Encrypt the secret
            public_key_rsa.public_encrypt(&shared_secret, &mut shared_secret_enc, Padding::PKCS1).unwrap();
            // Trim the padding
            let shared_secret_enc = &shared_secret_enc[0..16];

            // Encrypt verify token with the public key
            let mut verify_token_enc: Vec<u8> = vec![0; public_key_rsa.size() as usize];
            public_key_rsa.public_encrypt(&spec.verify_token, &mut verify_token_enc, Padding::PKCS1).unwrap();
            // Trim the padding
            let verify_token_enc = &verify_token_enc[0..16];

            // Create encryption response packet and send
            let response_spec = proto::LoginEncryptionResponseSpec {
                shared_secret: CountedArray::from(shared_secret_enc.to_vec()),
                verify_token: CountedArray::from(verify_token_enc.to_vec()),
            };
            let auth = self.profile.join_server(&spec.server_id, &shared_secret_enc[0..16], public_key).await;

            if let Ok(_) = auth {
                let respond = server
                    .write_packet(Packet::LoginEncryptionResponse(response_spec))
                    .await;
                if let Ok(_) = respond {
                    let enable = server.enable_encryption(&shared_secret, &shared_secret).await;
                    debug!("[Client] Enabled encryption");
                    if let Ok(_) = enable {
                        let read = self.read_packet().await;
                        if let Ok(packet) = read {
                            match &*packet {
                                Packet::LoginSetCompression(body) => {
                                    self.set_compression_threshold(body.threshold.0).await
                                }
                                Packet::LoginSuccess(spec) => self.login_success(spec).await,
                                _ => Err(anyhow::anyhow!(WRONG_PACKET_ERROR)),
                            }
                        } else {
                            Err(read.err().unwrap())
                        }
                    } else {
                        enable
                    }
                } else {
                    respond
                }
            } else {
                auth
            }
        } else {
            Err(anyhow::anyhow!(SERVER_NONE_ERROR))
        }
    }

    /// Executed when server login succeeds.
    async fn login_success(&mut self, spec: &proto::LoginSuccessSpec) -> Result<()> {
        debug!("[Client] Login success!");
        self.connected = true;
        if self.profile.offline {
            self.profile.game_profile.id = spec.uuid;
            self.profile.game_profile.name = spec.username.clone();
        }
        self.set_state(State::Play).await
    }

    /// Completes the clientside part of the Minecraft login sequence.
    async fn login(&mut self) -> Result<()> {
        if self.connected {
            return Err(anyhow::anyhow!("Already connected."));
        }
        debug!("[Client] Ready to start login procedure");
        let read = self.read_packet().await;
        if let Ok(packet) = read {
            match &*packet {
                Packet::LoginEncryptionRequest(body) => self.enable_encryption(body).await,
                Packet::LoginSetCompression(body) => {
                    self.set_compression_threshold(body.threshold.0).await
                }
                Packet::LoginSuccess(spec) => self.login_success(spec).await,
                _ => Err(anyhow::anyhow!(WRONG_PACKET_ERROR)),
            }
        } else {
            Err(read.err().unwrap())
        }
    }

    /// Handle received packets.
    async fn handle_packet(&self, packet: &Packet) {
        match &packet {
            // Packet::PlayServerChatMessage(body) => {
            //     if body.sender != self.profile.game_profile.id {
            //         if let Some(message) = body.message.to_traditional() {
            //             debug!("[Client] {}", message);
            //         } else {
            //             debug!("[Client] Raw message: {:?}", body);
            //         }
            //     }
            // },
            _ => {}
        };
    }

    fn subscribe_to_serverbound_packets(&self) -> broadcast::Receiver<Arc<Packet>> {
        self.server.as_ref().unwrap().subscribe_write_packets()
    }

    fn subscribe_to_clientbound_packets(&self) -> broadcast::Receiver<Arc<Packet>> {
        self.server.as_ref().unwrap().subscribe_read_packets()
    }
}
