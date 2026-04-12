#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${1:-/tmp/raven-log-reader}"
ITERATIONS="${2:-40}"
SLEEP_SECS="${3:-0.2}"

mkdir -p "${ROOT_DIR}"

PLAIN_OUT="${ROOT_DIR}/app-out.log"
PLAIN_ERR="${ROOT_DIR}/app-error.log"
DOCKER_LOG="${ROOT_DIR}/docker-json.log"

: > "${PLAIN_OUT}"
: > "${PLAIN_ERR}"
: > "${DOCKER_LOG}"

iso_now() {
  date -u +"%Y-%m-%dT%H:%M:%S.%3NZ"
}

write_docker() {
  local stream="$1"
  local line="$2"
  local ts
  ts="$(iso_now)"
  printf '{"log":"%s\\n","stream":"%s","time":"%s"}\n' "${line}" "${stream}" "${ts}" >> "${DOCKER_LOG}"
}

echo "feeding logs in ${ROOT_DIR}"

for i in $(seq 1 "${ITERATIONS}"); do
  echo "plain-out line ${i}" >> "${PLAIN_OUT}"
  echo "plain-err line ${i}" >> "${PLAIN_ERR}"
  write_docker "stdout" "docker stdout ${i}"
  write_docker "stderr" "docker stderr ${i}"

  # rename + recreate rotation every 10 lines
  if (( i % 10 == 0 )); then
    mv "${PLAIN_OUT}" "${PLAIN_OUT}.${i}.rot"
    : > "${PLAIN_OUT}"

    mv "${DOCKER_LOG}" "${DOCKER_LOG}.${i}.rot"
    : > "${DOCKER_LOG}"
  fi

  # copytruncate simulation every 15 lines
  if (( i % 15 == 0 )); then
    cp "${PLAIN_ERR}" "${PLAIN_ERR}.${i}.bak"
    : > "${PLAIN_ERR}"
  fi

  sleep "${SLEEP_SECS}"
done

echo "done"
