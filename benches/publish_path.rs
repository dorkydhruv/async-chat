use std::time::Duration;

use async_chat::state::{AppState, ChatEvent};
use criterion::{Criterion, criterion_group, criterion_main};

fn bench_publish_path(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .expect("runtime should build");

    c.bench_function("publish_path_end_to_end", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let app = AppState::new();
            let mut receiver = app.subscribe_room("#bench").await;
            let start = tokio::time::Instant::now();

            for i in 0..iters {
                app.publish(
                    "#bench",
                    ChatEvent::Message {
                        room: "#bench".to_string(),
                        user: "bench".to_string(),
                        text: format!("msg-{i}"),
                    },
                )
                .await;

                let _ = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                    .await
                    .expect("receive should finish")
                    .expect("room message should be present");
            }

            start.elapsed()
        });
    });
}

criterion_group!(benches, bench_publish_path);
criterion_main!(benches);
