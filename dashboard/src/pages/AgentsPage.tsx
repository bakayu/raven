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
} from "lucide-react";
import { formatDistanceToNow } from "date-fns";

function AgentCard({ agent }: { agent: api.Agent }) {
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
        <div style={{ marginLeft: "auto", fontSize: 11, color: "var(--text-muted)" }}>
          v{agent.agent_version}
        </div>
      </div>
    </div>
  );
}

export default function AgentsPage() {
  const { accessToken } = useAuth();
  const [agents, setAgents] = useState<api.Agent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

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

  const online = agents.filter((a) => a.online);
  const offline = agents.filter((a) => !a.online);

  return (
    <div className="fade-in">
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
                  <AgentCard key={a.id} agent={a} />
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
                  <AgentCard key={a.id} agent={a} />
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
