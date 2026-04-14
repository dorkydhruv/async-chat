use std::sync::Arc;
use std::time::Duration;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, tcp::OwnedWriteHalf},
    sync::{broadcast, watch},
    task::JoinSet,
    time::Instant,
};
use tracing::{error, info};

use crate::constants::WELCOME_GENERAL;
use crate::state::{AppState, ChatEvent, LocalUserState};
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

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let gc_state = app_state_main.clone();
    let mut gc_shutdown_rx = shutdown_rx.clone();
    tokio::spawn(async move {
        let mut gc_tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = gc_tick.tick() => {
                    let removed = gc_state.gc_empty_rooms().await;
                    for room in removed {
                        info!("gc removed empty room: {room}");
                    }
                }
                _ = gc_shutdown_rx.changed() => {
                    if *gc_shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });

    let metrics_state = app_state_main.clone();
    let mut metrics_shutdown_rx = shutdown_rx.clone();
    tokio::spawn(async move {
        let mut metrics_tick = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = metrics_tick.tick() => {
                    let snapshot = metrics_state.metrics_snapshot();
                    info!(
                        "metrics rooms_created={} rooms_deleted={} publish_total={} publish_send_errors={} replay_requests={} replayed_events={} lagged_receives={} connections_opened={} connections_closed={}",
                        snapshot.rooms_created,
                        snapshot.rooms_deleted,
                        snapshot.publish_total,
                        snapshot.publish_send_errors,
                        snapshot.replay_requests,
                        snapshot.replayed_events,
                        snapshot.lagged_receives,
                        snapshot.connections_opened,
                        snapshot.connections_closed
                    );
                }
                _ = metrics_shutdown_rx.changed() => {
                    if *metrics_shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });

    let mut client_tasks = JoinSet::new();

    loop {
        tokio::select! {
            accept_result = tcp_lsnr.accept() => {
                let (socket, addr) = accept_result?;
                info!("[{addr}] connected");
                let app_state = app_state_main.clone();
                let client_shutdown_rx = shutdown_rx.clone();

                client_tasks.spawn(async move {
                    app_state.record_connection_opened();
                    if let Err(err) = handle_client(socket, addr, app_state, client_shutdown_rx).await {
                        error!("[{addr}] connection error: {err}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                break;
            }
        }
    }

    app_state_main
        .publish_to_all_rooms(ChatEvent::System {
            text: "Server shutting down...".to_string(),
        })
        .await;

    // Allow the shutdown message to flush through room broadcasts.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let _ = shutdown_tx.send(true);

    let drain_deadline = Instant::now() + Duration::from_secs(3);
    while !client_tasks.is_empty() {
        let now = Instant::now();
        if now >= drain_deadline {
            info!("shutdown drain timeout reached; finishing shutdown");
            break;
        }

        let remaining = drain_deadline.saturating_duration_since(now);
        match tokio::time::timeout(remaining, client_tasks.join_next()).await {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                info!("shutdown drain timeout reached; finishing shutdown");
                break;
            }
        }
    }

    let final_snapshot = app_state_main.metrics_snapshot();
    info!(
        "final metrics rooms_created={} rooms_deleted={} publish_total={} publish_send_errors={} replay_requests={} replayed_events={} lagged_receives={} connections_opened={} connections_closed={}",
        final_snapshot.rooms_created,
        final_snapshot.rooms_deleted,
        final_snapshot.publish_total,
        final_snapshot.publish_send_errors,
        final_snapshot.replay_requests,
        final_snapshot.replayed_events,
        final_snapshot.lagged_receives,
        final_snapshot.connections_opened,
        final_snapshot.connections_closed
    );

    Ok(())
}

async fn handle_client(
    socket: TcpStream,
    addr: core::net::SocketAddr,
    app_state: Arc<AppState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let general_receiver = app_state.subscribe_room("#general").await;
    let mut user_state = LocalUserState::new_with_addr(addr, general_receiver);

    writer.write_all(WELCOME_GENERAL.as_bytes()).await?;
    replay_room_history(&app_state, &user_state.room, &mut writer).await?;

    app_state
        .publish(
            &user_state.room,
            ChatEvent::UserJoined {
                user: user_state.name.clone(),
                room: user_state.room.clone(),
            },
        )
        .await;

    loop {
        tokio::select! {
            biased;
            incoming = user_state.room_receiver.recv() => {
                match incoming {
                    Ok(message) => {
                        writer.write_all(message.render().as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        app_state.record_lagged_receive();
                        writer.write_all(b"(missed some messages)\n").await?;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            read_result = reader.read_line(&mut line) => {
                match read_result {
                    Ok(0) => {
                        app_state
                            .publish(
                                &user_state.room,
                                ChatEvent::UserLeft {
                                    user: user_state.name.clone(),
                                    room: user_state.room.clone(),
                                },
                            )
                            .await;
                        break;
                    }
                    Ok(_) => {
                        let input = line.trim_end().to_string();
                        line.clear();

                        match parse_action(&input) {
                            Ok(Action::Join(room_name)) => {
                                let normalized = normalize_room_name(&room_name);
                                let prev_room = user_state.room.clone();
                                app_state
                                    .publish(
                                        &prev_room,
                                        ChatEvent::UserLeft {
                                            user: user_state.name.clone(),
                                            room: prev_room.clone(),
                                        },
                                    )
                                    .await;

                                user_state.join_room(app_state.clone(), &normalized).await;

                                replay_room_history(&app_state, &user_state.room, &mut writer).await?;

                                app_state
                                    .publish(
                                        &user_state.room,
                                        ChatEvent::UserJoined {
                                            user: user_state.name.clone(),
                                            room: user_state.room.clone(),
                                        },
                                    )
                                    .await;
                            }
                            Ok(Action::Nick(name)) => {
                                let old = user_state.name.clone();
                                user_state.update_name(name);
                                app_state
                                    .publish(
                                        &user_state.room,
                                        ChatEvent::NickChanged {
                                            old,
                                            new: user_state.name.clone(),
                                        },
                                    )
                                    .await;
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
                                app_state
                                    .publish(
                                        &user_state.room,
                                        ChatEvent::UserLeft {
                                            user: user_state.name.clone(),
                                            room: user_state.room.clone(),
                                        },
                                    )
                                    .await;
                                break;
                            }
                            Ok(Action::Message(txt)) => {
                                if !txt.is_empty() {
                                    app_state
                                        .publish(
                                            &user_state.room,
                                            ChatEvent::Message {
                                                room: user_state.room.clone(),
                                                user: user_state.name.clone(),
                                                text: txt,
                                            },
                                        )
                                        .await;
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

    app_state.record_connection_closed();

    Ok(())
}

async fn replay_room_history(
    app_state: &Arc<AppState>,
    room: &str,
    writer: &mut OwnedWriteHalf,
) -> anyhow::Result<()> {
    for message in app_state.get_messages(room).await {
        writer.write_all(message.render().as_bytes()).await?;
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
