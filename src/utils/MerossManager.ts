import fs from "node:fs";
import path from "node:path";
import { MerossDevice } from "./MerossDevice";
import type { MerossDeviceConfig } from "./MerossDevice";

// ─── Meross Device Manager ───
// Manages multiple Meross MSS310 smart plugs via local HTTP control.
// Pattern follows existing DeviceManager for Tuya devices.

interface MerossDeviceInstance {
  config: MerossDeviceConfig;
  device: MerossDevice;
  isOnline: boolean;
  lastPing: number;
  pollInterval: NodeJS.Timeout | null;
}

const MEROSS_CONFIG_FILE = "meross-devices.json";
const POLL_INTERVAL_MS = 30000; // Poll status every 30s

export class MerossManager {
  private devices: Map<string, MerossDeviceInstance> = new Map();
  private configs: MerossDeviceConfig[] = [];

  constructor() {
    this.loadConfig();
  }

  private loadConfig(): void {
    try {
      const configPath = path.join(process.cwd(), MEROSS_CONFIG_FILE);
      if (!fs.existsSync(configPath)) {
        console.log(`[Meross] No ${MEROSS_CONFIG_FILE} found, skipping`);
        this.configs = [];
        return;
      }
      const configData = fs.readFileSync(configPath, "utf8");
      this.configs = JSON.parse(configData);
      console.log(`[Meross] Loaded ${this.configs.length} device configurations`);
    } catch (error) {
      console.error("[Meross] Failed to load configuration:", error);
      this.configs = [];
    }
  }

  /**
   * Initialize all configured devices and check connectivity
   */
  async initialize(): Promise<void> {
    console.log("[Meross] Initializing devices...");

    for (const config of this.configs) {
      const device = new MerossDevice(config);
      // Use ip as the unique device ID (each plug has a unique IP)
      const deviceId = config.ip;

      const instance: MerossDeviceInstance = {
        config,
        device,
        isOnline: false,
        lastPing: 0,
        pollInterval: null,
      };

      this.devices.set(deviceId, instance);
      console.log(`[Meross] Registered: ${config.name} (${config.ip})`);

      // Initial connectivity check
      try {
        const reachable = await device.ping();
        instance.isOnline = reachable;
        instance.lastPing = Date.now();
        if (reachable) {
          console.log(`[Meross] ${config.name} is online`);
        } else {
          console.log(`[Meross] ${config.name} is offline`);
        }
      } catch (error) {
        console.log(
          `[Meross] ${config.name} unreachable:`,
          error instanceof Error ? error.message : error,
        );
      }
    }

    // Start background polling for all devices
    this.startPolling();

    const onlineCount = Array.from(this.devices.values()).filter((d) => d.isOnline).length;
    console.log(`[Meross] Initialization complete: ${onlineCount}/${this.devices.size} online`);
  }

  /**
   * Start background status polling
   */
  private startPolling(): void {
    for (const [deviceId, instance] of this.devices) {
      if (instance.pollInterval) {
        clearInterval(instance.pollInterval);
      }

      instance.pollInterval = setInterval(async () => {
        try {
          const reachable = await instance.device.ping();
          const wasOnline = instance.isOnline;
          instance.isOnline = reachable;
          instance.lastPing = Date.now();

          if (!wasOnline && reachable) {
            console.log(`[Meross] ${instance.config.name} came online`);
          } else if (wasOnline && !reachable) {
            console.log(`[Meross] ${instance.config.name} went offline`);
          }
        } catch {
          instance.isOnline = false;
        }
      }, POLL_INTERVAL_MS);
    }
  }

  /**
   * Stop all background polling
   */
  shutdown(): void {
    console.log("[Meross] Shutting down...");
    for (const [, instance] of this.devices) {
      if (instance.pollInterval) {
        clearInterval(instance.pollInterval);
        instance.pollInterval = null;
      }
    }
  }

  // ─── Getters ───

  getDevice(deviceId: string): MerossDeviceInstance | undefined {
    return this.devices.get(deviceId);
  }

  getAllDevices(): MerossDeviceInstance[] {
    return Array.from(this.devices.values());
  }

  getDeviceByName(name: string): MerossDeviceInstance | undefined {
    return Array.from(this.devices.values()).find(
      (d) => d.config.name.toLowerCase() === name.toLowerCase(),
    );
  }

  /**
   * Get connection stats for all Meross devices
   */
  getStats(): {
    total: number;
    online: number;
    offline: number;
    devices: Array<{
      id: string;
      name: string;
      ip: string;
      isOnline: boolean;
      lastPing: number;
    }>;
  } {
    const devices = this.getAllDevices();
    const online = devices.filter((d) => d.isOnline).length;

    return {
      total: devices.length,
      online,
      offline: devices.length - online,
      devices: devices.map((d) => ({
        id: d.config.ip,
        name: d.config.name,
        ip: d.config.ip,
        isOnline: d.isOnline,
        lastPing: d.lastPing,
      })),
    };
  }
}
