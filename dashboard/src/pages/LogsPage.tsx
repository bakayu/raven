import { useEffect, useRef, useState, useCallback } from "react";
import { useAuth } from "../auth-context";
import { useSearchParams } from "react-router-dom";
import * as api from "../api";
import TimeRangeSelector from "../components/TimeWindowSelector";
import {
  Search,
  Radio,
  Pause,
  Play,
  ScrollText,
  AlertCircle,
  X,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";

const PAGE_SIZE = 200;

export default function LogsPage() {
  const { accessToken } = useAuth();
  const [searchParams, setSearchParams] = useSearchParams();

  const [hostname, setHostname] = useState(searchParams.get("hostname") ?? "");
  const [app, setApp] = useState(searchParams.get("app") ?? "");
  const [stream, setStream] = useState<"" | "stdout" | "stderr">(
    (searchParams.get("stream") as "" | "stdout" | "stderr") ?? "",
  );
  const [search, setSearch] = useState(searchParams.get("search") ?? "");
  const [range, setRange] = useState("1h");

  const [logs, setLogs] = useState<api.LogEntry[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Live tail
  const [live, setLive] = useState(false);
  const [paused, setPaused] = useState(false);
  const [buffered, setBuffered] = useState<api.LogEntry[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  // Sync URL params
  useEffect(() => {
    const p: Record<string, string> = {};
    if (hostname) p.hostname = hostname;
    if (app) p.app = app;
    if (stream) p.stream = stream;
    if (search) p.search = search;
    setSearchParams(p, { replace: true });
  }, [hostname, app, stream, search]);

  // Historical load
  const loadLogs = useCallback(async () => {
    if (!accessToken) return;
    setLoading(true);
    setError(null);
    try {
      const result = await api.getLogs(accessToken, {
        hostname: hostname || undefined,
        app: app || undefined,
        stream: (stream as "stdout" | "stderr") || undefined,
        search: search || undefined,
        range,
        limit: PAGE_SIZE,
        offset,
      });
      setLogs(result.logs);
      setTotal(result.total);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load logs");
    } finally {
      setLoading(false);
    }
  }, [accessToken, hostname, app, stream, search, range, offset]);

  useEffect(() => {
    if (!live) {
      loadLogs();
    }
  }, [loadLogs, live]);

  // Live tail
  useEffect(() => {
    if (!live) {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      return;
    }
    setLogs([]);
    setBuffered([]);
    setPaused(false);
    autoScrollRef.current = true;

    const ws = api.openLogTail(hostname || "*", app || "*", (entry) => {
      if (paused) {
        setBuffered((b) => [...b, entry]);
      } else {
        setLogs((l) => [...l.slice(-2000), entry]);
      }
    });
    wsRef.current = ws;
    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, [live, hostname, app]);

  // Auto-scroll
  useEffect(() => {
    if (autoScrollRef.current && scrollRef.current && !paused) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs]);

  function resumeLive() {
    setPaused(false);
    setLogs((l) => [...l.slice(-2000), ...buffered]);
    setBuffered([]);
    autoScrollRef.current = true;
  }

  function onScroll() {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    autoScrollRef.current = atBottom;
  }

  const allLogs = live && paused ? [...logs, ...buffered] : logs;


  return (
    <div className="fade-in" style={{ display: "flex", flexDirection: "column", height: "calc(100vh - 64px)" }}>
      {/* Header */}
      <div className="flex-between" style={{ marginBottom: 20, flexShrink: 0 }}>
        <div>
          <h1 style={{ fontSize: 22, fontWeight: 600, letterSpacing: "-0.02em" }}>
            Log Explorer
          </h1>
          {!live && (
            <p style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }}>
              {loading ? "Loading…" : `${total.toLocaleString()} entries`}
            </p>
          )}
        </div>

        <button
          onClick={() => setLive((v) => !v)}
          className={`btn btn-sm ${live ? "btn-secondary" : "btn-secondary"}`}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            color: live ? "var(--success)" : "var(--text-secondary)",
            borderColor: live ? "var(--success)" : undefined,
          }}
        >
          {live ? (
            <>
              <span className="live-dot" />
              Live
            </>
          ) : (
            <>
              <Radio size={12} />
              Live Tail
            </>
          )}
        </button>
      </div>

      {/* Filter bar */}
      <div
        className="card"
        style={{
          padding: "12px 16px",
          marginBottom: 12,
          flexShrink: 0,
          display: "flex",
          flexWrap: "wrap",
          gap: 8,
          alignItems: "center",
        }}
      >
        <div style={{ position: "relative", flex: "1 1 160px" }}>
          <input
            className="input"
            placeholder="Host"
            value={hostname}
            onChange={(e) => { setHostname(e.target.value); setOffset(0); }}
            style={{ paddingLeft: 32, fontSize: 13 }}
          />
          <Search
            size={12}
            style={{
              position: "absolute",
              left: 10,
              top: "50%",
              transform: "translateY(-50%)",
              color: "var(--text-muted)",
            }}
          />
        </div>

        <input
          className="input"
          placeholder="App"
          value={app}
          onChange={(e) => { setApp(e.target.value); setOffset(0); }}
          style={{ flex: "1 1 140px", fontSize: 13 }}
        />

        <select
          className="select"
          value={stream}
          onChange={(e) => { setStream(e.target.value as "" | "stdout" | "stderr"); setOffset(0); }}
          style={{ flex: "0 0 auto" }}
        >
          <option value="">All streams</option>
          <option value="stdout">stdout</option>
          <option value="stderr">stderr</option>
        </select>

        <div style={{ position: "relative", flex: "2 1 200px" }}>
          <input
            className="input"
            placeholder="Search logs…"
            value={search}
            onChange={(e) => { setSearch(e.target.value); setOffset(0); }}
            style={{ paddingLeft: 32, fontSize: 13 }}
          />
          <Search
            size={12}
            style={{
              position: "absolute",
              left: 10,
              top: "50%",
              transform: "translateY(-50%)",
              color: "var(--text-muted)",
            }}
          />
          {search && (
            <button
              onClick={() => setSearch("")}
              style={{
                position: "absolute",
                right: 8,
                top: "50%",
                transform: "translateY(-50%)",
                background: "none",
                border: "none",
                cursor: "pointer",
                color: "var(--text-muted)",
                display: "flex",
              }}
            >
              <X size={12} />
            </button>
          )}
        </div>

        {!live && (
          <TimeRangeSelector value={range} onChange={(v) => { setRange(v); setOffset(0); }} />
        )}
      </div>

      {/* Error */}
      {error && (
        <div className="alert alert-error" style={{ marginBottom: 12, flexShrink: 0 }}>
          <AlertCircle size={14} />
          {error}
        </div>
      )}

      {/* Paused banner */}
      {live && paused && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "8px 14px",
            background: "var(--warning-bg)",
            border: "1px solid rgba(240,180,41,0.2)",
            borderRadius: "var(--radius-sm)",
            marginBottom: 8,
            flexShrink: 0,
          }}
        >
          <span style={{ fontSize: 12, color: "var(--warning)" }}>
            Paused · {buffered.length} new line{buffered.length !== 1 ? "s" : ""} buffered
          </span>
          <button
            className="btn btn-sm"
            onClick={resumeLive}
            style={{ color: "var(--warning)", borderColor: "var(--warning)", fontSize: 11 }}
          >
            <Play size={11} />
            Resume
          </button>
        </div>
      )}

      {/* Log viewport */}
      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="card"
        style={{
          flex: 1,
          overflow: "auto",
          padding: "8px 0",
          minHeight: 0,
        }}
      >
        {loading && !live && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              height: 120,
              color: "var(--text-muted)",
              fontSize: 13,
            }}
          >
            Loading logs…
          </div>
        )}

        {!loading && allLogs.length === 0 && (
          <div className="empty-state">
            <ScrollText size={36} className="empty-icon" />
            <div className="empty-title">No logs found</div>
            <p className="empty-desc">
              {live
                ? "Waiting for live log data…"
                : "Try adjusting your filters or time range."}
            </p>
          </div>
        )}

        {allLogs.map((log, idx) => (
          <div
            key={idx}
            style={{
              display: "grid",
              gridTemplateColumns: "130px 90px 100px 1fr",
              gap: "0 12px",
              padding: "3px 16px",
              fontFamily: "var(--font-mono)",
              fontSize: 12,
              lineHeight: 1.6,
              borderBottom: "1px solid var(--border-subtle)",
              color: log.stream === "stderr" ? "var(--error)" : "var(--text-primary)",
            }}
          >
            <span style={{ color: "var(--text-muted)", flexShrink: 0 }}>
              {new Date(log.timestamp).toLocaleTimeString(undefined, {
                hour12: false,
                hour: "2-digit",
                minute: "2-digit",
                second: "2-digit",
              })}
            </span>
            <span style={{ color: "var(--info)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {log.hostname}
            </span>
            <span
              style={{
                color: "var(--text-secondary)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {log.app}
            </span>
            <span
              style={{
                color: log.stream === "stderr" ? "var(--error)" : "var(--text-primary)",
                wordBreak: "break-all",
              }}
            >
              {log.line}
            </span>
          </div>
        ))}
      </div>

      {/* Pagination */}
      {!live && total > PAGE_SIZE && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "10px 0",
            flexShrink: 0,
          }}
        >
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            Showing {offset + 1}–{Math.min(offset + PAGE_SIZE, total)} of {total.toLocaleString()}
          </span>
          <div style={{ display: "flex", gap: 6 }}>
            <button
              className="btn btn-secondary btn-sm"
              onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
              disabled={offset === 0}
            >
              <ChevronLeft size={12} />
              Prev
            </button>
            <button
              className="btn btn-secondary btn-sm"
              onClick={() => setOffset(offset + PAGE_SIZE)}
              disabled={offset + PAGE_SIZE >= total}
            >
              Next
              <ChevronRight size={12} />
            </button>
          </div>
        </div>
      )}

      {/* Live pause button (floating) */}
      {live && !paused && (
        <div style={{ display: "flex", justifyContent: "center", padding: "8px 0", flexShrink: 0 }}>
          <button
            className="btn btn-secondary btn-sm"
            onClick={() => setPaused(true)}
          >
            <Pause size={12} />
            Pause
          </button>
        </div>
      )}
    </div>
  );
}
