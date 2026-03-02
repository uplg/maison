/**
 * Philips Hue Bluetooth Lamp Manager
 *
 * Handles BLE scanning, connection management, and lamp control
 * Uses @stoprocent/noble for Bluetooth Low Energy communication
 */

import noble, { Peripheral, Characteristic } from "@stoprocent/noble";
import fs from "node:fs";
import path from "node:path";
import {
  HUE_UUIDS,
  HueLampState,
  HueLampInfo,
  isHueLamp,
  parseBrightness,
  toBrightness,
  parseTemperature,
  toTemperature,
  buildControlCommand,
} from "./HueLamp";

export interface HueLampConfig {
  id: string;
  name: string;
  address: string;
  model?: string;
  /** True if we've successfully connected to this lamp at least once */
  hasConnectedOnce?: boolean;
  /** Discovered temperature limits (persisted to avoid rediscovery) */
  temperatureMin?: number;
  temperatureMax?: number;
  /** Last known temperature (to restore after power cycle) */
  lastTemperature?: number;
}

export interface HueLampInstance {
  config: HueLampConfig;
  peripheral: Peripheral | null;
  characteristics: {
    power?: Characteristic;
    brightness?: Characteristic;
    temperature?: Characteristic;
    control?: Characteristic;
    model?: Characteristic;
    firmware?: Characteristic;
    manufacturer?: Characteristic;
    deviceName?: Characteristic;
  };
  state: HueLampState;
  info: Partial<HueLampInfo>;
  isConnected: boolean;
  isConnecting: boolean;
  lastSeen: Date | null;
  reconnectAttempts: number;
  reconnectTimeout: NodeJS.Timeout | null;
  /** True if the lamp requires pairing (not owned/authorized) */
  pairingRequired: boolean;
  /** Number of consecutive connection failures */
  connectionFailures: number;
}

// Manager configuration
const HUE_CONFIG = {
  SCAN_INTERVAL_MS: 10000, // Scan every 10 seconds
  SCAN_DURATION_MS: 5000, // Scan for 5 seconds each time
  MAX_RECONNECT_ATTEMPTS: 5,
  RECONNECT_DELAY_MS: 3000,
  CONNECTION_TIMEOUT_MS: 15000,
  POLL_INTERVAL_MS: 30000, // Poll state every 30 seconds
  LAMPS_CONFIG_FILE: "hue-lamps.json",
  BLACKLIST_FILE: "hue-lamps-blacklist.json",
};

export class HueLampManager {
  private lamps: Map<string, HueLampInstance> = new Map();
  private configs: HueLampConfig[] = [];
  private discoveredPeripherals: Map<string, Peripheral> = new Map();
  /** Blacklisted addresses (unauthorized/unpaired lamps) */
  private blacklistedAddresses: Set<string> = new Set();
  private isScanning: boolean = false;
  private scanInterval: NodeJS.Timeout | null = null;
  private pollInterval: NodeJS.Timeout | null = null;
  private isInitialized: boolean = false;
  private configPath: string;
  private blacklistPath: string;

  constructor() {
    this.configPath = path.join(process.cwd(), HUE_CONFIG.LAMPS_CONFIG_FILE);
    this.blacklistPath = path.join(process.cwd(), HUE_CONFIG.BLACKLIST_FILE);
    this.loadConfig();
    this.loadBlacklist();
    this.setupNobleEventHandlers();
  }

  /**
   * Load lamp configurations from file
   */
  private loadConfig(): void {
    try {
      if (fs.existsSync(this.configPath)) {
        const data = fs.readFileSync(this.configPath, "utf8");
        this.configs = JSON.parse(data);
        console.log(`💡 Loaded ${this.configs.length} Hue lamp configurations`);
      } else {
        console.log("💡 No Hue lamp configuration found, starting fresh");
        this.configs = [];
      }
    } catch (error) {
      console.error("❌ Failed to load Hue lamp configuration:", error);
      this.configs = [];
    }
  }

  /**
   * Load blacklisted addresses from file
   */
  private loadBlacklist(): void {
    try {
      if (fs.existsSync(this.blacklistPath)) {
        const data = fs.readFileSync(this.blacklistPath, "utf8");
        const addresses: string[] = JSON.parse(data);
        this.blacklistedAddresses = new Set(addresses);
        console.log(`🚫 Loaded ${this.blacklistedAddresses.size} blacklisted lamp addresses`);
      }
    } catch (error) {
      console.error("❌ Failed to load blacklist:", error);
    }
  }

  /**
   * Save blacklisted addresses to file
   */
  private saveBlacklist(): void {
    try {
      const addresses = Array.from(this.blacklistedAddresses);
      fs.writeFileSync(this.blacklistPath, JSON.stringify(addresses, null, 2));
      console.log(`💾 Saved ${addresses.length} blacklisted addresses`);
    } catch (error) {
      console.error("❌ Failed to save blacklist:", error);
    }
  }

  /**
   * Add an address to the blacklist
   */
  private blacklistAddress(address: string, name: string): void {
    this.blacklistedAddresses.add(address);
    this.saveBlacklist();
    console.log(`🚫 Blacklisted lamp: ${name} (${address})`);
  }

  /**
   * Manually blacklist and remove a lamp by ID
   * Use this for lamps that are stuck in config but can't be reached
   */
  blacklistLamp(lampId: string): boolean {
    const lamp = this.lamps.get(lampId);
    const config = this.configs.find((c) => c.id === lampId);

    if (!lamp && !config) {
      return false;
    }

    const name = lamp?.config.name || config?.name || lampId;
    const address = lamp?.config.address || config?.address || lampId;

    console.log(`🚫 Manually blacklisting lamp: ${name}`);
    this.removeLampFromConfig(lampId);

    return true;
  }

  /**
   * Get list of blacklisted addresses
   */
  getBlacklist(): string[] {
    return Array.from(this.blacklistedAddresses);
  }

  /**
   * Remove an address from blacklist (to allow re-discovery)
   */
  unblacklistAddress(address: string): boolean {
    if (this.blacklistedAddresses.has(address)) {
      this.blacklistedAddresses.delete(address);
      this.saveBlacklist();
      console.log(`✅ Removed ${address} from blacklist`);
      return true;
    }
    return false;
  }

  /**
   * Save lamp configurations to file
   */
  private saveConfig(): void {
    try {
      fs.writeFileSync(this.configPath, JSON.stringify(this.configs, null, 2));
      console.log(`💾 Saved ${this.configs.length} Hue lamp configurations`);
    } catch (error) {
      console.error("❌ Failed to save Hue lamp configuration:", error);
    }
  }

  /**
   * Remove a lamp from config (for unauthorized/unpaired lamps)
   * Also adds the address to the blacklist to prevent re-discovery
   */
  private removeLampFromConfig(lampId: string): void {
    const lamp = this.lamps.get(lampId);
    const configIndex = this.configs.findIndex((c) => c.id === lampId);

    if (configIndex !== -1) {
      const config = this.configs[configIndex];
      const lampName = config.name;
      const lampAddress = config.address;

      // Add to blacklist to prevent re-discovery
      this.blacklistAddress(lampAddress, lampName);
      if (lampId !== lampAddress) {
        this.blacklistAddress(lampId, lampName);
      }

      this.configs.splice(configIndex, 1);
      this.saveConfig();
      console.log(`🗑️ Removed unauthorized lamp from config: ${lampName}`);
    }

    // Also remove from lamps map
    if (lamp) {
      if (lamp.reconnectTimeout) {
        clearTimeout(lamp.reconnectTimeout);
      }
      this.lamps.delete(lampId);
    }
  }

  /**
   * Setup Noble BLE event handlers
   */
  private setupNobleEventHandlers(): void {
    noble.on("stateChange", (state) => {
      console.log(`🔵 Bluetooth state: ${state}`);
      if (state === "poweredOn") {
        this.isInitialized = true;
        this.startPeriodicScan();
      } else {
        this.isInitialized = false;
        this.stopPeriodicScan();
      }
    });

    noble.on("discover", (peripheral) => {
      this.handleDiscoveredPeripheral(peripheral);
    });

    noble.on("scanStart", () => {
      console.log("🔍 BLE scan started");
      this.isScanning = true;
    });

    noble.on("scanStop", () => {
      console.log("🔍 BLE scan stopped");
      this.isScanning = false;
    });
  }

  /**
   * Handle a discovered BLE peripheral
   */
  private handleDiscoveredPeripheral(peripheral: Peripheral): void {
    const localName = peripheral.advertisement?.localName;
    const manufacturerData = peripheral.advertisement?.manufacturerData;
    const serviceUuids = peripheral.advertisement?.serviceUuids || [];
    const address = peripheral.address || peripheral.id;

    // Debug: log all discovered devices with names (uncomment for debugging)
    // if (localName) {
    //   console.log(
    //     `🔎 BLE device: ${localName} (${address}) services: ${
    //       serviceUuids.join(", ") || "none"
    //     }`
    //   );
    // }

    // Check if this looks like a Hue lamp
    if (!isHueLamp(localName, manufacturerData, serviceUuids)) {
      return;
    }

    // Check if this address is blacklisted (unauthorized lamp)
    if (this.blacklistedAddresses.has(address) || this.blacklistedAddresses.has(peripheral.id)) {
      // Silently ignore blacklisted lamps
      return;
    }

    console.log(
      `💡 Discovered Hue lamp: ${
        localName || "Unknown"
      } (${address}) [services: ${serviceUuids.length}]`,
    );

    // Store discovered peripheral
    this.discoveredPeripherals.set(address, peripheral);

    // Check if we have a config for this lamp
    const existingConfig = this.configs.find(
      (c) => c.address === address || c.id === peripheral.id,
    );

    if (existingConfig) {
      // Update existing lamp instance
      const lamp = this.lamps.get(existingConfig.id);
      if (lamp) {
        lamp.peripheral = peripheral;
        lamp.lastSeen = new Date();

        // Check if peripheral is already connected (state can get out of sync)
        if (peripheral.state === "connected") {
          if (!lamp.isConnected) {
            console.log(`🔄 ${lamp.config.name} peripheral is connected, syncing state...`);
            lamp.isConnected = true;
            lamp.isConnecting = false;
            lamp.state.reachable = true;
          }
          return; // Already connected, nothing to do
        }

        // Auto-connect if not connected and not already trying
        if (!lamp.isConnected && !lamp.isConnecting) {
          this.connectLamp(existingConfig.id);
        }
      }
    } else {
      // New lamp discovered - add to config
      const newConfig: HueLampConfig = {
        id: peripheral.id,
        name: localName || `Hue Lamp ${address.slice(-5)}`,
        address: address,
      };
      this.configs.push(newConfig);
      this.saveConfig();

      // Create lamp instance
      this.createLampInstance(newConfig);

      // Auto-connect
      this.connectLamp(newConfig.id);
    }
  }

  /**
   * Create a lamp instance from config
   */
  private createLampInstance(config: HueLampConfig): HueLampInstance {
    const instance: HueLampInstance = {
      config,
      peripheral: null,
      characteristics: {},
      state: {
        isOn: false,
        brightness: 254,
        reachable: false,
      },
      info: {
        id: config.id,
        name: config.name,
        address: config.address,
      },
      isConnected: false,
      isConnecting: false,
      lastSeen: null,
      reconnectAttempts: 0,
      reconnectTimeout: null,
      pairingRequired: false,
      connectionFailures: 0,
    };

    this.lamps.set(config.id, instance);
    return instance;
  }

  /**
   * Initialize the manager
   */
  async initialize(): Promise<void> {
    console.log("💡 Initializing Hue Lamp Manager...");

    // Create lamp instances from configs
    for (const config of this.configs) {
      if (!this.lamps.has(config.id)) {
        this.createLampInstance(config);
      }
    }

    // Wait for Bluetooth to be ready
    if ((noble as any).state === "poweredOn") {
      this.isInitialized = true;
      this.startPeriodicScan();
    }

    console.log("💡 Hue Lamp Manager initialized");
  }

  /**
   * Start periodic BLE scanning
   */
  private startPeriodicScan(): void {
    if (this.scanInterval) {
      return;
    }

    console.log("💡 Starting periodic BLE scan...");

    // Initial scan
    this.performScan();

    // Periodic scan
    this.scanInterval = setInterval(() => {
      this.performScan();
    }, HUE_CONFIG.SCAN_INTERVAL_MS);

    // Start polling connected lamps
    this.startPolling();
  }

  /**
   * Stop periodic scanning
   */
  private stopPeriodicScan(): void {
    if (this.scanInterval) {
      clearInterval(this.scanInterval);
      this.scanInterval = null;
    }
    this.stopPolling();
  }

  /**
   * Perform a single BLE scan
   */
  private async performScan(): Promise<void> {
    if (!this.isInitialized || this.isScanning) {
      return;
    }

    try {
      // Scan ALL devices (no service filter) because Hue lamps don't always
      // advertise their service UUIDs. We'll filter in handleDiscoveredPeripheral
      await noble.startScanningAsync(
        [], // No filter - scan all devices
        true, // Allow duplicates to detect devices coming back in range
      );

      // Stop after duration
      setTimeout(async () => {
        if (this.isScanning) {
          await noble.stopScanningAsync();
        }
      }, HUE_CONFIG.SCAN_DURATION_MS);
    } catch (error) {
      console.error("❌ BLE scan error:", error);
    }
  }

  /**
   * Start polling connected lamps for state updates
   */
  private startPolling(): void {
    if (this.pollInterval) {
      return;
    }

    this.pollInterval = setInterval(async () => {
      for (const [id, lamp] of this.lamps) {
        if (lamp.isConnected) {
          try {
            await this.refreshLampState(id);
          } catch (error) {
            console.error(`❌ Failed to poll lamp ${id}:`, error);
          }
        }
      }
    }, HUE_CONFIG.POLL_INTERVAL_MS);
  }

  /**
   * Stop polling
   */
  private stopPolling(): void {
    if (this.pollInterval) {
      clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
  }

  /**
   * Connect to a specific lamp
   */
  async connectLamp(lampId: string): Promise<boolean> {
    const lamp = this.lamps.get(lampId);
    if (!lamp) {
      console.error(`❌ Lamp ${lampId} not found`);
      return false;
    }

    if (lamp.isConnected) {
      console.log(`💡 Lamp ${lamp.config.name} already connected`);
      return true;
    }

    if (lamp.isConnecting) {
      console.log(`💡 Lamp ${lamp.config.name} connection in progress...`);
      return false;
    }

    // Find peripheral
    let peripheral = lamp.peripheral;
    if (!peripheral) {
      peripheral =
        this.discoveredPeripherals.get(lamp.config.address) ||
        this.discoveredPeripherals.get(lamp.config.id) ||
        null;
    }

    if (!peripheral) {
      console.log(`💡 Lamp ${lamp.config.name} not in range, waiting for scan...`);
      lamp.state.reachable = false;
      return false;
    }

    // Check if peripheral is already connected at BLE level
    // This can happen if our state got out of sync
    if (peripheral.state === "connected") {
      console.log(`💡 ${lamp.config.name} peripheral already connected, syncing state...`);
      lamp.peripheral = peripheral;
      lamp.isConnected = true;
      lamp.isConnecting = false;
      lamp.state.reachable = true;
      lamp.reconnectAttempts = 0;

      // Setup disconnect handler if not already set
      peripheral.removeAllListeners("disconnect");
      peripheral.once("disconnect", () => {
        console.log(`📴 Lamp ${lamp.config.name} disconnected`);
        lamp.isConnected = false;
        lamp.state.reachable = false;
        lamp.characteristics = {};
        this.scheduleReconnect(lampId);
      });

      // Always rediscover characteristics when syncing state
      // Previous characteristics may be stale after a BLE reconnection
      try {
        console.log(`🔄 ${lamp.config.name} syncing - rediscovering characteristics...`);
        await this.discoverCharacteristics(lampId);
        await this.subscribeToNotifications(lampId);
        await this.refreshLampState(lampId, true);
      } catch (error) {
        console.error(`⚠️ Failed to rediscover characteristics for ${lamp.config.name}:`, error);
        // If we can't get characteristics, mark as not really connected
        lamp.isConnected = false;
        lamp.state.reachable = false;
        lamp.characteristics = {};
      }

      return true;
    }

    lamp.isConnecting = true;
    lamp.peripheral = peripheral;

    try {
      console.log(`🔗 Connecting to ${lamp.config.name}...`);

      // Remove any existing disconnect listeners to avoid duplicates
      peripheral.removeAllListeners("disconnect");

      // Setup disconnect handler
      peripheral.once("disconnect", () => {
        console.log(`📴 Lamp ${lamp.config.name} disconnected`);
        lamp.isConnected = false;
        lamp.state.reachable = false;
        lamp.characteristics = {};

        // Schedule reconnection
        this.scheduleReconnect(lampId);
      });

      // Connect with timeout
      await Promise.race([
        peripheral.connectAsync(),
        new Promise((_, reject) =>
          setTimeout(
            () => reject(new Error("Connection timeout")),
            HUE_CONFIG.CONNECTION_TIMEOUT_MS,
          ),
        ),
      ]);

      console.log(`✅ Connected to ${lamp.config.name}`);

      // Discover services and characteristics
      await this.discoverCharacteristics(lampId);

      // Subscribe to notifications for real-time updates
      await this.subscribeToNotifications(lampId);

      // Read initial state (skip connection check as we're still connecting)
      await this.refreshLampState(lampId, true);

      // Read device info
      await this.readDeviceInfo(lampId);

      lamp.isConnected = true;
      lamp.isConnecting = false;
      lamp.state.reachable = true;
      lamp.reconnectAttempts = 0;
      lamp.connectionFailures = 0;
      lamp.pairingRequired = false; // Reset if connection succeeds

      // Mark as successfully connected at least once
      if (!lamp.config.hasConnectedOnce) {
        lamp.config.hasConnectedOnce = true;
        const configIndex = this.configs.findIndex((c) => c.id === lampId);
        if (configIndex !== -1) {
          this.configs[configIndex].hasConnectedOnce = true;
          this.saveConfig();
        }
      }

      return true;
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      console.error(`❌ Failed to connect to ${lamp.config.name}:`, errorMessage);
      lamp.isConnected = false;
      lamp.isConnecting = false;
      lamp.state.reachable = false;
      lamp.connectionFailures++;

      // Detect authorization/pairing errors or persistent connection issues
      // These indicate the lamp belongs to someone else, needs pairing, or is unreachable
      const isAuthError =
        errorMessage.includes("401") ||
        errorMessage.includes("Unauthorized") ||
        errorMessage.includes("not authorized");

      // Connection timeout after multiple failures suggests lamp is not ours
      const isTimeoutError = errorMessage.toLowerCase().includes("timeout");

      if (isAuthError) {
        console.log(`🔒 Lamp ${lamp.config.name} requires pairing (not authorized)`);
        lamp.pairingRequired = true;

        this.scheduleReconnect(lampId);

        return false;
      }

      // Timeout errors: only blacklist NEW lamps (never connected before)
      if (isTimeoutError && lamp.connectionFailures >= 3 && !lamp.config.hasConnectedOnce) {
        console.log(
          `⏱️ New lamp ${lamp.config.name} timed out ${lamp.connectionFailures} times, blacklisting`,
        );
        lamp.pairingRequired = true;
        this.removeLampFromConfig(lampId);
        return false;
      }

      // If we've failed too many times on a NEW lamp, blacklist it
      if (
        lamp.connectionFailures >= HUE_CONFIG.MAX_RECONNECT_ATTEMPTS &&
        !lamp.config.hasConnectedOnce
      ) {
        console.log(
          `⚠️ New lamp ${lamp.config.name} failed ${lamp.connectionFailures} times, blacklisting`,
        );
        lamp.pairingRequired = true;
        this.removeLampFromConfig(lampId);
        return false;
      }

      this.scheduleReconnect(lampId);
      return false;
    }
  }

  /**
   * Normalize UUID for comparison
   * Noble may return short UUIDs (e.g., "2a24") or long UUIDs without dashes
   */
  private normalizeUuid(uuid: string): string {
    return uuid.replace(/-/g, "").toLowerCase();
  }

  /**
   * Check if a UUID matches (handles both short and long formats)
   * Short format: "2a24" matches "00002a24-0000-1000-8000-00805f9b34fb"
   */
  private uuidMatches(charUuid: string, targetUuid: string): boolean {
    const normalizedChar = this.normalizeUuid(charUuid);
    const normalizedTarget = this.normalizeUuid(targetUuid);

    // Exact match
    if (normalizedChar === normalizedTarget) {
      return true;
    }

    // Short UUID match (e.g., "2a24" should match the standard Bluetooth UUID)
    // Standard Bluetooth UUIDs have format: 0000XXXX-0000-1000-8000-00805f9b34fb
    // where XXXX is the short UUID
    if (normalizedChar.length === 4) {
      return normalizedTarget.startsWith(`0000${normalizedChar}`);
    }

    return false;
  }

  /**
   * Discover BLE characteristics for a lamp
   */
  private async discoverCharacteristics(lampId: string): Promise<void> {
    const lamp = this.lamps.get(lampId);
    if (!lamp?.peripheral) return;

    // Clear existing characteristics before rediscovery
    lamp.characteristics = {};

    const { characteristics } = await lamp.peripheral.discoverAllServicesAndCharacteristicsAsync();

    for (const char of characteristics) {
      const uuid = char.uuid;

      if (this.uuidMatches(uuid, HUE_UUIDS.POWER)) {
        lamp.characteristics.power = char;
      } else if (this.uuidMatches(uuid, HUE_UUIDS.BRIGHTNESS)) {
        lamp.characteristics.brightness = char;
      } else if (this.uuidMatches(uuid, HUE_UUIDS.TEMPERATURE)) {
        lamp.characteristics.temperature = char;
      } else if (this.uuidMatches(uuid, HUE_UUIDS.CONTROL)) {
        lamp.characteristics.control = char;
      } else if (this.uuidMatches(uuid, HUE_UUIDS.MODEL)) {
        lamp.characteristics.model = char;
      } else if (this.uuidMatches(uuid, HUE_UUIDS.FIRMWARE)) {
        lamp.characteristics.firmware = char;
      } else if (this.uuidMatches(uuid, HUE_UUIDS.MANUFACTURER)) {
        lamp.characteristics.manufacturer = char;
      } else if (this.uuidMatches(uuid, HUE_UUIDS.DEVICE_NAME)) {
        lamp.characteristics.deviceName = char;
      }
    }

    const foundChars = Object.keys(lamp.characteristics);
    console.log(
      `📋 Discovered ${foundChars.length} characteristics for ${
        lamp.config.name
      }: ${foundChars.join(", ")}`,
    );

    // Discover temperature limits if lamp supports temperature
    if (lamp.characteristics.temperature) {
      await this.discoverTemperatureLimits(lampId);
    }
  }

  /**
   * Discover the actual temperature limits of a lamp by testing extreme values
   * Some lamps have a narrower range than the full 0-100%
   * Limits are persisted in config to avoid rediscovery on reconnect
   */
  private async discoverTemperatureLimits(lampId: string): Promise<void> {
    const lamp = this.lamps.get(lampId);
    if (!lamp?.characteristics.temperature) return;

    // Check if we already have saved limits in config
    if (lamp.config.temperatureMin !== undefined && lamp.config.temperatureMax !== undefined) {
      lamp.state.temperatureMin = lamp.config.temperatureMin;
      lamp.state.temperatureMax = lamp.config.temperatureMax;
      console.log(
        `🌡️ ${lamp.config.name} using saved temperature limits: ${lamp.state.temperatureMin}% - ${lamp.state.temperatureMax}%`,
      );

      // Restore last saved temperature if available
      if (lamp.config.lastTemperature !== undefined) {
        try {
          const rawTemp = toTemperature(lamp.config.lastTemperature);
          await lamp.characteristics.temperature.writeAsync(Buffer.from([rawTemp, 0x01]), false);
          lamp.state.temperature = lamp.config.lastTemperature;
          console.log(
            `🌡️ ${lamp.config.name} restored temperature to ${lamp.config.lastTemperature}% (raw: ${rawTemp})`,
          );
        } catch (error) {
          console.error(`❌ Failed to restore temperature for ${lamp.config.name}:`, error);
        }
      }
      return;
    }

    console.log(`🌡️ Discovering temperature limits for ${lamp.config.name}...`);

    try {
      // Read current temperature to restore later
      const currentData = await lamp.characteristics.temperature.readAsync();
      let currentRaw = 122; // Default to middle if read fails
      if (currentData.length >= 2 && currentData[1] === 0x01) {
        currentRaw = currentData[0];
        console.log(
          `🌡️ ${lamp.config.name} current temperature before discovery: raw=${currentRaw}`,
        );
      }

      // Test minimum (warmest) - send raw 244 (0%)
      // This is the most restrictive limit - some lamps can't go very warm
      const minRaw = toTemperature(0);
      await lamp.characteristics.temperature.writeAsync(Buffer.from([minRaw, 0x01]), false);
      await new Promise((resolve) => setTimeout(resolve, 1000)); // Wait for lamp to settle
      const minData = await lamp.characteristics.temperature.readAsync();
      if (minData.length >= 2 && minData[1] === 0x01) {
        lamp.state.temperatureMin = parseTemperature(minData[0]);
        console.log(
          `🌡️ ${lamp.config.name} min temperature: ${lamp.state.temperatureMin}% (raw: ${minData[0]})`,
        );
      }

      // Always allow max to be 100% - lamps generally accept cool temperatures fine
      // The min limit is what matters for warm temperatures
      lamp.state.temperatureMax = 100;
      console.log(`🌡️ ${lamp.config.name} max temperature: 100% (always allowed)`);

      // Save limits to config for persistence
      lamp.config.temperatureMin = lamp.state.temperatureMin;
      lamp.config.temperatureMax = lamp.state.temperatureMax;
      // Also save current temperature as lastTemperature for future power cycles
      lamp.config.lastTemperature = lamp.state.temperature;
      this.saveConfig();

      // Restore to original value
      await lamp.characteristics.temperature.writeAsync(Buffer.from([currentRaw, 0x01]), false);
      lamp.state.temperature = parseTemperature(currentRaw);
      console.log(
        `🌡️ ${lamp.config.name} restored to ${lamp.state.temperature}% (raw: ${currentRaw})`,
      );

      console.log(
        `🌡️ ${lamp.config.name} temperature range: ${lamp.state.temperatureMin}% - ${lamp.state.temperatureMax}%`,
      );
    } catch (error) {
      console.error(`❌ Failed to discover temperature limits for ${lamp.config.name}:`, error);
      // Default to full range on error
      lamp.state.temperatureMin = 0;
      lamp.state.temperatureMax = 100;
    }
  }

  /**
   * Subscribe to BLE notifications for real-time state updates
   * This allows detecting changes made from the Hue app or other sources
   */
  private async subscribeToNotifications(lampId: string): Promise<void> {
    const lamp = this.lamps.get(lampId);
    if (!lamp) return;

    try {
      // Subscribe to power state notifications
      if (lamp.characteristics.power) {
        const powerChar = lamp.characteristics.power;

        // Check if characteristic supports notifications
        if (powerChar.properties.includes("notify")) {
          powerChar.on("data", (data: Buffer) => {
            const wasOn = lamp.state.isOn;
            lamp.state.isOn = data[0] === 0x01;
            if (wasOn !== lamp.state.isOn) {
              console.log(
                `💡 ${lamp.config.name} power changed: ${lamp.state.isOn ? "ON" : "OFF"}`,
              );
            }
          });

          await powerChar.subscribeAsync();
          console.log(`🔔 Subscribed to power notifications for ${lamp.config.name}`);
        }
      }

      // Subscribe to brightness notifications
      if (lamp.characteristics.brightness) {
        const brightnessChar = lamp.characteristics.brightness;

        if (brightnessChar.properties.includes("notify")) {
          brightnessChar.on("data", (data: Buffer) => {
            const oldBrightness = lamp.state.brightness;
            lamp.state.brightness = data[0];
            if (oldBrightness !== lamp.state.brightness) {
              console.log(`💡 ${lamp.config.name} brightness changed: ${lamp.state.brightness}`);
            }
          });

          await brightnessChar.subscribeAsync();
          console.log(`🔔 Subscribed to brightness notifications for ${lamp.config.name}`);
        }
      }

      // Subscribe to control characteristic notifications (combined state)
      if (lamp.characteristics.control) {
        const controlChar = lamp.characteristics.control;

        if (controlChar.properties.includes("notify")) {
          controlChar.on("data", (data: Buffer) => {
            // Parse control characteristic data
            // Format varies but typically includes power and brightness
            if (data.length >= 1) {
              console.log(`💡 ${lamp.config.name} control notification: ${data.toString("hex")}`);
              // Refresh full state on control changes
              this.refreshLampState(lampId).catch(() => {});
            }
          });

          await controlChar.subscribeAsync();
          console.log(`🔔 Subscribed to control notifications for ${lamp.config.name}`);
        }
      }
    } catch (error) {
      console.error(`⚠️ Failed to subscribe to notifications for ${lamp.config.name}:`, error);
      // Non-fatal error - polling will still work as fallback
    }
  }

  /**
   * Read device info from lamp
   */
  private async readDeviceInfo(lampId: string): Promise<void> {
    const lamp = this.lamps.get(lampId);
    if (!lamp) return;

    try {
      if (lamp.characteristics.model) {
        const data = await lamp.characteristics.model.readAsync();
        lamp.info.model = data.toString("utf8").trim();
      }

      if (lamp.characteristics.manufacturer) {
        const data = await lamp.characteristics.manufacturer.readAsync();
        lamp.info.manufacturer = data.toString("utf8").trim();
      }

      if (lamp.characteristics.firmware) {
        const data = await lamp.characteristics.firmware.readAsync();
        lamp.info.firmware = data.toString("utf8").trim();
      }

      if (lamp.characteristics.deviceName) {
        const data = await lamp.characteristics.deviceName.readAsync();
        lamp.info.name = data.toString("utf8").trim();
        lamp.config.name = lamp.info.name;
      }

      // Update config with model if discovered
      if (lamp.info.model && !lamp.config.model) {
        lamp.config.model = lamp.info.model;
        this.saveConfig();
      }

      console.log(
        `📋 Device info: ${lamp.info.name} (${lamp.info.model}) - ${lamp.info.manufacturer}`,
      );
    } catch (error) {
      console.error(`❌ Failed to read device info for ${lampId}:`, error);
    }
  }

  /**
   * Refresh lamp state by reading characteristics
   * @param skipConnectionCheck - Skip isConnected check (used during initial connection)
   */
  async refreshLampState(
    lampId: string,
    skipConnectionCheck = false,
  ): Promise<HueLampState | null> {
    const lamp = this.lamps.get(lampId);
    if (!lamp) {
      return null;
    }

    // During initial connection, we skip this check as isConnected is set after
    if (!skipConnectionCheck && !lamp.isConnected) {
      return null;
    }

    // Check peripheral state first - detect disconnects early
    if (!lamp.peripheral || lamp.peripheral.state !== "connected") {
      console.log(`🔌 ${lamp.config.name} peripheral disconnected during refresh`);
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};
      if (!skipConnectionCheck) {
        this.scheduleReconnect(lampId);
      }
      return null;
    }

    try {
      // Read power state
      if (lamp.characteristics.power) {
        const data = await lamp.characteristics.power.readAsync();
        lamp.state.isOn = data[0] === 0x01;
        console.log(`💡 ${lamp.config.name} power state: ${lamp.state.isOn ? "ON" : "OFF"}`);
      }

      // Read brightness
      if (lamp.characteristics.brightness) {
        const data = await lamp.characteristics.brightness.readAsync();
        lamp.state.brightness = data[0];
      }

      // Read temperature if available
      if (lamp.characteristics.temperature) {
        const data = await lamp.characteristics.temperature.readAsync();
        console.log(
          `🌡️ ${lamp.config.name} raw temperature data:`,
          data.toString("hex"),
          `bytes: [${data[0]}, ${data[1]}]`,
        );
        if (data.length >= 2 && data[1] === 0x01) {
          // Convert raw value (1-244) to percentage (0-100)
          const rawTemp = data[0];
          const parsedTemp = parseTemperature(rawTemp);
          console.log(
            `🌡️ ${lamp.config.name} temperature: raw=${rawTemp} -> parsed=${parsedTemp}%`,
          );
          lamp.state.temperature = parsedTemp;

          // Update observed min/max limits
          if (lamp.state.temperatureMin === undefined || parsedTemp < lamp.state.temperatureMin) {
            lamp.state.temperatureMin = parsedTemp;
          }
          if (lamp.state.temperatureMax === undefined || parsedTemp > lamp.state.temperatureMax) {
            lamp.state.temperatureMax = parsedTemp;
          }
        }
      }

      lamp.state.reachable = true;
      return lamp.state;
    } catch (error) {
      console.error(`❌ Failed to read state for ${lampId}:`, error);

      // Mark lamp as disconnected when read fails
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};

      console.log(`🔌 ${lamp.config.name} marked as disconnected due to read failure`);

      if (!skipConnectionCheck) {
        this.scheduleReconnect(lampId);
      }

      return null;
    }
  }

  /**
   * Schedule a reconnection attempt
   */
  private scheduleReconnect(lampId: string): void {
    const lamp = this.lamps.get(lampId);
    if (!lamp) return;

    if (lamp.reconnectAttempts >= HUE_CONFIG.MAX_RECONNECT_ATTEMPTS) {
      console.log(
        `💡 Max reconnect attempts reached for ${lamp.config.name}, waiting for next scan to rediscover...`,
      );
      lamp.reconnectAttempts = 0;
      // Clear the peripheral reference so next scan provides a fresh one
      lamp.peripheral = null;
      lamp.isConnecting = false;
      return;
    }

    if (lamp.reconnectTimeout) {
      clearTimeout(lamp.reconnectTimeout);
    }

    const delay = HUE_CONFIG.RECONNECT_DELAY_MS * Math.pow(2, lamp.reconnectAttempts);
    lamp.reconnectAttempts++;

    console.log(
      `🔄 Scheduling reconnect for ${lamp.config.name} in ${delay}ms (attempt ${lamp.reconnectAttempts})`,
    );

    lamp.reconnectTimeout = setTimeout(() => {
      this.connectLamp(lampId);
    }, delay);
  }

  /**
   * Disconnect a lamp
   */
  async disconnectLamp(lampId: string): Promise<void> {
    const lamp = this.lamps.get(lampId);
    if (!lamp?.peripheral) return;

    if (lamp.reconnectTimeout) {
      clearTimeout(lamp.reconnectTimeout);
      lamp.reconnectTimeout = null;
    }

    try {
      await lamp.peripheral.disconnectAsync();
    } catch (error) {
      console.error(`❌ Error disconnecting lamp ${lampId}:`, error);
    }

    lamp.isConnected = false;
    lamp.state.reachable = false;
  }

  /**
   * Connect all configured lamps
   */
  async connectAllLamps(): Promise<void> {
    for (const config of this.configs) {
      await this.connectLamp(config.id);
    }
  }

  /**
   * Disconnect all lamps
   */
  async disconnectAllLamps(): Promise<void> {
    for (const [id] of this.lamps) {
      await this.disconnectLamp(id);
    }
  }

  /**
   * Turn a lamp on or off
   */
  async setPower(lampId: string, on: boolean): Promise<boolean> {
    const lamp = this.lamps.get(lampId);
    if (!lamp?.isConnected) {
      console.error(`❌ Lamp ${lampId} not connected`);
      return false;
    }

    // Check if we have valid characteristics
    if (!lamp.characteristics.power && !lamp.characteristics.control) {
      console.error(
        `❌ Lamp ${lamp.config.name} has no power/control characteristics, trying to rediscover...`,
      );
      try {
        await this.discoverCharacteristics(lampId);
      } catch (error) {
        console.error(`❌ Failed to rediscover characteristics for ${lamp.config.name}:`, error);
        return false;
      }
    }

    try {
      if (lamp.characteristics.power) {
        const data = Buffer.from([on ? 0x01 : 0x00]);
        await lamp.characteristics.power.writeAsync(data, false);
        lamp.state.isOn = on;
        console.log(`💡 Lamp ${lamp.config.name} turned ${on ? "on" : "off"}`);
        return true;
      } else if (lamp.characteristics.control) {
        const command = buildControlCommand({ power: on });
        await lamp.characteristics.control.writeAsync(command, false);
        lamp.state.isOn = on;
        console.log(`💡 Lamp ${lamp.config.name} turned ${on ? "on" : "off"}`);
        return true;
      }
      console.error(
        `❌ Lamp ${lamp.config.name} has no power/control characteristics after rediscovery`,
      );
      return false;
    } catch (error) {
      console.error(`❌ Failed to set power for ${lamp.config.name}:`, error);
      // Mark as disconnected if write fails - characteristics may be stale
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};
      this.scheduleReconnect(lampId);
      return false;
    }
  }

  /**
   * Set lamp brightness (1-100 percentage)
   */
  async setBrightness(lampId: string, percentage: number): Promise<boolean> {
    const lamp = this.lamps.get(lampId);
    if (!lamp?.isConnected) {
      console.error(`❌ Lamp ${lampId} not connected`);
      return false;
    }

    const brightness = toBrightness(percentage);

    // Check if we have valid characteristics
    if (!lamp.characteristics.brightness && !lamp.characteristics.control) {
      console.error(
        `❌ Lamp ${lamp.config.name} has no brightness/control characteristics, trying to rediscover...`,
      );
      try {
        await this.discoverCharacteristics(lampId);
      } catch (error) {
        console.error(`❌ Failed to rediscover characteristics for ${lamp.config.name}:`, error);
        return false;
      }
    }

    try {
      if (lamp.characteristics.brightness) {
        const data = Buffer.from([brightness]);
        await lamp.characteristics.brightness.writeAsync(data, false);
        lamp.state.brightness = brightness;
        console.log(`💡 Lamp ${lamp.config.name} brightness set to ${percentage}%`);
        return true;
      } else if (lamp.characteristics.control) {
        const command = buildControlCommand({ brightness });
        await lamp.characteristics.control.writeAsync(command, false);
        lamp.state.brightness = brightness;
        console.log(`💡 Lamp ${lamp.config.name} brightness set to ${percentage}%`);
        return true;
      }
      console.error(
        `❌ Lamp ${lamp.config.name} has no brightness/control characteristics after rediscovery`,
      );
      return false;
    } catch (error) {
      console.error(`❌ Failed to set brightness for ${lamp.config.name}:`, error);
      // Mark as disconnected if write fails - characteristics may be stale
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};
      this.scheduleReconnect(lampId);
      return false;
    }
  }

  /**
   * Set lamp color temperature (warm to cool white)
   * @param lampId - Lamp ID
   * @param temperature - Temperature value 1-100 (1=warm/yellow, 100=cool/white)
   */
  async setTemperature(lampId: string, temperature: number): Promise<boolean> {
    const lamp = this.lamps.get(lampId);
    if (!lamp?.isConnected) {
      console.error(`❌ Lamp ${lampId} not connected`);
      return false;
    }

    // Convert percentage to raw value using toTemperature helper
    const rawTemp = toTemperature(temperature);

    // Check if we have temperature characteristic
    if (!lamp.characteristics.temperature && !lamp.characteristics.control) {
      console.error(
        `❌ Lamp ${lamp.config.name} has no temperature/control characteristics, trying to rediscover...`,
      );
      try {
        await this.discoverCharacteristics(lampId);
      } catch (error) {
        console.error(`❌ Failed to rediscover characteristics for ${lamp.config.name}:`, error);
        return false;
      }
    }

    try {
      if (lamp.characteristics.temperature) {
        // Temperature characteristic format: [value, 0x01 (enable flag)]
        const data = Buffer.from([rawTemp, 0x01]);
        await lamp.characteristics.temperature.writeAsync(data, false);
        lamp.state.temperature = temperature; // Store as percentage, not raw
        // Save last temperature to config for persistence across power cycles
        lamp.config.lastTemperature = temperature;
        this.saveConfig();
        console.log(
          `💡 Lamp ${lamp.config.name} temperature set to ${temperature}% (raw: ${rawTemp})`,
        );
        return true;
      } else if (lamp.characteristics.control) {
        const command = buildControlCommand({ temperature: rawTemp });
        await lamp.characteristics.control.writeAsync(command, false);
        lamp.state.temperature = temperature; // Store as percentage, not raw
        // Save last temperature to config for persistence across power cycles
        lamp.config.lastTemperature = temperature;
        this.saveConfig();
        console.log(
          `💡 Lamp ${lamp.config.name} temperature set to ${temperature}% (raw: ${rawTemp})`,
        );
        return true;
      }
      console.error(`❌ Lamp ${lamp.config.name} does not support color temperature`);
      return false;
    } catch (error) {
      console.error(`❌ Failed to set temperature for ${lamp.config.name}:`, error);
      // Mark as disconnected if write fails
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};
      this.scheduleReconnect(lampId);
      return false;
    }
  }

  /**
   * Set lamp power and brightness together
   */
  async setLampState(lampId: string, on: boolean, brightness?: number): Promise<boolean> {
    const lamp = this.lamps.get(lampId);
    if (!lamp?.isConnected) {
      console.error(`❌ Lamp ${lampId} not connected`);
      return false;
    }

    try {
      if (lamp.characteristics.control) {
        const command = buildControlCommand({
          power: on,
          brightness: brightness ? toBrightness(brightness) : undefined,
        });
        await lamp.characteristics.control.writeAsync(command, false);
        lamp.state.isOn = on;
        if (brightness !== undefined) {
          lamp.state.brightness = toBrightness(brightness);
        }
        console.log(
          `💡 Lamp ${lamp.config.name} state updated: on=${on}, brightness=${brightness}%`,
        );
        return true;
      } else {
        // Fall back to individual writes
        const powerResult = await this.setPower(lampId, on);
        if (brightness !== undefined) {
          const brightnessResult = await this.setBrightness(lampId, brightness);
          return powerResult && brightnessResult;
        }
        return powerResult;
      }
    } catch (error) {
      console.error(`❌ Failed to set lamp state for ${lamp.config.name}:`, error);
      // Mark as disconnected if write fails
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};
      this.scheduleReconnect(lampId);
      return false;
    }
  }

  /**
   * Rename a lamp
   */
  async renameLamp(lampId: string, newName: string): Promise<boolean> {
    const lamp = this.lamps.get(lampId);
    if (!lamp) {
      console.error(`❌ Lamp ${lampId} not found`);
      return false;
    }

    // Update local config
    lamp.config.name = newName;
    lamp.info.name = newName;

    // Update config file
    const configIndex = this.configs.findIndex((c) => c.id === lampId);
    if (configIndex !== -1) {
      this.configs[configIndex].name = newName;
      this.saveConfig();
    }

    // Try to update device name via BLE if connected
    if (lamp.isConnected && lamp.characteristics.deviceName) {
      try {
        const data = Buffer.from(newName, "utf8");
        await lamp.characteristics.deviceName.writeAsync(data, false);
        console.log(`💡 Lamp device name updated to: ${newName}`);
      } catch (error) {
        console.log(`⚠️ Could not update device name via BLE: ${error}`);
      }
    }

    return true;
  }

  /**
   * Get all lamps
   */
  getAllLamps(): HueLampInstance[] {
    return Array.from(this.lamps.values());
  }

  /**
   * Get a specific lamp
   */
  getLamp(lampId: string): HueLampInstance | undefined {
    return this.lamps.get(lampId);
  }

  /**
   * Get connection stats
   */
  getConnectionStats(): {
    total: number;
    connected: number;
    reachable: number;
  } {
    let connected = 0;
    let reachable = 0;

    for (const lamp of this.lamps.values()) {
      if (lamp.isConnected) connected++;
      if (lamp.state.reachable) reachable++;
    }

    return {
      total: this.lamps.size,
      connected,
      reachable,
    };
  }

  /**
   * Verify the actual connection state of a lamp
   * This actively checks if BLE communication is possible
   * @returns true if lamp is actually connected and responsive
   */
  private async verifyConnectionState(lampId: string): Promise<boolean> {
    const lamp = this.lamps.get(lampId);
    if (!lamp) return false;

    // If not marked as connected, nothing to verify
    if (!lamp.isConnected) return false;

    // Check peripheral state first
    if (!lamp.peripheral || lamp.peripheral.state !== "connected") {
      console.log(`🔌 ${lamp.config.name} peripheral not connected, updating state...`);
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};
      return false;
    }

    // Try to read a characteristic to verify the connection is working
    // Use a timeout to detect unresponsive lamps (e.g., power switch off)
    const READ_TIMEOUT_MS = 3000;

    try {
      const readWithTimeout = async (char: typeof lamp.characteristics.power) => {
        if (!char) throw new Error("No characteristic");
        return Promise.race([
          char.readAsync(),
          new Promise((_, reject) =>
            setTimeout(() => reject(new Error("Read timeout")), READ_TIMEOUT_MS),
          ),
        ]);
      };

      if (lamp.characteristics.power) {
        await readWithTimeout(lamp.characteristics.power);
        return true;
      } else if (lamp.characteristics.brightness) {
        await readWithTimeout(lamp.characteristics.brightness);
        return true;
      } else {
        // No characteristics - try to rediscover
        console.log(
          `⚠️ ${lamp.config.name} has no readable characteristics, trying to rediscover...`,
        );
        await Promise.race([
          this.discoverCharacteristics(lampId),
          new Promise((_, reject) =>
            setTimeout(() => reject(new Error("Discovery timeout")), READ_TIMEOUT_MS),
          ),
        ]);
        return (
          lamp.characteristics.power !== undefined || lamp.characteristics.brightness !== undefined
        );
      }
    } catch (error) {
      console.log(`🔌 ${lamp.config.name} failed connection verification: ${error}`);
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};

      // Try to disconnect the peripheral to clean up BLE state
      try {
        if (lamp.peripheral && lamp.peripheral.state === "connected") {
          console.log(`🔌 Force disconnecting stale peripheral for ${lamp.config.name}`);
          await lamp.peripheral.disconnectAsync();
        }
      } catch (disconnectError) {
        // Ignore disconnect errors
      }
      lamp.peripheral = null;

      return false;
    }
  }

  /**
   * Force refresh a lamp: rediscover characteristics and read all state
   */
  private async forceRefreshLamp(lampId: string): Promise<void> {
    const lamp = this.lamps.get(lampId);
    if (!lamp || !lamp.peripheral) return;

    // Verify connection first
    const isConnected = await this.verifyConnectionState(lampId);
    if (!isConnected) {
      console.log(`📴 ${lamp.config.name} is not connected, skipping refresh`);
      return;
    }

    try {
      console.log(`🔄 Force refreshing ${lamp.config.name}...`);

      // Rediscover all characteristics
      await this.discoverCharacteristics(lampId);

      // Re-subscribe to notifications
      await this.subscribeToNotifications(lampId);

      // Read current state
      await this.refreshLampState(lampId, true);

      // Read device info
      await this.readDeviceInfo(lampId);

      console.log(`✅ ${lamp.config.name} force refresh complete`);
    } catch (error) {
      console.error(`❌ Failed to force refresh ${lamp.config.name}:`, error);
      // Mark as disconnected if refresh fails
      lamp.isConnected = false;
      lamp.state.reachable = false;
      lamp.characteristics = {};
      this.scheduleReconnect(lampId);
    }
  }

  /**
   * Trigger a manual scan with full refresh
   * This will:
   * 1. Verify connection state of all lamps (detect disconnects)
   * 2. Start a BLE scan for new lamps
   * 3. Force refresh all connected lamps (rediscover characteristics + read state)
   * 4. Mark lamps not found in scan as disconnected
   */
  async triggerScan(): Promise<void> {
    console.log("🔍 Manual scan triggered - verifying all connections...");

    // Clear discovered peripherals before scan to get fresh data
    this.discoveredPeripherals.clear();

    // First, verify connection state of all lamps to detect disconnects
    const lampsToRefresh: string[] = [];
    for (const [id, lamp] of this.lamps) {
      if (lamp.isConnected) {
        console.log(`🔍 Verifying connection for ${lamp.config.name}...`);
        const stillConnected = await this.verifyConnectionState(id);
        if (stillConnected) {
          console.log(`✅ ${lamp.config.name} is still connected`);
          lampsToRefresh.push(id);
        } else {
          console.log(`❌ ${lamp.config.name} is no longer connected`);
        }
      }
    }

    // Perform BLE scan to discover new lamps
    await this.performScan();

    // Wait a bit for scan to discover peripherals
    await new Promise((resolve) => setTimeout(resolve, 3000));

    console.log(`📡 Discovered ${this.discoveredPeripherals.size} peripherals during scan`);

    // Check which lamps are in range after scan
    // IMPORTANT: Only check discoveredPeripherals, not lamp.peripheral (which may be stale)
    for (const [id, lamp] of this.lamps) {
      const discoveredPeripheral =
        this.discoveredPeripherals.get(lamp.config.address) ||
        this.discoveredPeripherals.get(lamp.config.id);

      const isInRange = discoveredPeripheral !== undefined;

      console.log(
        `📍 ${lamp.config.name}: isConnected=${lamp.isConnected}, isInRange=${isInRange}`,
      );

      if (lamp.isConnected && !isInRange) {
        // Lamp was connected but not found in scan - mark as disconnected
        console.log(`📡 ${lamp.config.name} no longer in range, marking as disconnected`);
        lamp.isConnected = false;
        lamp.state.reachable = false;
        lamp.characteristics = {};
        // Force disconnect the peripheral if it exists
        if (lamp.peripheral) {
          try {
            await lamp.peripheral.disconnectAsync();
          } catch (e) {
            // Ignore disconnect errors
          }
        }
        lamp.peripheral = null;
      } else if (!lamp.isConnected && !isInRange) {
        // Lamp was already disconnected and still not in range
        lamp.state.reachable = false;
        lamp.peripheral = null;
      }
    }

    // Force refresh all still-connected lamps
    console.log(`🔄 Force refreshing ${lampsToRefresh.length} connected lamps...`);
    for (const lampId of lampsToRefresh) {
      // Re-verify the lamp is still connected after range check
      const lamp = this.lamps.get(lampId);
      if (lamp?.isConnected) {
        await this.forceRefreshLamp(lampId);
      }
    }

    // Try to connect disconnected lamps that were discovered during scan
    const disconnectedLamps = Array.from(this.lamps.entries()).filter(
      ([id, lamp]) => !lamp.isConnected && !lampsToRefresh.includes(id),
    );

    if (disconnectedLamps.length > 0) {
      console.log(`🔗 Attempting to connect ${disconnectedLamps.length} disconnected lamps...`);
      for (const [lampId, lamp] of disconnectedLamps) {
        // Check if we have a peripheral for this lamp (discovered during scan)
        const peripheral =
          lamp.peripheral ||
          this.discoveredPeripherals.get(lamp.config.address) ||
          this.discoveredPeripherals.get(lamp.config.id);

        if (peripheral) {
          console.log(`🔗 Trying to connect to ${lamp.config.name}...`);
          await this.connectLamp(lampId);
        } else {
          console.log(`📡 ${lamp.config.name} not in range`);
        }
      }
    }

    console.log("✅ Manual scan complete");
  }

  /**
   * Shutdown the manager
   */
  async shutdown(): Promise<void> {
    console.log("💡 Shutting down Hue Lamp Manager...");

    this.stopPeriodicScan();

    // Disconnect all lamps
    await this.disconnectAllLamps();

    // Stop scanning
    if (this.isScanning) {
      try {
        await noble.stopScanningAsync();
      } catch (error) {
        // Ignore errors during shutdown
      }
    }

    console.log("💡 Hue Lamp Manager shut down");
  }
}
