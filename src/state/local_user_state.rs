use std::sync::Arc;

use tokio::sync::broadcast;

use crate::state::{AppState, ChatEvent};
pub struct LocalUserState {
    pub room: String,
    pub name: String,
    pub room_receiver: broadcast::Receiver<Arc<ChatEvent>>,
}

impl LocalUserState {
    pub fn new_with_addr(
        addr: core::net::SocketAddr,
        room_receiver: broadcast::Receiver<Arc<ChatEvent>>,
    ) -> Self {
        Self {
            room: "#general".to_string(),
            name: addr.ip().to_string(),
            room_receiver,
        }
    }

    pub fn update_name(&mut self, name: String) {
        self.name = name;
    }

    pub async fn join_room(&mut self, app_state: Arc<AppState>, room_name: &str) {
        self.room = room_name.to_string();
        self.room_receiver = app_state.subscribe_room(room_name).await;
    }
}
