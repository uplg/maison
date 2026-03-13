# Broadlink RM4 Pro and Mitsubishi MSZ-HJ5VA

This backend can control a Broadlink `RM4 Pro` locally and use it to replay IR codes for a Mitsubishi `MSZ-HJ5VA` air conditioner.

## Why the backend uses learned states today

This is not especially hard in theory, but it is a different kind of implementation.

- The Broadlink app feels simple because it already knows the Mitsubishi IR protocol and how to build a full AC state packet.
- Our backend currently talks to the `RM4 Pro` as a generic Broadlink remote and sends raw learned packets.
- To generate Mitsubishi packets directly, we would need to add a Mitsubishi Electric IR encoder in Rust, validate that it matches the `MSZ-HJ5VA` remote family, then map backend request fields like mode, temp, fan, vane, sleep, and timers into that packet format.
- The risky part is not sending IR itself, it is getting the exact state model right for this AC family so replayed commands are always correct.

So the current learned-state approach is the fastest reliable version. A generated protocol mode is realistic later, but it needs an AC-specific encoder layer, not just Broadlink support.

## Start the backend

From the backend directory:

```bash
cd /Users/leonard/Github/frog-hack/web-project/rust-rewrite/backend
cargo run
```

By default the backend:

- listens on `0.0.0.0:3033`
- reads users from `web-project/cat-monitor/users.json`
- reads Broadlink IR codes from `web-project/cat-monitor/broadlink-codes.json`

Useful environment variables:

- `HOST`
- `PORT` or `API_PORT`
- `JWT_SECRET`
- `CAT_MONITOR_SOURCE_ROOT`
- `BROADLINK_CODES_JSON_PATH`

Example:

```bash
HOST=127.0.0.1 PORT=3033 cargo run
```

## Get an auth token

The Broadlink routes require the same bearer token as the rest of the API.

### Login request

Use a username/password that exists in `web-project/cat-monitor/users.json`.

```bash
curl -X POST "http://localhost:3033/api/auth/login" \
  -H "Content-Type: application/json" \
  -d '{
    "username": "YOUR_USERNAME",
    "password": "YOUR_PASSWORD"
  }'
```

The response contains a `token` field.

### Export the token in your shell

If you have `jq`:

```bash
export TOKEN=$(curl -s -X POST "http://localhost:3033/api/auth/login" \
  -H "Content-Type: application/json" \
  -d '{
    "username": "YOUR_USERNAME",
    "password": "YOUR_PASSWORD"
  }' | jq -r '.token')
```

If you do not have `jq`:

```bash
TOKEN="paste-the-token-here"
export TOKEN
```

### Verify the token

```bash
curl -X POST "http://localhost:3033/api/auth/verify" \
  -H "Authorization: Bearer $TOKEN"
```

## Recommended approach for MSZ-HJ5VA

- Treat the Mitsubishi remote as a full-state IR remote.
- Learn complete states such as `cool_24_auto` or `heat_22_auto`, not incremental buttons like `temp_up`.
- Start with these commands:
  - `off`
  - `cool_24_auto`
  - `heat_22_auto`
  - `dry`
  - `cool_25_low`
- Learn each command from a clean remote state: no timer, no economy mode, normal vane position.

## Storage

- Saved IR codes are stored in `web-project/cat-monitor/broadlink-codes.json`.
- Override the path with `BROADLINK_CODES_JSON_PATH`.

## Routes

All routes are under `/api/broadlink` and require the usual authenticated bearer token.

### Discover devices

```bash
curl -X GET "http://localhost:3033/api/broadlink/discover" \
  -H "Authorization: Bearer $TOKEN"
```

Optional query param:

- `localIp`: force the local IPv4 interface used for Broadlink discovery.

Example:

```bash
curl -X GET "http://localhost:3033/api/broadlink/discover?localIp=192.168.1.10" \
  -H "Authorization: Bearer $TOKEN"
```

### Provision Wi-Fi in AP mode

Use this only when the RM4 Pro is in provisioning/AP mode and your machine is connected to the Broadlink AP.

```bash
curl -X POST "http://localhost:3033/api/broadlink/provision" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "ssid": "MyWifi",
    "password": "super-secret",
    "securityMode": "wpa2"
  }'
```

Allowed `securityMode` values:

- `none`
- `wep`
- `wpa`
- `wpa1`
- `wpa2`

### Learn an IR code

This starts learning immediately and waits until a code is received or the timeout expires.

```bash
curl -X POST "http://localhost:3033/api/broadlink/learn/ir" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "192.168.1.120",
    "timeoutSecs": 30,
    "saveCode": {
      "name": "Salon AC cool 24 auto",
      "brand": "Mitsubishi",
      "model": "MSZ-HJ5VA",
      "command": "cool_24_auto",
      "tags": ["salon", "clim"]
    }
  }'
```

Optional request fields:

- `localIp`: force the local IPv4 interface.
- `timeoutSecs`: defaults to `30`.
- `saveCode`: if present, the learned packet is stored directly.

### Save a known IR packet

```bash
curl -X POST "http://localhost:3033/api/broadlink/codes" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Salon AC off",
    "brand": "Mitsubishi",
    "model": "MSZ-HJ5VA",
    "command": "off",
    "packetBase64": "AQIDBA==",
    "tags": ["salon"]
  }'
```

### List all saved codes

```bash
curl -X GET "http://localhost:3033/api/broadlink/codes" \
  -H "Authorization: Bearer $TOKEN"
```

### List Mitsubishi codes only

```bash
curl -X GET "http://localhost:3033/api/broadlink/mitsubishi/codes?model=MSZ-HJ5VA" \
  -H "Authorization: Bearer $TOKEN"
```

### Send a raw packet directly

```bash
curl -X POST "http://localhost:3033/api/broadlink/send" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "192.168.1.120",
    "packetBase64": "AQIDBA=="
  }'
```

### Send a saved code by id

```bash
curl -X POST "http://localhost:3033/api/broadlink/codes/CODE_ID/send" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "192.168.1.120"
  }'
```

### Send a Mitsubishi command by logical name

This is the easiest route once codes have been learned and saved.

```bash
curl -X POST "http://localhost:3033/api/broadlink/mitsubishi/send" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "host": "192.168.1.120",
    "model": "MSZ-HJ5VA",
    "command": "cool_24_auto"
  }'
```

## Suggested first setup for your AC

Learn and save at least these entries:

```text
off
cool_24_auto
heat_22_auto
dry
cool_25_low
```

Then test each one with `/api/broadlink/mitsubishi/send`.

## Notes and caveats

- Broadlink control is local IPv4 only.
- Discovery can fail if your machine uses the wrong interface; pass `localIp` in that case.
- AP provisioning is local too, but only while the RM4 Pro is in AP mode.
- For the `MSZ-HJ5VA`, the learned-state approach is the safest today.
- A protocol-generated Mitsubishi mode may be possible later, but it is not implemented in this backend yet.
