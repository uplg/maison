import TuyAPI, { DPSObject } from "tuyapi";
import fs from "node:fs";
import path from "node:path";
import { parseFeederStatus } from "./Feeder";
import { parseLitterBoxStatus } from "./Litter";
import { parseFountainStatus, isCorruptedData } from "./Fountain";

export interface DeviceConfig {
  name: string;
  id: string;
  key: string;
  category: string;
  product_name: string;
  port?: number;
  model: string;
  ip: string;
  version: string;
}

export interface DeviceInstance {
  config: DeviceConfig;
  api: TuyAPI;
  isConnected: boolean;
  isConnecting: boolean;
  lastData: DPSObject;
  parsedData:
    | ReturnType<typeof parseFeederStatus>
    | ReturnType<typeof parseLitterBoxStatus>
    | ReturnType<typeof parseFountainStatus>
    | {};
  type: "feeder" | "litter-box" | "fountain" | "unknown";
  reconnectAttempts: number;
  reconnectTimeout: NodeJS.Timeout | null;
}

// Unified cache for device DPS values (meal plans, litter_level, etc.)
// Stores values that come via events or need to persist across restarts
interface DeviceDPSCache {
  [deviceId: string]: {
    [dpsId: string]: unknown;
  };
}

const DEVICE_CACHE_FILE = "device-cache.json";

// Connection configuration
const CONNECTION_CONFIG = {
  MAX_RECONNECT_ATTEMPTS: 10,
  INITIAL_RECONNECT_DELAY_MS: 1000,
  MAX_RECONNECT_DELAY_MS: 60000,
  HEARTBEAT_INTERVAL_MS: 30000,
  COMMAND_RETRY_ATTEMPTS: 3,
  COMMAND_RETRY_DELAY_MS: 1000,
  CONNECTION_TIMEOUT_MS: 10000,
  STATUS_REQUEST_TIMEOUT_MS: 8000,
};

export class DeviceManager {
  private devices: Map<string, DeviceInstance> = new Map();
  private configs: DeviceConfig[] = [];
  private deviceCache: Map<string, Record<string, unknown>> = new Map(); // deviceId -> cached DPS values
  private deviceCachePath: string;
  private heartbeatIntervals: Map<string, NodeJS.Timeout> = new Map();
  private isShuttingDown: boolean = false;

  constructor() {
    this.deviceCachePath = path.join(process.cwd(), DEVICE_CACHE_FILE);
    this.loadDevicesConfig();
    this.loadDeviceCache();
  }

  private loadDevicesConfig(): void {
    try {
      const configPath = path.join(process.cwd(), "devices.json");
      const configData = fs.readFileSync(configPath, "utf8");
      this.configs = JSON.parse(configData);
      console.log(`📱 Loaded ${this.configs.length} device configurations`);
    } catch (error) {
      console.error("❌ Failed to load devices configuration:", error);
      this.configs = [];
    }
  }

  private loadDeviceCache(): void {
    try {
      if (fs.existsSync(this.deviceCachePath)) {
        const cacheData = fs.readFileSync(this.deviceCachePath, "utf8");
        const cache: DeviceDPSCache = JSON.parse(cacheData);

        for (const [deviceId, dpsValues] of Object.entries(cache)) {
          this.deviceCache.set(deviceId, dpsValues);
        }

        console.log(`📋 Loaded device cache for ${this.deviceCache.size} devices`);
      } else {
        console.log(`📋 No device cache found, starting fresh`);
      }
    } catch (error) {
      console.error("⚠️ Failed to load device cache:", error);
    }
  }

  private saveDeviceCache(): void {
    try {
      const cache: DeviceDPSCache = {};

      for (const [deviceId, dpsValues] of this.deviceCache.entries()) {
        cache[deviceId] = dpsValues;
      }

      fs.writeFileSync(this.deviceCachePath, JSON.stringify(cache, null, 2), "utf8");
      console.log(`💾 Saved device cache for ${this.deviceCache.size} devices`);
    } catch (error) {
      console.error("❌ Failed to save device cache:", error);
    }
  }

  /**
   * Cache a DPS value (for report-only DPS like litter_level, or meal plans)
   */
  private cacheDPS(deviceId: string, dpsId: string, value: unknown): void {
    let cache = this.deviceCache.get(deviceId);
    if (!cache) {
      cache = {};
      this.deviceCache.set(deviceId, cache);
    }
    cache[dpsId] = value;
    // Save immediately since these are important values
    this.saveDeviceCache();
    console.log(`💾 Cached DPS ${dpsId} for device ${deviceId}`);
  }

  /**
   * Get all cached DPS for a device
   */
  private getDeviceCachedDPS(deviceId: string): Record<string, unknown> {
    return this.deviceCache.get(deviceId) ?? {};
  }

  private determineDeviceType(
    config: DeviceConfig,
  ): "feeder" | "litter-box" | "fountain" | "unknown" {
    const productName = config.product_name.toLowerCase();
    const category = config.category.toLowerCase();

    if (productName.includes("feeder") || category === "cwwsq") {
      return "feeder";
    } else if (productName.includes("litter") || category === "msp") {
      return "litter-box";
    } else if (productName.includes("fountain") || category === "cwysj") {
      return "fountain";
    }

    return "unknown";
  }

  async initializeDevices(): Promise<void> {
    console.log("🔄 Initializing devices...");

    for (const config of this.configs) {
      try {
        const api = new TuyAPI({
          id: config.id,
          key: config.key,
          ip: config.ip,
          port: config.port ?? 6668,
          version: config.version,
        });

        const deviceType = this.determineDeviceType(config);

        const deviceInstance: DeviceInstance = {
          config,
          api,
          isConnected: false,
          isConnecting: false,
          lastData: { dps: {} },
          parsedData: {},
          type: deviceType,
          reconnectAttempts: 0,
          reconnectTimeout: null,
        };

        api.on("connected", () => {
          console.log(`✅ Device connected: ${config.name} (${config.id})`);
          deviceInstance.isConnected = true;
          deviceInstance.isConnecting = false;
          deviceInstance.reconnectAttempts = 0;

          // Clear any pending reconnect timeout
          if (deviceInstance.reconnectTimeout) {
            clearTimeout(deviceInstance.reconnectTimeout);
            deviceInstance.reconnectTimeout = null;
          }

          // Start heartbeat for this device
          this.startHeartbeat(config.id);
        });

        api.on("disconnected", () => {
          console.log(`❌ Device disconnected: ${config.name} (${config.id})`);
          deviceInstance.isConnected = false;
          deviceInstance.isConnecting = false;

          // Stop heartbeat
          this.stopHeartbeat(config.id);

          // Schedule reconnection if not shutting down
          if (!this.isShuttingDown) {
            this.scheduleReconnect(config.id);
          }
        });

        api.on("error", (error) => {
          console.log(`⚠️ Device error for ${config.name}:`, error.message);
          deviceInstance.isConnecting = false;

          // On error, also try to reconnect
          if (!this.isShuttingDown && !deviceInstance.isConnected) {
            this.scheduleReconnect(config.id);
          }
        });

        api.on("data", (data) => {
          // Check for corrupted data
          if (data && typeof data === "object") {
            const dataStr = JSON.stringify(data);
            if (isCorruptedData(dataStr)) {
              console.error(
                `⚠️ Corrupted data detected from ${config.name}. This usually indicates:
                - Incorrect device key in devices.json
                - Wrong protocol version
                - Device communication issues`,
              );
              console.log(`Raw corrupted data:`, data);
              return;
            }
          }

          console.log(`📡 Data from ${config.name}:`, data);
          deviceInstance.lastData = { ...deviceInstance.lastData, ...data };

          if (deviceInstance.type === "feeder") {
            deviceInstance.parsedData = parseFeederStatus(deviceInstance.lastData);
          } else if (deviceInstance.type === "litter-box") {
            deviceInstance.parsedData = parseLitterBoxStatus(deviceInstance.lastData);
          } else if (deviceInstance.type === "fountain") {
            deviceInstance.parsedData = parseFountainStatus(deviceInstance.lastData);
          }

          this.handleDeviceData(deviceInstance, data);
        });

        this.devices.set(config.id, deviceInstance);
        console.log(`📱 Registered device: ${config.name} (${deviceType})`);
      } catch (error) {
        console.error(`❌ Failed to initialize device ${config.name}:`, error);
      }
    }
  }

  private handleDeviceData(device: DeviceInstance, data: DPSObject): void {
    if (!data.dps) return;

    switch (device.type) {
      case "feeder":
        if (data.dps["1"]) {
          console.log(`🍽️ Meal plan reported by ${device.config.name}:`, data.dps["1"]);
          // Cache meal plan (DPS 1) - persists across restarts
          this.cacheDPS(device.config.id, "1", data.dps["1"]);
        }
        if (data.dps["3"]) {
          console.log(`🐱 Feeding activity from ${device.config.name}:`, data.dps["3"]);
        }
        break;

      case "litter-box":
        console.warn(`🚽 Litter box data received from ${device.config.name}:`, data.dps);
        if (data.dps["105"]) {
          console.log(
            `🚽 Litter box defecation frequency from ${device.config.name}:`,
            data.dps["105"],
          );
        }
        if (data.dps["112"]) {
          console.log(`🗑️ Litter level event from ${device.config.name}:`, data.dps["112"]);
          // Cache this value - DPS 112 is a "report-only" datapoint that only comes via push events
          // It won't be returned by get() or refresh() calls, so we need to cache it
          this.cacheDPS(device.config.id, "112", data.dps["112"]);
        }
        // Log all DPS received for debugging
        console.log(`📋 Litter box DPS received:`, Object.keys(data.dps).join(", "));
        break;

      case "fountain":
        if (data.dps["1"]) {
          console.log(`💧 Fountain power state from ${device.config.name}:`, data.dps["1"]);
        }
        if (data.dps["101"]) {
          console.log(`💧 Water level from ${device.config.name}:`, data.dps["101"]);
        }
        break;
    }
  }

  /**
   * Calculate exponential backoff delay for reconnection
   */
  private getReconnectDelay(attempts: number): number {
    const delay = Math.min(
      CONNECTION_CONFIG.INITIAL_RECONNECT_DELAY_MS * Math.pow(2, attempts),
      CONNECTION_CONFIG.MAX_RECONNECT_DELAY_MS,
    );
    // Add jitter to prevent thundering herd
    return delay + Math.random() * 1000;
  }

  /**
   * Schedule a reconnection attempt for a device
   */
  private scheduleReconnect(deviceId: string): void {
    const device = this.devices.get(deviceId);
    if (!device) return;

    // Clear any existing reconnect timeout
    if (device.reconnectTimeout) {
      clearTimeout(device.reconnectTimeout);
      device.reconnectTimeout = null;
    }

    // Check if we've exceeded max attempts
    if (device.reconnectAttempts >= CONNECTION_CONFIG.MAX_RECONNECT_ATTEMPTS) {
      console.log(
        `🚫 Max reconnection attempts reached for ${device.config.name}. Will retry on next API call.`,
      );
      return;
    }

    const delay = this.getReconnectDelay(device.reconnectAttempts);
    device.reconnectAttempts++;

    console.log(
      `🔄 Scheduling reconnection for ${device.config.name} in ${Math.round(
        delay / 1000,
      )}s (attempt ${device.reconnectAttempts}/${CONNECTION_CONFIG.MAX_RECONNECT_ATTEMPTS})`,
    );

    device.reconnectTimeout = setTimeout(async () => {
      if (this.isShuttingDown || device.isConnected || device.isConnecting) {
        return;
      }

      try {
        await this.connectDeviceWithTimeout(deviceId);
      } catch (error) {
        console.log(
          `⚠️ Reconnection failed for ${device.config.name}:`,
          error instanceof Error ? error.message : error,
        );
        // Will be rescheduled by the disconnect event
      }
    }, delay);
  }

  /**
   * Connect to a device with timeout
   */
  private async connectDeviceWithTimeout(deviceId: string): Promise<boolean> {
    const device = this.devices.get(deviceId);
    if (!device) {
      throw new Error(`Device ${deviceId} not found`);
    }

    if (device.isConnected) {
      return true;
    }

    if (device.isConnecting) {
      // Wait for existing connection attempt
      return new Promise((resolve) => {
        const checkInterval = setInterval(() => {
          if (!device.isConnecting) {
            clearInterval(checkInterval);
            resolve(device.isConnected);
          }
        }, 100);

        // Timeout the wait
        setTimeout(() => {
          clearInterval(checkInterval);
          resolve(device.isConnected);
        }, CONNECTION_CONFIG.CONNECTION_TIMEOUT_MS);
      });
    }

    device.isConnecting = true;

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        device.isConnecting = false;
        reject(new Error(`Connection timeout for ${device.config.name}`));
      }, CONNECTION_CONFIG.CONNECTION_TIMEOUT_MS);

      device.api
        .connect()
        .then(() => {
          clearTimeout(timeout);
          resolve(true);
        })
        .catch((error) => {
          clearTimeout(timeout);
          device.isConnecting = false;
          reject(error);
        });
    });
  }

  /**
   * Start heartbeat for a device to keep connection alive
   */
  private startHeartbeat(deviceId: string): void {
    this.stopHeartbeat(deviceId);

    const device = this.devices.get(deviceId);
    if (!device) return;

    const interval = setInterval(async () => {
      if (!device.isConnected || this.isShuttingDown) {
        this.stopHeartbeat(deviceId);
        return;
      }

      try {
        // Send a lightweight status request to keep connection alive
        await device.api.get({ schema: true });
        console.log(`💓 Heartbeat OK for ${device.config.name}`);
      } catch (error) {
        console.log(
          `💔 Heartbeat failed for ${device.config.name}:`,
          error instanceof Error ? error.message : error,
        );
      }
    }, CONNECTION_CONFIG.HEARTBEAT_INTERVAL_MS);

    this.heartbeatIntervals.set(deviceId, interval);
  }

  /**
   * Stop heartbeat for a device
   */
  private stopHeartbeat(deviceId: string): void {
    const interval = this.heartbeatIntervals.get(deviceId);
    if (interval) {
      clearInterval(interval);
      this.heartbeatIntervals.delete(deviceId);
    }
  }

  /**
   * Ensure a device is connected, with retry logic
   */
  private async ensureConnected(deviceId: string): Promise<void> {
    const device = this.devices.get(deviceId);
    if (!device) {
      throw new Error(`Device ${deviceId} not found`);
    }

    if (device.isConnected) {
      return;
    }

    // If stuck in connecting state, force reset
    if (device.isConnecting) {
      console.log(`⚠️ Device ${device.config.name} stuck in connecting state, forcing reset...`);
      this.forceDisconnect(deviceId);
    }

    // Reset reconnect attempts for manual connection
    device.reconnectAttempts = 0;

    let lastError: Error | null = null;
    for (let attempt = 1; attempt <= CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS; attempt++) {
      try {
        console.log(
          `🔌 Connecting to ${device.config.name} (attempt ${attempt}/${CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS})...`,
        );
        await this.connectDeviceWithTimeout(deviceId);

        if (device.isConnected) {
          return;
        }
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));
        console.log(
          `⚠️ Connection attempt ${attempt} failed for ${device.config.name}:`,
          lastError.message,
        );

        // Force disconnect to clean up any bad state
        this.forceDisconnect(deviceId);

        if (attempt < CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS) {
          await this.delay(CONNECTION_CONFIG.COMMAND_RETRY_DELAY_MS * attempt);
        }
      }
    }

    throw new Error(
      `Failed to connect to ${device.config.name} after ${CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS} attempts: ${lastError?.message}`,
    );
  }

  /**
   * Helper to delay execution
   */
  private delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  /**
   * Wrap a promise with a timeout
   */
  private withTimeout<T>(promise: Promise<T>, ms: number, errorMessage: string): Promise<T> {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error(errorMessage));
      }, ms);

      promise
        .then((result) => {
          clearTimeout(timeout);
          resolve(result);
        })
        .catch((error) => {
          clearTimeout(timeout);
          reject(error);
        });
    });
  }

  async connectDevice(deviceId: string): Promise<boolean> {
    const device = this.devices.get(deviceId);
    if (!device) {
      throw new Error(`Device ${deviceId} not found`);
    }

    try {
      await this.connectDeviceWithTimeout(deviceId);
      return device.isConnected;
    } catch (error) {
      console.error(`Failed to connect device ${deviceId}:`, error);
      return false;
    }
  }

  async disconnectDevice(deviceId: string): Promise<void> {
    const device = this.devices.get(deviceId);
    if (!device) {
      throw new Error(`Device ${deviceId} not found`);
    }

    // Stop heartbeat and cancel reconnection
    this.stopHeartbeat(deviceId);
    if (device.reconnectTimeout) {
      clearTimeout(device.reconnectTimeout);
      device.reconnectTimeout = null;
    }

    device.api.disconnect();
  }

  /**
   * Force disconnect a device and reset its state
   * Used when socket is in a bad state (hang up, timeout, etc.)
   */
  private forceDisconnect(deviceId: string): void {
    const device = this.devices.get(deviceId);
    if (!device) return;

    console.log(`🔌 Force disconnecting ${device.config.name}...`);

    // Stop heartbeat
    this.stopHeartbeat(deviceId);

    // Cancel any pending reconnect
    if (device.reconnectTimeout) {
      clearTimeout(device.reconnectTimeout);
      device.reconnectTimeout = null;
    }

    // Reset state flags
    device.isConnected = false;
    device.isConnecting = false;

    // Force disconnect the socket
    try {
      device.api.disconnect();
    } catch (e) {
      // Ignore disconnect errors
    }
  }

  async connectAllDevices(): Promise<void> {
    console.log("🔗 Connecting to all devices...");

    const connectionPromises = Array.from(this.devices).map(async ([deviceId, device]) => {
      try {
        await this.connectDevice(deviceId);
        console.log(`✅ Connected to ${device.config.name}`);
      } catch (error) {
        console.error(
          `⚠️ Failed to connect ${device.config.name}:`,
          error instanceof Error ? error.message : error,
        );
        // Schedule reconnection for failed devices
        this.scheduleReconnect(deviceId);
      }
    });

    await Promise.allSettled(connectionPromises);

    const connectedCount = Array.from(this.devices.values()).filter((d) => d.isConnected).length;
    console.log(`🔗 Connection complete: ${connectedCount}/${this.devices.size} devices connected`);
  }

  disconnectAllDevices(): void {
    console.log("🔌 Disconnecting all devices...");
    this.isShuttingDown = true;

    for (const [deviceId, device] of Array.from(this.devices)) {
      try {
        this.stopHeartbeat(deviceId);
        if (device.reconnectTimeout) {
          clearTimeout(device.reconnectTimeout);
          device.reconnectTimeout = null;
        }
        device.api.disconnect();
      } catch (error) {
        console.error(`Failed to disconnect ${device.config.name}:`, error);
      }
    }
  }

  getDevice(deviceId: string): DeviceInstance | undefined {
    return this.devices.get(deviceId);
  }

  getAllDevices(): DeviceInstance[] {
    return Array.from(this.devices.values());
  }

  getDevicesByType(type: "feeder" | "litter-box"): DeviceInstance[] {
    return this.getAllDevices().filter((device) => device.type === type);
  }

  async sendCommand(
    deviceId: string,
    dps: number,
    value: string | number | boolean,
    disconnectAfter: boolean = false,
  ): Promise<void> {
    const device = this.devices.get(deviceId);
    if (!device) {
      throw new Error(`Device ${deviceId} not found`);
    }

    let lastError: Error | null = null;

    for (let attempt = 1; attempt <= CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS; attempt++) {
      try {
        await this.ensureConnected(deviceId);

        await device.api.set({ dps, set: value });
        console.log(`📤 Command sent to ${device.config.name}: DPS ${dps} = ${value}`);

        if (disconnectAfter) {
          await this.disconnectDevice(deviceId);
        }

        return;
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));
        console.log(
          `⚠️ Command attempt ${attempt}/${CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS} failed for ${device.config.name}:`,
          lastError.message,
        );

        device.isConnected = false;

        if (attempt < CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS) {
          await this.delay(CONNECTION_CONFIG.COMMAND_RETRY_DELAY_MS * attempt);
        }
      }
    }

    throw new Error(
      `Failed to send command to ${device.config.name} after ${CONNECTION_CONFIG.COMMAND_RETRY_ATTEMPTS} attempts: ${lastError?.message}`,
    );
  }

  async getDeviceStatus(deviceId: string): Promise<DPSObject> {
    const device = this.devices.get(deviceId);
    if (!device) {
      throw new Error(`Device ${deviceId} not found`);
    }

    let lastError: Error | null = null;

    // Use fewer retries for status requests to fail faster
    const maxAttempts = 2;

    for (let attempt = 1; attempt <= maxAttempts; attempt++) {
      try {
        await this.ensureConnected(deviceId);

        const response = await this.withTimeout(
          device.api.get({ schema: true }) as Promise<DPSObject>,
          CONNECTION_CONFIG.STATUS_REQUEST_TIMEOUT_MS,
          `Status request timeout for ${device.config.name}`,
        );
        console.log(`📥 Status received from ${device.config.name}`);

        // Merge with cached lastData to include any DPS received via events
        // This is important for DPS like 112 (litter_level) that may only come via push events
        if (device.lastData && device.lastData.dps) {
          const mergedDps = { ...device.lastData.dps, ...response.dps };
          console.log(`📦 Merged DPS (lastData + current):`, Object.keys(mergedDps).join(", "));
          response.dps = mergedDps;
        }

        // Merge with persisted device cache (for report-only DPS like litter_level)
        // This ensures values are available even after server restart
        const cachedDPS = this.getDeviceCachedDPS(deviceId);
        if (Object.keys(cachedDPS).length > 0) {
          // Only merge cached values that are not already in response (don't overwrite fresh data)
          for (const [dpsId, value] of Object.entries(cachedDPS)) {
            if (response.dps[dpsId] === undefined) {
              response.dps[dpsId] = value as string | number | boolean;
              console.log(`📦 Added cached DPS ${dpsId}=${value} to response`);
            }
          }
        }

        return response;
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));
        console.log(
          `⚠️ Status request attempt ${attempt}/${maxAttempts} failed for ${device.config.name}:`,
          lastError.message,
        );

        this.forceDisconnect(deviceId);

        if (attempt < maxAttempts) {
          await this.delay(CONNECTION_CONFIG.COMMAND_RETRY_DELAY_MS);
        }
      }
    }

    throw new Error(
      `Failed to get status from ${device.config.name} after ${maxAttempts} attempts: ${lastError?.message}`,
    );
  }

  /**
   * Get connection statistics for all devices
   */
  getConnectionStats(): {
    total: number;
    connected: number;
    disconnected: number;
    devices: Array<{
      id: string;
      name: string;
      type: string;
      isConnected: boolean;
      reconnectAttempts: number;
    }>;
  } {
    const devices = Array.from(this.devices.values());
    const connected = devices.filter((d) => d.isConnected).length;

    return {
      total: devices.length,
      connected,
      disconnected: devices.length - connected,
      devices: devices.map((d) => ({
        id: d.config.id,
        name: d.config.name,
        type: d.type,
        isConnected: d.isConnected,
        reconnectAttempts: d.reconnectAttempts,
      })),
    };
  }

  /**
   * Force reconnection of all disconnected devices
   */
  async reconnectDisconnected(): Promise<void> {
    console.log("🔄 Reconnecting disconnected devices...");

    for (const [deviceId, device] of Array.from(this.devices)) {
      if (!device.isConnected && !device.isConnecting) {
        device.reconnectAttempts = 0;
        this.scheduleReconnect(deviceId);
      }
    }
  }
}
