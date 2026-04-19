#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.."; pwd)"
DEV_DIR="${ROOT_DIR}/.dev"
RUNTIME_DIR="${DEV_DIR}/runtime"
LOG_DIR="${DEV_DIR}/logs"

# One-time local setup for persisted agent id path used by the agent:
# /var/lib/raven/agent-id
if [[ ! -d /var/lib/raven || ! -w /var/lib/raven ]]; then
  echo "Preparing /var/lib/raven for local agent-id persistence..."
  sudo install -d -m 0755 -o "$USER" -g "$(id -gn)" /var/lib/raven
fi

mkdir -p "${RUNTIME_DIR}" "${LOG_DIR}" /tmp/raven-log-reader

pushd "${ROOT_DIR}" >/dev/null
cargo build -p raven-server -p raven-agent
popd >/dev/null

cp "${ROOT_DIR}/config/agent.toml" "${RUNTIME_DIR}/agent.toml"
sed -i 's|^address = .*|address = "127.0.0.1:9090"|' "${RUNTIME_DIR}/agent.toml"
sed -i 's|^token = .*|token = "rvn_dev_token"|' "${RUNTIME_DIR}/agent.toml"
sed -i 's|^tls = .*|tls = false|' "${RUNTIME_DIR}/agent.toml"

"${ROOT_DIR}/target/debug/raven-server" > "${LOG_DIR}/server.log" 2>&1 &
SERVER_PID=$!

"${ROOT_DIR}/target/debug/raven-agent" --config "${RUNTIME_DIR}/agent.toml" > "${LOG_DIR}/agent.log" 2>&1 &
AGENT_PID=$!

cleanup() {
  kill "${AGENT_PID}" >/dev/null 2>&1 || true
  kill "${SERVER_PID}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

sleep 2
"${ROOT_DIR}/scripts/log_reader_test.sh" /tmp/raven-log-reader 30 0.1
sleep 3

echo "Server log: ${LOG_DIR}/server.log"
echo "Agent log: ${LOG_DIR}/agent.log"
