use bytes::{Bytes, BytesMut};

#[derive(Clone, Debug)]
pub enum ChatEvent {
    System {
        text: String,
    },
    UserJoined {
        user: String,
        room: String,
    },
    UserLeft {
        user: String,
        room: String,
    },
    NickChanged {
        old: String,
        new: String,
    },
    Message {
        room: String,
        user: String,
        text: String,
    },
}

impl ChatEvent {
    pub fn to_wire_message(&self) -> Bytes {
        let line = match self {
            Self::System { text } => text.clone(),
            Self::UserJoined { user, room } => format!("* {user} joined {room}"),
            Self::UserLeft { user, room } => format!("* {user} left {room}"),
            Self::NickChanged { old, new } => format!("* {old} is now known as {new}"),
            Self::Message { room, user, text } => format!("[{room}] {user} > {text}"),
        };

        let mut bytes = BytesMut::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.extend_from_slice(b"\n");
        bytes.freeze()
    }
}
