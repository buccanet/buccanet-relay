use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderName;
use mongodb::{Client, Database};

use crate::config::Config;
use crate::features::delivery::hub::Hub;
use crate::features::delivery::inbox::InboxRepo;
use crate::features::delivery::messages::MessagesRepo;
use crate::features::devices::repo::DevicesRepo;
use crate::features::rooms::repo::RoomsRepo;
use crate::limits::RateLimiter;
use crate::locks::{KeyedGuard, KeyedLocks};

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);

/// Every limit the relay enforces without reading content.
pub struct Limits {
    /// Requests per device.
    pub device: RateLimiter,
    /// New devices per client address.
    pub register: RateLimiter,
    /// Invite previews per client address; they need no device.
    pub preview: RateLimiter,
    /// Join requests per device.
    pub join: RateLimiter,
    /// Messages per room.
    pub room: RateLimiter,
    /// Bytes of sealed messages per device.
    pub upload: RateLimiter,
}

impl Limits {
    fn new() -> Self {
        Self {
            device: RateLimiter::new(600, MINUTE),
            register: RateLimiter::new(30, HOUR),
            preview: RateLimiter::new(120, MINUTE),
            join: RateLimiter::new(20, HOUR),
            room: RateLimiter::new(300, MINUTE),
            upload: RateLimiter::new(50 * 1024 * 1024, HOUR),
        }
    }

    fn sweep(&self) {
        for limiter in [&self.device, &self.register, &self.preview, &self.join, &self.room, &self.upload] {
            limiter.sweep();
        }
    }
}

/// Shared by every request. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    /// Origin of the web app, used to build invite links.
    pub public_origin: Arc<str>,
    /// Header the reverse proxy sets to the client's address, when one is in front.
    pub client_ip_header: Option<HeaderName>,
    pub devices: DevicesRepo,
    pub rooms: RoomsRepo,
    pub messages: MessagesRepo,
    pub inbox: InboxRepo,
    pub hub: Hub,
    room_locks: KeyedLocks,
    pub limits: Arc<Limits>,
}

impl AppState {
    pub async fn connect(config: &Config) -> Result<Self, mongodb::error::Error> {
        let client = Client::with_uri_str(&config.mongodb_uri).await?;
        let db: Database = client.database(&config.mongodb_db);
        let ttl = Duration::from_secs(config.message_ttl_days * 86_400);
        let (devices, messages, hub) = (DevicesRepo::new(&db), MessagesRepo::new(&db, ttl), Hub::default());
        Ok(Self {
            public_origin: Arc::from(config.public_origin.trim_end_matches('/')),
            client_ip_header: config.client_ip_header.clone(),
            inbox: InboxRepo::new(&db, ttl, devices.clone(), messages.clone(), hub.clone()),
            rooms: RoomsRepo::new(&db),
            devices,
            messages,
            hub,
            room_locks: KeyedLocks::default(),
            limits: Arc::new(Limits::new()),
        })
    }

    pub async fn ensure_indexes(&self) -> Result<(), mongodb::error::Error> {
        self.devices.ensure_indexes().await?;
        self.rooms.ensure_indexes().await?;
        self.messages.ensure_indexes().await?;
        self.inbox.ensure_indexes().await
    }

    /// Serializes every state change of one room: epochs and seqs never race. Take it only
    /// once the caller is known to belong to the room.
    pub async fn lock_room(&self, room_id: &str) -> KeyedGuard {
        self.room_locks.lock(room_id).await
    }

    /// Drops what the limits and the hub keep for keys no longer in use.
    pub fn sweep(&self) {
        self.limits.sweep();
        self.hub.sweep();
    }
}
