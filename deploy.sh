#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PI_HOST="${PI_HOST:-}"
PI_PASSWORD="${PI_PASSWORD:-}"
PI_APP_DIR="${PI_APP_DIR:-/opt/cat-monitor}"
PI_SERVICE_USER="${PI_SERVICE_USER:-catmonitor}"
PI_SERVICE_GROUP="${PI_SERVICE_GROUP:-${PI_SERVICE_USER}}"
PI_ENV_FILE="${PI_ENV_FILE:-${ROOT_DIR}/.env}"
BACKEND_TARGET="${BACKEND_TARGET:-arm-unknown-linux-musleabihf}"
BACKEND_BIN="${ROOT_DIR}/backend/target/${BACKEND_TARGET}/release/cat-monitor-rust-backend"
ZIGBEE_PROBE_BIN="${ROOT_DIR}/backend/target/${BACKEND_TARGET}/release/zigbee_probe"
ZIGBEE_RAW_PROBE_BIN="${ROOT_DIR}/backend/target/${BACKEND_TARGET}/release/zigbee_raw_probe"

RUNTIME_FILES=(
  devices.json
  users.json
  meross-devices.json
)

MUTABLE_RUNTIME_FILES=(
  device-cache.json
  hue-lamps.json
  hue-lamps-blacklist.json
  zigbee-lamps.json
  zigbee-lamps-blacklist.json
)

usage() {
  cat <<'EOF'
Usage:
  PI_HOST=pi@raspberrypi ./deploy.sh [all|build|push|upgrade|start|stop|logs|status]

Commands:
  all      Build locally, push to the Pi, upgrade host services, restart everything
  build    Build the frontend and cross-build the backend only
  push     Push artifacts and configs to the Pi only
  upgrade  Install or upgrade host-native dependencies on the Pi
  start    Install service definitions and restart the stack on the Pi
  stop     Stop the running stack on the Pi
  logs     Follow logs for one service or the full stack
  status   Show service status, dependency hints, and final URLs

Environment:
  PI_HOST          SSH target, required for push/upgrade/start/all/logs/status
  PI_PASSWORD      Optional SSH password used via sshpass when installed locally
  PI_APP_DIR       Remote app directory, default /opt/cat-monitor
  PI_SERVICE_USER  Service user on the Pi, default catmonitor
  PI_SERVICE_GROUP Service group on the Pi, default catmonitor
  PI_ENV_FILE      Local env file to deploy, default ./.env
  BACKEND_TARGET   Rust target triple, default arm-unknown-linux-musleabihf

Examples:
  PI_HOST=pi@192.168.1.50 ./deploy.sh all
  PI_HOST=pi@192.168.1.50 ./deploy.sh push
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

ssh_base_cmd() {
  if [ -n "${PI_PASSWORD}" ]; then
    if ! command -v sshpass >/dev/null 2>&1; then
      printf '%s\n' 'PI_PASSWORD is set but sshpass is not installed. Install it first, for example: brew install hudochenkov/sshpass/sshpass' >&2
      exit 1
    fi
    SSHPASS="${PI_PASSWORD}" sshpass -e "$@"
  else
    "$@"
  fi
}

run_local() {
  log "$*"
  "$@"
}

ssh_pi() {
  ssh_base_cmd ssh "${PI_HOST}" "$@"
}

rsync_pi() {
  ssh_base_cmd rsync "$@"
}

build_local() {
  run_local bun install --cwd "${ROOT_DIR}/frontend" --frozen-lockfile
  run_local bun run --cwd "${ROOT_DIR}/frontend" build
  run_local env TARGET="${BACKEND_TARGET}" bash "${ROOT_DIR}/scripts/build-rpi1-backend.sh"
}

prepare_remote_push() {
  require_host
  log "Preparing remote host for file sync"
  ssh_pi sh -s -- "${PI_APP_DIR}" "${PI_SERVICE_USER}" "${PI_SERVICE_GROUP}" <<'EOF'
set -eu

APP_DIR="$1"
SERVICE_USER="$2"
SERVICE_GROUP="$3"
REMOTE_USER="$(id -un)"
REMOTE_GROUP="$(id -gn)"

if [ -r /etc/os-release ]; then
  . /etc/os-release
else
  ID=unknown
fi

apk add --no-cache rsync
if ! getent group "${SERVICE_GROUP}" >/dev/null 2>&1; then
  addgroup -S "${SERVICE_GROUP}"
fi
if ! id -u "${SERVICE_USER}" >/dev/null 2>&1; then
  adduser -S -D -H -h "${APP_DIR}" -G "${SERVICE_GROUP}" -s /sbin/nologin "${SERVICE_USER}"
fi


mkdir -p \
  "${APP_DIR}/backend/target/release" \
  "${APP_DIR}/frontend/dist" \
  "${APP_DIR}/deploy/systemd" \
  "${APP_DIR}/deploy/openrc" \
  "${APP_DIR}/deploy/mosquitto" \
  "${APP_DIR}/mosquitto/certs" \
  "${APP_DIR}/cache"

chown -R "${REMOTE_USER}:${REMOTE_GROUP}" "${APP_DIR}"
chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${APP_DIR}/cache"
EOF
}

push_to_pi() {
  require_host
  prepare_remote_push

  if [ ! -f "${BACKEND_BIN}" ]; then
    printf 'Missing backend artifact: %s\n' "${BACKEND_BIN}" >&2
    printf '%s\n' 'Run ./deploy.sh build first.' >&2
    exit 1
  fi

  if [ ! -f "${ZIGBEE_PROBE_BIN}" ]; then
    printf 'Missing zigbee probe artifact: %s\n' "${ZIGBEE_PROBE_BIN}" >&2
    printf '%s\n' 'Run ./deploy.sh build first.' >&2
    exit 1
  fi

  if [ ! -f "${ZIGBEE_RAW_PROBE_BIN}" ]; then
    printf 'Missing zigbee raw probe artifact: %s\n' "${ZIGBEE_RAW_PROBE_BIN}" >&2
    printf '%s\n' 'Run ./deploy.sh build first.' >&2
    exit 1
  fi

  if [ ! -d "${ROOT_DIR}/frontend/dist" ]; then
    printf '%s\n' 'Missing frontend/dist. Run ./deploy.sh build first.' >&2
    exit 1
  fi

  log "Pushing backend artifact"
  rsync_pi -avz "${BACKEND_BIN}" "${PI_HOST}:${PI_APP_DIR}/backend/target/release/"

  log "Pushing zigbee probe artifact"
  rsync_pi -avz "${ZIGBEE_PROBE_BIN}" "${PI_HOST}:${PI_APP_DIR}/backend/target/release/"

  log "Pushing zigbee raw probe artifact"
  rsync_pi -avz "${ZIGBEE_RAW_PROBE_BIN}" "${PI_HOST}:${PI_APP_DIR}/backend/target/release/"

  log "Pushing frontend bundle"
  rsync_pi -avz "${ROOT_DIR}/frontend/dist/" "${PI_HOST}:${PI_APP_DIR}/frontend/dist/"

  if [ -f "${PI_ENV_FILE}" ]; then
    log "Pushing env file"
    rsync_pi -avz "${PI_ENV_FILE}" "${PI_HOST}:${PI_APP_DIR}/.env"
  else
    warn "Env file not found at ${PI_ENV_FILE}; keeping remote .env untouched"
  fi

  log "Pushing service templates and configs"
  rsync_pi -avz "${ROOT_DIR}/deploy/systemd/cat-monitor.service" "${PI_HOST}:${PI_APP_DIR}/deploy/systemd/"
  rsync_pi -avz "${ROOT_DIR}/deploy/systemd/cloudflared-cat-monitor.service" "${PI_HOST}:${PI_APP_DIR}/deploy/systemd/"
  rsync_pi -avz "${ROOT_DIR}/deploy/openrc/cat-monitor" "${PI_HOST}:${PI_APP_DIR}/deploy/openrc/"
  rsync_pi -avz "${ROOT_DIR}/deploy/openrc/cloudflared-cat-monitor" "${PI_HOST}:${PI_APP_DIR}/deploy/openrc/"
  rsync_pi -avz "${ROOT_DIR}/deploy/mosquitto/cat-monitor.conf" "${PI_HOST}:${PI_APP_DIR}/deploy/mosquitto/"

  if [ -d "${ROOT_DIR}/mosquitto/certs" ]; then
    log "Pushing Mosquitto certificates"
    rsync_pi -avz "${ROOT_DIR}/mosquitto/certs/" "${PI_HOST}:${PI_APP_DIR}/mosquitto/certs/"
  else
    warn "mosquitto/certs is missing locally; TLS listener deployment may fail"
  fi

  for relative_path in "${RUNTIME_FILES[@]}"; do
    if [ -f "${ROOT_DIR}/${relative_path}" ]; then
      log "Pushing ${relative_path}"
      rsync_pi -avz "${ROOT_DIR}/${relative_path}" "${PI_HOST}:${PI_APP_DIR}/"
    fi
  done

  for relative_path in "${MUTABLE_RUNTIME_FILES[@]}"; do
    if [ -f "${ROOT_DIR}/${relative_path}" ]; then
      warn "Skipping push of mutable runtime file ${relative_path}; keeping remote state"
      ## Enable on first deploy
      #log "Pushing ${relative_path}"
      #rsync_pi -avz "${ROOT_DIR}/${relative_path}" "${PI_HOST}:${PI_APP_DIR}/"
    fi
  done

  if [ -d "${ROOT_DIR}/cache" ]; then
    log "Pushing cache directory"
    rsync_pi -avz "${ROOT_DIR}/cache/" "${PI_HOST}:${PI_APP_DIR}/cache/"
  fi
}

upgrade_pi() {
  require_host
  log "Upgrading host-native services on the Pi"
  ssh_pi sh -s -- "${PI_APP_DIR}" "${PI_SERVICE_USER}" "${PI_SERVICE_GROUP}" <<'EOF'
set -eu

APP_DIR="$1"
SERVICE_USER="$2"
SERVICE_GROUP="$3"

mkdir -p "${APP_DIR}/cache/tempo"

for runtime_file in \
  "${APP_DIR}/device-cache.json" \
  "${APP_DIR}/broadlink-codes.json" \
  "${APP_DIR}/hue-lamps.json" \
  "${APP_DIR}/hue-lamps-blacklist.json" \
  "${APP_DIR}/zigbee-lamps.json" \
  "${APP_DIR}/zigbee-lamps-blacklist.json"
do
  if [ ! -e "${runtime_file}" ]; then
    case "${runtime_file}" in
      *broadlink-codes.json)
        printf '%s\n' '{"codes":[]}' > "${runtime_file}"
        ;;
      *)
        printf '%s\n' '[]' > "${runtime_file}"
        ;;
    esac
  fi
done

for mutable_path in \
  "${APP_DIR}/cache" \
  "${APP_DIR}/device-cache.json" \
  "${APP_DIR}/broadlink-codes.json" \
  "${APP_DIR}/hue-lamps.json" \
  "${APP_DIR}/hue-lamps-blacklist.json" \
  "${APP_DIR}/zigbee-lamps.json" \
  "${APP_DIR}/zigbee-lamps-blacklist.json"
do
  if [ -e "${mutable_path}" ]; then
    chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${mutable_path}"
  fi
done

if [ -r /etc/os-release ]; then
  . /etc/os-release
else
  ID=unknown
fi

apk update
apk add --no-cache bash ca-certificates curl git mosquitto rsync
if ! getent group "${SERVICE_GROUP}" >/dev/null 2>&1; then
  addgroup -S "${SERVICE_GROUP}"
fi
if ! id -u "${SERVICE_USER}" >/dev/null 2>&1; then
  adduser -S -D -H -h "${APP_DIR}" -G "${SERVICE_GROUP}" -s /sbin/nologin "${SERVICE_USER}"
fi

mkdir -p /etc/mosquitto/conf.d /etc/mosquitto/certs/cat-monitor /var/log/mosquitto /var/log

if [ -f "${APP_DIR}/deploy/mosquitto/cat-monitor.conf" ]; then
  cp "${APP_DIR}/deploy/mosquitto/cat-monitor.conf" /etc/mosquitto/conf.d/cat-monitor.conf
fi

if [ -f "${APP_DIR}/mosquitto/certs/ca.pem" ]; then
  cp "${APP_DIR}/mosquitto/certs/ca.pem" /etc/mosquitto/certs/cat-monitor/ca.pem
fi
if [ -f "${APP_DIR}/mosquitto/certs/server.pem" ]; then
  cp "${APP_DIR}/mosquitto/certs/server.pem" /etc/mosquitto/certs/cat-monitor/server.pem
fi
if [ -f "${APP_DIR}/mosquitto/certs/server-key.pem" ]; then
  cp "${APP_DIR}/mosquitto/certs/server-key.pem" /etc/mosquitto/certs/cat-monitor/server-key.pem
  chmod 600 /etc/mosquitto/certs/cat-monitor/server-key.pem
fi

chown -R mosquitto:mosquitto /etc/mosquitto/certs/cat-monitor /var/log/mosquitto 2>/dev/null || true
touch /var/log/cat-monitor.log /var/log/cloudflared-cat-monitor.log
chown "${SERVICE_USER}:${SERVICE_GROUP}" /var/log/cat-monitor.log /var/log/cloudflared-cat-monitor.log
chmod 644 /var/log/cat-monitor.log /var/log/cloudflared-cat-monitor.log

if ! command -v cloudflared >/dev/null 2>&1; then
  printf '%s\n' 'Warning: cloudflared is not installed on the Pi.' >&2
  if [ "${ID:-}" = alpine ]; then
    printf '%s\n' 'Install it manually on Alpine if you want the tunnel service enabled.' >&2
  else
    printf '%s\n' 'Install it manually, then re-run ./deploy.sh start.' >&2
  fi
fi

chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${APP_DIR}/cache" 2>/dev/null || true
EOF
}

start_pi() {
  require_host
  log "Installing service definitions and restarting the stack"
  ssh_pi sh -s -- "${PI_APP_DIR}" "${PI_SERVICE_USER}" "${PI_SERVICE_GROUP}" <<'EOF'
set -eu

APP_DIR="$1"
SERVICE_USER="$2"
SERVICE_GROUP="$3"

mkdir -p \
  "${APP_DIR}/cache" \
  "${APP_DIR}/cache/tempo" \
  "${APP_DIR}/frontend/dist" \
  "${APP_DIR}/backend/target/release" \
  "${APP_DIR}/deploy/openrc" \
  "${APP_DIR}/deploy/mosquitto"

for runtime_file in \
  "${APP_DIR}/device-cache.json" \
  "${APP_DIR}/broadlink-codes.json" \
  "${APP_DIR}/hue-lamps.json" \
  "${APP_DIR}/hue-lamps-blacklist.json" \
  "${APP_DIR}/zigbee-lamps.json" \
  "${APP_DIR}/zigbee-lamps-blacklist.json"
do
  if [ ! -e "${runtime_file}" ]; then
    case "${runtime_file}" in
      *broadlink-codes.json)
        printf '%s\n' '{"codes":[]}' > "${runtime_file}"
        ;;
      *)
        printf '%s\n' '[]' > "${runtime_file}"
        ;;
    esac
  fi
done

for mutable_path in \
  "${APP_DIR}/cache" \
  "${APP_DIR}/device-cache.json" \
  "${APP_DIR}/broadlink-codes.json" \
  "${APP_DIR}/hue-lamps.json" \
  "${APP_DIR}/hue-lamps-blacklist.json" \
  "${APP_DIR}/zigbee-lamps.json" \
  "${APP_DIR}/zigbee-lamps-blacklist.json"
do
  if [ -e "${mutable_path}" ]; then
    chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${mutable_path}"
  fi
done

if [ -r /etc/os-release ]; then
  . /etc/os-release
else
  ID=unknown
fi

sed \
  -e "s#@@APP_DIR@@#${APP_DIR}#g" \
  -e "s#@@SERVICE_USER@@#${SERVICE_USER}#g" \
  -e "s#@@SERVICE_GROUP@@#${SERVICE_GROUP}#g" \
  "${APP_DIR}/deploy/openrc/cat-monitor" | tee /etc/init.d/cat-monitor >/dev/null
chmod +x /etc/init.d/cat-monitor

sed \
  -e "s#@@APP_DIR@@#${APP_DIR}#g" \
  -e "s#@@SERVICE_USER@@#${SERVICE_USER}#g" \
  -e "s#@@SERVICE_GROUP@@#${SERVICE_GROUP}#g" \
  "${APP_DIR}/deploy/openrc/cloudflared-cat-monitor" | tee /etc/init.d/cloudflared-cat-monitor >/dev/null
chmod +x /etc/init.d/cloudflared-cat-monitor

rc-update add mosquitto default >/dev/null 2>&1 || true
rc-update add cat-monitor default >/dev/null 2>&1 || true
rc-service mosquitto restart || rc-service mosquitto start
rc-service cat-monitor restart || rc-service cat-monitor start

if command -v cloudflared >/dev/null 2>&1 && grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' "${APP_DIR}/.env"; then
  rc-update add cloudflared-cat-monitor default >/dev/null 2>&1 || true
  rc-service cloudflared-cat-monitor restart || rc-service cloudflared-cat-monitor start
else
  if ! command -v cloudflared >/dev/null 2>&1; then
    printf '%s\n' 'Skipping cloudflared service start: cloudflared is not installed on the Pi.' >&2
  elif ! grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' "${APP_DIR}/.env"; then
    printf '%s\n' 'Skipping cloudflared service start: CLOUDFLARE_TUNNEL_TOKEN is missing from the remote .env.' >&2
  fi
fi

sleep 2

for service in mosquitto cat-monitor cloudflared-cat-monitor; do
  if rc-service "${service}" status >/dev/null 2>&1; then
    printf '%s: active\n' "${service}"
  else
    printf '%s: inactive\n' "${service}"
  fi
done

EOF
}

stop_pi() {
  require_host
  log "Stopping the stack on the Pi"
  ssh_pi sh -s <<'EOF'
set -eu

if [ -r /etc/os-release ]; then
  . /etc/os-release
else
  ID=unknown
fi

for service in cloudflared-cat-monitor cat-monitor mosquitto; do
  if [ -x "/etc/init.d/${service}" ]; then
    rc-service "${service}" stop >/dev/null 2>&1 || true
  fi
done

for service in mosquitto cat-monitor cloudflared-cat-monitor; do
  if [ -x "/etc/init.d/${service}" ] && rc-service "${service}" status >/dev/null 2>&1; then
    printf '%s: active\n' "${service}"
  elif [ -x "/etc/init.d/${service}" ]; then
    printf '%s: inactive\n' "${service}"
  fi
done

EOF
}

logs_pi() {
  require_host
  local target="${1:-stack}"
  log "Following ${target} logs on the Pi"
  ssh_pi sh -s -- "${target}" <<'EOF'
set -eu
TARGET="$1"

if [ -r /etc/os-release ]; then
  . /etc/os-release
else
  ID=unknown
fi

if [ "${ID:-}" = alpine ]; then
  case "${TARGET}" in
    stack)
      touch /var/log/mosquitto/mosquitto.log /var/log/cat-monitor.log /var/log/cloudflared-cat-monitor.log
      exec tail -f /var/log/mosquitto/mosquitto.log /var/log/cat-monitor.log /var/log/cloudflared-cat-monitor.log
      ;;
    mosquitto)
      touch /var/log/mosquitto/mosquitto.log
      exec tail -f /var/log/mosquitto/mosquitto.log
      ;;
    backend|cat-monitor)
      touch /var/log/cat-monitor.log
      exec tail -f /var/log/cat-monitor.log
      ;;
    cloudflared|tunnel)
      touch /var/log/cloudflared-cat-monitor.log
      exec tail -f /var/log/cloudflared-cat-monitor.log
      ;;
    *)
      printf 'Unknown log target: %s\n' "${TARGET}" >&2
      printf '%s\n' 'Valid targets: stack, mosquitto, backend, cloudflared' >&2
      exit 1
      ;;
  esac
else
  case "${TARGET}" in
    stack)
      exec journalctl -f -u mosquitto -u cat-monitor.service -u cloudflared-cat-monitor.service
      ;;
    mosquitto)
      exec journalctl -f -u mosquitto
      ;;
    backend|cat-monitor)
      exec journalctl -f -u cat-monitor.service
      ;;
    cloudflared|tunnel)
      exec journalctl -f -u cloudflared-cat-monitor.service
      ;;
    *)
      printf 'Unknown log target: %s\n' "${TARGET}" >&2
      printf '%s\n' 'Valid targets: stack, mosquitto, backend, cloudflared' >&2
      exit 1
      ;;
  esac
fi
EOF
}

status_pi() {
  require_host
  log "Collecting deployment status from the Pi"
  ssh_pi sh -s -- "${PI_APP_DIR}" <<'EOF'
set -eu

APP_DIR="$1"

if [ -r /etc/os-release ]; then
  . /etc/os-release
else
  ID=unknown
fi

service_state_openrc() {
  if rc-service "$1" status >/dev/null 2>&1; then
    printf 'active'
  elif [ -x "/etc/init.d/$1" ]; then
    printf 'inactive'
  else
    printf 'not-installed'
  fi
}

service_state_systemd() {
  if systemctl list-unit-files "$1" >/dev/null 2>&1; then
    systemctl is-active "$1" 2>/dev/null || true
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

printf 'Platform:\n'
printf '  os: %s\n' "${PRETTY_NAME:-unknown}"
if [ "${ID:-}" = alpine ]; then
  printf '%s\n' '  init: openrc'
else
  printf '%s\n' '  init: systemd'
fi

printf '\nServices:\n'
if [ "${ID:-}" = alpine ]; then
  printf '  mosquitto: %s\n' "$(service_state_openrc mosquitto)"
  printf '  cat-monitor: %s\n' "$(service_state_openrc cat-monitor)"
  printf '  cloudflared: %s\n' "$(service_state_openrc cloudflared-cat-monitor)"
else
  printf '  mosquitto: %s\n' "$(service_state_systemd mosquitto)"
  printf '  cat-monitor: %s\n' "$(service_state_systemd cat-monitor.service)"
  printf '  cloudflared: %s\n' "$(service_state_systemd cloudflared-cat-monitor.service)"
fi

printf '\nRuntime:\n'
if command -v cloudflared >/dev/null 2>&1; then
  printf '  cloudflared: installed (%s)\n' "$(cloudflared --version 2>/dev/null | head -n1)"
else
  printf '%s\n' '  cloudflared: missing'
fi

printf '\nPaths:\n'
printf '  app: %s\n' "${APP_DIR}"
printf '  backend: %s\n' "${APP_DIR}/backend/target/release/cat-monitor-rust-backend"
printf '  frontend: %s\n' "${APP_DIR}/frontend/dist"

printf '\nAccess:\n'
printf '  local: http://%s:3033\n' "$(hostname -i 2>/dev/null | awk '{print $1}' || hostname)"
if [ -n "${public_hostname}" ]; then
  printf '  public: https://%s\n' "${public_hostname}"
else
  printf '%s\n' '  public: not configured'
fi

printf '\nHints:\n'
if ! command -v cloudflared >/dev/null 2>&1; then
  printf '%s\n' '  - Cloudflare Tunnel cannot start because cloudflared is not installed.'
elif [ "${tunnel_token_present}" != 'yes' ]; then
  printf '%s\n' '  - Cloudflare Tunnel cannot start because CLOUDFLARE_TUNNEL_TOKEN is missing in the remote .env.'
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
  stop)
    stop_pi
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
