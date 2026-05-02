import { useEffect, useState, useCallback } from "react";
import { useAuth } from "../auth-context";
import * as api from "../api";
import { useNavigate } from "react-router-dom";
import {
  Server,
  RefreshCw,
  Circle,
  Clock,
  ChevronRight,
  Wifi,
  WifiOff,
  Trash2,
  AlertTriangle,
  X,
} from "lucide-react";
import { formatDistanceToNow } from "date-fns";

// ──────────────────────────────────────────────────────────────────────────────
// Confirmation dialog component
// ──────────────────────────────────────────────────────────────────────────────

function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  loading,
}: {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  loading?: boolean;
}) {
  if (!open) return null;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 9999,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0, 0, 0, 0.6)",
        backdropFilter: "blur(4px)",
        animation: "fadeIn 0.15s ease",
      }}
      onClick={onCancel}
    >
      <div
        className="card"
        onClick={(e) => e.stopPropagation()}
        style={{
          padding: 24,
          maxWidth: 420,
          width: "90%",
          animation: "fadeIn 0.15s ease",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 14 }}>
          <div
            style={{
              width: 36,
              height: 36,
              borderRadius: 8,
              background: "var(--error-bg)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <AlertTriangle size={18} color="var(--error)" strokeWidth={2} />
          </div>
          <div>
            <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
              {title}
            </div>
          </div>
          <button
            onClick={onCancel}
            style={{
              marginLeft: "auto",
              background: "none",
              border: "none",
              cursor: "pointer",
              color: "var(--text-muted)",
              padding: 4,
              display: "flex",
            }}
          >
            <X size={16} />
          </button>
        </div>

        <p style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.6, marginBottom: 20 }}>
          {message}
        </p>

        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button className="btn btn-secondary btn-sm" onClick={onCancel} disabled={loading}>
            Cancel
          </button>
          <button className="btn btn-danger btn-sm" onClick={onConfirm} disabled={loading}>
            <Trash2 size={12} />
            {loading ? "Removing…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────────────────
// Agent card
// ──────────────────────────────────────────────────────────────────────────────

function AgentCard({
  agent,
  onRemove,
}: {
  agent: api.Agent;
  onRemove: (agent: api.Agent) => void;
}) {
  const navigate = useNavigate();
  const lastSeen = formatDistanceToNow(new Date(agent.last_seen_at), {
    addSuffix: true,
  });

  return (
    <div
      className="card card-hover"
      onClick={() => navigate(`/agents/${agent.hostname}`)}
      style={{ padding: 0, overflow: "hidden" }}
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          padding: "16px 20px 12px",
          borderBottom: "1px solid var(--border-subtle)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <div
            style={{
              width: 32,
              height: 32,
              borderRadius: 6,
              background: "var(--bg-muted)",
              border: "1px solid var(--border)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <Server size={14} color="var(--text-secondary)" strokeWidth={1.75} />
          </div>
          <div>
            <div
              style={{
                fontSize: 14,
                fontWeight: 600,
                color: "var(--text-primary)",
                letterSpacing: "-0.01em",
              }}
            >
              {agent.hostname}
            </div>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
              {agent.ip ?? "—"} · {agent.os}
            </div>
          </div>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span className={agent.online ? "badge badge-online" : "badge badge-offline"}>
            <span
              className={`dot ${agent.online ? "dot-online" : "dot-offline"}${agent.online ? " dot-pulse" : ""}`}
            />
            {agent.online ? "Online" : "Offline"}
          </span>
          <ChevronRight size={14} color="var(--text-muted)" />
        </div>
      </div>

      {/* Footer */}
      <div
        style={{
          padding: "10px 20px",
          display: "flex",
          alignItems: "center",
          gap: 16,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            fontSize: 11,
            color: "var(--text-muted)",
          }}
        >
          <Clock size={11} strokeWidth={1.75} />
          {lastSeen}
        </div>
        {agent.log_files && agent.log_files.length > 0 && (
          <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
            {agent.log_files.length} log source{agent.log_files.length !== 1 ? "s" : ""}
          </div>
        )}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
            v{agent.agent_version}
          </span>
          <button
            className="btn btn-ghost btn-sm"
            onClick={(e) => {
              e.stopPropagation();
              onRemove(agent);
            }}
            title="Remove agent"
            style={{
              padding: "3px 6px",
              color: "var(--text-muted)",
              transition: "color 0.15s ease",
            }}
            onMouseEnter={(e) => {
              (e.currentTarget as HTMLButtonElement).style.color = "var(--error)";
            }}
            onMouseLeave={(e) => {
              (e.currentTarget as HTMLButtonElement).style.color = "var(--text-muted)";
            }}
          >
            <Trash2 size={12} strokeWidth={1.75} />
          </button>
        </div>
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────────────────
// Agents page
// ──────────────────────────────────────────────────────────────────────────────

export default function AgentsPage() {
  const { accessToken } = useAuth();
  const [agents, setAgents] = useState<api.Agent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  // Remove dialog state
  const [removeTarget, setRemoveTarget] = useState<api.Agent | null>(null);
  const [removing, setRemoving] = useState(false);

  const load = useCallback(
    async (silent = false) => {
      if (!accessToken) return;
      if (!silent) setLoading(true);
      else setRefreshing(true);
      setError(null);
      try {
        const data = await api.getAgents(accessToken);
        setAgents(data);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load agents");
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [accessToken],
  );

  useEffect(() => {
    load();
    const interval = setInterval(() => load(true), 15_000);
    return () => clearInterval(interval);
  }, [load]);

  async function onConfirmRemove() {
    if (!removeTarget || !accessToken) return;
    setRemoving(true);
    try {
      await api.deleteAgent(accessToken, removeTarget.id);
      setRemoveTarget(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to remove agent");
      setRemoveTarget(null);
    } finally {
      setRemoving(false);
    }
  }

  const online = agents.filter((a) => a.online);
  const offline = agents.filter((a) => !a.online);

  return (
    <div className="fade-in">
      {/* Confirm remove dialog */}
      <ConfirmDialog
        open={!!removeTarget}
        title="Remove agent"
        message={`Are you sure you want to remove "${removeTarget?.hostname ?? ""}"? This will delete the agent's registration. The agent can re-register if it's still running with a valid token.`}
        confirmLabel="Remove"
        onConfirm={onConfirmRemove}
        onCancel={() => setRemoveTarget(null)}
        loading={removing}
      />

      {/* Page header */}
      <div className="flex-between" style={{ marginBottom: 28 }}>
        <div>
          <h1 style={{ fontSize: 22, fontWeight: 600, letterSpacing: "-0.02em" }}>
            Agents
          </h1>
          <p style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }}>
            {agents.length > 0
              ? `${online.length} online · ${offline.length} offline`
              : "Monitored hosts"}
          </p>
        </div>
        <button
          className="btn btn-secondary btn-sm"
          onClick={() => load(true)}
          disabled={refreshing}
          style={{ display: "flex", alignItems: "center", gap: 6 }}
        >
          <RefreshCw
            size={12}
            strokeWidth={2}
            style={{ animation: refreshing ? "spin 0.8s linear infinite" : "none" }}
          />
          Refresh
        </button>
        <style>{`@keyframes spin { to { transform: rotate(360deg); } }`}</style>
      </div>

      {/* Stats row */}
      {agents.length > 0 && (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3, 1fr)",
            gap: 12,
            marginBottom: 28,
          }}
        >
          {[
            {
              label: "Total Agents",
              value: agents.length,
              icon: Server,
              color: "var(--text-primary)",
            },
            {
              label: "Online",
              value: online.length,
              icon: Wifi,
              color: "var(--success)",
            },
            {
              label: "Offline",
              value: offline.length,
              icon: WifiOff,
              color: offline.length > 0 ? "var(--error)" : "var(--text-muted)",
            },
          ].map(({ label, value, icon: Icon, color }) => (
            <div key={label} className="card stat-card">
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                <Icon size={14} color={color} strokeWidth={1.75} />
                <span className="stat-label">{label}</span>
              </div>
              <div className="stat-value" style={{ fontSize: 22, color }}>
                {value}
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Content */}
      {loading && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))", gap: 12 }}>
          {[1, 2, 3].map((i) => (
            <div key={i} className="card" style={{ height: 96 }}>
              <div className="skeleton" style={{ margin: 16, height: 16, width: "60%", borderRadius: 4 }} />
              <div className="skeleton" style={{ margin: "4px 16px", height: 12, width: "40%", borderRadius: 4 }} />
            </div>
          ))}
        </div>
      )}

      {error && (
        <div className="alert alert-error">
          <Circle size={14} />
          {error}
        </div>
      )}

      {!loading && agents.length === 0 && !error && (
        <div className="card">
          <div className="empty-state">
            <Server size={40} className="empty-icon" />
            <div className="empty-title">No agents connected</div>
            <p className="empty-desc">
              Generate a token in Settings, install the agent on a server, and it will appear here automatically.
            </p>
          </div>
        </div>
      )}

      {!loading && agents.length > 0 && (
        <>
          {online.length > 0 && (
            <div style={{ marginBottom: 24 }}>
              <div className="section-header">
                <span className="section-title">Online</span>
                <span style={{ fontSize: 12, color: "var(--text-muted)" }}>{online.length}</span>
              </div>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(auto-fill, minmax(340px, 1fr))",
                  gap: 12,
                }}
              >
                {online.map((a) => (
                  <AgentCard key={a.id} agent={a} onRemove={setRemoveTarget} />
                ))}
              </div>
            </div>
          )}
          {offline.length > 0 && (
            <div>
              <div className="section-header">
                <span className="section-title" style={{ color: "var(--text-muted)" }}>
                  Offline
                </span>
                <span style={{ fontSize: 12, color: "var(--text-muted)" }}>{offline.length}</span>
              </div>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(auto-fill, minmax(340px, 1fr))",
                  gap: 12,
                }}
              >
                {offline.map((a) => (
                  <AgentCard key={a.id} agent={a} onRemove={setRemoveTarget} />
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
