![raven](./raven-logo-horizontal-full-light.png)

# Technical Documentation

A lightweight, self-hostable server monitoring and centralized logging tool.

---

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Tech Stack](#tech-stack)
- [Language Choice: Rust](#language-choice-rust)
- [Component Design](#component-design)
  - [Agent](#agent)
  - [Central Server](#central-server)
  - [Dashboard](#dashboard)
- [Agent Setup Flow](#agent-setup-flow)
- [Security Model](#security-model)
- [Deployment Strategy](#deployment-strategy)

---

## Architecture Overview

```
┌─────────────── Monitored VPS ────────────────┐
│                                              │
│  ┌────────────┐       ┌──────────────┐       │
│  │ /proc      │       │ inotify log  │       │
│  │ reader     │       │ tailer       │       │
│  │ (10s poll) │       │ (event-based)│       │
│  └──────┬─────┘       └──────┬───────┘       │
│         │ MetricBatch        │ LogBatch      │
│         ▼                    ▼               │
│  ┌─────────────────────────────────────┐     │
│  │  Internal Channel (mpsc, bounded)   │     │
│  └────────────────┬────────────────────┘     │
│                   │                          │
│                   ▼                          │
│  ┌─────────────────────────────────────┐     │
│  │  gRPC Sender Task                   │     │
│  │  - Batch: 100 items or 5s flush     │     │
│  │  - On disconnect: buffer to WAL     │     │
│  │  - Reconnect with exp. backoff      │     │
│  └────────────────┬────────────────────┘     │
│                   │                          │
└───────────────────┼──────────────────────────┘
                    │
                    │ gRPC over TLS (port 9090)
                    │ Auth: Bearer token in metadata
                    ▼
┌─────────────── Central Server ──────────────────┐
│                                                 │
│    ┌─────────────────────────────────────┐      │
│    │           raven-server              │      │
│    │                                     │      │
│    │  gRPC Ingestion --> Route data:     │      │
│    │    MetricBatch  --> VictoriaMetrics │      │
│    │    LogBatch     --> ClickHouse      │      │
│    │    Heartbeat    --> Agent Registry  │      │
│    │                                     │      │
│    │  HTTP API (axum) --> Query layer    │      │
│    │  WebSocket       --> Live log tail  │      │
│    │  Alert Engine    --> Notifications  │      │
│    └─────────────────────────────────────┘      │
│                                                 │
│  ┌───────────────┐  ┌────────────┐  ┌────────┐  │
│  │VictoriaMetrics│  │ ClickHouse │  │ SQLite │  │
│  │ (metrics TSDB)│  │ (log store)│  │ (users)│  │
│  └───────────────┘  └────────────┘  └────────┘  │
│                                                 │
└─────────────────────────────────────────────────┘
                    │
                    │ HTTPS
                    ▼
┌──────────────── Browser Clients ──────────────┐
│  Static dashboard served by axum (`/`)        │
│  - Agents overview with sparklines            │
│  - Host detail with time-series charts        │
│  - Log explorer with live tail                │
│  - Alert rule management                      │
└───────────────────────────────────────────────┘
```

**Data flow summary:**

1. Agent reads `/proc` every 10s for system metrics.
2. Agent watches log files via `inotify` for new lines.
3. Data is batched and streamed to the central server over gRPC.
4. Server routes metrics to VictoriaMetrics (TSDB) and logs to ClickHouse (columnar DB).
5. Dashboard queries the server's HTTP API. Live log tail uses WebSocket.
6. Alert engine evaluates threshold rules and sends notifications.

---

## Tech Stack

### Agent (Rust)

| Component | Technology | Purpose |
|---|---|---|
| Async runtime | `tokio` | Event loop, timers, channels, I/O |
| Proc parsing | `procfs` crate | Type-safe `/proc` filesystem parsing |
| File watching | `inotify` crate | Kernel-level file change notifications |
| gRPC client | `tonic` | Client-streaming RPC to central server |
| Protobuf codegen | `prost` + `tonic-build` | Compile `.proto` schemas to Rust types |
| TLS | `rustls` (via `tonic`) | Encrypted transport without OpenSSL |
| Config | `serde` + `toml` | Parse `agent.toml` configuration |
| Logging | `tracing` + `tracing-subscriber` | Structured logging for agent internals |
| CLI args | `clap` | `--config` flag, version, help |
| WAL storage | `redb` or append-only bincode file | Buffer data during server disconnects |

### Central Server (Rust)

| Component | Technology | Purpose |
|---|---|---|
| gRPC server | `tonic` | Accept agent streams |
| HTTP API | `axum` | REST endpoints + WebSocket for dashboard |
| HTTP client | `reqwest` | Write to VictoriaMetrics remote-write API |
| ClickHouse client | `clickhouse-rs` crate | Write and query log data |
| JWT auth | `jsonwebtoken` crate | Dashboard and API authentication |
| Alert notifications | `reqwest` (Discord/Slack webhooks) | HTTP POST to webhook URLs |
| Email | `lettre` crate | SMTP email notifications |
| Broadcast | `tokio::sync::broadcast` | Fan-out live logs to WebSocket subscribers |
| Application DB | `sqlx` + SQLite | User accounts, agent tokens, alert rules, notification channels |
| Password hashing | `argon2` crate | Secure password storage for dashboard users |
| Config | `serde` + `toml` | Parse `server.toml` configuration |

### Databases

| Database | Role | Why |
|---|---|---|
| **VictoriaMetrics** | Time-series metrics storage | Single binary, Prometheus-compatible remote-write API, extremely lightweight (~256 MB RAM). Handles downsampling via query `step` parameter. |
| **ClickHouse** | Log storage and search | Columnar engine, sub-second queries over millions of rows, SQL interface, `tokenbf_v1` bloom filter index for full-text log search. 30-day TTL auto-cleanup. |
| **SQLite** | Application data (users, tokens, alert configs) | Embedded, zero-config, no extra container. Handles the CRUD workload that ClickHouse (OLAP) is not suited for. Single file on a Docker volume. |

### Dashboard

| Component | Technology | Purpose |
|---|---|---|
| Framework | React + Vite (Bun) | Built to static files and served directly by the axum server |
| Styling | Tailwind CSS | Utility-first, responsive, dark mode |
| Charts | Apache ECharts (`echarts-for-react`) | Time-series visualization, handles large datasets |
| Data fetching | TanStack Query | Caching, refetching, loading states for REST API |
| Live tail | Native WebSocket API | Real-time log streaming from server |

### Infrastructure

| Component | Technology | Purpose |
|---|---|---|
| Containerization | Docker + Docker Compose | Self-hostable single-command deployment |
| CI/CD | GitHub Actions | Lint (clippy), test, build, cross-compile, push images |
| Container registry | GitHub Container Registry (GHCR) | Free for public repos |

---

## Language Choice: Rust

- **Daemon suitability**: No garbage collector pauses, predictable memory usage, single static binary - ideal for a long-running agent process.
- **Ecosystem coverage**: `tokio` + `tonic` + `procfs` + `inotify` cover every architectural component with no gaps.
- **Production characteristics**: Memory safety without runtime cost. The agent runs on servers alongside production workloads - minimal resource footprint matters.

---

## Component Design

### Agent

The agent is a Linux daemon that collects system metrics and tails application log files, then streams everything to the central server.

#### System Metrics

Reads the following from `/proc` using the `procfs` crate every 10 seconds (configurable):

| Metric | Source | Computation |
|---|---|---|
| CPU usage (%) | `/proc/stat` | Delta of jiffies between samples, per-core and total |
| Memory (total, used, available, swap) | `/proc/meminfo` | Direct read |
| Disk I/O (read/write bytes/sec, IOPS) | `/proc/diskstats` | Delta between samples |
| Network (bytes in/out per interface) | `/proc/net/dev` | Delta between samples |
| Load average (1m, 5m, 15m) | `/proc/loadavg` | Direct read |

#### Log Tailing

Uses kernel-level `inotify` file watching. Supports two log formats:

| Format | Source | Parsing |
|---|---|---|
| `plain` | PM2 logs, nginx, generic apps | Raw text lines, no transformation |
| `docker-json` | Docker container logs | Parse JSON envelope `{"log":"...","stream":"...","time":"..."}` - extract log line, stream type, and timestamp |

Features: log rotation handling (reopen on rotate), glob pattern support for file paths, bounded internal channel (drop oldest on overflow - tailer never blocks).

#### gRPC Transport

- Client-streaming RPCs: `StreamMetrics` and `StreamLogs`
- Batching: flush every 100 items or 5 seconds (whichever first)
- Authentication: pre-shared bearer token in gRPC metadata
- TLS via `rustls`
- Reconnection: exponential backoff (1s → 60s max)
- WAL: buffer to local disk on disconnect, replay on reconnect (100 MB cap)
- Heartbeat: ping every 30 seconds

### Central Server

The server receives data from agents, stores it, serves the HTTP API, and runs alerting.

#### gRPC Ingestion

- Validates bearer tokens against hashes in SQLite, rejects invalid with `UNAUTHENTICATED`
- Agent registry: tracks connected agents (hostname, IP, version, last heartbeat, online/offline status). Registration metadata persisted in SQLite, runtime status held in memory.
- Routes `MetricBatch` → VictoriaMetrics via Prometheus remote-write API
- Routes `LogBatch` → ClickHouse via HTTP interface
- Backpressure: slows gRPC reads if DB writes are slow
- Broadcast channel: publishes incoming logs for live tail WebSocket subscribers

#### HTTP API

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/auth/setup` | POST | First-time admin account creation (disabled after first user exists) |
| `/api/auth/login` | POST | Authenticate, return JWT |
| `/api/users` | GET/POST | List users, invite new user (admin only) |
| `/api/users/:id` | GET/PUT/DELETE | Get, update, or remove a user (admin or self) |
| `/api/users/me` | GET/PUT | Current user profile and preferences |
| `/api/users/me/password` | PUT | Change own password |
| `/api/agents` | GET | List registered agents + live status |
| `/api/agents/tokens` | GET/POST | List or generate agent tokens (scoped to user) |
| `/api/agents/tokens/:id` | DELETE | Revoke an agent token |
| `/api/metrics` | GET | Query metrics from VictoriaMetrics |
| `/api/logs` | GET | Query logs from ClickHouse |
| `/api/ws/logs` | WebSocket | Live log tail |
| `/api/alerts/rules` | GET/POST/PUT/DELETE | CRUD alert rules (scoped to user) |
| `/api/alerts/channels` | GET/POST/PUT/DELETE | CRUD notification channels (scoped to user) |
| `/api/alerts/test` | POST | Send a test notification |

Metrics support shorthand ranges (`5m`, `15m`, `1h`, `6h`, `24h`, `7d`) and custom absolute time ranges. The server auto-adjusts the query `step` for appropriate downsampling.

#### Alerting Engine

- Alert rules and notification channels stored in SQLite, owned by the user who created them
- Background task evaluates rules every 30 seconds
- State machine per rule: `OK → Pending → Firing → Resolved`
- Notifies only on state transitions (prevents spam)
- Channels: Discord webhooks, Slack webhooks, SMTP email

### Dashboard

React + Vite application built with Bun and served as static assets by `raven-server`.

- Build output: `dashboard/dist/`
- Served by axum at `/`
- API + WebSocket stay same-origin (`/api/*`, `/api/ws/*`), so no `RAVEN_API_URL` is required in production

**Pages:**

- **Login / Setup** - First-time setup creates admin account. JWT auth, token in httpOnly cookie.
- **Agents Overview** - Grid of host cards with online/offline badge, CPU/memory/disk sparklines, click to drill down
- **Host Detail** - Time range picker, full-width charts (CPU, memory, disk I/O, network, load average), auto-refresh
- **Log Explorer** - Filter by host/app/stream/time/search text, live tail toggle with WebSocket, pause/resume, stderr highlighted in red, pagination for historical queries
- **Alerts** - Active/resolved alert history, CRUD for rules and notification channels, test button
- **Settings** - Agent token management (generate, list, revoke), user profile, password change, user management (admin)

Design: responsive layout, Tailwind CSS, dark mode.

---

## Agent Setup Flow

### Step 1: Deploy the Central Server

```bash
git clone https://github.com/you/raven.git
cd raven
docker compose up -d
```

Starts `raven-server`, VictoriaMetrics, and ClickHouse - pre-configured, zero wiring. Auto-runs SQLite and ClickHouse migrations on first boot.

### Step 2: Generate an Agent Token

Open the dashboard → Settings → Agents → "Add New Agent". The server generates a token, stores its hash, and displays a one-time install command pre-filled with the server address and token.

### Step 3: Install the Agent

SSH into the target server and paste the install command:

```bash
curl -sSL https://github.com/you/raven/releases/latest/download/install.sh | sudo sh -s -- \
  --server <server-ip>:9090 \
  --token rvn_a1b2c3d4e5f6g7h8i9j0
```

The install script:
1. Detects architecture (`x86_64` or `aarch64`)
2. Downloads the correct binary to `/usr/local/bin/raven-agent`
3. Creates `/etc/raven/agent.toml` with server address and token pre-filled
4. Creates the `raven` system user with appropriate group memberships (`adm`, `docker`)
5. Installs and starts the systemd service

### Step 4: Agent Connects Automatically

Within seconds, the agent opens a gRPC connection, authenticates, sends registration info, and begins streaming metrics. It appears on the dashboard as "Online" within 10 seconds.

### Step 5: Configure Log Files

Edit `/etc/raven/agent.toml` to add log file paths, then restart:

```bash
sudo nano /etc/raven/agent.toml
sudo systemctl restart raven-agent
```

### Networking

The agent initiates an outbound TCP connection to the server on port 9090. No inbound ports need to be opened on the monitored server. Only the central server needs port 9090 (gRPC) and port 8080 (HTTP API) open. Agents behind NAT work fine.

---

## Security Model

| Concern | Solution |
|---|---|
| Agent → Server authentication | Pre-shared token, sent as gRPC metadata (`Bearer rvn_...`) |
| Transport encryption | TLS via `rustls` in `tonic` |
| Token storage on agent | `/etc/raven/agent.toml`, file permissions `root:raven 640` |
| Token revocation | Delete token from server registry → agent gets `UNAUTHENTICATED` on next reconnect |
| Dashboard authentication | JWT issued by `POST /api/auth/login`. Stored in httpOnly cookie. |
| Password storage | Argon2id hashes in SQLite. Plaintext passwords never stored. |
| User roles | `admin` (full access, user management) and `member` (read-only dashboards, own alert rules). |
| Agent token scope | Write-only. Agents can push data but cannot query or access the dashboard. Tokens are owned by the user who created them. |
| Dashboard ↔ API | CORS configured to allow only the dashboard origin |

---

## Deployment Strategy

### Self-Hosted (Docker Compose)

Single `docker-compose.yml` in the repository:

```
services:
  raven-server:      # Rust binary, ports 8080 (HTTP) + 9090 (gRPC), embeds SQLite
  victoriametrics:   # Official image, port 8428 internal
  clickhouse:        # Official image, port 8123 internal
  # no separate dashboard service
```

All services pre-configured over Docker's internal network. Volumes for VictoriaMetrics data, ClickHouse data, and SQLite database file. The dashboard bundle is embedded in the `raven-server` image and served directly by axum.

### Demo Deployment (Oracle Cloud Free Tier)

| Resource | Allocation | Purpose |
|---|---|---|
| ARM VM #1 | 2 OCPU, 12 GB RAM | Central server: `docker compose` with raven-server + VictoriaMetrics + ClickHouse |
| x86 micro VM #1 | 1/8 OCPU, 1 GB RAM | Monitored server: raven-agent + nginx + sample app |
| x86 micro VM #2 | 1/8 OCPU, 1 GB RAM | Second monitored server (optional) |
| Dashboard hosting | Included in `raven-server` | Static assets served directly by axum |

Resource usage on the ARM VM: ~2-3 GB of 12 GB RAM, ~5-10 GB of 200 GB disk. Total cost: **$0/month**.

### CI/CD (GitHub Actions)

1. On push: `clippy` lint + `cargo test`
2. On tag: cross-compile agent binary for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`
3. Build and push Docker image (`raven-server`, includes dashboard assets) to GHCR
4. Attach agent binaries to GitHub Release

---

## Further Reading

See the [implementation specification](./spec.md) for detailed component design, implementation phases, stretch goals, testing strategy, key decisions, and user stories.
