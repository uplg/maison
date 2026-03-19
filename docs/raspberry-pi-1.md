# Raspberry Pi 1 deployment

This repository can be prepared for a Raspberry Pi 1 Model B by running the full stack in host-native mode, without Docker and without Hue BLE.

Recommended constraints for this target:

- no Docker
- no on-device frontend build
- no Bluetooth build support
- Mosquitto, Zigbee2MQTT, the Rust backend, and optionally Cloudflared should be managed by systemd

## Recommended deployment shape

On the Pi, run:

- Mosquitto on the host
- Zigbee2MQTT on the host
- the Rust backend release binary
- the prebuilt frontend files in `frontend/dist`

The backend now serves the frontend directly when `FRONTEND_DIST_DIR/index.html` exists.

That means there is no separate nginx or frontend container on the Pi.

## Build on a faster machine

Build the frontend:

```bash
bun --cwd frontend run build
```

Build a Pi-oriented backend binary without Bluetooth support:

```bash
cargo build --release --manifest-path backend/Cargo.toml --no-default-features
```

If you want the Make target version:

```bash
make BACKEND_BUILD_FLAGS=--no-default-features backend-build-release
```

Copy these artifacts to the Pi:

- `backend/target/release/cat-monitor-rust-backend`
- `frontend/dist/`
- `.env`
- `deploy/mosquitto/cat-monitor.conf`
- `zigbee2mqtt/configuration.yaml`
- `zigbee2mqtt/zigbee2mqtt.service`
- `deploy/systemd/cat-monitor.service`
- `deploy/systemd/cloudflared-cat-monitor.service`
- runtime JSON files from the repo root that your installation needs

## Cross-compiling from the dev machine

Your existing dev workflow stays unchanged. The Raspberry Pi flow is isolated behind a separate helper script and Make target.

Install prerequisites on the dev machine:

```bash
rustup toolchain install stable
cargo install cargo-zigbuild
brew install zig
rustup target add arm-unknown-linux-gnueabihf
```

Then run:

```bash
make backend-build-pi-cross
```

Or use the one-shot deploy script:

```bash
PI_HOST=pi@192.168.1.50 ./deploy.sh all
```

Useful subcommands:

```bash
PI_HOST=pi@192.168.1.50 ./deploy.sh build
PI_HOST=pi@192.168.1.50 ./deploy.sh push
PI_HOST=pi@192.168.1.50 ./deploy.sh upgrade
PI_HOST=pi@192.168.1.50 ./deploy.sh start
PI_HOST=pi@192.168.1.50 ./deploy.sh status
PI_HOST=pi@192.168.1.50 ./deploy.sh logs backend
```

That runs `scripts/build-rpi1-backend.sh`, which:

- builds `backend/Cargo.toml` in `release`
- disables Bluetooth at compile time with `--no-default-features`
- targets a Pi 1 compatible ARMv6 hard-float binary
- builds only the main backend binary to keep the linker workload smaller on macOS
- leaves the normal host build and normal `cargo` workflow untouched

Default artifact path:

```bash
target/arm-unknown-linux-gnueabihf/release/cat-monitor-rust-backend
```

You can still keep `make backend`, `cargo check`, and the Docker-based dev flow exactly as before.

## Recommended `.env` values on the Pi

```bash
HOST=0.0.0.0
PORT=3033
JWT_SECRET=replace-this
FRONTEND_DIST_DIR=frontend/dist
DISABLE_BLUETOOTH=true
AUTH_COOKIE_SECURE=false
MQTT_HOST=127.0.0.1
MQTT_PORT=1883
ZIGBEE_ENABLED=true
```

Notes:

- keep `AUTH_COOKIE_SECURE=true` only if the Pi is behind real HTTPS
- `DISABLE_BLUETOOTH=true` is still recommended even if you built with `--no-default-features`
- `MQTT_HOST=127.0.0.1` is the expected local Mosquitto setup on the Pi

## Mosquitto on the Pi

Install Mosquitto directly on the host:

```bash
sudo apt-get update
sudo apt-get install -y mosquitto mosquitto-clients
```

Install the repository config and certificates:

```bash
sudo mkdir -p /etc/mosquitto/conf.d /etc/mosquitto/certs/cat-monitor
sudo cp deploy/mosquitto/cat-monitor.conf /etc/mosquitto/conf.d/cat-monitor.conf
sudo cp mosquitto/certs/ca.pem /etc/mosquitto/certs/cat-monitor/ca.pem
sudo cp mosquitto/certs/server.pem /etc/mosquitto/certs/cat-monitor/server.pem
sudo cp mosquitto/certs/server-key.pem /etc/mosquitto/certs/cat-monitor/server-key.pem
sudo chown -R mosquitto:mosquitto /etc/mosquitto/certs/cat-monitor
sudo chmod 600 /etc/mosquitto/certs/cat-monitor/server-key.pem
sudo systemctl restart mosquitto
sudo systemctl enable mosquitto
```

This keeps:

- `1883` for backend and Zigbee2MQTT
- `8883` for Meross devices that need TLS MQTT

## Zigbee2MQTT on the Pi

Install Zigbee2MQTT on the host and copy the repository config:

```bash
sudo mkdir -p /opt/zigbee2mqtt
sudo chown -R ${USER}: /opt/zigbee2mqtt
git clone --depth 1 https://github.com/Koenkk/zigbee2mqtt.git /opt/zigbee2mqtt
cd /opt/zigbee2mqtt
pnpm install --frozen-lockfile
mkdir -p /opt/zigbee2mqtt/data
cp /opt/cat-monitor/zigbee2mqtt/configuration.yaml /opt/zigbee2mqtt/data/configuration.yaml
```

Then install the service:

```bash
sudo cp /opt/cat-monitor/zigbee2mqtt/zigbee2mqtt.service /etc/systemd/system/zigbee2mqtt.service
sudo systemctl daemon-reload
sudo systemctl enable zigbee2mqtt.service
sudo systemctl start zigbee2mqtt.service
```

## Systemd service

The repository includes `deploy/systemd/cat-monitor.service`.

Adapt paths and user, then install it:

```bash
sudo cp deploy/systemd/cat-monitor.service /etc/systemd/system/cat-monitor.service
sudo systemctl daemon-reload
sudo systemctl enable cat-monitor.service
sudo systemctl start cat-monitor.service
```

Check logs:

```bash
journalctl -u cat-monitor.service -f
journalctl -u zigbee2mqtt.service -f
journalctl -u mosquitto -f
```

## Optional Cloudflare Tunnel on the Pi

Install `cloudflared` directly on the host, keep `CLOUDFLARE_TUNNEL_TOKEN` in `/opt/cat-monitor/.env`, then install the repository service:

```bash
sudo cp deploy/systemd/cloudflared-cat-monitor.service /etc/systemd/system/cloudflared-cat-monitor.service
sudo systemctl daemon-reload
sudo systemctl enable cloudflared-cat-monitor.service
sudo systemctl start cloudflared-cat-monitor.service
```

Check logs:

```bash
journalctl -u cloudflared-cat-monitor.service -f
```

The service uses the same `.env` file as the backend and runs only when `CLOUDFLARE_TUNNEL_TOKEN` is set.

## Important caveats

- Hue BLE on Raspberry Pi 1 is not a good default target; build without the Bluetooth feature unless you explicitly need it.
- Zigbee2MQTT depends on modern Node tooling and is still the weakest link on ARMv6. This repo is now prepared for it, but the Node runtime on the Pi remains the hardest external dependency.
- The frontend is served by the Rust backend on the same port as the API, so the default access URL is `http://<pi-ip>:3033`.
- `cloudflared` itself is host-native now, but its availability still depends on Cloudflare providing a working ARMv6 binary for the OS you install on the Pi.
