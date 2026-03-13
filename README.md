![raven](./docs/raven-logo-horizontal-full-light.png)

> [!CAUTION]
> The project is currently under development.

Raven is a lightweight, self-hosted server monitoring and centralized logging tool. Drop a tiny agent on your Linux machines and get live system metrics, searchable logs, and automated alerts — all from one clean dashboard, without the expensive SaaS fees.

---

## The Problem

When you run multiple servers, SSHing into each one to check logs or run `htop` during an outage takes too much time. You need to know immediately if a server runs out of memory or an application crashes. There's no single place to see what's happening across all your machines.

## How Raven Solves It

Raven gives you a simple, self-hostable alternative to heavy enterprise monitoring tools. You install a tiny Rust agent on each machine you want to monitor. The agent quietly collects CPU, memory, disk, and network stats, and tails your application log files. It streams everything back to a central server in batches, where you get:

- **Live dashboard:** Real-time charts for CPU, memory, disk I/O, network, and load average across all your servers
- **Centralized logs:** Search across all your application logs in one place with full-text search, filter by host, app, time range, or stream (stdout/stderr)
- **Live log tailing:** Watch logs in real time from your browser, with color-coded stderr and pause/resume
- **Automated alerts:** Set threshold rules (e.g. CPU > 90% for 5 minutes) and get notified via Discord, Slack, or email
- **Multi-host support:** One central server, unlimited agents. Each agent connects outbound - no ports to open on monitored servers
- **Docker & PM2 logs:** Tail Docker container logs and PM2-managed Node.js app logs with zero application code changes
- **Offline resilience:** Agents buffer data locally when the server is unreachable and replay it on reconnect

## Quick Start

**1. Deploy the central server:**

<!-- ```bash
git clone https://github.com/bakayu/raven.git
cd raven
docker compose up -d
``` -->

```rs
todo!()
```

**2. Install the agent on any server you want to monitor:**

<!-- ```bash
curl -sSL https://github.com/bakayu/raven/releases/latest/download/install.sh | sudo sh -s -- \
  --server your-server-ip:9090 \
  --token rvn_your_token_here
``` -->

```rs
todo!()
```

Metrics start flowing within 10 seconds. Add log file paths to `/etc/raven/agent.toml` and restart the agent to start collecting logs.

## How It Works

1. The agent reads system metrics from `/proc` every 10 seconds and watches log files for new lines using kernel-level notifications
2. Data is batched and streamed to the central server over gRPC with TLS encryption
3. The server stores metrics in VictoriaMetrics and logs in ClickHouse
4. The dashboard queries the server's API for charts, log search, and live tailing via WebSocket
5. An alert engine evaluates threshold rules every 30 seconds and sends notifications on state changes

## Development

### Prerequisites

- `rust`: `v1.85+`
- `protoc`: any recent version
- `bun`: `v1.x`
- `docker` and `docker-compose` (optional, if you want to run the raven-server with docker): any recent version

### Running locally (without docker)

1. Start the server

```bash
cargo run -p raven-server
```

Listens on `0.0.0.0:9090` (gRPC) by default.

2. Run the agent (seperate terminal)

```bash
cargo run -p raven-agent
```

The agent connects to `localhost:9090` by default. You should see heartbeat logs in both terminals.

### Running with docker

The only difference here is how you run the server:

```bash
docker compose up --build
```

## Documentation

See the [technical documentation](./docs/) for the architecture overview, tech stack, component design, setup flow, security model, and deployment strategy.

See the [implementation specification](./docs/spec.md) for detailed design, implementation phases, user stories, testing strategy, and key decisions.

## LICENSE

The project is published under: [MIT LICENSE](./LICENSE)
