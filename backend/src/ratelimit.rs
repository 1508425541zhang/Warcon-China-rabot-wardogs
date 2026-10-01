//! Sliding-window budgets, matching the existing panel rather than allowing boundary bursts.
use crate::error::{ApiError, Result};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct RateLimiter {
    windows: HashMap<String, VecDeque<Instant>>,
}
impl RateLimiter {
    pub fn check_at(
        &mut self,
        key: String,
        limit: usize,
        duration: Duration,
        count: bool,
        now: Instant,
    ) -> Result<()> {
        self.windows.retain(|_, v| {
            v.back()
                .is_some_and(|t| now.saturating_duration_since(*t) < duration)
        });
        if self.windows.len() >= 10000 && !self.windows.contains_key(&key) {
            return Err(ApiError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many request sources.",
            ));
        }
        let stamps = self.windows.entry(key).or_default();
        while stamps
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= duration)
        {
            stamps.pop_front();
        }
        if stamps.len() >= limit {
            return Err(ApiError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests; try again shortly.",
            ));
        }
        if count {
            stamps.push_back(now);
        }
        Ok(())
    }
}
pub fn allow(key: String, limit: usize, count: bool) -> Result<()> {
    static LIMITER: OnceLock<Mutex<RateLimiter>> = OnceLock::new();
    LIMITER
        .get_or_init(|| Mutex::new(RateLimiter::default()))
        .lock()
        .map_err(|_| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "rate_limit",
                "Rate limiter unavailable.",
            )
        })?
        .check_at(key, limit, Duration::from_secs(60), count, Instant::now())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_window_events_do_not_reset_at_fixed_boundary() {
        let now = Instant::now();
        let mut limiter = RateLimiter::default();
        let d = Duration::from_secs(60);
        assert!(limiter.check_at("source".into(), 2, d, true, now).is_ok());
        assert!(
            limiter
                .check_at("source".into(), 2, d, true, now + Duration::from_secs(59))
                .is_ok()
        );
        assert!(
            limiter
                .check_at("source".into(), 2, d, true, now + Duration::from_secs(61))
                .is_ok()
        );
        assert!(
            limiter
                .check_at("source".into(), 2, d, false, now + Duration::from_secs(62))
                .is_err()
        );
        assert!(
            limiter
                .check_at("source".into(), 2, d, true, now + Duration::from_secs(119))
                .is_ok()
        );
    }
}
