#!/usr/bin/env bash
set -euo pipefail

REPO="rvnhq/raven"
VERSION=""
SERVER_ADDRESS=""
TOKEN=""
TLS="false"

usage() {
  echo "Usage: $0 --version [latest|v0.1.0-alpha.1] --server 1.2.3.4:9090 --token rvn_xxx [--tls true|false]"
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --server) SERVER_ADDRESS="$2"; shift 2 ;;
    --token) TOKEN="$2"; shift 2 ;;
    --tls) TLS="$2"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$VERSION" && -n "$SERVER_ADDRESS" && -n "$TOKEN" ]] || usage

if [[ $EUID -ne 0 ]]; then
  echo "Run as root"
  exit 1
fi

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
  *) echo "Unsupported arch: $ARCH"; exit 1 ;;
esac

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
BIN_NAME="raven-agent-${TARGET}"

curl -fsSL -o "${TMP_DIR}/${BIN_NAME}" "${BASE_URL}/${BIN_NAME}"
curl -fsSL -o "${TMP_DIR}/checksums.txt" "${BASE_URL}/checksums.txt"
curl -fsSL -o "${TMP_DIR}/agent.toml.template" "${BASE_URL}/agent.toml.template"

(cd "${TMP_DIR}" && grep " ${BIN_NAME}$" checksums.txt | sha256sum -c -)

install -m 0755 "${TMP_DIR}/${BIN_NAME}" /usr/local/bin/raven-agent

id -u raven >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin raven
getent group raven >/dev/null 2>&1 || groupadd --system raven || true

install -d -m 0750 -o root -g raven /etc/raven
install -d -m 0750 -o raven -g raven /var/lib/raven

if [[ ! -f /etc/raven/agent.toml ]]; then
  cp "${TMP_DIR}/agent.toml.template" /etc/raven/agent.toml
  sed -i "s|__SERVER_ADDRESS__|${SERVER_ADDRESS}|g" /etc/raven/agent.toml
  sed -i "s|__TOKEN__|${TOKEN}|g" /etc/raven/agent.toml
  sed -i "s|__TLS__|${TLS}|g" /etc/raven/agent.toml
  chown root:raven /etc/raven/agent.toml
  chmod 0640 /etc/raven/agent.toml
  echo "Config written to /etc/raven/agent.toml"
else
  cp "${TMP_DIR}/agent.toml.template" /etc/raven/agent.toml.new
  sed -i "s|__SERVER_ADDRESS__|${SERVER_ADDRESS}|g" /etc/raven/agent.toml.new
  sed -i "s|__TOKEN__|${TOKEN}|g" /etc/raven/agent.toml.new
  sed -i "s|__TLS__|${TLS}|g" /etc/raven/agent.toml.new
  chown root:raven /etc/raven/agent.toml.new
  chmod 0640 /etc/raven/agent.toml.new
  echo "Existing config kept. New template written to /etc/raven/agent.toml.new"
fi

cat >/etc/systemd/system/raven-agent.service <<'UNIT'
[Unit]
Description=Raven Agent
After=network-online.target
Wants=network-online.target

[Service]
User=raven
Group=raven
ExecStart=/usr/local/bin/raven-agent --config /etc/raven/agent.toml
Restart=always
RestartSec=5
StateDirectory=raven
StateDirectoryMode=0750
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now raven-agent
systemctl status --no-pager raven-agent || true
