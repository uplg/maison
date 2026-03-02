/**
 * Philips Hue Bluetooth Lamp Control
 *
 * Based on reverse-engineered BLE protocol:
 * https://gist.github.com/shinyquagsire23/f7907fdf6b470200702e75a30135caf3
 * https://github.com/evan-brass/huecontrol
 *
 * Supported models: LWV001, LWA001, LTG002
 */

// Philips Hue BLE Service and Characteristic UUIDs
export const HUE_UUIDS = {
  // Light control service
  LIGHT_CONTROL_SERVICE: "932c32bd-0000-47a2-835a-a8d455b859dd",

  // Characteristics
  POWER: "932c32bd-0002-47a2-835a-a8d455b859dd", // On/Off (0x00=off, 0x01=on)
  BRIGHTNESS: "932c32bd-0003-47a2-835a-a8d455b859dd", // 0x01-0xfe (1-254)
  TEMPERATURE: "932c32bd-0004-47a2-835a-a8d455b859dd", // Color temperature
  COLOR: "932c32bd-0005-47a2-835a-a8d455b859dd", // XY color
  CONTROL: "932c32bd-0007-47a2-835a-a8d455b859dd", // Combined control

  // Device info service
  DEVICE_INFO_SERVICE: "0000180a-0000-1000-8000-00805f9b34fb",
  MODEL: "00002a24-0000-1000-8000-00805f9b34fb",
  FIRMWARE: "00002a28-0000-1000-8000-00805f9b34fb",
  MANUFACTURER: "00002a29-0000-1000-8000-00805f9b34fb",

  // Configuration service
  CONFIG_SERVICE: "0000fe0f-0000-1000-8000-00805f9b34fb",
  DEVICE_NAME: "97fe6561-0003-4f62-86e9-b71ee2da3d22",
} as const;

// Known Hue Bluetooth lamp models
export const HUE_LAMP_MODELS = [
  "LWV001", // Filament bulb
  "LWA001", // White ambiance
  "LTG002", // GU10 spot
  "LWA004", // E27 bulb
  "LWB010", // White bulb
  "LCA001", // Color bulb
  "LCT024", // Play light bar
] as const;

export type HueLampModel = (typeof HUE_LAMP_MODELS)[number];

export interface HueLampState {
  isOn: boolean;
  brightness: number; // 1-254
  temperature?: number; // Color temperature percentage (0-100)
  temperatureMin?: number; // Minimum temperature the lamp supports (0-100)
  temperatureMax?: number; // Maximum temperature the lamp supports (0-100)
  reachable: boolean;
}

export interface HueLampInfo {
  id: string;
  name: string;
  model: string;
  manufacturer: string;
  firmware: string;
  address: string;
}

/**
 * Parse brightness from raw BLE value (1-254) to percentage (1-100)
 */
export function parseBrightness(rawValue: number): number {
  // Hue uses 1-254 range, convert to 1-100 percentage
  return Math.round((rawValue / 254) * 100);
}

/**
 * Convert percentage (1-100) to raw BLE brightness value (1-254)
 */
export function toBrightness(percentage: number): number {
  // Clamp to valid range
  const clamped = Math.max(1, Math.min(100, percentage));
  return Math.round((clamped / 100) * 254);
}

/**
 * Parse temperature from raw BLE value (1-244) to percentage (0-100)
 * Hue uses inverted Mirek-like values: 1=cool/white (6500K), 244=warm/yellow (2000K)
 * We invert to make 0%=warm, 100%=cool for intuitive UI
 * Linear mapping: raw 244 → 0%, raw 1 → 100%
 */
export function parseTemperature(rawValue: number): number {
  const clamped = Math.max(1, Math.min(244, rawValue));
  // Linear interpolation: raw 244 → 0%, raw 1 → 100%
  return Math.round(((244 - clamped) / 243) * 100);
}

/**
 * Convert percentage (0-100) to raw BLE temperature value (1-244)
 * Inverted: 0%=warm=244raw, 100%=cool=1raw
 * Linear mapping: 0% → raw 244, 100% → raw 1
 */
export function toTemperature(percentage: number): number {
  const clamped = Math.max(0, Math.min(100, percentage));
  // Linear interpolation: 0% → 244, 100% → 1
  return Math.round(244 - (clamped / 100) * 243);
}

/**
 * Build a combined control command for the CONTROL characteristic
 * This allows setting multiple values in one write
 */
export function buildControlCommand(options: {
  power?: boolean;
  brightness?: number; // 1-254
  temperature?: number;
}): Buffer {
  const commands: number[] = [];

  if (options.power !== undefined) {
    // Type 0x01 (power), Length 0x01 (1 byte), Value
    commands.push(0x01, 0x01, options.power ? 0x01 : 0x00);
  }

  if (options.brightness !== undefined) {
    // Type 0x02 (brightness), Length 0x01 (1 byte), Value
    const brightness = Math.max(1, Math.min(254, options.brightness));
    commands.push(0x02, 0x01, brightness);
  }

  if (options.temperature !== undefined) {
    // Type 0x03 (temperature), Length 0x02 (2 bytes), Value + Enable flag
    const temp = Math.max(1, Math.min(244, options.temperature));
    commands.push(0x03, 0x02, temp, 0x01);
  }

  return Buffer.from(commands);
}

/**
 * Parse state from characteristic reads
 */
export function parseHueLampState(
  power: Buffer | null,
  brightness: Buffer | null,
): Partial<HueLampState> {
  const state: Partial<HueLampState> = {};

  if (power && power.length >= 1) {
    state.isOn = power[0] === 0x01;
  }

  if (brightness && brightness.length >= 1) {
    state.brightness = brightness[0];
  }

  return state;
}

/**
 * Check if a device name/advertisement looks like a Philips Hue lamp
 */
export function isHueLamp(
  localName?: string,
  manufacturerData?: Buffer,
  serviceUuids?: string[],
): boolean {
  // Check by service UUIDs FIRST - most reliable method
  // Hue lamps advertise "fe0f" (Philips/Signify short UUID)
  if (serviceUuids && serviceUuids.length > 0) {
    const hueServiceUuids = [
      "fe0f", // Philips/Signify short UUID
      "932c32bd000047a2835aa8d455b859dd", // Light control service (no dashes)
      "0000fe0f00001000800000805f9b34fb", // Philips config service (full)
    ];
    for (const uuid of serviceUuids) {
      const cleanUuid = uuid.toLowerCase().replace(/-/g, "");
      if (hueServiceUuids.includes(cleanUuid)) {
        console.log(`✅ Detected Hue lamp by service UUID: ${uuid}`);
        return true;
      }
    }
  }

  // Check by name - Hue lamps often have "Hue" or model in name
  if (localName) {
    const name = localName.toLowerCase();
    if (
      name.includes("hue") ||
      name.startsWith("philips") ||
      name.includes("lwa") ||
      name.includes("lwv") ||
      name.includes("ltg") ||
      name.includes("lct") ||
      name.includes("lwb") ||
      name.includes("lca")
    ) {
      return true;
    }
  }

  // Philips/Signify manufacturer ID
  if (manufacturerData && manufacturerData.length >= 2) {
    const manufacturerId = manufacturerData.readUInt16LE(0);
    // 0x0075 = Philips, 0x0105 = Signify (new Philips)
    if (manufacturerId === 0x0075 || manufacturerId === 0x0105) {
      return true;
    }
  }

  return false;
}

/**
 * Check if a model string is a known Hue lamp model
 */
export function isKnownHueModel(model: string): boolean {
  return HUE_LAMP_MODELS.includes(model as HueLampModel);
}
