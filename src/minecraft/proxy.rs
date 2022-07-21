use super::server::{
    MinecraftServer,
    event::Event,
};
use super::client::MinecraftClient;
use crate::mojang::auth;
use mcproto_rs::v1_16_3::{Packet753 as Packet, PlayerInfoActionList,EntityMetadataFieldData};
use mcproto_rs::uuid::UUID4;
use std::net::SocketAddr;
use tokio::task::JoinHandle;
use std::sync::Arc;

pub struct MinecraftProxy {
    server: Arc<MinecraftServer>,
    endpoint: SocketAddr,
}

impl MinecraftProxy {
    pub fn new(server: MinecraftServer, endpoint: SocketAddr) -> Self {
        Self {
            server: Arc::new(server),
            endpoint,
        }
    }

    pub async fn start(&self) -> JoinHandle<()> {
        let server = self.server.clone();
        server.start().await.unwrap();

        let mut server_event_listener = server.subscribe_to_events().await;

        let endpoint = self.endpoint.clone();

        // Listen to packets from the server
        let on_server_packet_loop = async move {
            loop {
                if let Ok(e) = server_event_listener.recv().await {
                    match e {
                        Event::PlayerJoin(id) => {
                            let username = id.0.clone();
                            info!("{} joined! Faking connection", username);
                            // The client as connected to this server
                            let server_client = server.get_server_client(&id).await.unwrap();
                            let mut sc_packet_receive = {
                                server_client.read().await.subscribe_to_serverbound_packets()
                            };
                            debug!("Got server_client packet listener");
                            // Packets sent by the client to the server for this player
                            // Create a proxy client to connect to a real server
                            let mut client = MinecraftClient::new(
                                endpoint,
                                auth::Profile::new(&username, "", true),
                            );
                            debug!("Attempting connection for {}...", username);
                            let (_handle, tx_fake_client) = client.connect().await.unwrap();
                            info!("Fake {} joined!", username);
                            let mut client_packet_listener = client.subscribe_to_clientbound_packets().await;
                            debug!("Got client packet listener");

                            let real_uuid = id.1;
                            let fake_uuid = client.get_name_uuid().await.1;

                            debug!("Read UUID: {}, Fake UUID: {}", real_uuid, fake_uuid);

                            tokio::spawn(async move {
                                let client = client;
                                debug!("[CTS] Starting loop");
                                let tx_fake_client = tx_fake_client;
                                // Forward all packets received by this server to the real server
                                let cts = async move {
                                    loop {
                                        match sc_packet_receive.recv().await {
                                            Ok(p) => {
                                                let p = MinecraftProxy::convert_packet_player_uuid(p, &real_uuid, &fake_uuid);
                                                debug!("[Client] Writing packet: {:?}", p);
                                                if let Err(e) = client.send_packet_arc(p).await {
                                                    error!("[Proxy] {e}");
                                                    break;
                                                }
                                            },
                                            Err(e) => {
                                                debug!("{e}");
                                                break;
                                            },
                                        }
                                    }
                                    debug!("[CTS] Finished loop");
                                };
                                // Forward all packets received by the real server to the real client
                                let stc = async move {
                                    debug!("[STC] Starting loop");
                                    loop {
                                        match client_packet_listener.recv().await {
                                            Ok(p) => {
                                                let p = MinecraftProxy::convert_packet_player_uuid(p, &real_uuid, &fake_uuid);
                                                match &*p {
                                                    Packet::PlayDisconnect(spec) => {
                                                        debug!("[STC] Received disconnect packet");
                                                        server_client.read().await.kick(spec.reason.clone()).await.unwrap();
                                                        break;
                                                    },
                                                    _ => {},
                                                }
                                                if let Err(e) = server_client.read().await.send_packet_arc(p).await {
                                                    error!("[Proxy] {e}");
                                                    drop(tx_fake_client);
                                                    break;
                                                }
                                            },
                                            Err(e) => {
                                                debug!("{e}");
                                                break;
                                            },
                                        }
                                    }
                                    debug!("[STC] Finished loop");
                                };
                                debug!("Starting fake client loops");
                                let cts_handle = tokio::spawn(cts);
                                let stc_handle = tokio::spawn(stc);
                                let res = tokio::join!(cts_handle, stc_handle);
                                res.0.unwrap();
                                res.1.unwrap();
                                debug!("[Proxy] {} dropped", username);
                            });
                        }
                    }
                };
            }
        };
        
        tokio::spawn(on_server_packet_loop).await.unwrap()
    }

    /// Mutate a packet by converting any fake, generated UUIDs into a player's real one
    fn convert_packet_player_uuid(packet: Arc<Packet>, real_uuid: &UUID4, fake_uuid: &UUID4) -> Arc<Packet> {
        // Match only packets that contain a UUID
        match &*packet {
            // Clientbound packets - use the real UUID
            Packet::LoginSuccess(spec) => {
                if spec.uuid == *fake_uuid {
                    let mut spec = spec.clone();
                    spec.uuid = *real_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::LoginSuccess(spec))
                } else {
                    packet
                }
            },
            Packet::PlaySpawnEntity(spec) => {
                if spec.object_uuid == *fake_uuid {
                    let mut spec = spec.clone();
                    spec.object_uuid = *real_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::PlaySpawnEntity(spec))
                } else {
                    packet
                }
            },
            Packet::PlaySpawnLivingEntity(spec) => {
                if spec.entity_uuid == *fake_uuid {
                    let mut spec = spec.clone();
                    spec.entity_uuid = *real_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::PlaySpawnLivingEntity(spec))
                } else {
                    packet
                }
            },
            Packet::PlaySpawnPainting(spec) => {
                if spec.entity_uuid == *fake_uuid {
                    let mut spec = spec.clone();
                    spec.entity_uuid = *real_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::PlaySpawnPainting(spec))
                } else {
                    packet
                }
            },
            Packet::PlaySpawnPlayer(spec) => {
                if spec.uuid == *fake_uuid {
                    let mut spec = spec.clone();
                    spec.uuid = *real_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::PlaySpawnPlayer(spec))
                } else {
                    packet
                }
            },
            Packet::PlayBossBar(spec) => {
                if spec.uuid == *fake_uuid {
                    let mut spec = spec.clone();
                    spec.uuid = *real_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::PlayBossBar(spec))
                } else {
                    packet
                }
            },
            Packet::PlayServerChatMessage(spec) => {
                if spec.sender == *fake_uuid {
                    let mut spec = spec.clone();
                    spec.sender = *real_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::PlayServerChatMessage(spec))
                } else {
                    packet
                }
            },
            Packet::PlayPlayerInfo(spec) => {
                let mut spec = spec.clone();
                let actions = &mut spec.actions;
                match actions {
                    PlayerInfoActionList::Add(ca) => {
                        ca.iter_mut()
                            .for_each(|a| {
                                if a.uuid == *fake_uuid {
                                    a.uuid = *real_uuid;
                                    debug!("Replace uuid!");
                                }
                            })
                    },
                    PlayerInfoActionList::Remove(ca) => {
                        ca.iter_mut()
                            .for_each(|a| {
                                if *a == *fake_uuid {
                                    *a = *real_uuid;
                                    debug!("Replace uuid!");
                                }
                            })
                    },
                    PlayerInfoActionList::UpdateDisplayName(ca) => {
                        ca.iter_mut()
                            .for_each(|a| {
                                if a.uuid == *fake_uuid {
                                    a.uuid = *real_uuid;
                                    debug!("Replace uuid!");
                                }
                            })
                    },
                    PlayerInfoActionList::UpdateGameMode(ca) => {
                        ca.iter_mut()
                            .for_each(|a| {
                                if a.uuid == *fake_uuid {
                                    a.uuid = *real_uuid;
                                    debug!("Replace uuid!");
                                }
                            })
                    },
                    PlayerInfoActionList::UpdateLatency(ca) => {
                        ca.iter_mut()
                            .for_each(|a| {
                                if a.uuid == *fake_uuid {
                                    a.uuid = *real_uuid;
                                    debug!("Replace uuid!");
                                }
                            })
                    }
                };
                Arc::new(Packet::PlayPlayerInfo(spec))
            },
            Packet::PlayEntityProperties(spec) => {
                let mut spec = spec.clone();
                spec.properties.iter_mut().for_each(|prop| {
                        prop.modifiers.iter_mut().for_each(|m| {
                            if m.uuid == *fake_uuid {
                                m.uuid = *real_uuid;
                                debug!("Replace uuid!");
                            }
                        });
                    });
                Arc::new(Packet::PlayEntityProperties(spec))
            },
            Packet::PlayEntityMetadata(spec) => {
                let mut spec = spec.clone();
                spec.metadata.fields.iter_mut().for_each(|field| {
                    if let EntityMetadataFieldData::OptUUID(Some(uuid)) = &mut field.data {
                        if *uuid == *fake_uuid {
                            *uuid = *real_uuid;
                            debug!("Replace uuid!");
                        }
                    }
                });
                Arc::new(Packet::PlayEntityMetadata(spec))
            }
    
            // Serverbound packets - use the fake UUID
            Packet::PlaySpectate(spec) => {
                if spec.target == *real_uuid {
                    let mut spec = spec.clone();
                    spec.target = *fake_uuid;
                    debug!("Replace uuid!");
                    Arc::new(Packet::PlaySpectate(spec))
                } else {
                    packet
                }
            }
    
    
            // All other packets can stay the same
            _ => packet
        }
    }
}