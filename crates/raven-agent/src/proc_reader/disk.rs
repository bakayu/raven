use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    fs::File,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Instant,
};

use procfs::{DiskStat, diskstats, process::Process};
use rustix::fs::fstatvfs;

/// Linux kernel exposes disk sectors as 512-byte units in /proc/diskstats.
const DISK_SECTOR_BYTES: u64 = 512;

#[derive(Debug, Clone, Default)]
pub struct DiskStats {
    /// Delta-based metrics keyed by physical block device (e.g. nvme0n1, sda).
    pub devices_io: Vec<DeviceIoStats>,
    /// Point-in-time filesystem capacity gauges.
    pub filesystems: Vec<FilesystemTelemetry>,
}

#[derive(Debug, Clone, Default)]
pub struct DiskInventory {
    /// Slow-changing metadata for filesystems and mount topology.
    pub filesystems: Vec<FilesystemInventory>,
}

#[derive(Debug, Clone, Default)]
pub struct DiskCollectOutput {
    pub telemetry: DiskStats,
    /// Only present when inventory changed (or first emission).
    pub inventory: Option<DiskInventory>,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceIoStats {
    pub device: String,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
    pub read_iops: f64,
    pub write_iops: f64,
}

#[derive(Debug, Clone, Default)]
pub struct FilesystemTelemetry {
    /// Backing source device (e.g. /dev/nvme0n1p2).
    pub source: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub avail_bytes: u64,
    pub used_percent: f64,
}

#[derive(Debug, Clone, Default)]
pub struct FilesystemInventory {
    /// Backing source device (e.g. /dev/nvme0n1p2).
    pub source: String,
    /// Mapped physical parent device when derivable (e.g. nvme0n1).
    pub parent_device: Option<String>,
    pub fs_type: String,
    /// All mount points for this same source+fs_type (e.g. btrfs subvolumes).
    pub mount_points: Vec<String>,
}

#[derive(Debug, Clone)]
struct FilesystemRow {
    source: String,
    fs_type: String,
    mount_points: Vec<String>,
    parent_device: Option<String>,
    total_bytes: u64,
    used_bytes: u64,
    free_bytes: u64,
    avail_bytes: u64,
    used_percent: f64,
}

#[derive(Debug, Default)]
pub struct DiskSampler {
    prev_devices: Option<HashMap<String, DiskStat>>,
    prev_at: Option<Instant>,
    prev_inventory_hash: Option<u64>,
}

impl DiskSampler {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn sample(&mut self) -> anyhow::Result<DiskCollectOutput> {
        let current_devices = tokio::task::spawn_blocking(read_diskstats_map).await??;
        let filesystem_rows = tokio::task::spawn_blocking(read_filesystem_rows).await??;
        let now = Instant::now();

        let devices_io = match (&self.prev_devices, self.prev_at) {
            (Some(prev), Some(prev_at)) => {
                let elapsed_secs = (now - prev_at).as_secs_f64().max(f64::EPSILON);
                compute_device_io(prev, &current_devices, elapsed_secs)
            }
            _ => Vec::new(),
        };

        self.prev_devices = Some(current_devices);
        self.prev_at = Some(now);

        let telemetry = DiskStats {
            devices_io,
            filesystems: filesystem_rows
                .iter()
                .map(|row| FilesystemTelemetry {
                    source: row.source.clone(),
                    total_bytes: row.total_bytes,
                    used_bytes: row.used_bytes,
                    free_bytes: row.free_bytes,
                    avail_bytes: row.avail_bytes,
                    used_percent: row.used_percent,
                })
                .collect(),
        };

        let inventory_data = DiskInventory {
            filesystems: filesystem_rows
                .iter()
                .map(|row| FilesystemInventory {
                    source: row.source.clone(),
                    parent_device: row.parent_device.clone(),
                    fs_type: row.fs_type.clone(),
                    mount_points: row.mount_points.clone(),
                })
                .collect(),
        };

        let inventory_hash = hash_inventory(&inventory_data);
        let inventory = match self.prev_inventory_hash {
            Some(prev) if prev == inventory_hash => None,
            _ => Some(inventory_data),
        };
        self.prev_inventory_hash = Some(inventory_hash);

        Ok(DiskCollectOutput {
            telemetry,
            inventory,
        })
    }
}

fn read_diskstats_map() -> anyhow::Result<HashMap<String, DiskStat>> {
    let stats = diskstats()?;
    let map = stats
        .into_iter()
        .filter(|d| is_physical_device(&d.name))
        .map(|d| (d.name.clone(), d))
        .collect();

    Ok(map)
}

fn compute_device_io(
    prev: &HashMap<String, DiskStat>,
    cur: &HashMap<String, DiskStat>,
    elapsed_secs: f64,
) -> Vec<DeviceIoStats> {
    let mut out = Vec::new();

    for (name, cur_stat) in cur {
        let Some(prev_stat) = prev.get(name) else {
            continue;
        };

        let delta_reads = cur_stat.reads.saturating_sub(prev_stat.reads) as f64;
        let delta_writes = cur_stat.writes.saturating_sub(prev_stat.writes) as f64;

        let delta_sectors_read =
            cur_stat.sectors_read.saturating_sub(prev_stat.sectors_read) as f64;
        let delta_sectors_written = cur_stat
            .sectors_written
            .saturating_sub(prev_stat.sectors_written) as f64;

        out.push(DeviceIoStats {
            device: name.clone(),
            read_iops: delta_reads / elapsed_secs,
            write_iops: delta_writes / elapsed_secs,
            read_bytes_per_sec: (delta_sectors_read * DISK_SECTOR_BYTES as f64) / elapsed_secs,
            write_bytes_per_sec: (delta_sectors_written * DISK_SECTOR_BYTES as f64) / elapsed_secs,
        });
    }

    out.sort_by(|a, b| a.device.cmp(&b.device));
    out
}

fn read_filesystem_rows() -> anyhow::Result<Vec<FilesystemRow>> {
    let mounts = Process::myself()?.mountinfo()?;

    let mut grouped: HashMap<(String, String), HashSet<PathBuf>> = HashMap::new();

    for mount in mounts {
        let Some(source) = mount.mount_source.clone() else {
            continue;
        };

        if !should_include_mount(&mount.fs_type, &source, &mount.mount_point) {
            continue;
        }

        grouped
            .entry((source, mount.fs_type))
            .or_default()
            .insert(mount.mount_point);
    }

    let mut rows = Vec::new();

    for ((source, fs_type), mount_points_set) in grouped {
        let mut mount_points: Vec<PathBuf> = mount_points_set.into_iter().collect();
        mount_points.sort();

        let probe_mount = match mount_points.first() {
            Some(path) => path,
            None => continue,
        };

        let fd = match File::open(probe_mount) {
            Ok(fd) => fd,
            Err(_) => continue,
        };

        let st = fstatvfs(&fd)?;

        let frsize = st.f_frsize as u64;
        let bsize = st.f_bsize as u64;
        let block_size = if frsize > 0 { frsize } else { bsize.max(1) };

        let total_bytes = (st.f_blocks as u64).saturating_mul(block_size);
        let free_bytes = (st.f_bfree as u64).saturating_mul(block_size);
        let avail_bytes = (st.f_bavail as u64).saturating_mul(block_size);
        let used_bytes = total_bytes.saturating_sub(free_bytes);

        rows.push(FilesystemRow {
            parent_device: resolve_parent_device(&source),
            source,
            fs_type,
            mount_points: mount_points
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            total_bytes,
            used_bytes,
            free_bytes,
            avail_bytes,
            used_percent: calc_used_percent(used_bytes, total_bytes),
        });
    }

    rows.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.fs_type.cmp(&b.fs_type))
    });
    Ok(rows)
}

fn should_include_mount(fs_type: &str, source: &str, _mount_point: &Path) -> bool {
    if !source.starts_with("/dev/") {
        return false;
    }

    let excluded = [
        "proc",
        "sysfs",
        "tmpfs",
        "devtmpfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "pstore",
        "securityfs",
        "debugfs",
        "tracefs",
        "configfs",
        "overlay",
        "squashfs",
        "ramfs",
        "autofs",
        "mqueue",
        "hugetlbfs",
        "fusectl",
    ];

    !excluded.contains(&fs_type)
}

fn calc_used_percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
    }
}

fn is_physical_device(name: &str) -> bool {
    if name.starts_with("loop")
        || name.starts_with("ram")
        || name.starts_with("zram")
        || name.starts_with("dm-")
        || name.starts_with("md")
    {
        return false;
    }

    if name.starts_with("nvme") {
        return !name.contains('p');
    }

    if let Some(prefix) = ["sd", "vd", "xvd", "hd"]
        .iter()
        .find(|p| name.starts_with(*p))
    {
        return name[prefix.len()..].chars().all(|c| c.is_ascii_lowercase());
    }

    if name.starts_with("mmcblk") {
        return !name.contains('p');
    }

    false
}

fn resolve_parent_device(source: &str) -> Option<String> {
    let dev_name = source.strip_prefix("/dev/")?;

    let partition_marker = Path::new("/sys/class/block")
        .join(dev_name)
        .join("partition");
    if partition_marker.exists() {
        let parent = Path::new("/sys/class/block").join(dev_name).join("..");
        if let Ok(canon) = std::fs::canonicalize(parent) {
            if let Some(name) = canon.file_name().and_then(|s| s.to_str()) {
                if !name.is_empty() && name != "block" {
                    return Some(name.to_string());
                }
            }
        }
    }

    Some(parse_parent_from_device_name(dev_name))
}

fn parse_parent_from_device_name(dev_name: &str) -> String {
    if let Some(stripped) = dev_name
        .strip_prefix("nvme")
        .and_then(|_| dev_name.rsplit_once('p').map(|(left, _)| left))
    {
        return stripped.to_string();
    }

    if let Some(stripped) = dev_name
        .strip_prefix("mmcblk")
        .and_then(|_| dev_name.rsplit_once('p').map(|(left, _)| left))
    {
        return stripped.to_string();
    }

    let trimmed = dev_name.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        dev_name.to_string()
    } else {
        trimmed.to_string()
    }
}

fn hash_inventory(inventory: &DiskInventory) -> u64 {
    let mut hasher = DefaultHasher::new();

    for fs in &inventory.filesystems {
        fs.source.hash(&mut hasher);
        fs.fs_type.hash(&mut hasher);
        fs.parent_device.hash(&mut hasher);
        for mount in &fs.mount_points {
            mount.hash(&mut hasher);
        }
    }

    hasher.finish()
}
