#!/usr/bin/env bash
set -euo pipefail

# Cross-compile cloudflared for Raspberry Pi 1 (ARMv6)
#
# Expects the cloudflared source repo at ../cloudflared relative to this repo,
# or override with CLOUDFLARED_SRC.
#
# The compiled binary lands in this repo at:
#   cloudflared-arm (gitignored)
#
# Usage:
#   ./scripts/build-cloudflared-armv6.sh           # build from ../cloudflared
#   CLOUDFLARED_SRC=/path/to/cloudflared ./scripts/build-cloudflared-armv6.sh

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "${SCRIPT_DIR}")"
CLOUDFLARED_SRC="${CLOUDFLARED_SRC:-${REPO_ROOT}/../cloudflared}"
OUTPUT="${REPO_ROOT}/cloudflared-arm"

if [ ! -d "${CLOUDFLARED_SRC}" ]; then
  printf 'Error: cloudflared source not found at %s\n' "${CLOUDFLARED_SRC}" >&2
  printf 'Clone it with: git clone https://github.com/cloudflare/cloudflared.git %s\n' "${CLOUDFLARED_SRC}" >&2
  exit 1
fi

if ! command -v go >/dev/null 2>&1; then
  printf 'Error: go is not installed. Install Go first.\n' >&2
  exit 1
fi

# Pull latest unless SKIP_PULL is set
if [ "${SKIP_PULL:-0}" != "1" ]; then
  printf '==> Pulling latest cloudflared source\n'
  git -C "${CLOUDFLARED_SRC}" pull --ff-only || {
    printf 'Warning: git pull failed, building from current checkout\n' >&2
  }
fi

# Determine version and commit from git
VERSION="$(git -C "${CLOUDFLARED_SRC}" describe --tags --abbrev=0 2>/dev/null || echo "unknown")"
COMMIT="$(git -C "${CLOUDFLARED_SRC}" rev-parse --short HEAD)"
DATE="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

printf '==> Cross-compiling cloudflared %s (%s) for linux/arm (ARMv6)\n' "${VERSION}" "${COMMIT}"

cd "${CLOUDFLARED_SRC}"

CGO_ENABLED=0 GOOS=linux GOARCH=arm GOARM=6 \
  go build \
    -ldflags "-X main.Version=${VERSION} -X main.BuildTime=${DATE} -X main.GitCommit=${COMMIT}" \
    -o "${OUTPUT}" \
    ./cmd/cloudflared

printf '==> Built: %s (%s)\n' "${OUTPUT}" "$(du -h "${OUTPUT}" | cut -f1)"
printf '==> Version: %s\n' "${VERSION}"
