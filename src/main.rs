use std::sync::Arc;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, tcp::OwnedWriteHalf},
    sync::broadcast,
};
use tracing::{error, info};

use crate::constants::WELCOME_GENERAL;
use crate::state::{AppState, LocalUserState};
mod constants;
mod state;

enum Action {
    Join(String),
    Nick(String),
    Rooms,
    Quit,
    Message(String),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let tcp_lsnr = TcpListener::bind("127.0.0.1:8080").await?;
    info!("the server started at 127.0.0.1:8080");

    let app_state_main = Arc::new(AppState::new());
    let _ = app_state_main.get_or_create_room("#general").await;

    loop {
        let (socket, addr) = tcp_lsnr.accept().await?;
        info!("[{addr}] connected");
        let app_state = app_state_main.clone();

        tokio::spawn(async move {
            if let Err(err) = handle_client(socket, addr, app_state).await {
                error!("[{addr}] connection error: {err}");
            }
        });
    }
}

async fn handle_client(
    socket: TcpStream,
    addr: core::net::SocketAddr,
    app_state: Arc<AppState>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let general_sender = app_state.get_or_create_room("#general").await;
    let mut user_state = LocalUserState::new_with_addr(addr, general_sender);

    writer.write_all(WELCOME_GENERAL.as_bytes()).await?;
    replay_room_history(&app_state, &user_state.room, &mut writer).await?;

    let joined = format!("* {} joined {}", user_state.name, user_state.room);
    app_state
        .store_message(&user_state.room, joined.clone())
        .await;
    let _ = user_state.room_sender.send(joined);

    loop {
        tokio::select! {
            incoming = user_state.room_receiver.recv() => {
                match incoming {
                    Ok(message) => {
                        writer.write_all(message.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        writer.write_all(b"(missed some messages)\n").await?;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            read_result = reader.read_line(&mut line) => {
                match read_result {
                    Ok(0) => {
                        let left = format!("* {} disconnected", user_state.name);
                        app_state.store_message(&user_state.room, left.clone()).await;
                        let _ = user_state.room_sender.send(left);
                        break;
                    }
                    Ok(_) => {
                        let input = line.trim_end().to_string();
                        line.clear();

                        match parse_action(&input) {
                            Ok(Action::Join(room_name)) => {
                                let normalized = normalize_room_name(&room_name);
                                let prev_room = user_state.room.clone();
                                let left = format!("* {} left {}", user_state.name, prev_room);
                                app_state.store_message(&prev_room, left.clone()).await;
                                let _ = user_state.room_sender.send(left);

                                user_state.join_room(app_state.clone(), &normalized).await;

                                let joined = format!("* {} joined {}", user_state.name, user_state.room);
                                app_state.store_message(&user_state.room, joined.clone()).await;
                                let _ = user_state.room_sender.send(joined);

                                replay_room_history(&app_state, &user_state.room, &mut writer).await?;
                            }
                            Ok(Action::Nick(name)) => {
                                let old = user_state.name.clone();
                                user_state.update_name(name);
                                let note = format!("* {old} is now known as {}", user_state.name);
                                app_state.store_message(&user_state.room, note.clone()).await;
                                let _ = user_state.room_sender.send(note);
                            }
                            Ok(Action::Rooms) => {
                                let rooms = app_state.list_rooms().await;
                                let room_text = if rooms.is_empty() {
                                    "rooms: (none)".to_string()
                                } else {
                                    format!("rooms: {}", rooms.join(", "))
                                };
                                writer.write_all(room_text.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                            }
                            Ok(Action::Quit) => {
                                let left = format!("* {} quit", user_state.name);
                                app_state.store_message(&user_state.room, left.clone()).await;
                                let _ = user_state.room_sender.send(left);
                                break;
                            }
                            Ok(Action::Message(txt)) => {
                                if !txt.is_empty() {
                                    app_state
                                        .store_message(&user_state.room, format!("[{}] {} > {}", user_state.room, user_state.name, txt))
                                        .await;
                                    let _ = user_state.write_message(&txt);
                                }
                            }
                            Err(err) => {
                                writer.write_all(format!("error: {err}\n").as_bytes()).await?;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    Ok(())
}

async fn replay_room_history(
    app_state: &Arc<AppState>,
    room: &str,
    writer: &mut OwnedWriteHalf,
) -> anyhow::Result<()> {
    for message in app_state.get_messages(room).await {
        writer.write_all(message.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

fn normalize_room_name(room_name: &str) -> String {
    if room_name.starts_with('#') {
        room_name.to_string()
    } else {
        format!("#{room_name}")
    }
}

fn parse_action(line: &str) -> Result<Action, anyhow::Error> {
    if line.is_empty() {
        return Ok(Action::Message(String::new()));
    }

    if !line.starts_with('/') {
        return Ok(Action::Message(line.to_string()));
    }

    let mut partitions = line.split_whitespace();
    let command = partitions
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    match command {
        "/join" => {
            let room = partitions
                .next()
                .ok_or_else(|| anyhow::anyhow!("usage: /join <room>"))?;
            Ok(Action::Join(room.to_string()))
        }
        "/nick" => {
            let nick = partitions
                .next()
                .ok_or_else(|| anyhow::anyhow!("usage: /nick <name>"))?;
            Ok(Action::Nick(nick.to_string()))
        }
        "/rooms" => Ok(Action::Rooms),
        "/quit" => Ok(Action::Quit),
        _ => Err(anyhow::anyhow!("unknown command")),
    }
}
