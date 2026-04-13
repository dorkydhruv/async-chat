use std::collections::{HashMap, VecDeque};

use tokio::{
    sync::{RwLock, broadcast},
};

type RoomMap = RwLock<HashMap<String, broadcast::Sender<String>>>;
type HistoryMap = RwLock<HashMap<String, VecDeque<String>>>;

pub struct AppState {
    rooms: RoomMap,
    // <room> : [<from>:<msg>,..]
    last_messages: HistoryMap,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            last_messages: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get_or_create_room(&self, name: &str) -> broadcast::Sender<String> {
        self.rooms
            .write()
            .await
            .entry(name.to_string())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(100);
                tx
            })
            .clone()
    }

    pub async fn list_rooms(&self) -> Vec<String> {
        self.rooms.read().await.keys().cloned().collect()
    }

    pub async fn store_message(&self, room: &str, message: String) {
        let mut history_map = self.last_messages.write().await;
        let history = history_map
            .entry(room.to_string())
            .or_insert_with(VecDeque::new);
        history.push_back(message);

        while history.len() > 20 {
            history.pop_front();
        }
    }

    pub async fn get_messages(&self, room: &str) -> Vec<String> {
        self.last_messages
            .read()
            .await
            .get(room)
            .map(|history| history.iter().cloned().collect())
            .unwrap_or_default()
    }
}
