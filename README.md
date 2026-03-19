# Maison

## What it can do

- Monitor and control local home devices from a single interface.
- Manage Tuya-based devices such as feeders, fountains, and litter boxes.
- Track energy and status data for Meross plugs.
- Control Philips Hue lamps over Bluetooth and Zigbee.
- Query Tempo data, predictions, history, and calibration helpers.
- Keep access private with local authentication and secure session cookies.

![Maison](/screenshots/maison.jpg)

Exposes only two app components:

- `frontend/`: the current Vite/React frontend
- `backend/`: the Rust backend

## Runtime files kept in place

The Rust backend reads these files directly from the repo root:

- `devices.json`
- `device-cache.json`
- `users.json`
- `meross-devices.json`
- `hue-lamps.json`
- `hue-lamps-blacklist.json`
- `zigbee-lamps.json`
- `zigbee-lamps-blacklist.json`
- `zigbee2mqtt/`
- `mosquitto/`

Tempo cache and calibration files now live in `cache/tempo/`.

Tempo recalibration workflow is documented in `docs/tempo-calibration.md`.

## Prerequisites

- `bun` for the frontend
- Rust and `cargo` for the backend
- Docker only for the optional containerized frontend, Mosquitto, and the optional Cloudflare tunnel

## Raspberry Pi 1

For Raspberry Pi 1 deployments, the intended setup is fully host-native:

- run Mosquitto directly on the Pi
- run Zigbee2MQTT directly on the Pi
- build the frontend once, then let the Rust backend serve `frontend/dist`
- run a Rust release binary instead of `cargo run`
- compile without Bluetooth support: `cargo build --release --manifest-path backend/Cargo.toml --no-default-features`
- set `DISABLE_BLUETOOTH=true`
- set `AUTH_COOKIE_SECURE=false` if the Pi is exposed only over plain HTTP on the LAN

Deployment notes and host-native service files are in `docs/raspberry-pi-1.md`, `deploy/systemd/cat-monitor.service`, `deploy/systemd/cloudflared-cat-monitor.service`, `zigbee2mqtt/zigbee2mqtt.service`, and `deploy/mosquitto/cat-monitor.conf`.

There is also a one-shot deployment helper for the Pi: `deploy.sh`.
It supports `all`, `build`, `push`, `upgrade`, `start`, `stop`, `status`, and `logs`.

The Raspberry Pi 1 target now assumes Alpine Linux with OpenRC and a musl backend build.

For first boot without screen or keyboard, use `scripts/flash-alpine-headless-macos.sh` and `docs/alpine-headless-flash-macos.md`.

## Environment

```bash
cp .env.example .env
```

Main settings:

- `PORT` / `API_PORT`: Rust backend port, default `3033`
- `JWT_SECRET`: auth signing secret
- `FRONTEND_DIST_DIR`: built frontend directory served directly by the backend when `index.html` exists
- `DISABLE_BLUETOOTH`: set `true` to disable Hue BLE support
- `MQTT_HOST` / `MQTT_PORT`: local MQTT broker used by Zigbee2MQTT and the backend
- `ZIGBEE_ENABLED`: set `true` to start Zigbee2MQTT from `make start`
- `ZIGBEE_SERIAL_PORT`: serial path of the Zigbee dongle
- `ZIGBEE_ADAPTER`: adapter type, `ember` for the Sonoff Dongle Lite MG21
- `ZIGBEE2MQTT_DIR`: local installation directory of Zigbee2MQTT
- `AUTH_COOKIE_NAME`: session cookie name
- `AUTH_COOKIE_SECURE`: keep `true` when the app is exposed through HTTPS/Cloudflare
- `AUTH_RATE_LIMIT_ATTEMPTS`: max failed login attempts per IP+username window
- `AUTH_RATE_LIMIT_WINDOW_SECONDS`: backend login throttling window
- `CLOUDFLARE_TUNNEL_TOKEN`: optional token for the Cloudflare tunnel profile
- `CLOUDFLARED_PROTOCOL`: Cloudflare transport protocol, default `http2` for better compatibility behind Docker/NAT
- `CLOUDFLARE_PUBLIC_HOSTNAME`: optional stable public hostname, for example `home.example.com`

## Security notes

- `JWT_SECRET` must be set to a strong unique value; the backend now refuses to start with the default secret.
- `users.json` must exist and contain at least one account with `password_hash`; plaintext passwords are refused.
- Browser access is expected through the frontend only.
- Auth uses an `HttpOnly` cookie.
- Login throttling.
- Simple audit logs are emitted for login success, failure, and rate-limit hits.

To generate a password hash for `users.json`:

```bash
cargo run --manifest-path backend/Cargo.toml --bin hash_password -- 'your-password'
```

Then :
```bash
cp users.json.template users.json
# copy previous argon2i hashes into this file.
```

## Run locally

Start the backend on the host:

```bash
make backend
```

Or start it in background:

```bash
make backend-start
```

Start the frontend dev server:

```bash
make frontend
```

The frontend proxies `/api` to `http://localhost:3033` by default.

## Zigbee2MQTT on host

Zigbee2MQTT now runs directly on the host, not in Docker.

- Raspberry Pi 1 should use the fully host-native layout: host backend + host-served frontend + host Mosquitto + host Zigbee2MQTT
- Docker is only an optional convenience for stronger machines during development
- the Sonoff Dongle Lite MG21 should use `adapter: ember`
- repository config lives in `zigbee2mqtt/configuration.yaml`
- full setup instructions are in `docs/zigbee2mqtt-host-setup.md`

Useful targets:

```bash
make zigbee2mqtt
make zigbee2mqtt-start
make zigbee2mqtt-stop
```

## Docker

Docker is kept only for the frontend, Mosquitto, and the optional Cloudflare tunnel.
The Rust backend always runs directly on the host.

On low-resource targets like Raspberry Pi 1, Docker should be treated as optional. The backend can now serve the built frontend directly from `frontend/dist`.

For Raspberry Pi 1, the recommended production path is no Docker at all.

Start frontend + Mosquitto:

```bash
docker compose up -d --build frontend mqtt
```

The frontend container proxies API requests to `host.docker.internal:${API_PORT:-3033}`.

## Optional Cloudflare tunnel

Set `CLOUDFLARE_TUNNEL_TOKEN` in `.env`, then run:

```bash
docker compose --profile tunnel up -d cloudflared
```

No local SSL certificates or hybrid deployment files are required.

If you want a stable public URL, create a named Cloudflare Tunnel in the Cloudflare dashboard,
attach your chosen subdomain to it, then put the tunnel token in `CLOUDFLARE_TUNNEL_TOKEN`.
Set the same hostname in `CLOUDFLARE_PUBLIC_HOSTNAME` so `make start` prints the final URL.

For Raspberry Pi 1, prefer the host-native systemd service in `deploy/systemd/cloudflared-cat-monitor.service` instead of Docker.

## One-command lifecycle

Start everything:

```bash
make start
```

This starts:

- the Rust backend on the host
- Zigbee2MQTT on the host when `ZIGBEE_ENABLED=true`
- the frontend container
- the Mosquitto container
- optionally the Cloudflare tunnel container

For Raspberry Pi 1, prefer systemd-managed host services instead of `make start`.

Stop everything:

```bash
make stop
```

## Validation

- Frontend build: `bun --cwd frontend run build`
- Backend tests: `cargo test --manifest-path backend/Cargo.toml`
- Minimal Pi-oriented backend check: `cargo check --manifest-path backend/Cargo.toml --no-default-features`

## Cross-compilation

- local dev stays unchanged; the Pi flow is opt-in
- native Pi-oriented build: `make backend-build-pi`
- cross-build helper: `make backend-build-pi-cross`
- full instructions are in `docs/raspberry-pi-1.md`

### Planned

- Unify code, everything is a device, no more "specific" types everything derived from base "object".
- Matter bridge (but will not handle cats-related devices such as litter as it's not yet in the specification.)
