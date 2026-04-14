use std::{collections::VecDeque, sync::Arc};

use dashmap::{DashMap, mapref::entry::Entry};
use tokio::sync::{Mutex, broadcast};

use crate::state::{AppMetricsSnapshot, ChatEvent, metrics::AppMetrics};

const ROOM_BUFFER_SIZE: usize = 100;
const ROOM_HISTORY_LIMIT: usize = 20;
const GENERAL_ROOM: &str = "#general";

struct Room {
    sender: broadcast::Sender<Arc<ChatEvent>>,
    history: Mutex<VecDeque<Arc<ChatEvent>>>,
}

impl Room {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(ROOM_BUFFER_SIZE);
        Self {
            sender,
            history: Mutex::new(VecDeque::new()),
        }
    }
}

type RoomMap = DashMap<String, Arc<Room>>;

pub struct AppState {
    rooms: RoomMap,
    metrics: AppMetrics,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            rooms: DashMap::new(),
            metrics: AppMetrics::default(),
        }
    }

    fn room_handle(&self, name: &str) -> Arc<Room> {
        match self.rooms.entry(name.to_string()) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                self.metrics.inc_room_created();
                let room = Arc::new(Room::new());
                entry.insert(room.clone());
                room
            }
        }
    }

    async fn push_history(room: &Arc<Room>, event: Arc<ChatEvent>) {
        let mut history = room.history.lock().await;
        history.push_back(event);
        while history.len() > ROOM_HISTORY_LIMIT {
            history.pop_front();
        }
    }

    pub async fn get_or_create_room(&self, name: &str) -> broadcast::Sender<Arc<ChatEvent>> {
        self.room_handle(name).sender.clone()
    }

    pub async fn subscribe_room(&self, name: &str) -> broadcast::Receiver<Arc<ChatEvent>> {
        self.room_handle(name).sender.subscribe()
    }

    pub async fn list_rooms(&self) -> Vec<String> {
        self.rooms.iter().map(|entry| entry.key().clone()).collect()
    }

    pub async fn publish(&self, room: &str, event: ChatEvent) {
        self.metrics.inc_publish();
        let room_state = self.room_handle(room);
        let event = Arc::new(event);
        Self::push_history(&room_state, event.clone()).await;
        if room_state.sender.send(event).is_err() {
            self.metrics.inc_publish_send_error();
        }
    }

    pub async fn publish_to_all_rooms(&self, event: ChatEvent) {
        let rooms: Vec<Arc<Room>> = self
            .rooms
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let event = Arc::new(event);
        for room in rooms {
            self.metrics.inc_publish();
            Self::push_history(&room, event.clone()).await;
            if room.sender.send(event.clone()).is_err() {
                self.metrics.inc_publish_send_error();
            }
        }
    }

    pub async fn get_messages(&self, room: &str) -> Vec<Arc<ChatEvent>> {
        self.metrics.inc_replay_request();
        let room = self.rooms.get(room).map(|entry| entry.value().clone());
        let Some(room) = room else {
            return Vec::new();
        };

        let history = room.history.lock().await;
        self.metrics.add_replayed_events(history.len() as u64);
        history.iter().cloned().collect()
    }

    pub async fn gc_empty_rooms(&self) -> Vec<String> {
        let to_remove: Vec<String> = self
            .rooms
            .iter()
            .filter(|entry| {
                entry.key().as_str() != GENERAL_ROOM && entry.value().sender.receiver_count() == 0
            })
            .map(|entry| entry.key().clone())
            .collect();

        for room in &to_remove {
            let _ = self.rooms.remove(room);
        }

        self.metrics.add_rooms_deleted(to_remove.len() as u64);

        to_remove
    }

    pub fn record_lagged_receive(&self) {
        self.metrics.inc_lagged_receive();
    }

    pub fn record_connection_opened(&self) {
        self.metrics.inc_connection_opened();
    }

    pub fn record_connection_closed(&self) {
        self.metrics.inc_connection_closed();
    }

    pub fn metrics_snapshot(&self) -> AppMetricsSnapshot {
        self.metrics.snapshot()
    }
}
