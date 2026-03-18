use std::{collections::HashMap, time::Instant};

use procfs::net::dev_status;

/// NetworkStats contains host-level total tx/rx rate and per-interface tx/rx rates.
#[derive(Debug, Clone, Default)]
pub struct NetworkStats {
    pub total: NetworkRate,
    pub interfaces: Vec<InterfaceNetworkRate>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NetworkRate {
    pub tx_bytes_per_sec: f64,
    pub rx_bytes_per_sec: f64,
}

#[derive(Debug, Clone)]
pub struct InterfaceNetworkRate {
    pub name: String,
    pub rate: NetworkRate,
}

#[derive(Debug, Clone, Copy)]
struct InterfaceCounters {
    tx_bytes: u64,
    rx_bytes: u64,
}

#[derive(Debug, Default)]
pub struct NetworkSampler {
    prev_counters: Option<HashMap<String, InterfaceCounters>>,
    prev_at: Option<Instant>,
}

impl NetworkSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns concrete stats every tick:
    /// - first tick: zero total + empty interfaces (baseline warm-up)
    /// - subsequent ticks: delta-based rates
    pub async fn sample(&mut self) -> anyhow::Result<NetworkStats> {
        let cur_interfaces = read_interface_counters().await?;
        let now = Instant::now();

        if let (Some(prev_counters), Some(prev_at)) = (&self.prev_counters, self.prev_at) {
            let elapsed_secs = now
                .saturating_duration_since(prev_at)
                .as_secs_f64()
                .max(f64::EPSILON);

            let mut interface_rates =
                compute_interface_rates(prev_counters, &cur_interfaces, elapsed_secs);
            interface_rates.sort_by(|a, b| a.name.cmp(&b.name));

            let total_rate = compute_total_rate(&interface_rates);

            self.prev_counters = Some(cur_interfaces);
            self.prev_at = Some(now);

            return Ok(NetworkStats {
                total: total_rate,
                interfaces: interface_rates,
            });
        }

        self.prev_counters = Some(cur_interfaces);
        self.prev_at = Some(now);

        Ok(NetworkStats::default())
    }
}

async fn read_interface_counters() -> anyhow::Result<HashMap<String, InterfaceCounters>> {
    let interface_data = tokio::task::spawn_blocking(dev_status).await??;

    Ok(interface_data
        .into_iter()
        .filter(|(name, _)| should_include_interface(name))
        .map(|(name, iface)| {
            (
                name,
                InterfaceCounters {
                    tx_bytes: iface.sent_bytes,
                    rx_bytes: iface.recv_bytes,
                },
            )
        })
        .collect())
}

fn should_include_interface(name: &str) -> bool {
    // Default noisy-interface exclusions; make configurable later.
    if name == "lo" {
        return false;
    }

    let excluded_prefixes = [
        "veth", "docker", "br-", "cni", "flannel", "cali", "virbr", "tap",
    ];

    !excluded_prefixes
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn compute_interface_rates(
    prev: &HashMap<String, InterfaceCounters>,
    cur: &HashMap<String, InterfaceCounters>,
    elapsed_secs: f64,
) -> Vec<InterfaceNetworkRate> {
    let mut out = Vec::new();

    for (name, cur_stats) in cur {
        let Some(prev_stats) = prev.get(name) else {
            continue;
        };

        let delta_tx_bytes = cur_stats.tx_bytes.saturating_sub(prev_stats.tx_bytes) as f64;
        let delta_rx_bytes = cur_stats.rx_bytes.saturating_sub(prev_stats.rx_bytes) as f64;

        out.push(InterfaceNetworkRate {
            name: name.clone(),
            rate: NetworkRate {
                tx_bytes_per_sec: delta_tx_bytes / elapsed_secs,
                rx_bytes_per_sec: delta_rx_bytes / elapsed_secs,
            },
        });
    }

    out
}

fn compute_total_rate(interfaces: &[InterfaceNetworkRate]) -> NetworkRate {
    let mut total_tx = 0.0;
    let mut total_rx = 0.0;

    for interface in interfaces {
        total_tx += interface.rate.tx_bytes_per_sec;
        total_rx += interface.rate.rx_bytes_per_sec;
    }

    NetworkRate {
        tx_bytes_per_sec: total_tx,
        rx_bytes_per_sec: total_rx,
    }
}
