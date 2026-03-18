# Zigbee2MQTT Host Setup

This project runs Zigbee2MQTT directly on the host.

The expected architecture is:

- `backend` on host
- `zigbee2mqtt` on host
- `mosquitto` in Docker
- `frontend` in Docker

## Shared project configuration

Copy the environment file first:

```bash
cp .env.example .env
```

Set at least:

```bash
ZIGBEE_ENABLED=true
ZIGBEE_SERIAL_PORT=/path/to/your/dongle
ZIGBEE_ADAPTER=ember
ZIGBEE2MQTT_DIR=/opt/zigbee2mqtt
MQTT_HOST=127.0.0.1
MQTT_PORT=1883
Z2M_BASE_TOPIC=zigbee2mqtt
```

The project includes a baseline configuration in `zigbee2mqtt/configuration.yaml`.

Important values for the Sonoff Dongle Lite MG21:

```yaml
serial:
  adapter: ember
```

## macOS installation

Install prerequisites:

```bash
brew install node
corepack enable
```

Clone Zigbee2MQTT locally, for example in `/opt/zigbee2mqtt` or anywhere under your home directory:

```bash
git clone --depth 1 https://github.com/Koenkk/zigbee2mqtt.git /opt/zigbee2mqtt
cd /opt/zigbee2mqtt
pnpm install --frozen-lockfile
```

Find the Sonoff serial device:

```bash
ls /dev/cu.usb* /dev/tty.usb*
system_profiler SPUSBDataType | grep -E "Sonoff|Silicon|CP210|Zigbee" -A 5 -B 2
```

Typical serial path on macOS:

```bash
/dev/cu.usbserial-8320
```

Set it in `.env`:

```bash
ZIGBEE_SERIAL_PORT=/dev/cu.usbserial-8320
ZIGBEE2MQTT_DIR=/opt/zigbee2mqtt
```

Copy the project config into the Zigbee2MQTT installation, then adjust the serial port if needed:

```bash
mkdir -p /opt/zigbee2mqtt/data
cp zigbee2mqtt/configuration.yaml /opt/zigbee2mqtt/data/configuration.yaml
```

Start project services:

```bash
make start
```

Useful commands:

```bash
make zigbee2mqtt-start
make zigbee2mqtt-stop
tail -f logs/zigbee2mqtt.log
```

Notes for macOS:

- Zigbee2MQTT must run on the host because Docker Desktop does not reliably expose USB serial devices.
- `mosquitto` still runs fine in Docker because Zigbee2MQTT connects to `127.0.0.1:1883` on the host.

## Linux installation

Install prerequisites:

```bash
sudo apt-get update
sudo apt-get install -y curl git make g++ gcc libsystemd-dev nodejs
corepack enable
```

Install Zigbee2MQTT:

```bash
sudo mkdir -p /opt/zigbee2mqtt
sudo chown -R ${USER}: /opt/zigbee2mqtt
git clone --depth 1 https://github.com/Koenkk/zigbee2mqtt.git /opt/zigbee2mqtt
cd /opt/zigbee2mqtt
pnpm install --frozen-lockfile
```

Find the stable serial path:

```bash
ls -l /dev/serial/by-id
```

Typical Linux path:

```bash
/dev/serial/by-id/usb-ITead_Sonoff_Zigbee_3.0_USB_Dongle_Plus_V2_...
```

Set it in `.env`:

```bash
ZIGBEE_SERIAL_PORT=/dev/serial/by-id/usb-ITead_Sonoff_Zigbee_3.0_USB_Dongle_Plus_V2_...
ZIGBEE2MQTT_DIR=/opt/zigbee2mqtt
```

Copy the project config into the Zigbee2MQTT installation:

```bash
mkdir -p /opt/zigbee2mqtt/data
cp zigbee2mqtt/configuration.yaml /opt/zigbee2mqtt/data/configuration.yaml
```

## Linux systemd service

The repository includes a ready-to-adapt service file: `zigbee2mqtt/zigbee2mqtt.service`.

Install it like this:

```bash
sudo cp zigbee2mqtt/zigbee2mqtt.service /etc/systemd/system/zigbee2mqtt.service
sudo systemctl daemon-reload
sudo systemctl enable zigbee2mqtt.service
sudo systemctl start zigbee2mqtt.service
```

Then check status and logs:

```bash
systemctl status zigbee2mqtt.service
journalctl -u zigbee2mqtt.service -f
```

If you use the systemd service on the final Linux box, you can still use the project Make targets for the backend/frontend stack, but it is cleaner to let systemd own Zigbee2MQTT.

## Pairing flow

Once services are up:

1. open the dashboard
2. use the Zigbee pairing controls
3. power-cycle or reset the Hue bulb for pairing
4. wait for interview completion
5. test power, brightness, and temperature from the Zigbee lamp UI

## Troubleshooting

- `Zigbee2MQTT is not connected`: make sure Mosquitto is running on `1883`
- `USB adapter discovery error`: confirm `serial.port` and `serial.adapter: ember`
- device seen on macOS but not in Docker: expected, run Zigbee2MQTT on host
- no lamp appears in UI: check `logs/zigbee2mqtt.log` and backend logs
