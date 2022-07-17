//! Event system for server

use tokio::sync::broadcast;

use super::NameUUID;

#[derive(Clone, Debug)]
pub enum Event {
    // Emitted when a new player joins the server
    PlayerJoin(NameUUID),
}

pub type EventListener = broadcast::Receiver<Event>;

pub type EventEmitter = broadcast::Sender<Event>;

pub struct EventSystem {
    emitter: EventEmitter,
}

impl EventSystem {
    pub fn new() -> Self {
        let (emitter, _) = broadcast::channel(256);
        Self {
            emitter,
        }
    }

    pub fn emit(&self, event: Event) {
        self.emitter.send(event).unwrap();
    }

    pub fn register_listener(&self) -> EventListener {
        self.emitter.subscribe()
    }

    pub fn listener_count(&self) -> usize {
        self.emitter.receiver_count()
    }
}
