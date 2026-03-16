use procfs::{CpuTime, CurrentSI, KernelStats};

/// CpuStats contains the CPU usage, stored as percentage of total.
/// Derived from the delta of CpuTime between 2 ticks with CpuSampler.
#[derive(Debug, Default)]
pub struct CpuStats {
    pub used: f64,
}

/// CpuSampler serves as a state, it contains the last CpuTime recorded.
/// Used to calculate CpuStats.
///
/// When constructed with `CpuSampler::new()`, its field `prev` will be
/// `None` and will stay as `None` until `CpuSampler.sample()` is called.
/// Calling `sample()` will set `prev` as the current CpuTime, subsequent
/// calls return the delta calculated as CpuStats.
#[derive(Default)]
pub struct CpuSampler {
    prev: Option<CpuTime>,
}

impl CpuSampler {
    pub fn new() -> Self {
        Self { prev: None }
    }

    /// sample CpuStats, if called for the first time return `None`
    /// if CpuSampler exists, calculate the cpu usage delta and return
    /// new CpuStat
    pub async fn sample(&mut self) -> anyhow::Result<Option<CpuStats>> {
        let cur_cpu_time = tokio::task::spawn_blocking(KernelStats::current)
            .await??
            .total;
        if let Some(prev_cpu_time) = &self.prev {
            let used = cpu_time_delta(&cur_cpu_time, prev_cpu_time);
            self.prev = Some(cur_cpu_time);
            return Ok(Some(CpuStats {
                used: f64::clamp(used, 0f64, 100f64),
            }));
        }

        self.prev = Some(cur_cpu_time);
        Ok(None)
    }
}

/// Calculates delta between two `CpuTime`
fn cpu_time_delta(cur_cpu_time: &CpuTime, prev: &CpuTime) -> f64 {
    let total_duration = |cpu_time: &CpuTime| {
        (cpu_time.user_duration()
            + cpu_time.nice_duration()
            + cpu_time.system_duration()
            + cpu_time.idle_duration()
            + cpu_time.iowait_duration().unwrap_or_default()
            + cpu_time.irq_duration().unwrap_or_default()
            + cpu_time.softirq_duration().unwrap_or_default()
            + cpu_time.steal_duration().unwrap_or_default())
        .as_secs_f64()
    };

    let total_cur = total_duration(cur_cpu_time);
    let total_prev = total_duration(prev);
    let total_delta = total_cur - total_prev;

    if total_delta == 0.0 {
        return 0.0;
    }

    let idle_delta = (cur_cpu_time.idle_duration() - prev.idle_duration()).as_secs_f64();
    let iowait_delta = (cur_cpu_time.iowait_duration().unwrap_or_default()
        - prev.iowait_duration().unwrap_or_default())
    .as_secs_f64();

    // used = everything except idle and iowait
    (total_delta - idle_delta - iowait_delta) / total_delta * 100.0
}
