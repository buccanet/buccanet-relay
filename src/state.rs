use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use mongodb::{Client, Database};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::features::delivery::hub::Hub;
use crate::features::delivery::inbox::InboxRepo;
use crate::features::delivery::messages::MessagesRepo;
use crate::features::devices::repo::DevicesRepo;
use crate::features::rooms::repo::RoomsRepo;
use crate::limits::RateLimiter;

/// Shared by every request. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    /// Origin of the web app, used to build invite links.
    pub public_origin: Arc<str>,
    pub devices: DevicesRepo,
    pub rooms: RoomsRepo,
    pub messages: MessagesRepo,
    pub inbox: InboxRepo,
    pub hub: Hub,
    room_locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
    pub device_limiter: Arc<RateLimiter>,
    pub register_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub async fn connect(config: &Config) -> Result<Self, mongodb::error::Error> {
        let client = Client::with_uri_str(&config.mongodb_uri).await?;
        let db: Database = client.database(&config.mongodb_db);
        let ttl = Duration::from_secs(config.message_ttl_days * 86_400);
        Ok(Self {
            public_origin: Arc::from(config.public_origin.trim_end_matches('/')),
            devices: DevicesRepo::new(&db),
            rooms: RoomsRepo::new(&db),
            messages: MessagesRepo::new(&db, ttl),
            inbox: InboxRepo::new(&db, ttl),
            hub: Hub::default(),
            room_locks: Arc::new(DashMap::new()),
            device_limiter: Arc::new(RateLimiter::new(600, Duration::from_secs(60))),
            register_limiter: Arc::new(RateLimiter::new(30, Duration::from_secs(3600))),
        })
    }

    pub async fn ensure_indexes(&self) -> Result<(), mongodb::error::Error> {
        self.devices.ensure_indexes().await?;
        self.rooms.ensure_indexes().await?;
        self.messages.ensure_indexes().await?;
        self.inbox.ensure_indexes().await
    }

    /// Serializes every state change of one room: epochs and seqs never race.
    pub fn room_lock(&self, room_id: &str) -> Arc<Mutex<()>> {
        self.room_locks.entry(room_id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }
}
