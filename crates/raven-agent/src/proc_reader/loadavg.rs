use procfs::{Current, LoadAverage};

/// LoadAvgStats contains the averages of jobs run in the
/// last 1, 5 and 15 minutes.
#[derive(Debug, Clone)]
pub struct LoadAvgStats {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

impl LoadAvgStats {
    pub async fn collect() -> anyhow::Result<Self> {
        let load_avg = tokio::task::spawn_blocking(LoadAverage::current).await??;

        Ok(Self {
            one: load_avg.one as f64,
            five: load_avg.five as f64,
            fifteen: load_avg.fifteen as f64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collect_returns_finite_non_negative_values() {
        let stats = LoadAvgStats::collect()
            .await
            .expect("loadavg collect should succeed");

        assert!(stats.one.is_finite());
        assert!(stats.five.is_finite());
        assert!(stats.fifteen.is_finite());

        assert!(stats.one >= 0.0);
        assert!(stats.five >= 0.0);
        assert!(stats.fifteen >= 0.0);
    }

    #[tokio::test]
    async fn collect_can_be_called_multiple_times() {
        let a = LoadAvgStats::collect()
            .await
            .expect("first collect should succeed");
        let b = LoadAvgStats::collect()
            .await
            .expect("second collect should succeed");

        assert!(a.one.is_finite() && b.one.is_finite());
        assert!(a.five.is_finite() && b.five.is_finite());
        assert!(a.fifteen.is_finite() && b.fifteen.is_finite());
    }
}
