set shell := ["bash", "-euo", "pipefail", "-c"]

agent_config := "config/agent.toml"

# Meta
default:
    @just --list

help:
    @just --list

tools:
    @command -v cargo >/dev/null || (echo "cargo is required" && exit 1)
    @command -v bun >/dev/null || (echo "bun is required" && exit 1)
    @command -v docker >/dev/null || (echo "docker is required" && exit 1)

# Bootstrap
bootstrap: tools
    cargo fetch
    cd dashboard && bun install --frozen-lockfile

clean:
    cargo clean
    rm -rf dashboard/dist

# Rust (workspace)
check:
    cargo check --workspace

build:
    cargo build --workspace

build-release:
    cargo build --workspace --release

test:
    cargo test --workspace

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

ci: fmt-check clippy test

# Rust (binaries)
run-server:
    cargo run -p raven-server

run-server-bunyan:
    cargo run -p raven-server | bunyan

run-agent:
    cargo run -p raven-agent -- --config {{ agent_config }}

run-agent-bunyan:
    cargo run -p raven-agent -- --config {{ agent_config }} | bunyan

watch-server:
    cargo watch -w crates/raven-server -w crates/raven-proto -w proto -x "run -p raven-server"

watch-agent:
    cargo watch -w crates/raven-agent -w crates/raven-proto -w proto -x "run -p raven-agent -- --config {{ agent_config }}"

# Dev (fast overrides for heartbeat + metrics)
dev-agent-fast:
    env \
      RUST_LOG=debug \
      RAVEN_METRICS__INTERVAL_SECONDS=1 \
      RAVEN_TRANSPORT__HEARTBEAT_INTERVAL_SECONDS=3 \
      cargo run -p raven-agent -- --config {{ agent_config }}

dev-agent-fast-bunyan:
    env \
      RUST_LOG=debug \
      RAVEN_METRICS__INTERVAL_SECONDS=1 \
      RAVEN_TRANSPORT__HEARTBEAT_INTERVAL_SECONDS=3 \
      cargo run -p raven-agent -- --config {{ agent_config }} | bunyan

dev-watch-agent-fast:
    env \
      RUST_LOG=debug \
      RAVEN_METRICS__INTERVAL_SECONDS=1 \
      RAVEN_TRANSPORT__HEARTBEAT_INTERVAL_SECONDS=3 \
      cargo watch -w crates/raven-agent -w crates/raven-proto -w proto -x "run -p raven-agent -- --config {{ agent_config }}"

dev-watch-server:
    RUST_LOG=debug cargo watch -w crates/raven-server -w crates/raven-proto -w proto -x "run -p raven-server"

# Proto
proto-check:
    cargo check -p raven-proto

# Dashboard
dash-install:
    cd dashboard && bun install --frozen-lockfile

dash-dev:
    cd dashboard && bun run dev

dash-build:
    cd dashboard && bun run build

dash-lint:
    cd dashboard && bun run lint

dash-preview:
    cd dashboard && bun run preview

# Docker / Compose
docker-build-server:
    docker buildx -f Dockerfile.server -t raven-server:local .

compose-build:
    docker compose build

compose-up:
    docker compose up -d

compose-down:
    docker compose down

compose-logs service="raven-server":
    docker compose logs -f {{ service }}

compose-ps:
    docker compose ps

compose-restart service="raven-server":
    docker compose restart {{ service }}

compose-prune:
    docker compose down -v --remove-orphans
