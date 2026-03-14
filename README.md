# Maison

## What it can do

- Monitor and control local home devices from a single interface.
- Manage Tuya-based devices such as feeders, fountains, and litter boxes.
- Track energy and status data for Meross plugs.
- Control Philips Hue lamps, including power, brightness, and color temperature (through Bluetooth, Zigbee coming soon.)
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
- `mosquitto/`

Tempo cache and calibration files now live in `cache/tempo/`.

## Prerequisites

- `bun` for the frontend
- Rust and `cargo` for the backend
- Docker for the frontend container, Mosquitto, and the optional Cloudflare tunnel

## Environment

```bash
cp .env.example .env
```

Main settings:

- `PORT` / `API_PORT`: Rust backend port, default `3033`
- `JWT_SECRET`: auth signing secret
- `DISABLE_BLUETOOTH`: set `true` to disable Hue BLE support
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

## Docker

Docker is kept only for the frontend, Mosquitto, and the optional Cloudflare tunnel.
The Rust backend always runs directly on the host.

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

## One-command lifecycle

Start everything:

```bash
make start
```

This starts:

- the Rust backend on the host
- the frontend container
- the Mosquitto container (for Meross devices, avoid flashing light)
- the Cloudflare tunnel container

Stop everything:

```bash
make stop
```

## Validation

- Frontend build: `bun --cwd frontend run build`
- Backend tests: `cargo test --manifest-path backend/Cargo.toml`

## Notes

- `users.json.template` remains as a simple auth example.
