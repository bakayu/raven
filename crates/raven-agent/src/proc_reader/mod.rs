mod cpu;

use cpu::{CpuSampler, CpuStats};

/// Main handler to collect all proc information
pub struct Collector {
    pub cpu_sampler: CpuSampler,
}

/// Snapshot is one instance of `proc` data collected
/// by the Collector.
///
/// It consists of all data collected every tick.
#[derive(Debug)]
pub struct Snapshot {
    pub cpu: CpuStats,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            cpu_sampler: CpuSampler::new(),
        }
    }

    // TODO: currently only supports cpu, need to add: memory, disk
    // network and loadavg

    /// Run all sub-handlers
    pub async fn collect(&mut self) -> anyhow::Result<Snapshot> {
        let cpu = self.cpu_sampler.sample().await?.unwrap_or_default();
        Ok(Snapshot { cpu })
    }
}
