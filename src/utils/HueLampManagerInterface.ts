/**
 * Common interface for HueLampManager (real and stub implementations)
 */

export interface HueLampConfig {
  id: string;
  name: string;
  address: string;
  model?: string;
  hasConnectedOnce?: boolean;
}

export interface HueLampState {
  isOn: boolean;
  brightness: number;
  temperature?: number | null;
  temperatureMin?: number | null;
  temperatureMax?: number | null;
  reachable: boolean;
}

export interface HueLampInfo {
  model?: string;
  manufacturer?: string;
  firmware?: string;
}

export interface HueLampInstance {
  config: HueLampConfig;
  state: HueLampState;
  info: Partial<HueLampInfo>;
  isConnected: boolean;
  isConnecting: boolean;
  lastSeen: Date | null;
  pairingRequired: boolean;
}

export interface ConnectionStats {
  total: number;
  connected: number;
  disconnected?: number;
  reachable: number;
  scanning?: boolean;
  disabled?: boolean;
  message?: string;
}

export interface IHueLampManager {
  initialize(): Promise<void>;
  shutdown(): Promise<void>;
  getAllLamps(): HueLampInstance[];
  getLamp(id: string): HueLampInstance | null | undefined;
  getConnectionStats(): ConnectionStats;
  triggerScan(): Promise<void>;
  connectAllLamps(): Promise<void>;
  disconnectAllLamps(): Promise<void>;
  connectLamp(id: string): Promise<boolean>;
  disconnectLamp(id: string): Promise<void>;
  refreshLampState(id: string, skipConnectionCheck?: boolean): Promise<HueLampState | null | void>;
  setPower(id: string, on: boolean): Promise<boolean>;
  setBrightness(id: string, brightness: number): Promise<boolean>;
  setTemperature(id: string, temperature: number): Promise<boolean>;
  setLampState(id: string, isOn: boolean, brightness?: number): Promise<boolean>;
  renameLamp(id: string, name: string): Promise<boolean>;
  getBlacklist(): string[];
  blacklistLamp(id: string): boolean;
  unblacklistAddress(address: string): boolean;
}
