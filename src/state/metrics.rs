use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct AppMetrics {
    rooms_created: AtomicU64,
    rooms_deleted: AtomicU64,
    publish_total: AtomicU64,
    publish_send_errors: AtomicU64,
    replay_requests: AtomicU64,
    replayed_events: AtomicU64,
    lagged_receives: AtomicU64,
    connections_opened: AtomicU64,
    connections_closed: AtomicU64,
}

#[derive(Clone, Copy, Debug)]
pub struct AppMetricsSnapshot {
    pub rooms_created: u64,
    pub rooms_deleted: u64,
    pub publish_total: u64,
    pub publish_send_errors: u64,
    pub replay_requests: u64,
    pub replayed_events: u64,
    pub lagged_receives: u64,
    pub connections_opened: u64,
    pub connections_closed: u64,
}

impl AppMetrics {
    pub fn inc_room_created(&self) {
        self.rooms_created.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_rooms_deleted(&self, count: u64) {
        self.rooms_deleted.fetch_add(count, Ordering::Relaxed);
    }

    pub fn inc_publish(&self) {
        self.publish_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_publish_send_error(&self) {
        self.publish_send_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_replay_request(&self) {
        self.replay_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_replayed_events(&self, count: u64) {
        self.replayed_events.fetch_add(count, Ordering::Relaxed);
    }

    pub fn inc_lagged_receive(&self) {
        self.lagged_receives.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_connection_opened(&self) {
        self.connections_opened.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_connection_closed(&self) {
        self.connections_closed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> AppMetricsSnapshot {
        AppMetricsSnapshot {
            rooms_created: self.rooms_created.load(Ordering::Relaxed),
            rooms_deleted: self.rooms_deleted.load(Ordering::Relaxed),
            publish_total: self.publish_total.load(Ordering::Relaxed),
            publish_send_errors: self.publish_send_errors.load(Ordering::Relaxed),
            replay_requests: self.replay_requests.load(Ordering::Relaxed),
            replayed_events: self.replayed_events.load(Ordering::Relaxed),
            lagged_receives: self.lagged_receives.load(Ordering::Relaxed),
            connections_opened: self.connections_opened.load(Ordering::Relaxed),
            connections_closed: self.connections_closed.load(Ordering::Relaxed),
        }
    }
}
