# Home Monitor

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
- `CLOUDFLARE_TUNNEL_TOKEN`: optional token for the Cloudflare tunnel profile
- `CLOUDFLARED_PROTOCOL`: Cloudflare transport protocol, default `http2` for better compatibility behind Docker/NAT
- `CLOUDFLARE_PUBLIC_HOSTNAME`: optional stable public hostname, for example `cat-monitor.example.com`

## Security notes

- `JWT_SECRET` must be set to a strong unique value; the backend now refuses to start with the default secret.
- `users.json` must exist and contain at least one account; the backend no longer falls back to a default admin/admin account.
- Browser access is expected through the frontend only; permissive backend CORS has been removed.
- The frontend proxy now adds security headers and rate-limits `POST /api/auth/login`.

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
- the Mosquitto container
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
- Broadlink IR codes are stored in `broadlink-codes.json` when created.
- Meross TLS broker data remains under `mosquitto/`.
