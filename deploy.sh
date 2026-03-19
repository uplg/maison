#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PI_HOST="${PI_HOST:-}"
PI_APP_DIR="${PI_APP_DIR:-/opt/cat-monitor}"
PI_Z2M_DIR="${PI_Z2M_DIR:-/opt/zigbee2mqtt}"
PI_Z2M_REPO="${PI_Z2M_REPO:-https://github.com/Koenkk/zigbee2mqtt.git}"
PI_SERVICE_USER="${PI_SERVICE_USER:-catmonitor}"
PI_SERVICE_GROUP="${PI_SERVICE_GROUP:-${PI_SERVICE_USER}}"
PI_ENV_FILE="${PI_ENV_FILE:-${ROOT_DIR}/.env}"
BACKEND_TARGET="${BACKEND_TARGET:-arm-unknown-linux-gnueabihf}"
BACKEND_BIN="${ROOT_DIR}/backend/target/${BACKEND_TARGET}/release/cat-monitor-rust-backend"

RUNTIME_FILES=(
  devices.json
  device-cache.json
  users.json
  meross-devices.json
  hue-lamps.json
  hue-lamps-blacklist.json
  zigbee-lamps.json
  zigbee-lamps-blacklist.json
)

usage() {
  cat <<'EOF'
Usage:
  PI_HOST=pi@raspberrypi ./deploy.sh [all|build|push|upgrade|start|logs|status]

Commands:
  all      Build locally, push to the Pi, upgrade host services, restart everything
  build    Build the frontend and cross-build the backend only
  push     Push artifacts and configs to the Pi only
  upgrade  Upgrade/install host-native pieces on the Pi only
  start    Install/reload systemd units and restart services on the Pi only
  logs     Follow logs for one service or the full stack
  status   Show service status, dependency hints, and final URLs

Environment:
  PI_HOST          SSH target, required for push/upgrade/start/all
  PI_APP_DIR       Remote app directory, default /opt/cat-monitor
  PI_Z2M_DIR       Remote Zigbee2MQTT directory, default /opt/zigbee2mqtt
  PI_Z2M_REPO      Zigbee2MQTT git URL
  PI_SERVICE_USER  Service user on the Pi, default catmonitor
  PI_SERVICE_GROUP Service group on the Pi, default catmonitor
  PI_ENV_FILE      Local env file to deploy, default ./.env
  BACKEND_TARGET   Rust target triple, default arm-unknown-linux-gnueabihf

Examples:
  PI_HOST=pi@192.168.1.50 ./deploy.sh all
  PI_HOST=pi@192.168.1.50 ./deploy.sh push
  PI_HOST=pi@192.168.1.50 PI_APP_DIR=/srv/cat-monitor ./deploy.sh start
  PI_HOST=pi@192.168.1.50 ./deploy.sh logs backend
  PI_HOST=pi@192.168.1.50 ./deploy.sh status
EOF
}

log() {
  printf '\n==> %s\n' "$*"
}

warn() {
  printf 'Warning: %s\n' "$*" >&2
}

require_host() {
  if [ -z "${PI_HOST}" ]; then
    printf '%s\n' 'Missing PI_HOST. Example: PI_HOST=pi@192.168.1.50 ./deploy.sh all' >&2
    exit 1
  fi
}

run_local() {
  log "$*"
  "$@"
}

ssh_pi() {
  ssh "${PI_HOST}" "$@"
}

prepare_remote_push() {
  require_host
  log "Preparing remote host for file sync"
  ssh "${PI_HOST}" bash -s -- "${PI_APP_DIR}" "${PI_Z2M_DIR}" "${PI_SERVICE_USER}" "${PI_SERVICE_GROUP}" <<'EOF'
set -euo pipefail

APP_DIR="$1"
Z2M_DIR="$2"
SERVICE_USER="$3"
SERVICE_GROUP="$4"

sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y rsync

if ! getent group "${SERVICE_GROUP}" >/dev/null 2>&1; then
  sudo groupadd --system "${SERVICE_GROUP}"
fi

if ! id -u "${SERVICE_USER}" >/dev/null 2>&1; then
  sudo useradd --system --gid "${SERVICE_GROUP}" --home-dir "${APP_DIR}" --create-home --shell /usr/sbin/nologin "${SERVICE_USER}"
fi

sudo usermod -a -G dialout "${SERVICE_USER}" || true

sudo mkdir -p \
  "${APP_DIR}/backend/target/release" \
  "${APP_DIR}/frontend/dist" \
  "${APP_DIR}/deploy/systemd" \
  "${APP_DIR}/deploy/mosquitto" \
  "${APP_DIR}/zigbee2mqtt" \
  "${APP_DIR}/mosquitto/certs" \
  "${APP_DIR}/cache" \
  "${Z2M_DIR}"

sudo chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${APP_DIR}" "${Z2M_DIR}"
EOF
}

build_local() {
  run_local bun --cwd "${ROOT_DIR}/frontend" install --frozen-lockfile
  run_local bun --cwd "${ROOT_DIR}/frontend" run build
  run_local bash "${ROOT_DIR}/scripts/build-rpi1-backend.sh"
}

push_to_pi() {
  require_host
  prepare_remote_push

  if [ ! -f "${BACKEND_BIN}" ]; then
    printf 'Missing backend artifact: %s\n' "${BACKEND_BIN}" >&2
    printf '%s\n' 'Run ./deploy.sh build first.' >&2
    exit 1
  fi

  if [ ! -d "${ROOT_DIR}/frontend/dist" ]; then
    printf '%s\n' 'Missing frontend/dist. Run ./deploy.sh build first.' >&2
    exit 1
  fi

  log "Pushing backend artifact"
  rsync -avz "${BACKEND_BIN}" "${PI_HOST}:${PI_APP_DIR}/backend/target/release/"

  log "Pushing frontend bundle"
  rsync -avz "${ROOT_DIR}/frontend/dist/" "${PI_HOST}:${PI_APP_DIR}/frontend/dist/"

  if [ -f "${PI_ENV_FILE}" ]; then
    log "Pushing env file"
    rsync -avz "${PI_ENV_FILE}" "${PI_HOST}:${PI_APP_DIR}/.env"
  else
    warn "Env file not found at ${PI_ENV_FILE}; keeping remote .env untouched"
  fi

  log "Pushing service templates and configs"
  rsync -avz "${ROOT_DIR}/deploy/systemd/cat-monitor.service" "${PI_HOST}:${PI_APP_DIR}/deploy/systemd/"
  rsync -avz "${ROOT_DIR}/deploy/systemd/cloudflared-cat-monitor.service" "${PI_HOST}:${PI_APP_DIR}/deploy/systemd/"
  rsync -avz "${ROOT_DIR}/deploy/mosquitto/cat-monitor.conf" "${PI_HOST}:${PI_APP_DIR}/deploy/mosquitto/"
  rsync -avz "${ROOT_DIR}/zigbee2mqtt/configuration.yaml" "${PI_HOST}:${PI_APP_DIR}/zigbee2mqtt/"
  rsync -avz "${ROOT_DIR}/zigbee2mqtt/zigbee2mqtt.service" "${PI_HOST}:${PI_APP_DIR}/zigbee2mqtt/"

  if [ -d "${ROOT_DIR}/mosquitto/certs" ]; then
    log "Pushing Mosquitto certificates"
    rsync -avz "${ROOT_DIR}/mosquitto/certs/" "${PI_HOST}:${PI_APP_DIR}/mosquitto/certs/"
  else
    warn "mosquitto/certs is missing locally; TLS listener deployment may fail"
  fi

  for relative_path in "${RUNTIME_FILES[@]}"; do
    if [ -f "${ROOT_DIR}/${relative_path}" ]; then
      log "Pushing ${relative_path}"
      rsync -avz "${ROOT_DIR}/${relative_path}" "${PI_HOST}:${PI_APP_DIR}/"
    fi
  done

  if [ -d "${ROOT_DIR}/cache" ]; then
    log "Pushing cache directory"
    rsync -avz "${ROOT_DIR}/cache/" "${PI_HOST}:${PI_APP_DIR}/cache/"
  fi
}

upgrade_pi() {
  require_host
  log "Upgrading host-native services on the Pi"
  ssh "${PI_HOST}" bash -s -- "${PI_APP_DIR}" "${PI_Z2M_DIR}" "${PI_Z2M_REPO}" "${PI_SERVICE_USER}" "${PI_SERVICE_GROUP}" <<'EOF'
set -euo pipefail

APP_DIR="$1"
Z2M_DIR="$2"
Z2M_REPO="$3"
SERVICE_USER="$4"
SERVICE_GROUP="$5"

sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y mosquitto mosquitto-clients git curl ca-certificates rsync
sudo DEBIAN_FRONTEND=noninteractive apt-get upgrade -y

if ! getent group "${SERVICE_GROUP}" >/dev/null 2>&1; then
  sudo groupadd --system "${SERVICE_GROUP}"
fi

if ! id -u "${SERVICE_USER}" >/dev/null 2>&1; then
  sudo useradd --system --gid "${SERVICE_GROUP}" --home-dir "${APP_DIR}" --create-home --shell /usr/sbin/nologin "${SERVICE_USER}"
fi

sudo usermod -a -G dialout "${SERVICE_USER}" || true

sudo mkdir -p "${Z2M_DIR}"
sudo chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${Z2M_DIR}"

if [ ! -d "${Z2M_DIR}/.git" ]; then
  sudo -u "${SERVICE_USER}" git clone --depth 1 "${Z2M_REPO}" "${Z2M_DIR}"
else
  sudo -u "${SERVICE_USER}" git -C "${Z2M_DIR}" fetch --depth 1 origin
  sudo -u "${SERVICE_USER}" git -C "${Z2M_DIR}" pull --ff-only
fi

if command -v pnpm >/dev/null 2>&1; then
  printf '%s\n' 'Using pnpm from PATH for Zigbee2MQTT install'
  sudo -u "${SERVICE_USER}" sh -lc 'cd "$1" && pnpm install --frozen-lockfile' sh "${Z2M_DIR}"
elif command -v corepack >/dev/null 2>&1; then
  printf '%s\n' 'pnpm not found; trying corepack enable for Zigbee2MQTT install'
  sudo -u "${SERVICE_USER}" sh -lc 'corepack enable && cd "$1" && pnpm install --frozen-lockfile' sh "${Z2M_DIR}"
else
  printf '%s\n' 'Warning: pnpm and corepack are both missing on the Pi.' >&2
  printf '%s\n' 'Install Node.js with pnpm support, or install pnpm globally before restarting Zigbee2MQTT.' >&2
  printf '%s\n' 'Zigbee2MQTT code was updated, but dependencies were not installed.' >&2
fi

sudo mkdir -p "${Z2M_DIR}/data"
if [ -f "${APP_DIR}/zigbee2mqtt/configuration.yaml" ]; then
  sudo cp "${APP_DIR}/zigbee2mqtt/configuration.yaml" "${Z2M_DIR}/data/configuration.yaml"
fi

sudo mkdir -p /etc/mosquitto/conf.d /etc/mosquitto/certs/cat-monitor
if [ -f "${APP_DIR}/deploy/mosquitto/cat-monitor.conf" ]; then
  sudo cp "${APP_DIR}/deploy/mosquitto/cat-monitor.conf" /etc/mosquitto/conf.d/cat-monitor.conf
fi

if [ -f "${APP_DIR}/mosquitto/certs/ca.pem" ]; then
  sudo cp "${APP_DIR}/mosquitto/certs/ca.pem" /etc/mosquitto/certs/cat-monitor/ca.pem
fi
if [ -f "${APP_DIR}/mosquitto/certs/server.pem" ]; then
  sudo cp "${APP_DIR}/mosquitto/certs/server.pem" /etc/mosquitto/certs/cat-monitor/server.pem
fi
if [ -f "${APP_DIR}/mosquitto/certs/server-key.pem" ]; then
  sudo cp "${APP_DIR}/mosquitto/certs/server-key.pem" /etc/mosquitto/certs/cat-monitor/server-key.pem
  sudo chmod 600 /etc/mosquitto/certs/cat-monitor/server-key.pem
fi
sudo chown -R mosquitto:mosquitto /etc/mosquitto/certs/cat-monitor || true

sudo chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${APP_DIR}" "${Z2M_DIR}"
EOF
}

start_pi() {
  require_host
  log "Installing service units and restarting the stack"
  ssh "${PI_HOST}" bash -s -- "${PI_APP_DIR}" "${PI_Z2M_DIR}" "${PI_SERVICE_USER}" "${PI_SERVICE_GROUP}" <<'EOF'
set -euo pipefail

APP_DIR="$1"
Z2M_DIR="$2"
SERVICE_USER="$3"
SERVICE_GROUP="$4"

sudo sed \
  -e "s#/opt/cat-monitor#${APP_DIR}#g" \
  -e "s#User=catmonitor#User=${SERVICE_USER}#g" \
  -e "s#Group=catmonitor#Group=${SERVICE_GROUP}#g" \
  "${APP_DIR}/deploy/systemd/cat-monitor.service" | sudo tee /etc/systemd/system/cat-monitor.service >/dev/null

sudo sed \
  -e "s#/opt/cat-monitor#${APP_DIR}#g" \
  -e "s#User=catmonitor#User=${SERVICE_USER}#g" \
  -e "s#Group=catmonitor#Group=${SERVICE_GROUP}#g" \
  "${APP_DIR}/deploy/systemd/cloudflared-cat-monitor.service" | sudo tee /etc/systemd/system/cloudflared-cat-monitor.service >/dev/null

sudo sed \
  -e "s#/opt/zigbee2mqtt#${Z2M_DIR}#g" \
  -e "s#User=catmonitor#User=${SERVICE_USER}#g" \
  "${APP_DIR}/zigbee2mqtt/zigbee2mqtt.service" | sudo tee /etc/systemd/system/zigbee2mqtt.service >/dev/null

sudo systemctl daemon-reload

sudo systemctl enable mosquitto
sudo systemctl enable zigbee2mqtt.service
sudo systemctl enable cat-monitor.service

sudo systemctl restart mosquitto
sudo systemctl restart zigbee2mqtt.service
sudo systemctl restart cat-monitor.service

if command -v cloudflared >/dev/null 2>&1 && grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' "${APP_DIR}/.env"; then
  sudo systemctl enable cloudflared-cat-monitor.service
  sudo systemctl restart cloudflared-cat-monitor.service
else
  if ! command -v cloudflared >/dev/null 2>&1; then
    printf '%s\n' 'Skipping cloudflared service start: cloudflared is not installed on the Pi.' >&2
    printf '%s\n' 'Install it manually, then re-run ./deploy.sh start or ./deploy.sh status.' >&2
  elif ! grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' "${APP_DIR}/.env"; then
    printf '%s\n' 'Skipping cloudflared service start: CLOUDFLARE_TUNNEL_TOKEN is missing from the remote .env.' >&2
  fi
fi

printf 'mosquitto: %s\n' "$(systemctl is-active mosquitto || true)"
printf 'zigbee2mqtt: %s\n' "$(systemctl is-active zigbee2mqtt.service || true)"
printf 'cat-monitor: %s\n' "$(systemctl is-active cat-monitor.service || true)"
if systemctl list-unit-files cloudflared-cat-monitor.service >/dev/null 2>&1; then
  printf 'cloudflared: %s\n' "$(systemctl is-active cloudflared-cat-monitor.service || true)"
fi
EOF
}

logs_pi() {
  require_host
  local target="${1:-stack}"

  case "${target}" in
    stack)
      log "Following stack logs on the Pi"
      ssh_pi "sudo journalctl -f -u mosquitto -u zigbee2mqtt.service -u cat-monitor.service -u cloudflared-cat-monitor.service"
      ;;
    mosquitto)
      log "Following mosquitto logs"
      ssh_pi "sudo journalctl -f -u mosquitto"
      ;;
    zigbee|zigbee2mqtt)
      log "Following Zigbee2MQTT logs"
      ssh_pi "sudo journalctl -f -u zigbee2mqtt.service"
      ;;
    backend|cat-monitor)
      log "Following cat-monitor backend logs"
      ssh_pi "sudo journalctl -f -u cat-monitor.service"
      ;;
    cloudflared|tunnel)
      log "Following cloudflared logs"
      ssh_pi "sudo journalctl -f -u cloudflared-cat-monitor.service"
      ;;
    *)
      printf 'Unknown log target: %s\n' "${target}" >&2
      printf '%s\n' 'Valid targets: stack, mosquitto, zigbee2mqtt, backend, cloudflared' >&2
      exit 1
      ;;
  esac
}

status_pi() {
  require_host
  log "Collecting deployment status from the Pi"
  ssh "${PI_HOST}" bash -s -- "${PI_APP_DIR}" "${PI_Z2M_DIR}" <<'EOF'
set -euo pipefail

APP_DIR="$1"
Z2M_DIR="$2"

service_state() {
  local unit="$1"
  if systemctl list-unit-files "$unit" >/dev/null 2>&1; then
    systemctl is-active "$unit" 2>/dev/null || true
  else
    printf 'not-installed'
  fi
}

public_hostname=''
if [ -f "${APP_DIR}/.env" ]; then
  public_hostname="$(grep -E '^CLOUDFLARE_PUBLIC_HOSTNAME=' "${APP_DIR}/.env" | tail -n1 | cut -d'=' -f2-)"
fi

tunnel_token_present='no'
if [ -f "${APP_DIR}/.env" ] && grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' "${APP_DIR}/.env"; then
  tunnel_token_present='yes'
fi

printf 'Services:\n'
printf '  mosquitto: %s\n' "$(service_state mosquitto)"
printf '  zigbee2mqtt: %s\n' "$(service_state zigbee2mqtt.service)"
printf '  cat-monitor: %s\n' "$(service_state cat-monitor.service)"
printf '  cloudflared: %s\n' "$(service_state cloudflared-cat-monitor.service)"

printf '\nRuntime:\n'
if command -v pnpm >/dev/null 2>&1; then
  printf '  pnpm: installed (%s)\n' "$(pnpm --version 2>/dev/null || printf 'unknown')"
elif command -v corepack >/dev/null 2>&1; then
  printf '%s\n' '  pnpm: missing, but corepack is available'
else
  printf '%s\n' '  pnpm: missing and corepack is missing'
fi

if command -v cloudflared >/dev/null 2>&1; then
  printf '  cloudflared: installed (%s)\n' "$(cloudflared --version 2>/dev/null | head -n1)"
else
  printf '%s\n' '  cloudflared: missing'
fi

printf '\nPaths:\n'
printf '  app: %s\n' "${APP_DIR}"
printf '  backend: %s\n' "${APP_DIR}/backend/target/release/cat-monitor-rust-backend"
printf '  frontend: %s\n' "${APP_DIR}/frontend/dist"
printf '  zigbee2mqtt: %s\n' "${Z2M_DIR}"

printf '\nAccess:\n'
printf '  local: http://%s:3033\n' "$(hostname -I 2>/dev/null | awk '{print $1}' || hostname)"
if [ -n "${public_hostname}" ]; then
  printf '  public: https://%s\n' "${public_hostname}"
else
  printf '%s\n' '  public: not configured'
fi

printf '\nHints:\n'
if [ "$(service_state zigbee2mqtt.service)" != 'active' ]; then
  if ! command -v pnpm >/dev/null 2>&1 && ! command -v corepack >/dev/null 2>&1; then
    printf '%s\n' '  - Zigbee2MQTT may fail because pnpm/corepack is missing on the Pi.'
  fi
fi

if [ "$(service_state cloudflared-cat-monitor.service)" != 'active' ]; then
  if ! command -v cloudflared >/dev/null 2>&1; then
    printf '%s\n' '  - Cloudflare Tunnel cannot start because cloudflared is not installed.'
  elif [ "${tunnel_token_present}" != 'yes' ]; then
    printf '%s\n' '  - Cloudflare Tunnel cannot start because CLOUDFLARE_TUNNEL_TOKEN is missing in the remote .env.'
  fi
fi
EOF
}

COMMAND="${1:-all}"
LOG_TARGET="${2:-stack}"

case "${COMMAND}" in
  all)
    build_local
    push_to_pi
    upgrade_pi
    start_pi
    ;;
  build)
    build_local
    ;;
  push)
    push_to_pi
    ;;
  upgrade)
    upgrade_pi
    ;;
  start)
    start_pi
    ;;
  logs)
    logs_pi "${LOG_TARGET}"
    ;;
  status)
    status_pi
    ;;
  help|-h|--help)
    usage
    ;;
  *)
    printf 'Unknown command: %s\n\n' "${COMMAND}" >&2
    usage >&2
    exit 1
    ;;
esac
