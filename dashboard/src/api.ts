// ─── Shared helpers ────────────────────────────────────────────────────────────

async function handleJSON(res: Response) {
  const text = await res.text();
  try {
    return text ? JSON.parse(text) : null;
  } catch {
    return text;
  }
}

// ─── Types ──────────────────────────────────────────────────────────────────────

export type Agent = {
  id: string;
  token_id: string;
  hostname: string;
  ip?: string | null;
  os: string;
  agent_version: string;
  log_files: string[] | null;
  first_seen_at: string;
  last_seen_at: string;
  online: boolean;
};

export type AgentToken = {
  id: string;
  name: string;
  created_by: string;
  created_at: string;
  last_used_at?: string | null;
};

export type User = {
  id: string;
  username: string;
  email?: string | null;
  role: "admin" | "member";
  failed_login_attempts: number;
  auth_locked_until?: string | null;
  last_login_at?: string | null;
  created_at: string;
  updated_at: string;
};

export type MetricPoint = {
  timestamp: number; // unix seconds
  value: number;
};

export type MetricsData = {
  cpu: MetricPoint[];
  memory: MetricPoint[];
  memory_total?: MetricPoint[];
  memory_used?: MetricPoint[];
  disk: MetricPoint[];
  disk_total?: MetricPoint[];
  disk_used?: MetricPoint[];
  network_rx?: MetricPoint[];
  network_tx?: MetricPoint[];
  load_avg?: MetricPoint[];
};

export type LogEntry = {
  timestamp: string;
  hostname: string;
  app: string;
  file?: string;
  stream: "stdout" | "stderr";
  line: string;
};

export type AlertRule = {
  id: string;
  name: string;
  metric: string;
  operator: ">" | "<" | ">=" | "<=";
  threshold: number;
  duration_seconds: number;
  hosts: string[];
  channels: string[];
  enabled: boolean;
  created_by: string;
  created_at: string;
  updated_at: string;
};

export type AlertEvent = {
  id: string;
  rule_id: string;
  hostname: string;
  status: "firing" | "resolved";
  value?: number | null;
  fired_at: string;
  resolved_at?: string | null;
};

export type NotificationChannel = {
  id: string;
  name: string;
  channel_type: "discord" | "slack" | "smtp";
  config: Record<string, string>;
  created_by: string;
  created_at: string;
  updated_at: string;
};

// ─── Auth ───────────────────────────────────────────────────────────────────────

export async function setup(username: string, password: string) {
  const res = await fetch(`/api/auth/setup`, {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as { access_token: string };
}

export async function login(username: string, password: string) {
  const res = await fetch(`/api/auth/login`, {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as { access_token: string };
}

let refreshPromise: Promise<{ access_token: string }> | null = null;

export async function refresh() {
  if (refreshPromise) return refreshPromise;
  
  refreshPromise = (async () => {
    try {
      const res = await fetch(`/api/auth/refresh`, {
        method: "POST",
        credentials: "include",
      });
      const data = await handleJSON(res);
      if (!res.ok) throw new Error(data?.message || res.statusText);
      return data as { access_token: string };
    } finally {
      refreshPromise = null;
    }
  })();
  
  return refreshPromise;
}

export async function logout() {
  const res = await fetch(`/api/auth/logout`, {
    method: "POST",
    credentials: "include",
  });
  if (!res.ok) throw new Error("Logout failed");
}

// Check if any user exists (if 404 or specific response → redirect to setup)
export async function checkSetup(): Promise<{ needs_setup: boolean }> {
  try {
    const res = await fetch(`/api/auth/setup`, { method: "GET" });
    if (res.status === 404) return { needs_setup: false }; // endpoint exists, users exist
    const data = await handleJSON(res);
    return { needs_setup: data?.needs_setup ?? false };
  } catch {
    return { needs_setup: false };
  }
}

// ─── Agents ─────────────────────────────────────────────────────────────────────

export async function getAgents(accessToken: string) {
  const res = await fetch(`/api/agents`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as Agent[];
}

export async function deleteAgent(accessToken: string, id: string) {
  const res = await fetch(`/api/agents/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
}

// ─── Agent Tokens ────────────────────────────────────────────────────────────────

export async function listAgentTokens(accessToken: string) {
  const res = await fetch(`/api/agents/tokens`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as AgentToken[];
}

export async function createAgentToken(accessToken: string, name: string) {
  const res = await fetch(`/api/agents/tokens`, {
    method: "POST",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify({ name }),
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as { token_id: string; raw_token: string };
}

export async function revokeAgentToken(accessToken: string, id: string) {
  const res = await fetch(`/api/agents/tokens/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
}

// ─── Metrics ────────────────────────────────────────────────────────────────────

function parseVMResponse(data: any): MetricPoint[] {
  if (!data || data.status !== "success" || !data.data || !data.data.result || data.data.result.length === 0) {
    return [];
  }
  return data.data.result[0].values.map((v: [number, string]) => ({
    timestamp: v[0],
    value: parseFloat(v[1])
  }));
}

export async function getMetrics(
  accessToken: string,
  hostname: string,
  timeRange: string,
): Promise<MetricsData> {
  const metricsList = ["cpu", "memory", "memory_total", "memory_used", "disk", "disk_total", "disk_used", "network_rx", "network_tx", "load_avg"];
  const promises = metricsList.map(async (metric) => {
    const params = new URLSearchParams({ host: hostname, range: timeRange, metric });
    const res = await fetch(`/api/metrics?${params}`, {
      headers: { Authorization: `Bearer ${accessToken}` },
    });
    const data = await handleJSON(res);
    if (!res.ok) throw new Error(data?.message || res.statusText);
    return parseVMResponse(data);
  });

  const [cpu, memory, memory_total, memory_used, disk, disk_total, disk_used, network_rx, network_tx, load_avg] = await Promise.all(promises);
  return { cpu, memory, memory_total, memory_used, disk, disk_total, disk_used, network_rx, network_tx, load_avg };
}

// ─── Logs ───────────────────────────────────────────────────────────────────────

export type LogQuery = {
  hostname?: string;
  app?: string;
  stream?: "stdout" | "stderr";
  search?: string;
  range?: string;
  from?: string;
  to?: string;
  limit?: number;
  offset?: number;
};

export async function getLogs(
  accessToken: string,
  query: LogQuery = {},
): Promise<{ logs: LogEntry[]; total: number }> {
  const params = new URLSearchParams();
  if (query.hostname) params.append("host", query.hostname);
  if (query.app) params.append("app", query.app);
  if (query.stream) params.append("stream", query.stream);
  if (query.search) params.append("search", query.search);
  if (query.range) params.append("range", query.range);
  if (query.from) params.append("from", query.from);
  if (query.to) params.append("to", query.to);
  if (query.limit) params.append("limit", String(query.limit));
  if (query.offset) params.append("offset", String(query.offset));

  const res = await fetch(`/api/logs?${params}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  // Server may return array directly or wrapped object
  if (Array.isArray(data)) return { logs: data, total: data.length };
  return data as { logs: LogEntry[]; total: number };
}

// ─── Users ──────────────────────────────────────────────────────────────────────

export async function getMe(accessToken: string) {
  const res = await fetch(`/api/users/me`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as User;
}

export async function updateMe(
  accessToken: string,
  body: { username?: string; email?: string },
) {
  const res = await fetch(`/api/users/me`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(body),
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as User;
}

export async function changePassword(
  accessToken: string,
  current_password: string,
  new_password: string,
) {
  const res = await fetch(`/api/users/me/password`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify({ current_password, new_password }),
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
}

export async function listUsers(accessToken: string) {
  const res = await fetch(`/api/users`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as User[];
}

export async function createUser(
  accessToken: string,
  body: { username: string; email?: string; password: string; role: "admin" | "member" },
) {
  const res = await fetch(`/api/users`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(body),
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as User;
}

export async function updateUser(
  accessToken: string,
  id: string,
  body: { username?: string; email?: string; role?: "admin" | "member" },
) {
  const res = await fetch(`/api/users/${encodeURIComponent(id)}`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(body),
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as User;
}

export async function deleteUser(accessToken: string, id: string) {
  const res = await fetch(`/api/users/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
}

// ─── Alerts ─────────────────────────────────────────────────────────────────────

export async function listAlertRules(accessToken: string) {
  const res = await fetch(`/api/alerts/rules`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as AlertRule[];
}

export async function listAlertEvents(accessToken: string) {
  const res = await fetch(`/api/alerts/events`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as AlertEvent[];
}

export async function listNotificationChannels(accessToken: string) {
  const res = await fetch(`/api/alerts/channels`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const data = await handleJSON(res);
  if (!res.ok) throw new Error(data?.message || res.statusText);
  return data as NotificationChannel[];
}

// ─── WebSocket live log tail ─────────────────────────────────────────────────────

export function openLogTail(
  hostname: string,
  app: string,
  onLine: (entry: LogEntry) => void,
): WebSocket {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const params = new URLSearchParams();
  if (hostname && hostname !== "*") params.append("host", hostname);
  if (app && app !== "*") params.append("app", app);
  const ws = new WebSocket(`${proto}://${location.host}/api/ws/logs?${params}`);
  ws.onmessage = (ev) => {
    try {
      const data = JSON.parse(ev.data);
      onLine(data as LogEntry);
    } catch {
      /* ignore parse errors */
    }
  };
  return ws;
}
