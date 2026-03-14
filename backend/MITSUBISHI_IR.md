# Mitsubishi IR notes

`broadlink-codes.json` does not contain opaque "scenes" anymore. The learned packets decode as Mitsubishi A/C 144-bit frames, repeated twice per transmission.

## Quick proof

- Broadlink token: `0x26` (`IR` packet)
- Decoded timings match Mitsubishi A/C very closely:
  - header mark/space: about `3400 / 1750 us`
  - bit mark: about `450 us`
  - zero space: about `420 us`
  - one space: about `1300 us`
  - repeat gap: about `14 ms`
- Each learned command contains two identical 144-bit frames.
- The decoded state starts with the fixed Mitsubishi signature `23 CB 26 01 00`.
- The last byte is a checksum equal to the sum of the previous 17 bytes modulo 256.

This matches the `MITSUBISHI_AC` protocol documented by `IRremoteESP8266`.

## Decoder

Run the decoder from the repository root:

```bash
cargo run --manifest-path backend/Cargo.toml --bin decode_mitsubishi_ir -- broadlink-codes.json
```

Without an argument it tries `broadlink-codes.json`, then `../broadlink-codes.json`.

The tool prints, for each learned code:

- packet timing summary
- raw 18-byte Mitsubishi frame
- checksum validity
- interpreted state: power, mode, temperature, fan, vane, timers, extras

## Known frame structure

The decoder uses the Mitsubishi 144-bit layout used by `IRremoteESP8266`.

- `byte 0..4`: fixed signature `23 CB 26 01 00`
- `byte 5`: power flag
- `byte 6`: mode bits, plus `i-see`
- `byte 7`: target temperature in 0.5 C steps
- `byte 8`: horizontal vane / wide vane
- `byte 9`: fan mode, vertical vane, fan auto flag
- `byte 10`: clock
- `byte 11`: stop timer
- `byte 12`: start timer
- `byte 13`: timer mode + weekly timer bit
- `byte 14`: `econo-cool`
- `byte 15`: direct/indirect, absence detect, i-save
- `byte 16`: natural flow + left vane
- `byte 17`: checksum

## What the current captures already show

From the current `broadlink-codes.json` set:

- `too-warm` changes the encoded target temperature by `-1 C`
- `fan` changes the fan-related byte only
- `vane` changes the vane-related bits only
- `timer`, `time-up`, `time-down` modify timer bytes (`11`, `12`, `13`)
- `off` is a full state frame with power disabled, not a simple toggle command

So the remote really does send a full state snapshot at each press.

## Capture matrix still needed to finish the reverse cleanly

To generate arbitrary commands in `Maison`, capture a full matrix while changing only one thing at a time.

Recommended sequence:

### Power / mode

- `cool`, `heat`, `dry`, `fan`, `auto`
- one capture per mode at the same temperature and fan setting
- one `off` capture from each major mode if possible

### Temperature

- at least `16`, `17`, `18`, `19`, `20`, `21`, `22`, `23`, `24`
- all in the same mode, same fan, same vane
- if half-degree is supported by the remote, also capture `20.5`, `21.5`, etc.

### Fan

- `auto`
- every manual fan step shown by the remote
- `silent` if the remote supports it

### Vertical vane

- `auto`
- each fixed position
- `swing`

### Horizontal vane

- each left/center/right position
- `wide`
- `auto` if present

### Extra features

- `econo-cool`
- `i-see`
- `direct` / `indirect`
- `natural flow`
- `absence detect`
- `i-save / 10C heat` if supported

### Timer fields

- timer off
- timer on with a known start time
- timer on with a known stop time
- one increment via `time-up`
- one decrement via `time-down`

## Practical integration plan for Maison

Once the matrix is captured, `Maison` can stop relying on saved Broadlink scenes and instead:

1. store a structured Mitsubishi state
2. generate the 18-byte frame directly
3. recompute the checksum
4. encode the timings back to a Broadlink IR packet
5. send it through the RM4 Pro

That would give `Maison` a real climate model instead of a small list of learned buttons.
