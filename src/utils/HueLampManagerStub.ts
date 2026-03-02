/**
 * Stub HueLampManager for Docker (no Bluetooth support)
 * Returns empty data for all operations
 */

import type { IHueLampManager, HueLampInstance, ConnectionStats } from "./HueLampManagerInterface";

const DISABLED_MESSAGE = "Bluetooth is disabled in Docker environment";

export class HueLampManager implements IHueLampManager {
  async initialize(): Promise<void> {
    console.log("💡 Hue Lamp manager disabled (DISABLE_BLUETOOTH=true)");
  }

  async shutdown(): Promise<void> {
    // No-op
  }

  getAllLamps(): HueLampInstance[] {
    return [];
  }

  getLamp(_id: string): HueLampInstance | null {
    return null;
  }

  getConnectionStats(): ConnectionStats {
    return {
      total: 0,
      connected: 0,
      disconnected: 0,
      reachable: 0,
      scanning: false,
      disabled: true,
      message: DISABLED_MESSAGE,
    };
  }

  async triggerScan(): Promise<void> {
    // No-op
  }

  async connectAllLamps(): Promise<void> {
    // No-op
  }

  async disconnectAllLamps(): Promise<void> {
    // No-op
  }

  async connectLamp(_id: string): Promise<boolean> {
    return false;
  }

  async disconnectLamp(_id: string): Promise<void> {
    // No-op
  }

  async refreshLampState(_id: string): Promise<void> {
    // No-op
  }

  async setPower(_id: string, _on: boolean): Promise<boolean> {
    return false;
  }

  async setBrightness(_id: string, _brightness: number): Promise<boolean> {
    return false;
  }

  async setTemperature(_id: string, _temperature: number): Promise<boolean> {
    return false;
  }

  async setLampState(_id: string, _isOn: boolean, _brightness?: number): Promise<boolean> {
    return false;
  }

  async renameLamp(_id: string, _name: string): Promise<boolean> {
    return false;
  }

  getBlacklist(): string[] {
    return [];
  }

  blacklistLamp(_id: string): boolean {
    return false;
  }

  unblacklistAddress(_address: string): boolean {
    return false;
  }
}
