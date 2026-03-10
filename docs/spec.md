# Raven - Implementation Specification

> Full design details, implementation phases, user stories, testing strategy, and key decisions.
>
> For the architecture overview and tech stack, see the [technical documentation](./README.md).

---

## Table of Contents

- [1. Project Scope](#1-project-scope)
- [2. Detailed Component Design](#2-detailed-component-design)
  - [2.1 Agent](#21-agent)
  - [2.2 Central Server](#22-central-server)
    - [Application Database (SQLite)](#application-database-sqlite)
  - [2.3 Dashboard](#23-dashboard)
- [3. Implementation Phases](#3-implementation-phases)
- [4. Stretch Goals](#4-stretch-goals)
- [5. Verification & Testing](#5-verification--testing)
- [6. Pre-Project Reading List](#6-pre-project-reading-list)
- [7. Key Decisions](#7-key-decisions)
- [8. User Stories](#8-user-stories)

---

## 1. Project Scope

**In scope:**

- System metrics collection: CPU, memory, disk I/O, network, load average
- Application log tailing: stdout/stderr from Docker containers and PM2-managed apps
- Centralized storage with time-range queries and full-text log search
- Live log tailing in the browser via WebSocket
- Alerting with Discord, Slack, and SMTP notifications
- Self-hostable: single Docker Compose file, zero external dependencies
- Multi-host: one central server, many agents

**Out of scope:**

- APM / distributed tracing
- Container orchestration monitoring (Kubernetes pod metrics, etc.)
- Custom application-level metrics via Unix sockets (stretch goal)
- Auto-discovery of Docker containers (stretch goal)

---

## 2. Detailed Component Design

### 2.1 Agent

The agent is a Linux daemon that collects system metrics and tails application log files, then streams everything to the central server.

#### System Metrics Collection

Reads the following from `/proc` using the `procfs` crate:

| Metric | Source | Computation |
|---|---|---|
| CPU usage (%) | `/proc/stat` | Delta of jiffies between samples, per-core and total |
| Memory (total, used, available, swap) | `/proc/meminfo` | Direct read |
| Disk I/O (read/write bytes/sec, IOPS) | `/proc/diskstats` | Delta between samples |
| Network (bytes in/out per interface) | `/proc/net/dev` | Delta between samples |
| Load average (1m, 5m, 15m) | `/proc/loadavg` | Direct read |

Collection interval: configurable, default 10 seconds.

#### Log Tailing

Uses the `inotify` crate for kernel-level file watching:

- Watches configured file paths for `IN_MODIFY` events.
- Maintains a seek offset per file. On modify: read new bytes, split by newline, package into `LogBatch`.
- **Log rotation handling**: Detect `IN_MOVE_SELF` / `IN_DELETE_SELF` + `IN_CREATE` events. Reopen the file and reset the seek offset.
- **Bounded channel**: A `tokio::sync::mpsc` channel (capacity 4096) sits between the tailer and the gRPC sender. If the channel is full (sender can't keep up), oldest entries are dropped. The tailer never blocks.

Supported log formats:

| Format | Source | Parsing |
|---|---|---|
| `plain` | PM2 logs (`~/.pm2/logs/*.log`), generic apps, nginx | Raw text lines, no transformation |
| `docker-json` | Docker container logs (`/var/lib/docker/containers/<id>/<id>-json.log`) | Parse JSON envelope: `{"log":"...","stream":"stdout","time":"..."}` — extract log line, stream type, and timestamp |

#### gRPC Transport

- Two client-streaming RPCs: `StreamMetrics(stream MetricBatch)` and `StreamLogs(stream LogBatch)`.
- **Batching**: Flush every 100 items or every 5 seconds, whichever comes first.
- **Authentication**: Pre-shared token sent as gRPC metadata (`authorization: Bearer rvn_...`).
- **TLS**: Encrypted via `rustls` through `tonic-transport`.
- **Reconnection**: Exponential backoff on disconnect (1s → 2s → 4s → ... → max 60s).
- **WAL (Write-Ahead Log)**: On disconnect, buffer batches to a local file on disk. On reconnect, replay WAL data first (no gaps in timeline), then resume live streaming. WAL has a configurable size cap (default 100 MB) — oldest entries dropped when full.
- **Heartbeat**: Periodic ping every 30 seconds. Server marks agent as offline if no heartbeat for 90 seconds.

#### Agent Configuration

File: `/etc/raven/agent.toml` (default, overridable with `--config` flag).

```toml
[server]
address = "raven.example.com:9090"
token = "rvn_a1b2c3d4e5f6..."
tls = true

[metrics]
interval_seconds = 10        # how often to read /proc (default: 10)

[transport]
batch_size = 100             # flush after this many items (default: 100)
flush_interval_seconds = 5   # or flush after this many seconds, whichever comes first (default: 5)
retry_max_interval_seconds = 60  # max backoff on disconnect (default: 60)
wal_max_size_mb = 100        # max disk WAL size before dropping oldest (default: 100)

[[logs]]
name = "nginx-access"
path = "/var/log/nginx/access.log"
format = "plain"

[[logs]]
name = "my-api"
path = "/var/lib/docker/containers/abc123*/abc123*-json.log"
format = "docker-json"

[[logs]]
name = "worker"
path = "/home/deploy/.pm2/logs/worker-out.log"
format = "plain"
```

#### Agent System User

The agent runs as a dedicated `raven` system user (not root):

```
useradd --system --no-create-home --shell /usr/sbin/nologin raven
usermod -aG adm raven       # read /var/log/* (nginx, syslog)
usermod -aG docker raven    # read Docker container logs
```

The systemd unit runs as this user:

```ini
[Service]
User=raven
Group=raven
ExecStart=/usr/local/bin/raven-agent --config /etc/raven/agent.toml
Restart=always
RestartSec=5
```

Config file permissions: `root:raven 640` — root can edit, agent can read, others cannot (protects the token).

---

### 2.2 Central Server

The central server receives data from agents, stores it in the appropriate databases, serves the HTTP API for the dashboard, and runs the alerting engine.

#### gRPC Ingestion

- Accepts `StreamMetrics` and `StreamLogs` client-streaming RPCs from agents.
- Validates the bearer token on connection (looks up hash in SQLite). Rejects with `UNAUTHENTICATED` if invalid.
- On first connection, the agent sends a `Register` message (hostname, OS, agent version, configured log files). Server upserts this into the `agents` table in SQLite.
- Routes data to storage:
  - `MetricBatch` → VictoriaMetrics via Prometheus remote-write HTTP API (`POST /api/v1/import/prometheus`).
  - `LogBatch` → ClickHouse via HTTP interface.
- If database writes are slow, the server slows down reading from the gRPC stream, applying backpressure to the agent.

#### ClickHouse Schema

```sql
CREATE TABLE logs (
    timestamp DateTime64(3),
    hostname  String,
    app       String,
    file      String,
    stream    Enum8('stdout' = 1, 'stderr' = 2),
    line      String,
    INDEX idx_line line TYPE tokenbf_v1(10240, 3, 0) GRANULARITY 4
) ENGINE = MergeTree()
ORDER BY (hostname, app, timestamp)
TTL timestamp + INTERVAL 30 DAY
```

The `tokenbf_v1` bloom filter index enables fast full-text search on log content.

#### Application Database (SQLite)

All user-facing CRUD data lives in an embedded SQLite database (`raven.db`), managed via `sqlx` with compile-time checked queries. SQLite is chosen over PostgreSQL to avoid an extra container in `docker-compose.yml` — consistent with the zero-external-dependencies philosophy. The file lives on a Docker volume for persistence.

Auto-migrated on first boot via `sqlx::migrate!`.

```sql
-- Dashboard users
CREATE TABLE users (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    username      TEXT NOT NULL UNIQUE,
    email         TEXT,
    password_hash TEXT NOT NULL,            -- Argon2id
    role          TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('admin', 'member')),
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Agent authentication tokens
CREATE TABLE agent_tokens (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    name          TEXT NOT NULL,            -- human label, e.g. "web-server-1"
    token_hash    TEXT NOT NULL UNIQUE,     -- SHA-256 of "rvn_..."
    created_by    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_used_at  TEXT                      -- updated on each agent heartbeat
);

-- Registered agents (upserted on Register RPC)
CREATE TABLE agents (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    token_id      TEXT NOT NULL REFERENCES agent_tokens(id) ON DELETE CASCADE,
    hostname      TEXT NOT NULL,
    ip            TEXT,
    os            TEXT,
    agent_version TEXT,
    log_files     TEXT,                     -- JSON array of configured log file names
    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_seen_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Alert rules
CREATE TABLE alert_rules (
    id                TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    name              TEXT NOT NULL,
    metric            TEXT NOT NULL,         -- e.g. "cpu_usage", "mem_usage", "disk_usage"
    operator          TEXT NOT NULL CHECK (operator IN ('>', '<', '>=', '<=')),
    threshold         REAL NOT NULL,
    duration_seconds  INTEGER NOT NULL,
    hosts             TEXT NOT NULL DEFAULT '[]',  -- JSON array, empty = all hosts
    channels          TEXT NOT NULL DEFAULT '[]',  -- JSON array of channel IDs
    enabled           INTEGER NOT NULL DEFAULT 1,
    created_by        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Notification channels
CREATE TABLE notification_channels (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    name          TEXT NOT NULL,
    channel_type  TEXT NOT NULL CHECK (channel_type IN ('discord', 'slack', 'smtp')),
    config        TEXT NOT NULL,             -- JSON: webhook_url, or smtp_host/port/user/pass/from/to
    created_by    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Alert history (fired/resolved events)
CREATE TABLE alert_events (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    rule_id       TEXT NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    hostname      TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('firing', 'resolved')),
    value         REAL,
    fired_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    resolved_at   TEXT
);
```

**Data ownership model:**

- **Agent tokens** are owned by the user who created them (`created_by`). Admins can see all tokens; members see only their own.
- **Agents** are associated with their token (`token_id`), which transitively links them to the user.
- **Alert rules** and **notification channels** are owned by the creating user. Admins can view/edit all; members manage only their own.
- **Alert events** are system-generated and visible to all authenticated users.

**Runtime state vs. persisted state:**

- The `agents` table stores registration metadata (hostname, OS, version, log files) and `last_seen_at`. This is persisted.
- Online/offline status and in-memory heartbeat tracking are held in a `DashMap<String, AgentState>` in the server process. On startup, all agents are loaded from SQLite and marked offline until their first heartbeat arrives.
- Alert rule state machine (`OK`, `Pending`, `Firing`) is held in memory. On restart, all rules start in `OK` — the evaluation loop will detect ongoing threshold violations within one cycle (30s).

#### HTTP API

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/auth/setup` | POST | First-time admin account creation (returns 409 if any user exists) |
| `/api/auth/login` | POST | Authenticate with username + password, return JWT |
| `/api/users` | GET | List all users (admin only) |
| `/api/users` | POST | Invite / create a new user (admin only) |
| `/api/users/:id` | GET | Get user profile (admin or self) |
| `/api/users/:id` | PUT | Update user (admin or self — admin can change roles) |
| `/api/users/:id` | DELETE | Delete user (admin only, cannot delete self) |
| `/api/users/me` | GET | Current authenticated user profile |
| `/api/users/me` | PUT | Update own profile (username, email) |
| `/api/users/me/password` | PUT | Change own password (requires current password) |
| `/api/agents` | GET | List registered agents + live status |
| `/api/agents/tokens` | GET | List agent tokens (admin: all, member: own) |
| `/api/agents/tokens` | POST | Generate new agent token |
| `/api/agents/tokens/:id` | DELETE | Revoke an agent token |
| `/api/metrics` | GET | Query metrics from VictoriaMetrics |
| `/api/logs` | GET | Query logs from ClickHouse |
| `/api/ws/logs` | WebSocket | Live log tail |
| `/api/alerts/rules` | GET/POST | List or create alert rules (scoped to user; admin sees all) |
| `/api/alerts/rules/:id` | PUT/DELETE | Update or delete an alert rule |
| `/api/alerts/channels` | GET/POST | List or create notification channels (scoped to user) |
| `/api/alerts/channels/:id` | PUT/DELETE | Update or delete a notification channel |
| `/api/alerts/events` | GET | List alert history (firing/resolved events) |
| `/api/alerts/test` | POST | Send a test notification to a channel |

**Metrics query parameters:**

```
GET /api/metrics?host=web-1&metric=cpu&range=5m
GET /api/metrics?host=web-1&metric=cpu&from=2026-03-01T00:00:00Z&to=2026-03-02T00:00:00Z
```

Supported shorthand ranges: `5m`, `15m`, `1h`, `6h`, `24h`, `7d`. The server translates to absolute timestamps and adjusts the query `step` for appropriate downsampling.

**Log query parameters:**

```
GET /api/logs?host=web-1&app=my-api&search=error&range=1h&limit=1000
GET /api/logs?host=web-1&from=2026-03-01T00:00:00Z&to=2026-03-02T00:00:00Z
```

#### Live Log Tail

1. Dashboard opens a WebSocket to `/api/ws/logs?host=web-1&app=my-api`.
2. Server registers the WebSocket as a subscriber to a `tokio::sync::broadcast` channel.
3. When `LogBatch` arrives from the matching agent via gRPC, the server publishes it to the broadcast channel.
4. Matching lines are forwarded to the WebSocket in real time.
5. If nobody is subscribed, broadcast data is dropped — zero cost.

#### Alerting Engine

**Alert rule model:**

```json
{
  "id": "a1b2c3d4...",
  "name": "High CPU on web-1",
  "metric": "cpu_usage",
  "operator": ">",
  "threshold": 90.0,
  "duration_seconds": 300,
  "hosts": ["web-1"],
  "channels": ["discord-ops-id", "email-oncall-id"],
  "enabled": true,
  "created_by": "user-id"
}
```

Stored in the `alert_rules` SQLite table. `hosts` and `channels` are JSON arrays.

**Evaluation loop:**

- Background `tokio` task runs every 30 seconds.
- For each rule: query VictoriaMetrics — "has metric X exceeded threshold for the last N seconds?"
- State machine per rule: `OK → Pending → Firing → Resolved`.
- Notify only on state transitions (`Pending → Firing` and `Firing → Resolved`). Prevents spam.

**Notification channels:**

| Channel | Implementation | Configuration |
|---|---|---|
| Discord | HTTP POST to webhook URL with embed payload | Webhook URL |
| Slack | HTTP POST to Incoming Webhook URL with Block Kit payload | Webhook URL |
| SMTP | `lettre` crate, send HTML email | SMTP host, port, username, password, from/to addresses |

---

### 2.3 Dashboard

React + Vite application built with Bun.

Dashboard assets are compiled to `dashboard/dist/` and served directly by `raven-server` via axum static file routes.
No external dashboard hosting is required.

#### Pages

**Login**: JWT authentication. Token stored in httpOnly cookie.

**Agents Overview**: Grid of cards, one per monitored host. Each card shows:
- Hostname and IP
- Online/offline badge (based on heartbeat)
- Last seen timestamp
- CPU, memory, disk usage sparkline charts
- Click to navigate to host detail

**Host Detail**: Full-page view for a single host:
- Time range picker: `5m | 15m | 1h | 6h | 24h | 7d | Custom`
- Time-series charts (ECharts): CPU usage, memory usage, disk I/O, network throughput, load average
- Auto-refresh on configurable interval
- Quick link to logs for this host

**Log Explorer**:
- Filters: hostname, app name, stream (stdout/stderr), time range, search text
- Searchable table with timestamp, host, app, stream, log line
- Syntax-highlighted log lines, stderr lines in red
- "Live Tail" toggle: opens WebSocket, new lines appear at bottom with auto-scroll
- Pause button to freeze auto-scroll without losing incoming data (buffered client-side)
- Pagination for historical queries

**Alerts**:
- List of active and resolved alerts with timestamps
- CRUD interface for alert rules (metric, threshold, duration, hosts, channels)
- CRUD interface for notification channels (Discord, Slack, SMTP)
- "Test" button to send a test notification to verify channel config

**Design**: Responsive layout with Tailwind CSS. Dark mode supported.

---

## 3. Implementation Phases

### Phase 0 — Scaffolding & Protobuf (Days 1-3)

1. Create Cargo workspace with crates: `raven-agent`, `raven-server`, `raven-proto`.
2. Define `.proto` files:
   - `MetricBatch`: hostname, timestamp, CPU struct, memory struct, disk struct, network struct, load average.
   - `LogBatch`: hostname, app name, file path, lines (repeated), timestamps (repeated), stream type.
   - `Register`: hostname, OS, agent version, configured log files.
   - `Heartbeat`: hostname, timestamp.
   - Service definition: `StreamMetrics`, `StreamLogs`, `Register`, `Heartbeat` RPCs.
3. Set up `tonic-build` in `raven-proto/build.rs` for codegen.
4. Scaffold React + Vite app in `dashboard/` using Bun.
5. Configure Vite build output and axum static serving (`ServeDir`) so `raven-server` serves `dashboard/dist` at `/`.
6. Create `docker-compose.yml` with `raven-server`, VictoriaMetrics, and ClickHouse. Verify services start and are reachable.
7. Init Git repo. Set up GitHub Actions CI (clippy + test + build + dashboard build with Bun).

### Phase 1 — Agent: System Metrics Collection

1. Build the `/proc` reader module using the `procfs` crate.
2. Parse `/proc/stat` (CPU jiffies), compute per-core and total usage % from deltas between samples.
3. Parse `/proc/meminfo` for total, available, used, swap, buffer/cache.
4. Parse `/proc/diskstats` for read/write bytes/sec and IOPS (delta-based).
5. Parse `/proc/net/dev` for bytes in/out per interface (delta-based).
6. Parse `/proc/loadavg` for 1m, 5m, 15m load averages.
7. Implement collection loop on configurable interval (default 10s) using `tokio::time::interval`.
8. Serialize into protobuf `MetricBatch`, push to internal `tokio::sync::mpsc` channel.
9. Unit tests: snapshot `/proc` file contents, verify parsed output matches expected values.

### Phase 2 — Agent: Log Tailing

1. Build the `inotify` watcher module. Watch configured file paths for `IN_MODIFY` events.
2. Maintain a seek offset per file. On modify → read new bytes → split by newline → wrap in `LogBatch`.
3. Implement `plain` format parser (raw text lines).
4. Implement `docker-json` format parser (unwrap `{"log":"...","stream":"...","time":"..."}` envelope).
5. Handle log rotation: detect `IN_MOVE_SELF` / `IN_DELETE_SELF` + `IN_CREATE` → reopen file, reset offset.
6. Bounded `mpsc` channel (capacity 4096) between tailer and gRPC sender. Drop oldest on overflow.
7. Support glob patterns in file paths (e.g., `/var/lib/docker/containers/abc123*/*.log`).
8. Tests: write to temp files → verify capture; rotate file → verify continuity; test both format parsers.

### Phase 3 — Transport: gRPC Streaming

1. Implement `tonic` gRPC client in the agent.
2. `Register` RPC: send hostname, OS, agent version on initial connect.
3. `StreamMetrics` and `StreamLogs` as client-streaming RPCs.
4. Batch + flush logic: flush every 100 items or every 5 seconds, whichever comes first.
5. Authentication: send token as gRPC metadata on every RPC.
6. TLS via `tonic-transport` + `rustls`.
7. Reconnection with exponential backoff (1s → 2s → 4s → ... → 60s max).
8. On-disk WAL: on disconnect, append batches to a local file. On reconnect, replay WAL then resume live. Size cap: 100 MB, drop oldest on overflow.
9. Heartbeat: send periodic ping every 30 seconds.
10. Integration test: start a mock gRPC server, verify agent connects, authenticates, and streams data.

### Phase 4 — Central Server: Ingestion & Application Database

1. Set up SQLite database with `sqlx`. Embed migrations in the binary via `sqlx::migrate!`. Auto-run on first boot.
2. Create SQLite schema: `users`, `agent_tokens`, `agents`, `alert_rules`, `notification_channels`, `alert_events` tables.
3. Implement `tonic` gRPC server: accept `Register`, `StreamMetrics`, `StreamLogs`, `Heartbeat`.
4. Validate bearer token on each connection — hash incoming token, look up in `agent_tokens` table. Reject invalid tokens.
5. Agent registry: upsert agent info into `agents` table on `Register`. Track live status in memory (`DashMap`). Update `last_seen_at` on heartbeat.
6. Route `MetricBatch` → VictoriaMetrics via Prometheus remote-write API (`POST /api/v1/import/prometheus`) using `reqwest`.
7. Route `LogBatch` → ClickHouse via HTTP interface. Auto-create the `logs` table on first boot (migration).
8. Backpressure: if DB writes are slow, slow gRPC stream reads.
9. Broadcast channel: publish incoming `LogBatch` to `tokio::sync::broadcast` for live tail subscribers.
10. Integration test: spin up server + databases via Docker Compose, stream synthetic data, verify it lands in both DBs.

### Phase 5 — Central Server: Query API

1. Build `axum` HTTP API with all endpoints listed in section 2.2.
2. Serve dashboard static assets (`dashboard/dist`) from axum at `/` and configure SPA fallback to `index.html`.
3. `POST /api/auth/setup`: first-time admin account creation. Hash password with Argon2id, insert into `users` table. Return 409 if any user already exists.
4. `POST /api/auth/login`: validate username + password against `users` table (Argon2id verify). Return JWT containing `user_id`, `role`, `exp`.
5. JWT auth middleware: extract and validate JWT from `Authorization` header or httpOnly cookie. Inject user context into request extensions.
6. User CRUD endpoints: `GET/POST /api/users` (admin only), `GET/PUT/DELETE /api/users/:id`, `GET/PUT /api/users/me`, `PUT /api/users/me/password`.
7. Agent token management: `GET/POST /api/agents/tokens`, `DELETE /api/agents/tokens/:id`. Tokens scoped to creating user (admin sees all). Store SHA-256 hash in SQLite, return raw token only on creation.
8. Agents endpoint: return registered agents from SQLite + live status from in-memory registry.
9. Metrics endpoint: proxy to VictoriaMetrics `/api/v1/query_range`. Translate shorthand ranges (`5m`, `1h`, `7d`) to absolute timestamps. Adjust `step` parameter for downsampling.
10. Logs endpoint: query ClickHouse with hostname, app, time range, search text filters. Paginate results.
11. WebSocket endpoint for live log tail: register subscriber on broadcast channel, filter by host/app, forward matching lines.
12. CORS configuration for dashboard origin.
13. Test: full request cycle — create user, create token, connect agent, push data, query via API, verify results.

### Phase 6 — Alerting Engine

1. Alert rule model: metric, operator, threshold, duration, hosts, channels. Stored in SQLite `alert_rules` table.
2. Notification channel model: type (discord/slack/smtp), configuration (webhook URL or SMTP settings). Stored in SQLite `notification_channels` table.
3. Alert event history: firing/resolved events stored in SQLite `alert_events` table.
3. Evaluation loop: background tokio task, runs every 30 seconds. For each rule, query VictoriaMetrics for recent data, evaluate threshold.
4. State machine per rule: `OK → Pending → Firing → Resolved`. Notify only on transitions.
5. Discord notifier: POST to webhook URL with JSON embed (title, description, color, fields for host/metric/value/timestamp).
6. Slack notifier: POST to webhook URL with Block Kit payload.
7. SMTP notifier: `lettre` crate, send HTML email with alert details.
8. CRUD API endpoints for rules and channels.
9. Test endpoint: send a test notification to verify channel configuration.
10. Test: create a rule, push metrics exceeding threshold, verify notification fires and state transitions correctly.

### Phase 7 — Dashboard

1. **Login page**: First visit → `POST /api/auth/setup` flow if no users exist (create admin). Otherwise, login form → `POST /api/auth/login` → store JWT → redirect.
2. **Agents overview page**: Fetch `GET /api/agents`. Cards per host with CPU/mem/disk sparklines (ECharts). Online/offline badge. Last seen. Click to drill down.
3. **Host detail page**: Time range picker (`5m | 15m | 1h | 6h | 24h | 7d | Custom`). Full-width charts for CPU, memory, disk I/O, network, load average. Auto-refresh via TanStack Query refetch interval.
4. **Log explorer page**: Filter bar (hostname, app, stream, search text, time range). Log table with syntax highlighting. stderr lines in red. "Live Tail" toggle opens WebSocket — new lines auto-scroll at bottom. Pause button buffers without losing data. Pagination for historical queries.
5. **Alerts page**: Active/resolved alert list (from `GET /api/alerts/events`). CRUD forms for rules and notification channels. Test button.
6. **Settings page**: Agent token management (generate, list, revoke). User profile (edit username, email, password). Admin panel: user management (list, invite, change roles, delete).
7. Responsive layout, Tailwind CSS, dark mode.

### Phase 8 — Packaging & Polish

1. Multi-stage Dockerfiles: `rust:slim` builder → `debian:bookworm-slim` runtime for server. Minimal image for agent.
2. Final `docker-compose.yml` with `raven-server`, VictoriaMetrics, and ClickHouse services, volumes, and health checks.
3. Agent install script (`install.sh`): detect arch, download binary, create config, create user, install systemd service.
4. README: architecture diagram, setup instructions, screenshots, configuration reference.
5. CI: clippy, tests, cross-compile `x86_64` + `aarch64` agent binaries, build + push Docker images to GHCR, attach binaries to GitHub Release on tag.

---

## 4. Stretch Goals

If time permits after core phases are complete:

1. **Unix Socket custom metrics intake**: Let apps push application-level metrics (request count, latency, queue depth) to the agent via a non-blocking `UnixDatagram` socket at `/var/run/raven.sock`. Provide a tiny `raven-client` library crate for easy integration.

2. **Docker auto-discovery**: Agent connects to `/var/run/docker.sock`, lists running containers, auto-configures log tailing for each without manual path config in `agent.toml`.

3. **Log-based alerting**: Alert on log patterns (e.g., "ERROR" appears more than N times in M minutes). Evaluate in the same alert engine loop using ClickHouse queries.

4. **Kubernetes deployment**: Helm chart for the central server. DaemonSet manifest for the agent.

---

## 5. Verification & Testing

| Test Type | What | How |
|---|---|---|
| **Unit tests** | Proc reader, log tailer, format parsers, protobuf serialization, alert evaluator | `cargo test` in each crate. Mock `/proc` files and log files. |
| **Integration test** | Full data pipeline: agent → server → databases → API | `docker compose up` test environment. Push synthetic data. Query API and verify results match. |
| **Alert test** | Rule evaluation and notification delivery | Create threshold rule. Push metrics exceeding it. Verify Discord/Slack/email notifications arrive. Verify state transitions (OK → Firing → Resolved). |
| **Reconnection test** | Agent WAL and replay | Start agent + server. Kill server. Verify agent buffers to WAL. Restart server. Verify data replays with no gaps. |
| **Load test** | gRPC ingestion throughput | Use `ghz` (gRPC benchmarking tool) to stress-test the ingestion server with synthetic `MetricBatch` and `LogBatch` streams. |
| **E2E test** | Dashboard showing real data | Agent running on a real VPS. Open dashboard. Verify live metrics charts and log tail. |

---

## 6. Pre-Project Reading List

1. **`man 5 proc`** — Sections on `/proc/stat`, `/proc/meminfo`, `/proc/diskstats`, `/proc/net/dev`. Foundation of the metrics collection module.
2. **`man 7 inotify`** — Understand `IN_MODIFY`, `IN_MOVE_SELF`, `IN_CREATE` events and gotchas around log rotation.
3. **[Tokio Tutorial](https://tokio.rs/tokio/tutorial)** — Channels, `select!`, I/O, and spawning tasks. Core of the agent and server async architecture.
4. **[tonic streaming examples](https://github.com/hyperium/tonic/tree/master/examples)** — Client-streaming and bidirectional patterns. Directly applicable to the agent-server transport.
5. **[Beej's Guide to Unix IPC](https://beej.us/guide/bgipc/)** — Datagram socket section. Relevant for the Unix socket stretch goal.
6. **"Linux System Programming" by Robert Love** — Chapters on file I/O, process scheduling, and memory. Background knowledge for the agent.
7. **[procfs crate docs](https://docs.rs/procfs/latest/procfs/)** — API surface for type-safe `/proc` parsing.
8. **[Docker logging drivers documentation](https://docs.docker.com/config/containers/logging/)** — Understand the `json-file` log driver format that the agent parses.

---

## 7. Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Deployment | Docker Compose over Kubernetes | Zero cost, minimal setup, sufficient for 1-5 nodes. K8s is a stretch goal. |
| Metrics DB | VictoriaMetrics over InfluxDB/TimescaleDB | Single binary, Prometheus-compatible, ~256 MB RAM, less operational overhead |
| Log DB | ClickHouse over Loki/Elasticsearch | True columnar engine, fastest log queries, SQL interface, bloom filter text search |
| Agent auth | Pre-shared token over mTLS | Simpler setup (copy one string vs. manage certificates). Sufficient for scope. |
| Agent user | Dedicated `raven` system user over root | Principle of least privilege. Standard pattern (matches Datadog, Prometheus, Telegraf). |
| Config location | `/etc/raven/agent.toml` (overridable with `--config`) | FHS standard for system daemons. Permissions protect the token. |
| Log tailing | `inotify` over polling | Kernel-level notification, zero CPU waste, instant detection of new lines |
| Unix socket intake | Stretch goal | Core value is system metrics + log tailing. Custom app metrics are nice-to-have. |
| Docker auto-discovery | Stretch goal (manual config first) | Manual path config is simpler to build and debug. Auto-discovery adds value later. |
| Dashboard hosting | Built static assets served by `raven-server` | Zero extra service, no external hosting dependency, same-origin API/WebSocket simplifies auth and CORS. |
| Self-hostable design | Single `docker compose up`, auto-migrations, zero wiring | Follows the Plausible/Umami/Uptime Kuma pattern. Makes the project accessible. |
| Application DB | SQLite (embedded) over PostgreSQL | No extra container, zero config, single file. CRUD workload is low-volume (users, tokens, alert rules) — SQLite handles it easily. ClickHouse is OLAP and unsuited for transactional CRUD. |
| User auth | Argon2id password hashing + JWT | Industry standard. Argon2id is the recommended password hashing algorithm (OWASP). Stateless JWT avoids session table lookups on every request. |
| Data ownership | Per-user scoping with admin override | Agent tokens, alert rules, and notification channels are owned by creating user. Admins see everything. Prevents accidental cross-user interference. |

---

## 8. User Stories

### US-1: First-Time Setup — From Zero to Monitoring

> As a developer with a VPS running a web app behind Nginx, I want to set up Raven from scratch and start seeing metrics and logs in a dashboard — without reading a 20-page guide.

**Scenario**: You have two VPS instances. One will be the central Raven server. The other runs your production web app and you want to monitor it.

**Flow:**

**A) Deploy the central server (5 minutes):**

```bash
# On your Oracle ARM VM (or any server):
git clone https://github.com/you/raven.git
cd raven
docker compose up -d
```

This single command starts four services — all pre-configured to talk to each other:
- `raven-server` (Rust binary) — gRPC ingestion on port 9090, HTTP API on port 8080
- `VictoriaMetrics` — time-series database for metrics (port 8428, internal only)
- `ClickHouse` — columnar database for logs (port 8123, internal only)
- Dashboard UI is served by `raven-server` as static files (React + Vite build output)

On first boot, `raven-server` auto-runs ClickHouse migrations (creates the `logs` table, alert rules table, etc.). No manual database setup.

**B) Create your admin account (30 seconds):**

Open `http://140.238.xx.xx:8080`. You're greeted with a first-time setup screen:
- Enter a username and password.
- This creates the admin account and issues a JWT.
- You're redirected to an empty Agents Overview page: "No agents connected yet. Add your first agent →"

**C) Generate an agent token (30 seconds):**

Click "Add Agent". The server generates a cryptographically random token, stores a hash of it, and shows you a one-time screen:

```
┌──────────────────────────────────────────────────────────────┐
│  🔑 New Agent Token                                          │
│                                                               │
│  Token: rvn_k8x2mP9qL4nR7vT1bW6y...                         │
│  ⚠️  Copy this now — it won't be shown again.                │
│                                                               │
│  Quick Install (run this on the server you want to monitor):  │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │ curl -sSL https://github.com/you/raven/releases/        │ │
│  │   latest/download/install.sh | sudo sh -s -- \           │ │
│  │   --server 140.238.xx.xx:9090 \                          │ │
│  │   --token rvn_k8x2mP9qL4nR7vT1bW6y                      │ │
│  └──────────────────────────────────────────────────────────┘ │
│                                                        [Copy] │
└──────────────────────────────────────────────────────────────┘
```

The dashboard knows its own server address, so the command is pre-filled. You just copy it.

**D) Install the agent on the monitored VPS (1 minute):**

SSH into the VPS you want to monitor and paste the command:

```bash
curl -sSL https://github.com/you/raven/releases/latest/download/install.sh | sudo sh -s -- \
  --server 140.238.xx.xx:9090 \
  --token rvn_k8x2mP9qL4nR7vT1bW6y
```

The install script runs automatically:
1. Detects your CPU architecture (`x86_64` or `aarch64`).
2. Downloads the correct `raven-agent` binary to `/usr/local/bin/raven-agent`.
3. Creates `/etc/raven/agent.toml` with the server address and token already filled in:
   ```toml
   [server]
   address = "140.238.xx.xx:9090"
   token = "rvn_k8x2mP9qL4nR7vT1bW6y"
   tls = true

   [metrics]
   interval_seconds = 10

   [transport]
   batch_size = 100
   flush_interval_seconds = 5
   retry_max_interval_seconds = 60
   wal_max_size_mb = 100

   # Add your log files below and restart the agent:
   # sudo systemctl restart raven-agent
   #
   # [[logs]]
   # name = "my-app"
   # path = "/var/log/app.log"
   # format = "plain"
   ```
4. Creates a `raven` system user with `adm` + `docker` group membership (for log file access).
5. Installs a systemd service and starts it.
6. Prints:
   ```
   ✓ raven-agent installed and running.
   ✓ Connected to 140.238.xx.xx:9090.

   Add log files to /etc/raven/agent.toml and restart:
     sudo systemctl restart raven-agent
   ```

**E) Metrics appear immediately (10 seconds):**

Switch back to your dashboard. The Agents Overview page now shows:

```
┌────────────────────────────────────────┐
│  web-server-1                          │
│  🟢 Online · Last seen: 3s ago         │
│                                        │
│  CPU  ▁▂▃▂▁▂▃▄▃▂   12%               │
│  MEM  ▅▅▅▅▅▅▅▅▅▅   68%               │
│  DISK ▁▁▁▁▁▁▁▁▁▁   34%               │
└────────────────────────────────────────┘
```

System metrics (CPU, memory, disk, network, load average) are already flowing. No log files configured yet though.

**F) Add log files (2 minutes):**

Back on the monitored VPS, edit the agent config:

```bash
sudo nano /etc/raven/agent.toml
```

Add your log file paths:

```toml
[[logs]]
name = "nginx-access"
path = "/var/log/nginx/access.log"
format = "plain"

[[logs]]
name = "nginx-error"
path = "/var/log/nginx/error.log"
format = "plain"

[[logs]]
name = "my-api"
path = "/var/lib/docker/containers/a1b2c3d4*/a1b2c3d4*-json.log"
format = "docker-json"

[[logs]]
name = "worker"
path = "/home/deploy/.pm2/logs/worker-out.log"
format = "plain"
```

Restart the agent:

```bash
sudo systemctl restart raven-agent
```

Within seconds, logs are flowing. Open the Log Explorer on the dashboard and you'll see nginx access logs, your API's stdout/stderr, and PM2 worker logs — all in one place.

**Total time: under 10 minutes from zero to full monitoring.**

---

### US-2: Checking Server Health at a Glance

> As a developer managing 3 VPS instances, I want to open one page and instantly see if anything is wrong — instead of SSHing into each machine and running `htop`.

**Scenario**: You have three servers — `web-server-1` (runs your API), `web-server-2` (runs a background worker), and `db-server` (runs PostgreSQL). All three have Raven agents installed.

**Flow:**

1. Open the **Agents Overview** page. You see three cards:

```
┌──────────────────────────┐  ┌──────────────────────────┐  ┌──────────────────────────┐
│  web-server-1             │  │  web-server-2             │  │  db-server                │
│  🟢 Online · 5s ago       │  │  🟢 Online · 3s ago       │  │  🟢 Online · 8s ago       │
│                           │  │                           │  │                           │
│  CPU  ▁▂▃▂▁▂▃▂▁▂   14%  │  │  CPU  ▅▆▇█▇▆▇█▇▆   94%  │  │  CPU  ▂▂▃▂▂▂▃▂▂▂   22%  │
│  MEM  ▅▅▅▅▅▅▅▅▅▅   62%  │  │  MEM  ▇▇▇▇▇▇▇▇▇▇   88%  │  │  MEM  ▃▃▃▃▃▃▃▃▃▃   41%  │
│  DISK ▂▂▂▂▂▂▂▂▂▂   34%  │  │  DISK ▃▃▃▃▃▃▃▃▃▃   45%  │  │  DISK ▆▆▆▆▆▆▆▆▆▆   78%  │
└──────────────────────────┘  └──────────────────────────┘  └──────────────────────────┘
```

2. Immediately see `web-server-2` is at **94% CPU** and 88% memory — something is wrong. Click on it.

3. The **Host Detail** page opens. You see a time range picker at the top:

```
[ Last 5m ] [ Last 15m ] [ Last 1h ▾ ] [ Last 6h ] [ Last 24h ] [ Last 7d ] [ Custom ]
```

   Select "Last 1h". The charts show:
   - **CPU**: was stable at ~30% until 40 minutes ago, then climbed steadily to 94%.
   - **Memory**: followed the same pattern, from 50% to 88%.
   - **Network**: outbound traffic spiked at the same time.
   - **Load average**: 1m load is 3.8 on a 2-core machine (overloaded).

4. Something started eating resources 40 minutes ago. Click "View Logs" for this host → opens the **Log Explorer** pre-filtered to `web-server-2` with the same 1-hour time range.

5. In the logs, you see the worker app logging `Processing batch of 50000 items...` in a tight loop — someone queued a massive job. You identify the root cause without ever opening an SSH session.

**What this replaces**: Previously you'd SSH into each of the three servers, run `htop`, check if anything looks off, then `tail -f` various log files trying to correlate timestamps. With Raven, it takes one page load.

---

### US-3: Debugging a Production Crash

> As a developer, my API went down 2 hours ago. Users reported 502 errors. The app has already been restarted by the process manager, so I can't just SSH in and look at the current state — I need historical logs and metrics from the moment of the crash.

**Scenario**: Your API runs inside a Docker container on `web-server-1`. Docker's restart policy restarted it automatically, so the app is back up, but you need to know why it crashed.

**Flow:**

1. Open the **Log Explorer**.

2. Set filters:
   - Host: `web-server-1`
   - App: `my-api`
   - Stream: `stderr` (crash output goes to stderr)
   - Time range: Custom → `2 hours ago` to `1 hour ago`

3. Hit search. ClickHouse queries its `logs` table using the time range and hostname index — results return in milliseconds even with millions of stored log lines:

```
2026-03-03 10:14:52.331  web-server-1  my-api  stderr  thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: Os { code: 12, kind: OutOfMemory }'
2026-03-03 10:14:52.331  web-server-1  my-api  stderr  stack backtrace:
2026-03-03 10:14:52.331  web-server-1  my-api  stderr     0: std::panicking::begin_panic_handler
2026-03-03 10:14:52.331  web-server-1  my-api  stderr     1: core::panicking::panic_fmt
2026-03-03 10:14:52.332  web-server-1  my-api  stderr     2: my_api::handlers::process_upload
2026-03-03 10:14:52.332  web-server-1  my-api  stderr     3: my_api::routes::upload_file
```

4. Found it — an OOM panic in the `process_upload` handler. But was the whole machine out of memory, or just the container?

5. Switch to the **Host Detail** page for `web-server-1`. Set the same custom time range (2 hours ago to 1 hour ago). The **Memory chart** shows:

```
Memory Usage (%)
100% ┤                              ╭──── crash here (10:14:52)
 90% ┤                          ╭───╯
 80% ┤                     ╭────╯
 70% ┤                ╭────╯
 60% ┤           ╭────╯
 50% ┤───────────╯
     └─────────────────────────────────────
     09:30    09:45    10:00    10:15
```

   Memory climbed steadily from 50% to 100% over 45 minutes, then the app crashed. The system ran completely out of memory.

6. Root cause: a file upload endpoint was reading entire files into memory. Large uploads exhausted RAM. You now know exactly what to fix and have the stack trace, timestamp, and server state all in one place.

**What this replaces**: Previously you'd SSH in, find that the app is already running again (Docker restarted it), and the old container's logs might be gone. You'd have to dig through Docker's log files manually with `docker logs` or `journalctl` with the right `--since` flags, then separately check `dmesg` for OOM killer events, and mentally correlate timestamps. Raven shows you logs + metrics on the same timeline.

---

### US-4: Live Tailing Logs During a Deployment

> As a developer, I just pushed a new version of my API to production. I want to watch the logs in real time to make sure it starts up correctly and handles requests without errors.

**Scenario**: You deploy via `docker compose pull && docker compose up -d` on `web-server-1`. The new container starts. You want to see what's happening right now.

**Flow:**

1. Open the **Log Explorer** on the dashboard.

2. Set filters:
   - Host: `web-server-1`
   - App: `my-api`
   - (No time range needed — we want live data)

3. Click the **"Live Tail"** toggle in the top-right corner. The button turns green: `● Live`.

4. Behind the scenes, the dashboard opens a WebSocket connection:
   ```
   ws://raven.example.com:8080/api/ws/logs?host=web-server-1&app=my-api
   ```
   The server registers this WebSocket as a subscriber to its internal broadcast channel. Whenever a `LogBatch` arrives from `web-server-1`'s agent containing `my-api` logs, those lines are forwarded to your browser in real time.

5. You watch log lines appear as they happen:

```
10:32:01.142  stdout  Starting server on 0.0.0.0:3000...
10:32:01.203  stdout  Connected to database (pool_size=10)
10:32:01.215  stdout  Loading routes...
10:32:01.218  stdout  Server ready. Accepting connections.
10:32:02.891  stdout  GET /api/health → 200 (2ms)
10:32:03.122  stdout  GET /api/users → 200 (14ms)
10:32:03.456  stderr  WARN: Deprecated header X-Custom-Auth used by 192.168.1.50
10:32:04.001  stdout  POST /api/upload → 201 (89ms)
```

   - `stdout` lines are in the default color.
   - `stderr` lines are highlighted in red so they stand out immediately.
   - Auto-scroll keeps the view pinned to the bottom as new lines arrive.

6. You spot a warning on stderr — click **"Pause"** to freeze auto-scroll. New lines still arrive and are buffered (you see a badge: "12 new lines"), but the view stays put so you can read the warning in detail.

7. Click **"Resume"** — the buffered lines appear, auto-scroll continues.

8. Everything looks clean. Toggle "Live Tail" off. The WebSocket disconnects. You're back to the normal historical log query view.

**What this replaces**: Running `ssh web-server-1` → `docker logs -f my-api --tail 100` in a terminal. Raven's live tail does the same thing but in the browser, with color coding, filtering, and the ability to pause without losing data. You can also share the dashboard URL with a teammate so they can watch alongside you.

---

### US-5: Setting Up Alerts for CPU, Disk, and Downtime

> As a developer, I don't want to stare at the dashboard all day. I want Discord and email notifications when something goes wrong so I can respond even when I'm not actively monitoring.

**Scenario**: You want three alerts: (1) CPU too high, (2) disk getting full, (3) a server goes offline.

**Flow:**

**A) Configure notification channels:**

1. Go to **Alerts → Notification Channels → "Add Channel"**.

2. Add Discord:
   - Type: **Discord**
   - Name: `discord-ops`
   - Webhook URL: `https://discord.com/api/webhooks/1234567890/abcdef...` (get this from Discord → Server Settings → Integrations → Webhooks)
   - Click **"Test"** → A test message appears in your Discord channel:
     ```
     🔔 Raven Test Alert
     This is a test notification from your Raven instance.
     If you can see this, the channel is configured correctly.
     ```
   - Save.

3. Add Email:
   - Type: **SMTP**
   - Name: `email-oncall`
   - SMTP Host: `smtp.gmail.com`, Port: `587`
   - Username: `your-email@gmail.com`, Password: `app-password-here`
   - From: `raven@yourdomain.com`, To: `oncall@yourdomain.com`
   - Click **"Test"** → Check your inbox for the test email.
   - Save.

**B) Create alert rules:**

1. Go to **Alerts → Rules → "Add Rule"**.

2. **High CPU alert:**
   - Name: `High CPU`
   - Metric: `cpu_usage`
   - Condition: `> 90%`
   - Duration: `5 minutes` (must be above threshold for 5 solid minutes — prevents one-off spikes from alerting)
   - Hosts: `All`
   - Notify via: `discord-ops`, `email-oncall`
   - Save.

3. **Disk full alert:**
   - Name: `Disk Almost Full`
   - Metric: `disk_usage`
   - Condition: `> 85%`
   - Duration: `10 minutes`
   - Hosts: `All`
   - Notify via: `discord-ops`
   - Save.

**C) How alerts fire:**

The alert engine runs in the background inside `raven-server`, evaluating rules every 30 seconds:

1. CPU on `web-server-2` hits 92%. The rule enters **Pending** state — the clock starts.
2. 30 seconds later, still 93%. Still Pending, waiting for the 5-minute duration.
3. After 5 minutes of sustained >90%: state transitions to **Firing**. A notification is sent:

   **Discord message:**
   ```
   🔴 FIRING: High CPU
   ─────────────────────────────
   Host:      web-server-2
   Metric:    cpu_usage
   Value:     94.2%
   Threshold: > 90% for 5m
   Since:     2026-03-03 10:30:00 UTC
   ```

   **Email:** Same content in a formatted HTML email.

4. You fix the issue. CPU drops to 45%.

5. After the next evaluation cycle, the rule transitions from **Firing → Resolved**. A resolution notification is sent:

   **Discord message:**
   ```
   🟢 RESOLVED: High CPU
   ─────────────────────────────
   Host:      web-server-2
   Metric:    cpu_usage
   Value:     45.1%
   Was firing: 2026-03-03 10:30:00 → 10:47:00 UTC (17 minutes)
   ```

6. The **Alerts page** on the dashboard shows a history of all alerts — when they fired, when they resolved, which host, which metric. You can review past incidents.

**Key behavior**: The state machine (`OK → Pending → Firing → Resolved`) ensures you only get notified on transitions. If CPU stays at 95% for 3 hours, you get **one** firing notification and **one** resolved notification — not 360 messages.

---

### US-6: Sharing Access with a Friend

> As a developer, my friend wants to monitor their own VPS using my Raven instance. I want to add their server without giving them access to my dashboard or my data.

**Scenario**: Your Raven central server is running on your Oracle ARM VM. Your friend has a VPS at Hetzner running a Node.js app with PM2. They want to use your monitoring setup.

**Flow:**

1. Open your Raven dashboard → **Settings → Agents → "Add Agent"**.

2. A new token is generated. The dashboard shows the install command pre-filled with your server's address:

```
curl -sSL https://github.com/you/raven/releases/latest/download/install.sh | sudo sh -s -- \
  --server 140.238.xx.xx:9090 \
  --token rvn_j7mK2pQ9xL4nR...
```

3. Send this command to your friend (via Discord DM, Slack, email — whatever).

4. Your friend SSHs into their Hetzner VPS and pastes the command. The install script runs:
   - Downloads the `raven-agent` binary (detects their `x86_64` architecture).
   - Creates `/etc/raven/agent.toml` with your server address and their unique token.
   - Creates the `raven` system user.
   - Starts the systemd service.

5. Your friend edits their agent config to add their PM2 log files:

   ```toml
   [[logs]]
   name = "node-api"
   path = "/home/deploy/.pm2/logs/api-out.log"
   format = "plain"

   [[logs]]
   name = "node-api-errors"
   path = "/home/deploy/.pm2/logs/api-error.log"
   format = "plain"
   ```

   Then restarts: `sudo systemctl restart raven-agent`.

6. Within 10 seconds, a new card appears on your Agents Overview:

```
┌──────────────────────────┐  ┌──────────────────────────┐
│  web-server-1 (yours)     │  │  hetzner-vps (friend's)   │
│  🟢 Online · 5s ago       │  │  🟢 Online · 3s ago       │
│                           │  │                           │
│  CPU  ▁▂▃▂▁▂▃▂▁▂   14%  │  │  CPU  ▃▃▄▃▃▃▄▃▃▃   38%  │
│  MEM  ▅▅▅▅▅▅▅▅▅▅   62%  │  │  MEM  ▄▄▄▄▄▄▄▄▄▄   55%  │
│  DISK ▂▂▂▂▂▂▂▂▂▂   34%  │  │  DISK ▂▂▂▂▂▂▂▂▂▂   28%  │
└──────────────────────────┘  └──────────────────────────┘
```

7. Both servers' metrics and logs are now in one place. You can see each other's servers on the dashboard (both of you can log in with separate dashboard accounts — or share one for simplicity).

**Networking**: Your friend's VPS connects **outbound** to your Oracle VM on port 9090. They don't need to open any ports on their server. As long as they can make outbound TCP connections (which virtually all servers can), it works.

**Security**: Your friend's agent token only grants write access — it can push metrics and logs, but cannot query or read any data. If you ever want to cut off their agent, delete their token from the dashboard → their agent gets `UNAUTHENTICATED` on next reconnect attempt and stops.

---

### US-7: Server Goes Down and Comes Back

> As a developer, one of my monitored servers crashed overnight. I want to know (1) when it went down, (2) that it went down (via notification), and (3) that no monitoring data was lost when it comes back.

**Scenario**: `web-server-2` loses power at 3:00 AM. It comes back online at 3:45 AM after an automated restart. You're asleep.

**Flow — what happens automatically:**

1. **3:00 AM** — `web-server-2` crashes. The agent process dies.

2. **3:00 AM to 3:01:30 AM** — The central server notices the heartbeat stopped. The agent sends a heartbeat every 30 seconds; the server expects one every 90 seconds. After 90 seconds of silence, the server marks `web-server-2` as **Offline**.

3. **3:01:30 AM** — The alert engine evaluates the "Agent Offline" condition. A notification fires:

   **Discord:**
   ```
   🔴 ALERT: Agent Offline
   ─────────────────────────────
   Host:      web-server-2
   Status:    Offline
   Last seen: 2026-03-03 03:00:12 UTC
   Note:      No heartbeat received for 90 seconds.
   ```

   **Email:** Same content, delivered to your inbox.

4. **3:00 AM to 3:45 AM** — The dashboard shows `web-server-2` with a red **"Offline"** badge:

```
┌──────────────────────────┐
│  web-server-2             │
│  🔴 Offline · 45m ago     │
│                           │
│  CPU  ▃▃▄▃▃▃··────   --  │
│  MEM  ▄▄▄▄▄▄··────   --  │
│  DISK ▂▂▂▂▂▂··────   --  │
│  Last seen: 3:00 AM       │
└──────────────────────────┘
```

   The metrics charts show a gap starting at 3:00 AM — no data because the agent was down.

5. **3:45 AM** — The server comes back online. The operating system starts, and systemd restarts `raven-agent` automatically (because of `Restart=always` in the unit file).

6. **3:45 AM** — The agent starts its reconnection sequence:
   - Reads `/etc/raven/agent.toml` for server address and token.
   - Opens gRPC connection to the central server.
   - Authenticates with the token.
   - Sends a `Register` message.
   - **Server marks `web-server-2` as Online again.**

7. **3:45 AM** — A resolution notification fires:

   **Discord:**
   ```
   🟢 RESOLVED: Agent Offline
   ─────────────────────────────
   Host:      web-server-2
   Status:    Online
   Was offline: 03:00 → 03:45 UTC (45 minutes)
   ```

8. The dashboard badge turns green: `🟢 Online · 3s ago`. Metrics start flowing again. The charts show a 45-minute gap between 3:00 and 3:45 — this is accurate, there was genuinely no data during the outage.

9. **The next morning**, you wake up, check Discord, see two messages (down at 3:00, back at 3:45). Open the dashboard, look at the Host Detail for `web-server-2` with the "Last 6h" time range, and see exactly when the outage happened.

**Note on WAL (Write-Ahead Log)**: The WAL covers a different scenario — when the **agent is running but can't reach the central server** (network issue, server restart, etc.). In that case, the agent buffers data locally and replays it when the connection is restored. In this US-7 scenario, the agent process itself died (the whole machine went down), so there's nothing to buffer — the gap in data is real and expected. The WAL protects against *network* outages, not *machine* outages.

---

### US-8: Investigating Slow Requests Using Time-Range Queries

> As a developer, users are reporting that the site was slow "sometime this afternoon." I want to correlate server metrics with application logs for a specific time window.

**Scenario**: Users in your Discord server mentioned slowness between roughly 2 PM and 4 PM. Your API runs on `web-server-1`.

**Flow:**

1. Open the **Host Detail** page for `web-server-1`.

2. Click **Custom** time range. Set:
   - From: `2026-03-03 14:00`
   - To: `2026-03-03 16:00`

3. The dashboard makes an API call:
   ```
   GET /api/metrics?host=web-server-1&metric=cpu&from=2026-03-03T14:00:00Z&to=2026-03-03T16:00:00Z
   ```
   The server proxies to VictoriaMetrics with appropriate step downsampling (2 hours of data → `step=30s` → ~240 data points). Charts render instantly.

4. The charts reveal:
   - **CPU**: Normal at 20% from 14:00 to 14:45, then spiked to 75% from 14:45 to 15:30, back to normal after.
   - **Memory**: Jumped from 60% to 92% at 14:45.
   - **Disk I/O**: Write throughput tripled during the same window.
   - **Network**: Nothing unusual.

   Something happened at 14:45 that caused high CPU, memory, and disk simultaneously.

5. Switch to the **Log Explorer**. Keep the same custom time range. Filter: host = `web-server-1`, app = `my-api`. Search for: `slow` or `timeout`.

6. The query hits ClickHouse:
   ```sql
   SELECT timestamp, app, stream, line FROM logs
   WHERE hostname = 'web-server-1'
     AND timestamp >= '2026-03-03 14:00:00'
     AND timestamp <= '2026-03-03 16:00:00'
     AND line ILIKE '%slow%' OR line ILIKE '%timeout%'
   ORDER BY timestamp ASC
   LIMIT 1000
   ```
   ClickHouse's `tokenbf_v1` bloom filter index makes this fast even over millions of log lines.

7. Results show a burst of logs starting at 14:45:
   ```
   14:45:02  stdout  Starting database migration (v42)...
   14:45:03  stdout  Migrating table: orders (1.2M rows)...
   14:47:15  stderr  WARN: Query timeout on /api/orders (threshold: 5000ms, actual: 12340ms)
   14:47:16  stderr  WARN: Query timeout on /api/orders (threshold: 5000ms, actual: 15201ms)
   14:48:00  stdout  Migration progress: 45% (orders)...
   ...
   15:28:44  stdout  Migration complete (v42). 1.2M rows migrated in 43m.
   ```

8. Root cause: a database migration locked the `orders` table for 43 minutes, causing query timeouts for all requests hitting that table. The CPU/memory/disk spikes were from the migration process.

**What you used**: Time-range queries on both metrics and logs. The ability to set the exact same custom time range on both views and switch between them. Full-text search over historical logs (the `ILIKE` query powered by ClickHouse's bloom filter index). All without SSHing into the machine.

---

### US-9: Monitoring Docker Container Logs

> As a developer, I run my apps in Docker containers that log to stdout/stderr. I want Raven to capture these logs without changing my app's logging setup.

**Scenario**: You have three Docker containers on `web-server-1`: `api`, `worker`, and `redis`. They all log to stdout/stderr as you'd normally do in a containerized app. Docker's default `json-file` log driver captures this output to files on disk.

**Flow:**

1. Docker writes each container's stdout/stderr to:
   ```
   /var/lib/docker/containers/<container-id>/<container-id>-json.log
   ```
   Each line is a JSON object:
   ```json
   {"log":"Processing request for /api/users\n","stream":"stdout","time":"2026-03-03T10:32:01.142Z"}
   {"log":"ERROR: Database connection refused\n","stream":"stderr","time":"2026-03-03T10:32:01.501Z"}
   ```

2. Find your container IDs:
   ```bash
   docker ps --format '{{.ID}} {{.Names}}'
   # a1b2c3d4e5f6  api
   # f6e5d4c3b2a1  worker
   # 1a2b3c4d5e6f  redis
   ```

3. Add them to `/etc/raven/agent.toml` using glob patterns (you only need the first few characters of the container ID):
   ```toml
   [[logs]]
   name = "api"
   path = "/var/lib/docker/containers/a1b2c3d4*/*-json.log"
   format = "docker-json"

   [[logs]]
   name = "worker"
   path = "/var/lib/docker/containers/f6e5d4c3*/*-json.log"
   format = "docker-json"

   [[logs]]
   name = "redis"
   path = "/var/lib/docker/containers/1a2b3c4d*/*-json.log"
   format = "docker-json"
   ```

4. Restart the agent: `sudo systemctl restart raven-agent`.

5. The agent:
   - Resolves the glob patterns to actual file paths.
   - Starts watching each file with `inotify` for `IN_MODIFY` events.
   - When a new line appears, the agent parses the `docker-json` format — strips the JSON envelope and extracts the raw log line, the stream type (`stdout`/`stderr`), and the timestamp.
   - Packages it into a `LogBatch` and streams it to the central server.

6. On the dashboard Log Explorer, you can now filter by app name (`api`, `worker`, `redis`) and stream type (`stdout`/`stderr`). The JSON envelope is gone — you see clean log lines just as if you ran `docker logs api`.

7. **Log rotation**: Docker rotates these log files when they reach a size limit (default: 100MB per file, up to 5 files). The agent handles this automatically: when `inotify` detects `IN_MOVE_SELF` (old file rotated away) and `IN_CREATE` (new file created), it closes the old file handle, opens the new file, and resets its seek offset. No log lines are lost during rotation.

**What you didn't have to change**: Nothing in your application. Your apps still log to stdout/stderr as normal. Docker still captures them as normal. The Raven agent just tails what Docker already writes to disk. Zero application code changes.

---

### US-10: Monitoring PM2-Managed Node.js Apps

> As a developer, I run my Node.js apps with PM2 on a VPS (no Docker). I want to monitor their logs alongside my server metrics.

**Scenario**: You have two PM2 processes on `web-server-1`: `api` and `cron-worker`. PM2 captures their stdout/stderr to log files automatically.

**Flow:**

1. PM2 writes logs to:
   ```
   ~/.pm2/logs/api-out.log     (stdout)
   ~/.pm2/logs/api-error.log   (stderr)
   ~/.pm2/logs/cron-worker-out.log
   ~/.pm2/logs/cron-worker-error.log
   ```

2. Add them to `/etc/raven/agent.toml`:
   ```toml
   [[logs]]
   name = "api-stdout"
   path = "/home/deploy/.pm2/logs/api-out.log"
   format = "plain"

   [[logs]]
   name = "api-stderr"
   path = "/home/deploy/.pm2/logs/api-error.log"
   format = "plain"

   [[logs]]
   name = "cron-stdout"
   path = "/home/deploy/.pm2/logs/cron-worker-out.log"
   format = "plain"

   [[logs]]
   name = "cron-stderr"
   path = "/home/deploy/.pm2/logs/cron-worker-error.log"
   format = "plain"
   ```

3. Ensure the `raven` system user can read PM2's log files:
   ```bash
   sudo usermod -aG deploy raven   # add raven to the deploy user's group
   chmod g+r /home/deploy/.pm2/logs/*.log   # ensure group-readable
   ```

4. Restart the agent: `sudo systemctl restart raven-agent`.

5. The agent tails these files using `inotify`. Since format is `plain`, it treats each newline-delimited chunk as a raw log line — no JSON parsing needed.

6. On the dashboard, you see logs from all four files, filterable by app name. You can view stdout and stderr separately or together.

**PM2 log rotation**: If you use `pm2-logrotate` (the standard PM2 log rotation module), it renames the old file and creates a new empty one — the same rotation pattern as Docker. The agent detects this via `inotify` events and seamlessly switches to the new file.
