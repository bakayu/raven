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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn include_interface_filters_noise() {
        assert!(!should_include_interface("lo"));
        assert!(!should_include_interface("docker0"));
        assert!(!should_include_interface("veth123"));
        assert!(!should_include_interface("cni0"));
        assert!(should_include_interface("eth0"));
        assert!(should_include_interface("ens18"));
    }

    #[test]
    fn compute_interface_rates_uses_deltas() {
        let prev = HashMap::from([(
            "eth0".to_string(),
            InterfaceCounters {
                tx_bytes: 1000,
                rx_bytes: 2000,
            },
        )]);

        let cur = HashMap::from([(
            "eth0".to_string(),
            InterfaceCounters {
                tx_bytes: 3000,
                rx_bytes: 5000,
            },
        )]);

        let rates = compute_interface_rates(&prev, &cur, 2.0);
        assert_eq!(rates.len(), 1);
        assert_eq!(rates[0].name, "eth0");
        assert_eq!(rates[0].rate.tx_bytes_per_sec, 1000.0);
        assert_eq!(rates[0].rate.rx_bytes_per_sec, 1500.0);
    }

    #[test]
    fn compute_interface_rates_saturates_on_counter_reset() {
        let prev = HashMap::from([(
            "eth0".to_string(),
            InterfaceCounters {
                tx_bytes: 5000,
                rx_bytes: 6000,
            },
        )]);

        let cur = HashMap::from([(
            "eth0".to_string(),
            InterfaceCounters {
                tx_bytes: 1000,
                rx_bytes: 1000,
            },
        )]);

        let rates = compute_interface_rates(&prev, &cur, 1.0);
        assert_eq!(rates.len(), 1);
        assert_eq!(rates[0].rate.tx_bytes_per_sec, 0.0);
        assert_eq!(rates[0].rate.rx_bytes_per_sec, 0.0);
    }

    #[test]
    fn compute_total_rate_sums_interfaces() {
        let interfaces = vec![
            InterfaceNetworkRate {
                name: "eth0".to_string(),
                rate: NetworkRate {
                    tx_bytes_per_sec: 10.0,
                    rx_bytes_per_sec: 20.0,
                },
            },
            InterfaceNetworkRate {
                name: "eth1".to_string(),
                rate: NetworkRate {
                    tx_bytes_per_sec: 5.0,
                    rx_bytes_per_sec: 7.0,
                },
            },
        ];

        let total = compute_total_rate(&interfaces);
        assert_eq!(total.tx_bytes_per_sec, 15.0);
        assert_eq!(total.rx_bytes_per_sec, 27.0);
    }
}
