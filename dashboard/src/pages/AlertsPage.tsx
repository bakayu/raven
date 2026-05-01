import { useEffect, useState, useCallback } from "react";
import { useAuth } from "../auth-context";
import * as api from "../api";
import {
  Bell,
  CheckCircle,
  RefreshCw,
  AlertCircle,
  Zap,
  List,
} from "lucide-react";
import { formatDistanceToNow } from "date-fns";

function StatusBadge({ status }: { status: "firing" | "resolved" }) {
  return (
    <span
      className={status === "firing" ? "badge badge-error" : "badge badge-online"}
    >
      <span className={`dot ${status === "firing" ? "dot-online" : ""}`}
        style={{ background: status === "firing" ? "var(--error)" : "var(--success)" }}
      />
      {status === "firing" ? "Firing" : "Resolved"}
    </span>
  );
}

export default function AlertsPage() {
  const { accessToken } = useAuth();
  const [events, setEvents] = useState<api.AlertEvent[]>([]);
  const [rules, setRules] = useState<api.AlertRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"events" | "rules">("events");

  const load = useCallback(async () => {
    if (!accessToken) return;
    setLoading(true);
    setError(null);
    try {
      const [evs, rls] = await Promise.all([
        api.listAlertEvents(accessToken).catch(() => [] as api.AlertEvent[]),
        api.listAlertRules(accessToken).catch(() => [] as api.AlertRule[]),
      ]);
      setEvents(evs);
      setRules(rls);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load alerts");
    } finally {
      setLoading(false);
    }
  }, [accessToken]);

  useEffect(() => {
    load();
    const interval = setInterval(load, 30_000);
    return () => clearInterval(interval);
  }, [load]);

  const firing = events.filter((e) => e.status === "firing");
  const resolved = events.filter((e) => e.status === "resolved");
  const activeRules = rules.filter((r) => r.enabled);

  return (
    <div className="fade-in">
      {/* Header */}
      <div className="flex-between" style={{ marginBottom: 28 }}>
        <div>
          <h1 style={{ fontSize: 22, fontWeight: 600, letterSpacing: "-0.02em" }}>
            Alerts
          </h1>
          <p style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }}>
            {firing.length > 0
              ? `${firing.length} active · ${resolved.length} resolved`
              : "Alert history and rules"}
          </p>
        </div>
        <button className="btn btn-secondary btn-sm" onClick={load}>
          <RefreshCw size={12} />
          Refresh
        </button>
      </div>

      {/* Stats */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(3, 1fr)",
          gap: 12,
          marginBottom: 24,
        }}
      >
        <div className="card stat-card">
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
            <Zap size={13} color="var(--error)" strokeWidth={1.75} />
            <span className="stat-label">Firing</span>
          </div>
          <div className="stat-value" style={{ color: firing.length > 0 ? "var(--error)" : undefined }}>
            {firing.length}
          </div>
        </div>
        <div className="card stat-card">
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
            <CheckCircle size={13} color="var(--success)" strokeWidth={1.75} />
            <span className="stat-label">Resolved</span>
          </div>
          <div className="stat-value">{resolved.length}</div>
        </div>
        <div className="card stat-card">
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
            <Bell size={13} color="var(--text-muted)" strokeWidth={1.75} />
            <span className="stat-label">Active Rules</span>
          </div>
          <div className="stat-value">{activeRules.length}</div>
        </div>
      </div>

      {error && (
        <div className="alert alert-error" style={{ marginBottom: 16 }}>
          <AlertCircle size={14} />
          {error}
        </div>
      )}

      {/* Tabs */}
      <div className="tabs" style={{ marginBottom: 20 }}>
        <button
          className={`tab${tab === "events" ? " active" : ""}`}
          onClick={() => setTab("events")}
        >
          Events
        </button>
        <button
          className={`tab${tab === "rules" ? " active" : ""}`}
          onClick={() => setTab("rules")}
        >
          Rules
        </button>
      </div>

      {/* Events tab */}
      {tab === "events" && (
        <div className="card" style={{ overflow: "hidden" }}>
          {loading ? (
            <div style={{ padding: 20 }}>
              {[1, 2, 3].map((i) => (
                <div
                  key={i}
                  className="skeleton"
                  style={{ height: 44, borderRadius: 6, marginBottom: 8 }}
                />
              ))}
            </div>
          ) : events.length === 0 ? (
            <div className="empty-state">
              <Bell size={40} className="empty-icon" />
              <div className="empty-title">No alert events</div>
              <p className="empty-desc">
                Alert events will appear here when rules trigger.
              </p>
            </div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Status</th>
                  <th>Host</th>
                  <th>Rule</th>
                  <th>Value</th>
                  <th>Fired</th>
                  <th>Resolved</th>
                </tr>
              </thead>
              <tbody>
                {events.map((ev) => {
                  const rule = rules.find((r) => r.id === ev.rule_id);
                  return (
                    <tr key={ev.id}>
                      <td>
                        <StatusBadge status={ev.status} />
                      </td>
                      <td style={{ color: "var(--info)", fontFamily: "var(--font-mono)", fontSize: 12 }}>
                        {ev.hostname}
                      </td>
                      <td style={{ color: "var(--text-primary)" }}>
                        {rule?.name ?? ev.rule_id}
                      </td>
                      <td style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}>
                        {ev.value != null ? ev.value.toFixed(1) : "—"}
                        {rule ? ` ${rule.operator} ${rule.threshold}` : ""}
                      </td>
                      <td>
                        <span
                          title={new Date(ev.fired_at).toLocaleString()}
                          style={{ cursor: "default" }}
                        >
                          {formatDistanceToNow(new Date(ev.fired_at), { addSuffix: true })}
                        </span>
                      </td>
                      <td>
                        {ev.resolved_at
                          ? formatDistanceToNow(new Date(ev.resolved_at), { addSuffix: true })
                          : "—"}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
      )}

      {/* Rules tab */}
      {tab === "rules" && (
        <div className="card" style={{ overflow: "hidden" }}>
          {loading ? (
            <div style={{ padding: 20 }}>
              {[1, 2].map((i) => (
                <div
                  key={i}
                  className="skeleton"
                  style={{ height: 44, borderRadius: 6, marginBottom: 8 }}
                />
              ))}
            </div>
          ) : rules.length === 0 ? (
            <div className="empty-state">
              <List size={40} className="empty-icon" />
              <div className="empty-title">No alert rules</div>
              <p className="empty-desc">
                Alert rules can be configured via the API. Rule management UI is coming soon.
              </p>
            </div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Metric</th>
                  <th>Condition</th>
                  <th>Duration</th>
                  <th>Hosts</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {rules.map((rule) => (
                  <tr key={rule.id}>
                    <td style={{ color: "var(--text-primary)", fontWeight: 500 }}>
                      {rule.name}
                    </td>
                    <td style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}>
                      {rule.metric}
                    </td>
                    <td style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}>
                      {rule.operator} {rule.threshold}
                    </td>
                    <td style={{ fontSize: 12 }}>
                      {rule.duration_seconds}s
                    </td>
                    <td style={{ fontSize: 12 }}>
                      {rule.hosts.length === 0 ? "all hosts" : rule.hosts.join(", ")}
                    </td>
                    <td>
                      <span
                        className={rule.enabled ? "badge badge-online" : "badge badge-neutral"}
                      >
                        {rule.enabled ? "Enabled" : "Disabled"}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  );
}