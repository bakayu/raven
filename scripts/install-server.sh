#!/usr/bin/env bash
set -euo pipefail

REPO="rvnhq/raven"
VERSION="latest"
PUBLIC_URL=""
DATA_DIR="/var/lib/raven"
CONFIG_DIR="/etc/raven"
COMPOSE_DIR="/opt/raven"
HTTP_PORT="8080"
GRPC_PORT="9090"
TLS="false"
TLS_CERT=""
TLS_KEY=""
JWT_SIGNING_KEY=""

usage() {
  echo "Usage: $0 --public-url https://raven.example.com [--version latest|vX.Y.Z] [--data-dir /var/lib/raven] [--http-port 8080] [--grpc-port 9090] [--tls true|false] [--tls-cert /path] [--tls-key /path] [--jwt-key <secret>]"
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --public-url) PUBLIC_URL="$2"; shift 2 ;;
    --data-dir) DATA_DIR="$2"; shift 2 ;;
    --http-port) HTTP_PORT="$2"; shift 2 ;;
    --grpc-port) GRPC_PORT="$2"; shift 2 ;;
    --tls) TLS="$2"; shift 2 ;;
    --tls-cert) TLS_CERT="$2"; shift 2 ;;
    --tls-key) TLS_KEY="$2"; shift 2 ;;
    --jwt-key) JWT_SIGNING_KEY="$2"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$PUBLIC_URL" ]] || usage

if [[ $EUID -ne 0 ]]; then
  echo "Run as root"
  exit 1
fi

command -v docker >/dev/null 2>&1 || { echo "docker is required"; exit 1; }
if ! docker compose version >/dev/null 2>&1; then
  echo "docker compose plugin is required"
  exit 1
fi

if [[ -z "$JWT_SIGNING_KEY" ]]; then
  if command -v openssl >/dev/null 2>&1; then
    JWT_SIGNING_KEY="$(openssl rand -hex 32)"
  else
    JWT_SIGNING_KEY="$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 48)"
  fi
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if [[ "$VERSION" == "latest" ]]; then
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
  BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
fi

curl -fsSL -o "${TMP_DIR}/docker-compose.yml" "${BASE_URL}/docker-compose.yml"
curl -fsSL -o "${TMP_DIR}/server.toml.template" "${BASE_URL}/server.toml.template"

install -d -m 0755 "${COMPOSE_DIR}"
install -d -m 0750 "${CONFIG_DIR}"
install -d -m 0750 "${DATA_DIR}"

CONFIG_PATH="${CONFIG_DIR}/server.toml"
TARGET_CONFIG="${CONFIG_PATH}"
if [[ -f "${CONFIG_PATH}" ]]; then
  TARGET_CONFIG="${CONFIG_PATH}.new"
  echo "Existing config kept. Writing new config to ${TARGET_CONFIG}"
fi

cp "${TMP_DIR}/server.toml.template" "${TARGET_CONFIG}"
sed -i "s|__PUBLIC_BASE_URL__|${PUBLIC_URL}|g" "${TARGET_CONFIG}"
sed -i "s|__JWT_SIGNING_KEY__|${JWT_SIGNING_KEY}|g" "${TARGET_CONFIG}"

if [[ "${TLS}" == "true" ]]; then
  sed -i "s|^enabled = .*|enabled = true|" "${TARGET_CONFIG}"
  if [[ -n "${TLS_CERT}" ]]; then
    sed -i "s|^cert_path = .*|cert_path = \"${TLS_CERT}\"|" "${TARGET_CONFIG}"
  fi
  if [[ -n "${TLS_KEY}" ]]; then
    sed -i "s|^key_path = .*|key_path = \"${TLS_KEY}\"|" "${TARGET_CONFIG}"
  fi
fi

chown root:root "${TARGET_CONFIG}"
chmod 0644 "${TARGET_CONFIG}"

cp "${TMP_DIR}/docker-compose.yml" "${COMPOSE_DIR}/docker-compose.yml"

cat > "${COMPOSE_DIR}/.env" <<ENV
RAVEN_VERSION=${VERSION}
RAVEN_HTTP_PORT=${HTTP_PORT}
RAVEN_GRPC_PORT=${GRPC_PORT}
RAVEN_DATA_DIR=${DATA_DIR}
RAVEN_CONFIG_PATH=${CONFIG_PATH}
ENV

( cd "${COMPOSE_DIR}" && docker compose up -d )

cat <<EOF
Raven server is starting.

Dashboard: ${PUBLIC_URL}
Config: ${CONFIG_PATH}
Data dir: ${DATA_DIR}
Compose dir: ${COMPOSE_DIR}

If this is a fresh install, open the dashboard to create the admin account.
EOF
