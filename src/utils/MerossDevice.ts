import crypto from "node:crypto";

// ─── Meross MSS310 Local HTTP Client ───
// Protocol reverse-engineered from https://github.com/arandall/meross
// Communicates directly with the device via HTTP at http://<ip>/config
// No cloud, no MQTT broker needed.

export interface MerossDeviceConfig {
  name: string;
  ip: string;
  key: string; // Pre-shared signing key (set during provisioning)
  uuid?: string; // Device UUID (optional, discovered via Appliance.System.All)
  mac?: string; // MAC address (optional)
}

export interface MerossPacket {
  header: {
    from: string;
    messageId: string;
    method: "GET" | "SET" | "GETACK" | "SETACK" | "PUSH" | "ERROR";
    namespace: string;
    payloadVersion: number;
    sign: string;
    timestamp: number;
    timestampMs: number;
  };
  payload: Record<string, unknown>;
}

export interface MerossElectricity {
  channel: number;
  current: number; // milliamps (mA)
  voltage: number; // deci-volts (dV) - divide by 10 for volts
  power: number; // milliwatts (mW)
}

export interface MerossSystemAll {
  system: {
    hardware: {
      type: string;
      subType: string;
      version: string;
      chipType: string;
      uuid: string;
      macAddress: string;
    };
    firmware: {
      version: string;
      compileTime: string;
      wifiMac: string;
      innerIp: string;
      server: string;
      port: number;
      secondServer: string;
      secondPort: number;
      userId: number;
    };
    time: {
      timestamp: number;
      timezone: string;
      timeRule: number[][];
    };
    online: {
      status: number; // 1 = online
    };
  };
  // Older firmware uses "control", newer uses "digest"
  control?: {
    toggle?: { onoff: number; lmTime: number };
    togglex?: { channel: number; onoff: number; lmTime: number }[];
    trigger?: unknown[];
    timer?: unknown[];
  };
  digest?: {
    toggle?: { onoff: number; lmTime: number };
    togglex?: { channel: number; onoff: number; lmTime: number }[];
    triggerx?: unknown[];
    timerx?: unknown[];
  };
}

export interface MerossConsumptionEntry {
  date: string; // YYYY-MM-DD
  time: number; // start timestamp
  value: number; // watt hours
}

export interface MerossWifiEntry {
  ssid: string; // base64 encoded
  bssid: string;
  signal: number;
  channel: number;
  encryption: number;
  cipher: number;
}

const HTTP_TIMEOUT_MS = 5000;

export class MerossDevice {
  readonly config: MerossDeviceConfig;
  private baseUrl: string;

  // Cached state
  private _isOnline: boolean = false;
  private _toggleState: boolean = false;
  private _electricity: MerossElectricity | null = null;
  private _systemAll: MerossSystemAll | null = null;
  private _lastUpdate: number = 0;

  constructor(config: MerossDeviceConfig) {
    this.config = config;
    this.baseUrl = `http://${config.ip}/config`;
  }

  // ─── Protocol Helpers ───

  /**
   * Generate a random 32-char hex messageId
   */
  private generateMessageId(): string {
    return crypto.randomBytes(16).toString("hex");
  }

  /**
   * Compute the signing hash: md5(messageId + key + timestamp)
   */
  private computeSign(messageId: string, timestamp: number): string {
    const signStr = messageId + this.config.key + timestamp;
    return crypto.createHash("md5").update(signStr).digest("hex");
  }

  /**
   * Build a Meross protocol packet
   */
  private buildPacket(
    method: "GET" | "SET",
    namespace: string,
    payload: Record<string, unknown> = {},
  ): MerossPacket {
    const messageId = this.generateMessageId();
    const timestamp = Math.floor(Date.now() / 1000);
    const timestampMs = Date.now() % 1000;

    return {
      header: {
        from: this.baseUrl,
        messageId,
        method,
        namespace,
        payloadVersion: 1,
        sign: this.computeSign(messageId, timestamp),
        timestamp,
        timestampMs,
      },
      payload,
    };
  }

  /**
   * Send a packet to the device via HTTP POST
   */
  private async sendPacket(packet: MerossPacket): Promise<MerossPacket> {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), HTTP_TIMEOUT_MS);

    try {
      const response = await fetch(this.baseUrl, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(packet),
        signal: controller.signal,
      });

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
      }

      const result = (await response.json()) as MerossPacket;

      // Check for protocol-level errors
      if (result.header.method === "ERROR") {
        const error = (result.payload as { error?: { code: number; detail: string } }).error;
        throw new Error(`Meross error ${error?.code}: ${error?.detail}`);
      }

      return result;
    } finally {
      clearTimeout(timeout);
    }
  }

  /**
   * Send a GET request to the device
   */
  private async get(
    namespace: string,
    payload: Record<string, unknown> = {},
  ): Promise<Record<string, unknown>> {
    const packet = this.buildPacket("GET", namespace, payload);
    const response = await this.sendPacket(packet);
    return response.payload;
  }

  /**
   * Send a SET request to the device
   */
  private async set(
    namespace: string,
    payload: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    const packet = this.buildPacket("SET", namespace, payload);
    const response = await this.sendPacket(packet);
    return response.payload;
  }

  // ─── System Commands ───

  /**
   * Get all system information (hardware, firmware, toggle state, etc.)
   */
  async getSystemAll(): Promise<MerossSystemAll> {
    const payload = await this.get("Appliance.System.All");
    const all = payload.all as MerossSystemAll;
    this._systemAll = all;
    // If we got a response, the device is reachable via HTTP — mark as online
    // (system.online.status reflects MQTT cloud status which is always 0 for local-only)
    this._isOnline = true;

    // Extract toggle state from either "control" (old fw) or "digest" (new fw)
    const ctrl = all.control || all.digest;
    if (ctrl?.togglex && ctrl.togglex.length > 0) {
      this._toggleState = ctrl.togglex[0].onoff === 1;
    } else if (ctrl?.toggle) {
      this._toggleState = ctrl.toggle.onoff === 1;
    }

    this._lastUpdate = Date.now();
    return all;
  }

  /**
   * Get supported abilities
   */
  async getAbilities(): Promise<Record<string, unknown>> {
    const payload = await this.get("Appliance.System.Ability");
    return payload.ability as Record<string, unknown>;
  }

  /**
   * Get debug information (uptime, memory, network, cloud status)
   */
  async getDebug(): Promise<Record<string, unknown>> {
    const payload = await this.get("Appliance.System.Debug");
    return payload.debug as Record<string, unknown>;
  }

  /**
   * Get runtime info (WiFi signal strength)
   */
  async getRuntime(): Promise<Record<string, unknown>> {
    const payload = await this.get("Appliance.System.Runtime");
    return payload;
  }

  /**
   * Set DND mode (disable status LED)
   */
  async setDNDMode(enabled: boolean): Promise<void> {
    await this.set("Appliance.System.DNDMode", {
      DNDMode: { mode: enabled ? 1 : 0 },
    });
  }

  // ─── Control Commands ───

  /**
   * Turn the plug on or off using ToggleX (newer firmware) with Toggle fallback
   */
  async toggle(on: boolean): Promise<void> {
    try {
      // Try ToggleX first (newer firmware)
      await this.set("Appliance.Control.ToggleX", {
        togglex: { channel: 0, onoff: on ? 1 : 0 },
      });
    } catch {
      // Fallback to Toggle (older firmware)
      await this.set("Appliance.Control.Toggle", {
        channel: 0,
        toggle: { onoff: on ? 1 : 0 },
      });
    }
    this._toggleState = on;
  }

  /**
   * Turn on
   */
  async turnOn(): Promise<void> {
    await this.toggle(true);
  }

  /**
   * Turn off
   */
  async turnOff(): Promise<void> {
    await this.toggle(false);
  }

  // ─── Electricity Monitoring ───

  /**
   * Get current electricity usage (voltage, current, power)
   */
  async getElectricity(): Promise<MerossElectricity> {
    const payload = await this.get("Appliance.Control.Electricity", {
      electricity: { channel: 0 },
    });
    const elec = payload.electricity as MerossElectricity;
    this._electricity = elec;
    this._lastUpdate = Date.now();
    return elec;
  }

  /**
   * Get parsed electricity with human-readable units
   */
  async getElectricityFormatted(): Promise<{
    voltage: number; // Volts
    current: number; // Amps
    power: number; // Watts
    raw: MerossElectricity;
  }> {
    const raw = await this.getElectricity();
    return {
      voltage: raw.voltage / 10,
      current: raw.current / 1000,
      power: raw.power / 1000,
      raw,
    };
  }

  /**
   * Get daily consumption history (last 30 days)
   */
  async getConsumption(): Promise<MerossConsumptionEntry[]> {
    const payload = await this.get("Appliance.Control.ConsumptionX");
    return (payload.consumptionx as MerossConsumptionEntry[]) || [];
  }

  /**
   * Get consumption config (voltage/electricity ratios)
   */
  async getConsumptionConfig(): Promise<Record<string, unknown>> {
    const payload = await this.get("Appliance.Control.ConsumptionConfig");
    return payload.config as Record<string, unknown>;
  }

  // ─── Provisioning (for unconfigured devices on Meross AP) ───

  /**
   * Scan WiFi networks visible to the device
   * Only works when connected to the device's AP (10.10.10.1)
   */
  static async scanWifi(deviceIp: string = "10.10.10.1"): Promise<MerossWifiEntry[]> {
    const url = `http://${deviceIp}/config`;
    const messageId = crypto.randomBytes(16).toString("hex");
    const timestamp = Math.floor(Date.now() / 1000);

    // During setup, sign is not validated - use empty key
    const sign = crypto
      .createHash("md5")
      .update(messageId + timestamp)
      .digest("hex");

    const packet: MerossPacket = {
      header: {
        from: url,
        messageId,
        method: "GET",
        namespace: "Appliance.Config.WifiList",
        payloadVersion: 1,
        sign,
        timestamp,
        timestampMs: Date.now() % 1000,
      },
      payload: {},
    };

    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(packet),
    });

    const result = (await response.json()) as MerossPacket;
    const wifiList = (result.payload as { wifiList?: MerossWifiEntry[] }).wifiList || [];

    // Decode base64 SSIDs for readability
    return wifiList.map((entry) => ({
      ...entry,
      ssid: Buffer.from(entry.ssid, "base64").toString("utf-8"),
    }));
  }

  /**
   * Configure MQTT key and server on an unconfigured device
   * Only works when connected to the device's AP (10.10.10.1)
   */
  static async configureKey(
    key: string,
    userId: string,
    mqttHost: string = "localhost",
    mqttPort: number = 8883,
    deviceIp: string = "10.10.10.1",
  ): Promise<void> {
    const url = `http://${deviceIp}/config`;
    const messageId = crypto.randomBytes(16).toString("hex");
    const timestamp = Math.floor(Date.now() / 1000);
    const sign = crypto
      .createHash("md5")
      .update(messageId + timestamp)
      .digest("hex");

    const packet: MerossPacket = {
      header: {
        from: url,
        messageId,
        method: "SET",
        namespace: "Appliance.Config.Key",
        payloadVersion: 1,
        sign,
        timestamp,
        timestampMs: Date.now() % 1000,
      },
      payload: {
        key: {
          gateway: {
            host: mqttHost,
            port: mqttPort,
            secondHost: mqttHost,
            secondPort: mqttPort,
          },
          key,
          userId,
        },
      },
    };

    await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(packet),
    });
  }

  /**
   * Configure WiFi on an unconfigured device
   * Only works when connected to the device's AP (10.10.10.1)
   * WARNING: Device will reboot after this command
   */
  static async configureWifi(
    ssid: string,
    password: string,
    bssid: string,
    channel: number,
    encryption: number = 6,
    cipher: number = 3,
    deviceIp: string = "10.10.10.1",
  ): Promise<void> {
    const url = `http://${deviceIp}/config`;
    const messageId = crypto.randomBytes(16).toString("hex");
    const timestamp = Math.floor(Date.now() / 1000);
    const sign = crypto
      .createHash("md5")
      .update(messageId + timestamp)
      .digest("hex");

    const packet: MerossPacket = {
      header: {
        from: url,
        messageId,
        method: "SET",
        namespace: "Appliance.Config.Wifi",
        payloadVersion: 1,
        sign,
        timestamp,
        timestampMs: Date.now() % 1000,
      },
      payload: {
        wifi: {
          bssid,
          channel,
          cipher,
          encryption,
          password: Buffer.from(password).toString("base64"),
          ssid: Buffer.from(ssid).toString("base64"),
        },
      },
    };

    await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(packet),
    });
  }

  // ─── Convenience ───

  /**
   * Check if device is reachable via HTTP
   */
  async ping(): Promise<boolean> {
    try {
      await this.getSystemAll();
      return true;
    } catch {
      this._isOnline = false;
      return false;
    }
  }

  /**
   * Get a full status snapshot
   */
  async getFullStatus(): Promise<{
    online: boolean;
    on: boolean;
    electricity: {
      voltage: number;
      current: number;
      power: number;
    } | null;
    hardware: {
      type: string;
      version: string;
      chipType: string;
      uuid: string;
      mac: string;
    } | null;
    firmware: {
      version: string;
      compileTime: string;
      innerIp: string;
    } | null;
    wifi: {
      signal: number | null;
    };
    lastUpdate: number;
  }> {
    const systemAll = await this.getSystemAll();

    let elec: { voltage: number; current: number; power: number } | null = null;
    try {
      const raw = await this.getElectricity();
      elec = {
        voltage: raw.voltage / 10,
        current: raw.current / 1000,
        power: raw.power / 1000,
      };
    } catch {
      // Electricity may not be available if plug is off
    }

    let signal: number | null = null;
    try {
      const runtime = await this.getRuntime();
      const report = runtime.runtime as { signal?: number } | undefined;
      if (report?.signal !== undefined) {
        signal = report.signal;
      }
    } catch {
      // Runtime may not be supported
    }

    return {
      online: this._isOnline,
      on: this._toggleState,
      electricity: elec,
      hardware: {
        type: systemAll.system.hardware.type,
        version: systemAll.system.hardware.version,
        chipType: systemAll.system.hardware.chipType,
        uuid: systemAll.system.hardware.uuid,
        mac: systemAll.system.hardware.macAddress,
      },
      firmware: {
        version: systemAll.system.firmware.version,
        compileTime: systemAll.system.firmware.compileTime,
        innerIp: systemAll.system.firmware.innerIp,
      },
      wifi: { signal },
      lastUpdate: Date.now(),
    };
  }

  // ─── Getters ───

  get isOnline(): boolean {
    return this._isOnline;
  }

  get isOn(): boolean {
    return this._toggleState;
  }

  get lastElectricity(): MerossElectricity | null {
    return this._electricity;
  }

  get lastUpdate(): number {
    return this._lastUpdate;
  }
}
