#!/usr/bin/env bash
set -euo pipefail

IMAGE_PATH=""
DISK_INPUT=""
OVERLAY_PATH=""
OVERLAY_URL="https://raw.githubusercontent.com/macmpi/alpine-linux-headless-bootstrap/main/headless.apkovl.tar.gz"
AUTHORIZED_KEYS_PATH=""
INTERFACES_PATH=""
WPA_SUPPLICANT_PATH=""
UNATTENDED_PATH=""
SSH_HOST_KEYS_DIR=""
HOSTNAME_VALUE="maison"
NO_CONFIRM=0
KEEP_MOUNTED=0

usage() {
  cat <<'EOF'
Usage:
  scripts/flash-alpine-headless-macos.sh --image alpine.img.xz --disk diskN [options]

Required:
  --image PATH              Alpine .img.xz image to flash
  --disk DISK              Target disk, for example disk4 or /dev/disk4

Optional:
  --overlay PATH           Local headless.apkovl.tar.gz file
  --authorized-keys PATH   Public keys file copied as authorized_keys
  --interfaces PATH        Alpine interfaces file copied to boot media root
  --wpa-supplicant PATH    wpa_supplicant.conf copied to boot media root
  --unattended PATH        unattended.sh copied to boot media root
  --ssh-host-keys DIR      Directory containing ssh_host_*_key* files
  --hostname NAME          Hostname applied at bootstrap, default maison
  --no-confirm             Skip destructive confirmation prompt
  --keep-mounted           Leave the boot partition mounted at the end
  -h, --help               Show this help

Behavior:
  - flashes the provided Alpine image to the SD card
  - mounts the first partition after flashing
  - copies headless.apkovl.tar.gz to the boot partition root
  - optionally injects authorized_keys and other Alpine headless bootstrap files

Notes:
  - if --authorized-keys is not provided, the script auto-detects the first local
    public key from ~/.ssh/id_ed25519.pub, ~/.ssh/id_ecdsa.pub, ~/.ssh/id_rsa.pub
  - if --interfaces is not provided, the script generates a DHCP ethernet config for eth0
  - if --unattended is not provided, the script generates one that sets the hostname
  - without authorized_keys, the overlay boots with Alpine's initial headless root SSH flow
EOF
}

log() {
  printf '\n==> %s\n' "$*"
}

warn() {
  printf 'Warning: %s\n' "$*" >&2
}

fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "Missing required command: $1"
}

cleanup() {
  if [ -n "${STAGING_DIR:-}" ] && [ -d "${STAGING_DIR}" ]; then
    rm -rf "${STAGING_DIR}"
  fi
}

trap cleanup EXIT

normalize_disk_names() {
  case "${DISK_INPUT}" in
    /dev/rdisk*)
      RAW_DISK="${DISK_INPUT}"
      BLOCK_DISK="/dev/${DISK_INPUT#/dev/r}"
      ;;
    /dev/disk*)
      BLOCK_DISK="${DISK_INPUT}"
      RAW_DISK="/dev/r${DISK_INPUT#/dev/}"
      ;;
    rdisk*)
      RAW_DISK="/dev/${DISK_INPUT}"
      BLOCK_DISK="/dev/${DISK_INPUT#r}"
      ;;
    disk*)
      BLOCK_DISK="/dev/${DISK_INPUT}"
      RAW_DISK="/dev/r${DISK_INPUT}"
      ;;
    *)
      fail "Unsupported disk identifier: ${DISK_INPUT}"
      ;;
  esac

  BOOT_PARTITION="${BLOCK_DISK}s1"
}

auto_detect_authorized_keys() {
  if [ -n "${AUTHORIZED_KEYS_PATH}" ]; then
    return
  fi

  for candidate in \
    "${HOME}/.ssh/id_ed25519.pub" \
    "${HOME}/.ssh/id_ecdsa.pub" \
    "${HOME}/.ssh/id_rsa.pub"
  do
    if [ -f "${candidate}" ]; then
      AUTHORIZED_KEYS_PATH="${candidate}"
      log "Using local SSH public key ${AUTHORIZED_KEYS_PATH}"
      return
    fi
  done

  warn "No local SSH public key auto-detected; overlay will boot without preloaded authorized_keys"
}

download_or_copy_overlay() {
  if [ -n "${OVERLAY_PATH}" ]; then
    [ -f "${OVERLAY_PATH}" ] || fail "Overlay file not found: ${OVERLAY_PATH}"
    cp "${OVERLAY_PATH}" "${STAGING_DIR}/headless.apkovl.tar.gz"
    return
  fi

  log "Downloading Alpine headless bootstrap overlay"
  curl -fsSL "${OVERLAY_URL}" -o "${STAGING_DIR}/headless.apkovl.tar.gz"
}

stage_optional_file() {
  local source_path="$1"
  local target_name="$2"

  if [ -n "${source_path}" ]; then
    [ -f "${source_path}" ] || fail "File not found: ${source_path}"
    cp "${source_path}" "${STAGING_DIR}/${target_name}"
  fi
}

stage_optional_ssh_host_keys() {
  local file_path

  if [ -z "${SSH_HOST_KEYS_DIR}" ]; then
    return
  fi

  [ -d "${SSH_HOST_KEYS_DIR}" ] || fail "SSH host keys directory not found: ${SSH_HOST_KEYS_DIR}"

  for file_path in "${SSH_HOST_KEYS_DIR}"/ssh_host_*_key*; do
    if [ -e "${file_path}" ]; then
      cp "${file_path}" "${STAGING_DIR}/"
    fi
  done
}

generate_default_interfaces() {
  if [ -n "${INTERFACES_PATH}" ]; then
    return
  fi

  cat > "${STAGING_DIR}/interfaces" <<'EOF'
auto lo
iface lo inet loopback

auto eth0
iface eth0 inet dhcp
EOF
}

generate_default_unattended() {
  if [ -n "${UNATTENDED_PATH}" ]; then
    return
  fi

  cat > "${STAGING_DIR}/unattended.sh" <<EOF
#!/bin/sh
set -eu

cat > /etc/hostname <<'EON'
${HOSTNAME_VALUE}
EON
hostname '${HOSTNAME_VALUE}' || true

cat > /etc/hosts <<'EON'
127.0.0.1 localhost ${HOSTNAME_VALUE}
::1 localhost ip6-localhost ip6-loopback
EON
EOF
  chmod +x "${STAGING_DIR}/unattended.sh"
}

confirm_target_disk() {
  if [ "${NO_CONFIRM}" -eq 1 ]; then
    return
  fi

  log "Target disk summary"
  diskutil info "${BLOCK_DISK}" || true
  printf '\nAbout to erase and flash %s\n' "${BLOCK_DISK}"
  printf 'Type the exact disk identifier to continue (%s): ' "${DISK_INPUT##*/}"

  read -r confirmation
  if [ "${confirmation}" != "${DISK_INPUT##*/}" ] && [ "${confirmation}" != "${BLOCK_DISK##*/}" ]; then
    fail "Confirmation did not match target disk"
  fi
}

flash_image() {
  log "Unmounting target disk"
  sudo diskutil unmountDisk force "${BLOCK_DISK}"

  log "Flashing Alpine image to ${RAW_DISK}"
  case "${IMAGE_PATH}" in
    *.img.xz|*.xz)
      if command -v xzcat >/dev/null 2>&1; then
        xzcat "${IMAGE_PATH}" | sudo dd of="${RAW_DISK}" bs=1m
      else
        xz -dc "${IMAGE_PATH}" | sudo dd of="${RAW_DISK}" bs=1m
      fi
      ;;
    *.img.gz|*.gz)
      gzip -dc "${IMAGE_PATH}" | sudo dd of="${RAW_DISK}" bs=1m
      ;;
    *.img)
      sudo dd if="${IMAGE_PATH}" of="${RAW_DISK}" bs=1m
      ;;
    *)
      fail "Unsupported image format for ${IMAGE_PATH}. Expected .img, .img.xz, or .img.gz"
      ;;
  esac

  sync
}

boot_mount_point() {
  diskutil info "${BOOT_PARTITION}" 2>/dev/null | sed -n 's/^ *Mount Point: *//p' | head -n1
}

mount_boot_partition() {
  local attempt=0
  local mount_point=""

  while [ "${attempt}" -lt 10 ]; do
    attempt=$((attempt + 1))
    diskutil mount "${BOOT_PARTITION}" >/dev/null 2>&1 || diskutil mountDisk "${BLOCK_DISK}" >/dev/null 2>&1 || true
    mount_point="$(boot_mount_point)"
    if [ -n "${mount_point}" ] && [ -d "${mount_point}" ]; then
      printf '%s\n' "${mount_point}"
      return
    fi
    sleep 2
  done

  fail "Could not mount ${BOOT_PARTITION} after flashing. Reinsert the card and copy files from the staging directory manually."
}

copy_bootstrap_files() {
  local mount_point="$1"

  log "Copying Alpine headless bootstrap files to ${mount_point}"
  cp "${STAGING_DIR}/"* "${mount_point}/"
  sync
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --image)
        IMAGE_PATH="$2"
        shift 2
        ;;
      --disk)
        DISK_INPUT="$2"
        shift 2
        ;;
      --overlay)
        OVERLAY_PATH="$2"
        shift 2
        ;;
      --authorized-keys)
        AUTHORIZED_KEYS_PATH="$2"
        shift 2
        ;;
      --interfaces)
        INTERFACES_PATH="$2"
        shift 2
        ;;
      --wpa-supplicant)
        WPA_SUPPLICANT_PATH="$2"
        shift 2
        ;;
      --unattended)
        UNATTENDED_PATH="$2"
        shift 2
        ;;
      --ssh-host-keys)
        SSH_HOST_KEYS_DIR="$2"
        shift 2
        ;;
      --hostname)
        HOSTNAME_VALUE="$2"
        shift 2
        ;;
      --no-confirm)
        NO_CONFIRM=1
        shift
        ;;
      --keep-mounted)
        KEEP_MOUNTED=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        fail "Unknown argument: $1"
        ;;
    esac
  done
}

main() {
  parse_args "$@"

  [ "$(uname -s)" = "Darwin" ] || fail "This script is intended for macOS"
  [ -n "${IMAGE_PATH}" ] || fail "Missing --image"
  [ -n "${DISK_INPUT}" ] || fail "Missing --disk"
  [ -f "${IMAGE_PATH}" ] || fail "Image not found: ${IMAGE_PATH}"

  require_command diskutil
  require_command dd
  require_command sudo
  require_command curl
  case "${IMAGE_PATH}" in
    *.img.xz|*.xz)
      if ! command -v xzcat >/dev/null 2>&1 && ! command -v xz >/dev/null 2>&1; then
        fail "Missing xz support. Install xz so the .img.xz file can be decompressed"
      fi
      ;;
    *.img.gz|*.gz)
      require_command gzip
      ;;
    *.img)
      ;;
    *)
      fail "Unsupported image format for ${IMAGE_PATH}. Expected .img, .img.xz, or .img.gz"
      ;;
  esac

  normalize_disk_names
  auto_detect_authorized_keys
  confirm_target_disk

  STAGING_DIR="$(mktemp -d)"
  download_or_copy_overlay
  stage_optional_file "${AUTHORIZED_KEYS_PATH}" "authorized_keys"
  stage_optional_file "${INTERFACES_PATH}" "interfaces"
  stage_optional_file "${WPA_SUPPLICANT_PATH}" "wpa_supplicant.conf"
  stage_optional_file "${UNATTENDED_PATH}" "unattended.sh"
  stage_optional_ssh_host_keys
  generate_default_interfaces
  generate_default_unattended

  flash_image

  BOOT_MOUNT_POINT="$(mount_boot_partition)"
  copy_bootstrap_files "${BOOT_MOUNT_POINT}"

  if [ "${KEEP_MOUNTED}" -eq 1 ]; then
    log "Done. Boot partition left mounted at ${BOOT_MOUNT_POINT}"
  else
    log "Ejecting SD card"
    diskutil eject "${BLOCK_DISK}"
  fi

  printf '\nReady. Insert the SD card into the Pi and boot it headless.\n'
  if [ -n "${AUTHORIZED_KEYS_PATH}" ]; then
    printf 'SSH should accept your injected public key.\n'
  else
    printf 'No authorized_keys was injected; follow Alpine headless bootstrap defaults for first login.\n'
  fi
}

main "$@"
