# Raspberry Pi 1 deployment

This repository can be prepared for a Raspberry Pi 1 Model B running Alpine Linux (`alpine-rpi-3.23.3-armhf`) by running the stack in host-native mode, without Docker and without Hue BLE.

For the SD card flashing and first headless SSH bootstrapping flow on macOS, see `docs/alpine-headless-flash-macos.md` and `scripts/flash-alpine-headless-macos.sh`.

Recommended constraints for this target:

- no Docker
- no on-device frontend build
- no Bluetooth build support
- Mosquitto, the Rust backend, and optionally Cloudflared should be managed by OpenRC

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
- `deploy/openrc/cat-monitor`
- `deploy/openrc/cloudflared-cat-monitor`
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
rustup target add arm-unknown-linux-musleabihf
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
PI_HOST=pi@192.168.1.50 ./deploy.sh stop
PI_HOST=pi@192.168.1.50 ./deploy.sh status
PI_HOST=pi@192.168.1.50 ./deploy.sh logs backend
PI_HOST=pi@192.168.1.50 PI_PASSWORD='secret' ./deploy.sh push
```

That runs `scripts/build-rpi1-backend.sh`, which:

- builds `backend/Cargo.toml` in `release`
- disables Bluetooth at compile time with `--no-default-features`
- targets a Pi 1 compatible ARMv6 hard-float musl binary
- builds only the main backend binary to keep the linker workload smaller on macOS
- leaves the normal host build and normal `cargo` workflow untouched

Default artifact path:

```bash
target/arm-unknown-linux-musleabihf/release/cat-monitor-rust-backend
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

## Mosquitto on Alpine

Install Mosquitto directly on the host:

```bash
sudo apk add --no-cache mosquitto
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
sudo rc-update add mosquitto default
sudo rc-service mosquitto restart
```

This keeps:

- `1883` for backend and future Zigbee bridge publishing
- `8883` for Meross devices that need TLS MQTT

## OpenRC services

The repository includes `deploy/openrc/cat-monitor` and `deploy/openrc/cloudflared-cat-monitor`.

The deploy script installs them automatically on Alpine. If you need to do it manually:

```bash
sudo cp deploy/openrc/cat-monitor /etc/init.d/cat-monitor
sudo chmod +x /etc/init.d/cat-monitor
sudo rc-update add cat-monitor default
sudo rc-service cat-monitor start
```

Check logs:

```bash
tail -f /var/log/cat-monitor.log
tail -f /var/log/mosquitto/mosquitto.log
```

## Optional Cloudflare Tunnel on Alpine

Install `cloudflared` directly on the host, keep `CLOUDFLARE_TUNNEL_TOKEN` in `/opt/cat-monitor/.env`, then install the repository service:

```bash
sudo cp deploy/openrc/cloudflared-cat-monitor /etc/init.d/cloudflared-cat-monitor
sudo chmod +x /etc/init.d/cloudflared-cat-monitor
sudo rc-update add cloudflared-cat-monitor default
sudo rc-service cloudflared-cat-monitor start
```

Check logs:

```bash
tail -f /var/log/cloudflared-cat-monitor.log
```

The service uses the same `.env` file as the backend and runs only when `CLOUDFLARE_TUNNEL_TOKEN` is set.

## Zigbee on Alpine

Node and Zigbee2MQTT are no longer the deployment target for Raspberry Pi 1 on Alpine.

Mosquitto stays in place because Meross still needs MQTT. The next step is a minimal Rust Zigbee bridge that can publish a compatible subset of topics on the local broker without dragging in Node.

You can already switch the backend scaffold with:

```bash
ZIGBEE_BACKEND=native
ZIGBEE_ADAPTER=ember
ZIGBEE_SERIAL_PORT=/dev/ttyUSB0
ZIGBEE_EZSP_PROTOCOL_VERSION=13
```

In `native` mode, the existing HTTP/API surface stays intact and persisted lamps remain available, but pairing and radio control still return an explicit "not implemented yet" error until the Rust driver lands.

The current scaffold already opens the serial transport and routes commands through a native driver abstraction, so the next implementation work is focused on real Ember/EZSP frames rather than on restructuring the app again.

For low-level adapter debugging on the Pi, use the dedicated probe binary instead of the web app:

```bash
ZIGBEE_SERIAL_PORT=/dev/ttyUSB0 ./backend/target/release/zigbee_probe
```

It tries EZSP init in both `rst-cts` and `xon-xoff` modes and reports where the handshake fails.

`deploy.sh push` now copies `zigbee_probe` together with the main backend binary.

If EZSP still hangs, use the raw serial probe:

```bash
ZIGBEE_SERIAL_PORT=/dev/ttyUSB0 ./backend/target/release/zigbee_raw_probe
```

It tries multiple baud rates and flow-control modes, sends a few low-level probe sequences, and dumps raw bytes returned by the dongle.

## Important caveats

- Hue BLE on Raspberry Pi 1 is not a good default target; build without the Bluetooth feature unless you explicitly need it.
- Zigbee2MQTT is not the target runtime anymore for Alpine Pi 1; the intended direction is a minimal Rust Zigbee bridge.
- The frontend is served by the Rust backend on the same port as the API, so the default access URL is `http://<pi-ip>:3033`.
- `cloudflared` itself is host-native now, but its availability still depends on Cloudflare providing a working ARMv6 binary for the OS you install on the Pi.
