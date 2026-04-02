mod cpu;
mod disk;
mod loadavg;
mod memory;
mod network;

use cpu::{CpuSampler, CpuStats};
use disk::{DiskInventory, DiskSampler, DiskStats};
use loadavg::LoadAvgStats;
use memory::MemoryStats;
use network::{NetworkSampler, NetworkStats};

/// Main handler to collect all proc information
#[derive(Default)]
pub struct Collector {
    pub cpu_sampler: CpuSampler,
    pub disk_sampler: DiskSampler,
    pub network_sampler: NetworkSampler,
}

#[derive(Debug, Clone)]
pub struct StatsSnapshot {
    pub cpu: CpuStats,
    pub memory: MemoryStats,
    pub disk: DiskStats,
    pub network: NetworkStats,
    pub loadavg: LoadAvgStats,
}

#[derive(Debug)]
pub struct InventorySnapshot {
    pub disk: DiskInventory,
}

#[derive(Debug)]
pub struct CollectOutput {
    pub telemetry: StatsSnapshot,
    pub inventory: Option<InventorySnapshot>,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            cpu_sampler: CpuSampler::new(),
            disk_sampler: DiskSampler::new(),
            network_sampler: NetworkSampler::new(),
        }
    }

    pub async fn collect(&mut self) -> anyhow::Result<CollectOutput> {
        let cpu = self.cpu_sampler.sample().await?.unwrap_or_default();
        let memory = MemoryStats::collect().await?;
        let disk_output = self.disk_sampler.sample().await?;
        let network = self.network_sampler.sample().await?;
        let loadavg = LoadAvgStats::collect().await?;

        let telemetry = StatsSnapshot {
            cpu,
            memory,
            disk: disk_output.telemetry,
            network,
            loadavg,
        };

        let inventory = disk_output.inventory.map(|disk| InventorySnapshot { disk });

        Ok(CollectOutput {
            telemetry,
            inventory,
        })
    }
}
