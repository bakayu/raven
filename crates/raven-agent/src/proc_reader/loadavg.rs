use procfs::{Current, LoadAverage};

/// LoadAvgStats contains the averages of jobs run in the
/// last 1, 5 and 15 minutes.
#[derive(Debug)]
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
