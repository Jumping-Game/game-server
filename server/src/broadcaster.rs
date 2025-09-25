use crate::proto::ServerFrame;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Notify;

const DEFAULT_CAPACITY: usize = 8;

#[derive(Clone)]
pub struct ConnectionQueue {
    inner: Arc<QueueInner>,
}

struct QueueInner {
    queue: tokio::sync::Mutex<VecDeque<ServerFrame>>,
    notify: Notify,
    capacity: usize,
    dropped_snapshots: tokio::sync::Mutex<u64>,
}

impl ConnectionQueue {
    pub fn new(capacity: Option<usize>) -> Self {
        let cap = capacity.unwrap_or(DEFAULT_CAPACITY);
        Self {
            inner: Arc::new(QueueInner {
                queue: tokio::sync::Mutex::new(VecDeque::with_capacity(cap)),
                notify: Notify::new(),
                capacity: cap,
                dropped_snapshots: tokio::sync::Mutex::new(0),
            }),
        }
    }

    pub async fn push(&self, frame: ServerFrame) {
        let mut queue = self.inner.queue.lock().await;
        if queue.len() >= self.inner.capacity {
            if matches!(frame, ServerFrame::Snapshot { .. }) {
                // drop oldest snapshot first
                if let Some(pos) = queue
                    .iter()
                    .position(|f| matches!(f, ServerFrame::Snapshot { .. }))
                {
                    queue.remove(pos);
                    *self.inner.dropped_snapshots.lock().await += 1;
                } else {
                    queue.pop_front();
                }
            } else {
                queue.pop_front();
            }
        }
        queue.push_back(frame);
        self.inner.notify.notify_waiters();
    }

    pub async fn next(&self) -> Option<ServerFrame> {
        loop {
            if let Some(mut frame) = self.try_pop().await {
                if let ServerFrame::Snapshot { meta } = &mut frame {
                    let mut dropped = self.inner.dropped_snapshots.lock().await;
                    meta.payload.stats.dropped_snapshots = *dropped;
                    *dropped = 0;
                }
                return Some(frame);
            }
            self.inner.notify.notified().await;
        }
    }

    async fn try_pop(&self) -> Option<ServerFrame> {
        let mut queue = self.inner.queue.lock().await;
        queue.pop_front()
    }

    pub async fn drained(&self) -> Vec<ServerFrame> {
        let mut queue = self.inner.queue.lock().await;
        queue.drain(..).collect()
    }

    pub async fn dropped_snapshots(&self) -> u64 {
        *self.inner.dropped_snapshots.lock().await
    }
}
