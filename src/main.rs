use std::{collections::{HashMap, VecDeque}, sync::Arc};

use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpListener, tcp::OwnedWriteHalf}, sync::{broadcast, RwLock}};
use tracing::{error, info};

use crate::constants::WELCOME_GENERAL;

mod constants;

type RoomMap = RwLock<HashMap<String, broadcast::Sender<String>>>;

enum Action {
    Join(room),
    Nick(name),
    Rooms,
    Quit,
    Message(text),
}

struct AppState{
    rooms: RoomMap,
    // <room> : [<from>:<msg>,..]
    last_messages: HashMap<String, VecDeque<String>>
}

impl AppState{
    pub fn new()->Self{
        let rooms = RwLock::new(HashMap::new());
        let last_messages = HashMap::new();
        Self { rooms, last_messages }
    }

    async fn get_or_create_room(&self, name: &str) -> (broadcast::Sender<String>, broadcast::Receiver<String>) {
        self.rooms.write().await.entry(name.to_string())
            .or_insert_with(|| {
                broadcast::channel(100)
            })
    }

    pub async  fn list_rooms(&self)-> &[String]{
        self.rooms.read().await.into_keys().collect()
    }
}

struct LocalUserState{
    room: String,
    name: String,
    addr: core::net::SocketAddr,
    room_sender: broadcast::Sender<String>,
    room_receiver: broadcast::Receiver<String>,
}

impl LocalUserState{
    pub fn new_with_addr(addr: core::net::SocketAddr)-> Self {
        Self { room: String::new(), name: addr.ip().to_string(), addr }
    }

    pub fn update_name(&mut self, name: String){
        self.name = name;
    }

    pub async fn join_room(&mut self, app_state: Arc<AppState>, room_name: &str) {
        self.room = String::from(room_name);
        (self.room_sender, self.room_receiver) = app_state.get_or_create_room(&room_name).await;
    }

    pub fn write_message(&self, msg: &str)-> Result<usize, broadcast::error::SendError<String>> {
        self.room_sender.send(format!("{} > {}", self.name, msg))
    }
}

#[tokio::main]
async fn main()-> anyhow::Result<()>{
    tracing_subscriber::fmt().init();
    let tcp_lsnr = TcpListener::bind("127.0.0.1:8080").await.expect("couldn't bind tcp listener");
    info!("the server started at 127.0.0.1:8080");
    let app_state_main = Arc::new(AppState::new());
    loop{
        let (socket, addr) = tcp_lsnr.accept().await?;
        info!("[{addr}] connected");
        let app_state = app_state_main.clone();
        let mut user_state = LocalUserState::new_with_addr(addr);
        tokio::spawn(async move {
            let (reader, mut writer) = socket.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            // initially everyone joins the #general and selects the name (start with random first and then use /nick <name>)
            let _ = writer.write_all(WELCOME_GENERAL.as_bytes()).await;
            user_state.join_room(app_state, "#general");
            // ideally we should join the #general channel and broadcast the message to other present
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        // here we read the commands like '/join <room>', '/nick <name>', /rooms, /quit
                        // check whether its a message or a command ideally a regex over first character
                        match parse_commands(&line, &mut writer){
                            Ok(Action::Join(room_name)) => {},
                            Ok(Action::Nick(name))=>{},
                            Ok(Action::Quit) => break,
                            Ok(Action::Rooms)=>{
                                let rooms = app_state.list_rooms().await;
                            },
                            Ok(Action::Message(txt)) => {user_state.write_message(txt).expect("couldnt send message to broadcast");},
                            Err(err) => error!("{:#?}", err)
                        }

                    }
                }
            }
        });
    }
}


fn parse_commands<'a>(command: &'a str, writer: &mut OwnedWriteHalf) -> Result<Action, anyhow::Error>{
    if command.starts_with("/") {
        
    }
    else {
        Ok(Action::Message(command))
    }
}