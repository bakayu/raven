use procfs::{Current, Meminfo};

/// MemoryStats contains memory information as bytes
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub physical_memory: PhysicalMem,
    pub swap: Swap,
    pub buffer: u64,
    pub cache: u64,
    pub zswap: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PhysicalMem {
    pub total: u64,
    pub available: u64,
    pub used: u64,
}

#[derive(Debug, Clone)]
pub struct Swap {
    pub total: u64,
    pub used: u64,
    pub cached: u64,
}

impl MemoryStats {
    pub async fn collect() -> anyhow::Result<Self> {
        let mem_info = tokio::task::spawn_blocking(Meminfo::current).await??;

        let physical_memory = PhysicalMem {
            total: mem_info.mem_total,
            available: mem_info.mem_available.unwrap_or_default(),
            used: mem_info
                .mem_total
                .saturating_sub(mem_info.mem_available.unwrap_or_default()),
        };

        let swap = Swap {
            total: mem_info.swap_total,
            used: mem_info.swap_total.saturating_sub(mem_info.swap_free),
            cached: mem_info.swap_cached,
        };

        Ok(Self {
            physical_memory,
            swap,
            buffer: mem_info.buffers,
            cache: mem_info.cached,
            zswap: mem_info.z_swapped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collect_returns_consistent_memory_values() {
        let stats = MemoryStats::collect()
            .await
            .expect("memory collect should succeed");

        assert!(stats.physical_memory.available <= stats.physical_memory.total);
        assert_eq!(
            stats.physical_memory.used,
            stats
                .physical_memory
                .total
                .saturating_sub(stats.physical_memory.available)
        );

        assert!(stats.swap.used <= stats.swap.total);
    }

    #[tokio::test]
    async fn collect_can_be_called_multiple_times() {
        let a = MemoryStats::collect()
            .await
            .expect("first collect should succeed");
        let b = MemoryStats::collect()
            .await
            .expect("second collect should succeed");

        assert!(a.physical_memory.total > 0 || b.physical_memory.total > 0);
        assert!(a.physical_memory.used <= a.physical_memory.total);
        assert!(b.physical_memory.used <= b.physical_memory.total);
        assert!(a.swap.used <= a.swap.total);
        assert!(b.swap.used <= b.swap.total);
    }
}
