use mcproto_rs::protocol::HasPacketKind;
use mcproto_rs::uuid::UUID4;
use mcproto_rs::v1_16_3::{Packet753 as Packet, PlayerInfoActionList, EntityMetadataFieldData};
use rust_mc::{MinecraftServer, MinecraftClient, event::Event};
use rust_mc::mojang::auth;
use tokio::{self, join};

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    env_logger::init();

    // let mut client = MinecraftClient::new(
    //     "67.8.68.209:25566".parse().unwrap(),
    //     auth::Profile::new("Xlash", "", true),
    // );
    // debug!("Attempting connection for {}...", "Xlash");
    // let (_handle, _txc) = client.connect().await.unwrap();
    // std::thread::sleep(Duration::from_millis(10000000000));
    // loop {};

    // let arr = [0x07, 0x65, 0x15, 0xE7, 0x41, 0x79, 0xD0, 0xD7];
    // let int1 = VarInt::mc_deserialize(&arr).unwrap();
    // let int2 = VarInt::mc_deserialize(int1.data).unwrap();
    // panic!("0x{:02X}, 0x{:02X}", int1.value.0, int2.value.0);


    let (server, _tx) = MinecraftServer::new(
        "127.0.0.1:25565",
        "Rust test MC server",
        5,
        true,
    );
    let server_handle = server.start().await.unwrap();

    let mut server_event_listener = server.subscribe_to_events().await;

    // Listen to packets from the server
    let on_server_packet_loop = async move {
        loop {
            if let Ok(e) = server_event_listener.recv().await {
                match e {
                    Event::PlayerJoin(id) => {
                        info!("{} joined! Faking connection", id.0);
                        // The client as connected to this server
                        let server_client = server.get_server_client(&id).await.unwrap();
                        let mut sc_packet_receive = {
                            server_client.read().await.subscribe_to_serverbound_packets()
                        };
                        debug!("Got server_client packet listener");
                        // Packets sent by the client to the server for this player
                        // Create a proxy client to connect to a real server
                        let mut client = MinecraftClient::new(
                            "127.0.0.1:25566".parse().unwrap(),
                            auth::Profile::new(&id.0, "", true),
                        );
                        debug!("Attempting connection for {}...", id.0);
                        let (_handle, _txc) = client.connect().await.unwrap();
                        info!("Fake {} joined!", id.0);
                        let mut client_packet_listener = client.subscribe_to_clientbound_packets().await;
                        debug!("Got client packet listener");

                        let real_uuid = id.1;
                        let fake_uuid = client.get_name_uuid().await.1;

                        debug!("Read UUID: {}, Fake UUID: {}", real_uuid, fake_uuid);

                        tokio::spawn(async move {
                            let client = client;
                            debug!("[CTS] Starting loop");
                            let _txc = _txc;
                            // Forward all packets received by this server to the real server
                            let cts = async move {
                                loop {
                                    match sc_packet_receive.recv().await {
                                        Ok(mut p) => {
                                            let kind = p.kind();
                                            convert_packet_player_uuid(&mut p, &real_uuid, &fake_uuid);
                                            client.send_packet(p).await.unwrap();
                                        },
                                        Err(e) => debug!("[CTS] Error forwarding packets: {:?}", e),
                                    }
                                }
                            };
                            // Forward all packets received by the real server to the real client
                            let stc = async move {
                                debug!("[STC] Starting loop");
                                loop {
                                    match client_packet_listener.recv().await {
                                        Ok(mut p) => {
                                            let kind = p.kind();
                                            convert_packet_player_uuid(&mut p, &real_uuid, &fake_uuid);
                                            if let Packet::PlayServerChatMessage(spec) = &p {
                                                debug!("{:?}", spec);
                                            };
                                            server_client.read().await.send_packet(p).await.unwrap();
                                        },
                                        Err(e) => debug!("[STC] Error receiving packets: {:?}", e),
                                    }
                                }
                            };
                            debug!("Starting fake client loops");
                            let cts_handle = tokio::spawn(cts);
                            let stc_handle = tokio::spawn(stc);
                            let res = join!(cts_handle, stc_handle);
                            res.0.unwrap();
                            res.1.unwrap();
                        });
                    }
                }
            };
        }
    };
    
    on_server_packet_loop.await;

    server_handle.await.unwrap();
}

fn convert_packet_player_uuid(packet: &mut Packet, real_uuid: &UUID4, fake_uuid: &UUID4) {
    // Match only packets that contain a UUID
    match packet {
        // Clientbound packets - use the real UUID
        Packet::LoginSuccess(spec) => {
            if spec.uuid == *fake_uuid {
                spec.uuid = *real_uuid;
                debug!("Replace uuid!");
            }
        },
        Packet::PlaySpawnEntity(spec) => {
            if spec.object_uuid == *fake_uuid {
                spec.object_uuid = *real_uuid;
                debug!("Replace uuid!");
            }
        },
        Packet::PlaySpawnLivingEntity(spec) => {
            if spec.entity_uuid == *fake_uuid {
                spec.entity_uuid = *real_uuid;
                debug!("Replace uuid!");
            }
        },
        Packet::PlaySpawnPainting(spec) => {
            if spec.entity_uuid == *fake_uuid {
                spec.entity_uuid = *real_uuid;
                debug!("Replace uuid!");
            }
        },
        Packet::PlaySpawnPlayer(spec) => {
            if spec.uuid == *fake_uuid {
                spec.uuid = *real_uuid;
                debug!("Replace uuid!");
            }
        },
        Packet::PlayBossBar(spec) => {
            if spec.uuid == *fake_uuid {
                spec.uuid = *real_uuid;
                debug!("Replace uuid!");
            }
        },
        Packet::PlayServerChatMessage(spec) => {
            if spec.sender == *fake_uuid {
                spec.sender = *real_uuid;
                debug!("Replace uuid!");
            }
        },
        Packet::PlayPlayerInfo(spec) => {
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
            }
        },
        Packet::PlayEntityProperties(spec) => {
            spec.properties.iter_mut().for_each(|prop| {
                    prop.modifiers.iter_mut().for_each(|m| {
                        if m.uuid == *fake_uuid {
                            m.uuid = *real_uuid;
                            debug!("Replace uuid!");
                        }
                    });
                });
        },
        Packet::PlayEntityMetadata(spec) => {
            spec.metadata.fields.iter_mut().for_each(|field| {
                if let EntityMetadataFieldData::OptUUID(Some(uuid)) = &mut field.data {
                    if *uuid == *fake_uuid {
                        *uuid = *real_uuid;
                        debug!("Replace uuid!");
                    }
                }
            });
        }

        // Serverbound packets - use the fake UUID
        Packet::PlaySpectate(spec) => {
            if spec.target == *real_uuid {
                spec.target = *fake_uuid;
                debug!("Replace uuid!");
            }
        }


        // All other packets can stay the same
        _ => ()
    }
}
