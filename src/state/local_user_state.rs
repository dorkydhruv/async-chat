use std::sync::Arc;

use tokio::sync::broadcast;

use crate::state::AppState;
pub struct LocalUserState {
    pub room: String,
    pub name: String,
    pub room_sender: broadcast::Sender<String>,
    pub room_receiver: broadcast::Receiver<String>,
}

impl LocalUserState {
    pub fn new_with_addr(
        addr: core::net::SocketAddr,
        room_sender: broadcast::Sender<String>,
    ) -> Self {
        let room_receiver = room_sender.subscribe();
        Self {
            room: "#general".to_string(),
            name: addr.ip().to_string(),
            room_sender,
            room_receiver,
        }
    }

    pub fn update_name(&mut self, name: String) {
        self.name = name;
    }

    pub async fn join_room(&mut self, app_state: Arc<AppState>, room_name: &str) {
        let sender = app_state.get_or_create_room(room_name).await;
        self.room = room_name.to_string();
        self.room_receiver = sender.subscribe();
        self.room_sender = sender;
    }

    pub fn write_message(&self, msg: &str) -> Result<usize, broadcast::error::SendError<String>> {
        self.room_sender
            .send(format!("[{}] {} > {}", self.room, self.name, msg))
    }
}
