use std::{
    io,
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::Instant,
};

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn pick_free_port() -> io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn wait_for_server(addr: &str) -> io::Result<()> {
    for _ in 0..50 {
        if TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "server did not become ready",
    ))
}

fn percentile(values: &mut [Duration], p: usize) -> Duration {
    values.sort_unstable();
    let idx = ((values.len() * p).div_ceil(100)).saturating_sub(1);
    values[idx]
}

async fn read_until_contains(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    needle: &str,
) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut line = String::new();

    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "message was not observed before timeout",
            ));
        }

        line.clear();
        let remaining = deadline.saturating_duration_since(Instant::now());
        let read_result = tokio::time::timeout(remaining, reader.read_line(&mut line))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "timed out waiting for line"))??;

        if read_result == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server closed connection",
            ));
        }

        if line.contains(needle) {
            return Ok(());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "performance-sensitive benchmark test; run manually"]
async fn chat_broadcast_p99_latency_under_threshold() -> anyhow::Result<()> {
    let port = pick_free_port()?;
    let addr = format!("127.0.0.1:{port}");
    let exe = env!("CARGO_BIN_EXE_async-chat");

    let child = Command::new(exe)
        .env("CHAT_ADDR", &addr)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let _guard = ChildGuard { child };

    wait_for_server(&addr).await?;

    let receiver_stream = TcpStream::connect(&addr).await?;
    let sender_stream = TcpStream::connect(&addr).await?;

    let (receiver_read_half, mut receiver_write_half) = receiver_stream.into_split();
    let mut receiver_reader = BufReader::new(receiver_read_half);

    let (sender_read_half, mut sender_write_half) = sender_stream.into_split();
    let mut sender_reader = BufReader::new(sender_read_half);

    sender_write_half.write_all(b"/nick sender\n").await?;
    receiver_write_half.write_all(b"/nick receiver\n").await?;

    read_until_contains(&mut sender_reader, "known as sender").await?;
    read_until_contains(&mut receiver_reader, "known as receiver").await?;

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let samples = 200usize;
    let mut latencies = Vec::with_capacity(samples);

    for i in 0..samples {
        let marker = format!("P99-LAT-{run_id}-{i}");
        let payload = format!("{marker}\n");
        let start = Instant::now();
        sender_write_half.write_all(payload.as_bytes()).await?;
        read_until_contains(&mut receiver_reader, &marker).await?;
        latencies.push(start.elapsed());
    }

    let p50 = percentile(&mut latencies.clone(), 50);
    let p99 = percentile(&mut latencies, 99);

    assert!(p50 < Duration::from_millis(250), "p50 too high: {p50:?}");
    assert!(p99 < Duration::from_millis(750), "p99 too high: {p99:?}");

    Ok(())
}
