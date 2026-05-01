use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct RateLimitEntry {
    pub count: u32,
    pub reset_at: Instant,
}

pub fn check_rate_limit(
    limits: &DashMap<String, RateLimitEntry>,
    key: &str,
    limit: u32,
    window: Duration,
) -> AppResult<()> {
    let now = Instant::now();

    let mut entry = limits.entry(key.to_string()).or_insert(RateLimitEntry {
        count: 0,
        reset_at: now + window,
    });

    if now >= entry.reset_at {
        entry.count = 0;
        entry.reset_at = now + window;
    }

    if entry.count >= limit {
        return Err(AppError::RateLimited);
    }

    entry.count += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn allows_requests_until_limit_is_reached() {
        let limits = DashMap::new();

        assert!(
            check_rate_limit(&limits, "auth:login:127.0.0.1", 2, Duration::from_secs(60)).is_ok()
        );
        assert!(
            check_rate_limit(&limits, "auth:login:127.0.0.1", 2, Duration::from_secs(60)).is_ok()
        );
        assert!(matches!(
            check_rate_limit(&limits, "auth:login:127.0.0.1", 2, Duration::from_secs(60)),
            Err(AppError::RateLimited)
        ));
    }
}
