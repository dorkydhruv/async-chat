use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::TcpListener};
use tracing::info;

#[tokio::main]
async fn main()-> anyhow::Result<()>{
    tracing_subscriber::fmt().init();
    let tcp_lsnr = TcpListener::bind("127.0.0.1:8080").await.expect("couldn't bind tcp listener");
    info!("the server started at 127.0.0.1:8080");
    loop{
        let (socket, addr) = tcp_lsnr.accept().await?;
        info!("[{addr}] connected");

        tokio::spawn(async move {
            let (reader, mut writer) = socket.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        info!("write {}, bytes", line.len());
                        let _ = writer.write_all(line.as_bytes()).await;
                    }
                }
            }
        });
    }
}