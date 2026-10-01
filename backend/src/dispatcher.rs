//! One bounded, priority ordered lane per game server. Cancelled/expired work is never dispatched.
use crate::error::{ApiError, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
const MAX_QUEUED: usize = 32;
#[derive(Default)]
struct Queue {
    active: bool,
    sequence: u64,
    jobs: Vec<Job>,
}
struct Job {
    id: u64,
    priority: u8,
    deadline: Instant,
}
#[derive(Default)]
struct Lane {
    queue: Mutex<Queue>,
    changed: Notify,
}
pub struct Guard {
    lane: Arc<Lane>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut q) = self.lane.queue.lock() {
            q.active = false;
        }
        self.lane.changed.notify_waiters();
    }
}
struct Waiting {
    lane: Arc<Lane>,
    id: u64,
    armed: bool,
}
impl Drop for Waiting {
    fn drop(&mut self) {
        if self.armed {
            if let Ok(mut q) = self.lane.queue.lock() {
                q.jobs.retain(|j| j.id != self.id);
            }
            self.lane.changed.notify_waiters();
        }
    }
}
fn lanes() -> &'static Mutex<HashMap<String, Weak<Lane>>> {
    static LANES: OnceLock<Mutex<HashMap<String, Weak<Lane>>>> = OnceLock::new();
    LANES.get_or_init(Default::default)
}
pub async fn acquire(server: &str, priority: u8, deadline: Duration) -> Result<Guard> {
    let lane = {
        let mut map = lanes().lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, v| v.strong_count() > 0);
        if let Some(lane) = map.get(server).and_then(Weak::upgrade) {
            lane
        } else {
            let lane = Arc::new(Lane::default());
            map.insert(server.into(), Arc::downgrade(&lane));
            lane
        }
    };
    let until = Instant::now() + deadline;
    let id = {
        let mut q = lane.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.jobs.retain(|j| j.deadline > Instant::now());
        if q.jobs.len() >= MAX_QUEUED {
            return Err(ApiError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "lane_full",
                "Too many requests are waiting for this server.",
            ));
        }
        let id = q.sequence;
        q.sequence = q.sequence.wrapping_add(1);
        q.jobs.push(Job {
            id,
            priority: priority.min(2),
            deadline: until,
        });
        id
    };
    let mut waiting = Waiting {
        lane: lane.clone(),
        id,
        armed: true,
    };
    loop {
        let changed = lane.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        {
            let mut q = lane.queue.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if now >= until {
                return Err(ApiError::new(
                    axum::http::StatusCode::GATEWAY_TIMEOUT,
                    "lane_timeout",
                    "The queued request expired before dispatch.",
                ));
            }
            q.jobs.retain(|j| j.deadline > now);
            let best = q
                .jobs
                .iter()
                .min_by_key(|j| (j.priority, j.id))
                .map(|j| j.id);
            if !q.active && best == Some(id) {
                q.jobs.retain(|j| j.id != id);
                q.active = true;
                waiting.armed = false;
                return Ok(Guard { lane: lane.clone() });
            }
        }
        if tokio::time::timeout_at(tokio::time::Instant::from_std(until), changed)
            .await
            .is_err()
        {
            return Err(ApiError::new(
                axum::http::StatusCode::GATEWAY_TIMEOUT,
                "lane_timeout",
                "The queued request expired before dispatch.",
            ));
        }
    }
}
pub fn stats() -> serde_json::Value {
    let map = lanes().lock().unwrap_or_else(|e| e.into_inner());
    let mut busy = 0;
    let mut queued = 0;
    for lane in map.values().filter_map(Weak::upgrade) {
        let q = lane.queue.lock().unwrap_or_else(|e| e.into_inner());
        busy += usize::from(q.active);
        queued += q
            .jobs
            .iter()
            .filter(|j| j.deadline > Instant::now())
            .count();
    }
    serde_json::json!({"busy":busy,"queued":queued})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn priority_deadline_and_cancel() {
        let key = format!("lane-{}", uuid::Uuid::new_v4());
        let held = acquire(&key, 0, Duration::from_secs(1)).await.unwrap();
        let order = Arc::new(Mutex::new(vec![]));
        let mut tasks = vec![];
        for (priority, name) in [(2, "observe"), (1, "delivery"), (0, "command")] {
            let key = key.clone();
            let order = order.clone();
            tasks.push(tokio::spawn(async move {
                let _guard = acquire(&key, priority, Duration::from_secs(2))
                    .await
                    .unwrap();
                order.lock().unwrap().push(name);
                tokio::task::yield_now().await;
            }));
            tokio::task::yield_now().await;
        }
        let cancelled = tokio::spawn({
            let key = key.clone();
            async move { acquire(&key, 0, Duration::from_secs(2)).await }
        });
        tokio::task::yield_now().await;
        cancelled.abort();
        let _ = cancelled.await;
        assert!(acquire(&key, 0, Duration::from_millis(2)).await.is_err());
        drop(held);
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(
            *order.lock().unwrap(),
            vec!["command", "delivery", "observe"]
        );
        assert!(acquire(&key, 0, Duration::from_secs(1)).await.is_ok());
    }
    #[tokio::test]
    async fn independent_servers_and_bounded_queue() {
        let key = format!("lane-{}", uuid::Uuid::new_v4());
        let held = acquire(&key, 0, Duration::from_secs(1)).await.unwrap();
        assert!(
            acquire("independent", 0, Duration::from_secs(1))
                .await
                .is_ok()
        );
        let mut tasks = vec![];
        for _ in 0..32 {
            tasks.push(tokio::spawn({
                let key = key.clone();
                async move { acquire(&key, 0, Duration::from_secs(10)).await }
            }));
            tokio::task::yield_now().await;
        }
        assert_eq!(
            acquire(&key, 0, Duration::from_secs(1))
                .await
                .err()
                .unwrap()
                .code,
            "lane_full"
        );
        for task in tasks {
            task.abort();
            let _ = task.await;
        }
        drop(held);
        assert!(acquire(&key, 0, Duration::from_secs(1)).await.is_ok());
    }
}
