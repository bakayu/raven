import { useParams, useNavigate, Link } from "react-router-dom";
import { useEffect, useState, useCallback } from "react";
import { useAuth } from "../auth-context";
import * as api from "../api";
import MetricChart from "../components/MetricsChart";
import TimeRangeSelector from "../components/TimeWindowSelector";
import {
  ChevronLeft,
  RefreshCw,
  ScrollText,
  AlertCircle,
  Cpu,
  MemoryStick,
  HardDrive,
  Network,
  Activity,
} from "lucide-react";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes.toFixed(0)} B/s`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB/s`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB/s`;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes.toFixed(0)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes < 1024 * 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  return `${(bytes / (1024 * 1024 * 1024 * 1024)).toFixed(1)} TB`;
}

export default function HostDetailPage() {
  const { hostname } = useParams<{ hostname: string }>();
  const { accessToken } = useAuth();
  const navigate = useNavigate();
  const [range, setRange] = useState("1h");
  const [metrics, setMetrics] = useState<api.MetricsData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(
    async (silent = false) => {
      if (!accessToken || !hostname) return;
      if (!silent) setLoading(true);
      else setRefreshing(true);
      setError(null);
      try {
        const data = await api.getMetrics(accessToken, hostname, range);
        setMetrics(data);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load metrics");
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [accessToken, hostname, range],
  );

  useEffect(() => {
    load();
    const interval = setInterval(() => load(true), 30_000);
    return () => clearInterval(interval);
  }, [load]);

  const latestCpu =
    metrics?.cpu?.length
      ? metrics.cpu[metrics.cpu.length - 1].value.toFixed(1)
      : null;
  const latestMem =
    metrics?.memory?.length
      ? metrics.memory[metrics.memory.length - 1].value.toFixed(1)
      : null;
  const latestDisk =
    metrics?.disk?.length
      ? metrics.disk[metrics.disk.length - 1].value.toFixed(1)
      : null;

  const latestMemUsed = metrics?.memory_used?.length
    ? metrics.memory_used[metrics.memory_used.length - 1].value
    : null;
  const latestMemTotal = metrics?.memory_total?.length
    ? metrics.memory_total[metrics.memory_total.length - 1].value
    : null;
  const latestDiskUsed = metrics?.disk_used?.length
    ? metrics.disk_used[metrics.disk_used.length - 1].value
    : null;
  const latestDiskTotal = metrics?.disk_total?.length
    ? metrics.disk_total[metrics.disk_total.length - 1].value
    : null;

  return (
    <div className="fade-in">
      {/* Header */}
      <div style={{ marginBottom: 28 }}>
        <button
          onClick={() => navigate("/agents")}
          className="btn btn-ghost btn-sm"
          style={{ marginBottom: 12, padding: "4px 0", color: "var(--text-muted)" }}
        >
          <ChevronLeft size={14} />
          Agents
        </button>

        <div className="flex-between">
          <div>
            <h1
              style={{
                fontSize: 22,
                fontWeight: 600,
                letterSpacing: "-0.02em",
                display: "flex",
                alignItems: "center",
                gap: 8,
              }}
            >
              {hostname}
            </h1>
            <p style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }}>
              Host metrics — {range}
            </p>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <Link
              to={`/logs?hostname=${hostname}`}
              className="btn btn-secondary btn-sm"
            >
              <ScrollText size={12} />
              View logs
            </Link>
            <button
              className="btn btn-secondary btn-sm"
              onClick={() => load(true)}
              disabled={refreshing}
            >
              <RefreshCw
                size={12}
                strokeWidth={2}
                style={{ animation: refreshing ? "spin 0.8s linear infinite" : "none" }}
              />
              Refresh
            </button>
          </div>
        </div>

        <style>{`@keyframes spin { to { transform: rotate(360deg); } }`}</style>
      </div>

      {/* Time range */}
      <div style={{ marginBottom: 24 }}>
        <TimeRangeSelector value={range} onChange={setRange} />
      </div>

      {/* Current stats */}
      {metrics && (latestCpu || latestMem || latestDisk) && (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3, 1fr)",
            gap: 12,
            marginBottom: 24,
          }}
        >
          {latestCpu && (
            <div className="card stat-card">
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
                <Cpu size={13} color="var(--chart-1)" strokeWidth={1.75} />
                <span className="stat-label">CPU</span>
              </div>
              <div className="stat-value">{latestCpu}%</div>
            </div>
          )}
          {latestMem && (
            <div className="card stat-card">
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
                <MemoryStick size={13} color="var(--chart-2)" strokeWidth={1.75} />
                <span className="stat-label">Memory</span>
              </div>
              <div className="stat-value" style={{ color: "var(--chart-2)" }}>
                {latestMem}%
              </div>
              {latestMemUsed != null && latestMemTotal != null && (
                <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 6 }}>
                  {formatSize(latestMemUsed)} / {formatSize(latestMemTotal)}
                </div>
              )}
            </div>
          )}
          {latestDisk && (
            <div className="card stat-card">
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
                <HardDrive size={13} color="var(--chart-3)" strokeWidth={1.75} />
                <span className="stat-label">Disk</span>
              </div>
              <div className="stat-value" style={{ color: "var(--chart-3)" }}>
                {latestDisk}%
              </div>
              {latestDiskUsed != null && latestDiskTotal != null && (
                <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 6 }}>
                  {formatSize(latestDiskUsed)} / {formatSize(latestDiskTotal)}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="alert alert-error" style={{ marginBottom: 16 }}>
          <AlertCircle size={14} />
          {error}
        </div>
      )}

      {/* Charts */}
      {loading && !metrics ? (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(500px, 1fr))",
            gap: 16,
          }}
        >
          {[1, 2, 3].map((i) => (
            <div
              key={i}
              className="card"
              style={{ height: 230, padding: 20 }}
            >
              <div
                className="skeleton"
                style={{ height: 12, width: 80, borderRadius: 4, marginBottom: 16 }}
              />
              <div
                className="skeleton"
                style={{ height: 160, borderRadius: 6 }}
              />
            </div>
          ))}
        </div>
      ) : metrics ? (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(500px, 1fr))",
            gap: 16,
          }}
        >
          {metrics.cpu?.length > 0 && (
            <div className="card" style={{ padding: 20 }}>
              <MetricChart
                data={metrics.cpu}
                label="CPU Usage"
                unit="%"
                color="var(--chart-1)"
                height={180}
              />
            </div>
          )}
          {metrics.memory?.length > 0 && (
            <div className="card" style={{ padding: 20 }}>
              <MetricChart
                data={metrics.memory}
                label="Memory Usage"
                unit="%"
                color="var(--chart-2)"
                height={180}
              />
            </div>
          )}
          {metrics.disk?.length > 0 && (
            <div className="card" style={{ padding: 20 }}>
              <MetricChart
                data={metrics.disk}
                label="Disk Usage"
                unit="%"
                color="var(--chart-3)"
                height={180}
              />
            </div>
          )}
          {(metrics.load_avg?.length ?? 0) > 0 && (
            <div className="card" style={{ padding: 20 }}>
              <MetricChart
                data={metrics.load_avg!}
                label="Load Average (1m)"
                unit=""
                color="var(--chart-4)"
                domain={[0, "auto" as unknown as number]}
                height={180}
                formatValue={(v) => v.toFixed(2)}
              />
            </div>
          )}
          {metrics.network_rx?.length && metrics.network_tx?.length ? (
            <div className="card" style={{ padding: 20, gridColumn: "1 / -1" }}>
              <div
                style={{
                  fontSize: 12,
                  fontWeight: 500,
                  color: "var(--text-muted)",
                  textTransform: "uppercase",
                  letterSpacing: "0.04em",
                  marginBottom: 10,
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                }}
              >
                <Network size={12} strokeWidth={1.75} />
                Network Throughput
              </div>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1fr",
                  gap: 16,
                }}
              >
                <MetricChart
                  data={metrics.network_rx}
                  label="RX (Inbound)"
                  unit=""
                  color="var(--chart-1)"
                  domain={[0, "auto" as unknown as number]}
                  height={160}
                  formatValue={formatBytes}
                />
                <MetricChart
                  data={metrics.network_tx}
                  label="TX (Outbound)"
                  unit=""
                  color="var(--chart-5)"
                  domain={[0, "auto" as unknown as number]}
                  height={160}
                  formatValue={formatBytes}
                />
              </div>
            </div>
          ) : null}
        </div>
      ) : null}

      {!loading && !error && metrics &&
        !metrics.cpu?.length &&
        !metrics.memory?.length && (
          <div className="card">
            <div className="empty-state">
              <Activity size={40} className="empty-icon" />
              <div className="empty-title">No metrics yet</div>
              <p className="empty-desc">
                Metrics will appear here once the agent starts sending data.
              </p>
            </div>
          </div>
        )}
    </div>
  );
}
