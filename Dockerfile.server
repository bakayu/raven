# Stage 1 - Build dashboard 
FROM oven/bun:1 AS dashboard-builder
WORKDIR /app/dashboard
COPY dashboard/package.json dashboard/bun.lock* ./
RUN bun install --frozen-lockfile
COPY dashboard/ ./
RUN bun run build

# Stage 2 - Build Rust server 
FROM rust:1.85-slim AS rust-builder
RUN apt-get update && apt-get install -y protobuf-compiler pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY proto/ proto/
RUN cargo build --release -p raven-server

# Stage 3 -Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /var/lib/raven /etc/raven

COPY --from=rust-builder /app/target/release/raven-server /usr/local/bin/raven-server
COPY --from=dashboard-builder /app/dashboard/dist /opt/raven/dashboard

ENV RUST_LOG=raven_server=info

EXPOSE 8080 9090
CMD ["raven-server", "--config", "/etc/raven/server.toml"]
