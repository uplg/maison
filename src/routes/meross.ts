import { Elysia } from "elysia";
import { MerossManager } from "../utils/MerossManager";
import { MerossDevice } from "../utils/MerossDevice";
import {
  MerossToggleSchema,
  MerossDNDModeSchema,
  MerossProvisionKeySchema,
  MerossProvisionWifiSchema,
} from "../schemas";

/**
 * Meross smart plug routes
 * Local HTTP control for MSS310 (and compatible) devices
 */
export function createMerossRoutes(merossManager: MerossManager) {
  return new Elysia({ prefix: "/meross", tags: ["meross"] })

    // ─── List all Meross devices ───
    .get("/", () => {
      const devices = merossManager.getAllDevices().map((d) => ({
        id: d.config.ip,
        name: d.config.name,
        ip: d.config.ip,
        isOnline: d.isOnline,
        isOn: d.device.isOn,
        lastPing: d.lastPing,
      }));

      return {
        success: true,
        devices,
        total: devices.length,
        message: "Meross devices list retrieved",
      };
    })

    // ─── Connection stats ───
    .get("/stats", () => {
      const stats = merossManager.getStats();
      return { success: true, ...stats };
    })

    // ─── Full status of a specific plug ───
    .get("/:deviceId/status", async ({ params, set }) => {
      const instance = merossManager.getDevice(params.deviceId);
      if (!instance) {
        set.status = 404;
        return { success: false, error: "Device not found" };
      }

      try {
        const status = await instance.device.getFullStatus();
        return {
          success: true,
          device: { id: instance.config.ip, name: instance.config.name },
          status,
          message: "Status retrieved",
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Toggle plug on/off ───
    .post(
      "/:deviceId/toggle",
      async ({ params, body, set }) => {
        const instance = merossManager.getDevice(params.deviceId);
        if (!instance) {
          set.status = 404;
          return { success: false, error: "Device not found" };
        }

        try {
          await instance.device.toggle(body.on);
          return {
            success: true,
            device: { id: instance.config.ip, name: instance.config.name },
            on: body.on,
            message: `${instance.config.name} turned ${body.on ? "on" : "off"}`,
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      },
      { body: MerossToggleSchema },
    )

    // ─── Turn on ───
    .post("/:deviceId/on", async ({ params, set }) => {
      const instance = merossManager.getDevice(params.deviceId);
      if (!instance) {
        set.status = 404;
        return { success: false, error: "Device not found" };
      }

      try {
        await instance.device.turnOn();
        return {
          success: true,
          device: { id: instance.config.ip, name: instance.config.name },
          on: true,
          message: `${instance.config.name} turned on`,
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Turn off ───
    .post("/:deviceId/off", async ({ params, set }) => {
      const instance = merossManager.getDevice(params.deviceId);
      if (!instance) {
        set.status = 404;
        return { success: false, error: "Device not found" };
      }

      try {
        await instance.device.turnOff();
        return {
          success: true,
          device: { id: instance.config.ip, name: instance.config.name },
          on: false,
          message: `${instance.config.name} turned off`,
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Get real-time electricity usage ───
    .get("/:deviceId/electricity", async ({ params, set }) => {
      const instance = merossManager.getDevice(params.deviceId);
      if (!instance) {
        set.status = 404;
        return { success: false, error: "Device not found" };
      }

      try {
        const elec = await instance.device.getElectricityFormatted();
        return {
          success: true,
          device: { id: instance.config.ip, name: instance.config.name },
          electricity: {
            voltage: `${elec.voltage}V`,
            current: `${elec.current}A`,
            power: `${elec.power}W`,
            raw: elec.raw,
          },
          message: "Electricity data retrieved",
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Get consumption history (30 days) ───
    .get("/:deviceId/consumption", async ({ params, set }) => {
      const instance = merossManager.getDevice(params.deviceId);
      if (!instance) {
        set.status = 404;
        return { success: false, error: "Device not found" };
      }

      try {
        const consumption = await instance.device.getConsumption();
        const totalWh = consumption.reduce((sum, entry) => sum + entry.value, 0);
        return {
          success: true,
          device: { id: instance.config.ip, name: instance.config.name },
          consumption,
          summary: {
            days: consumption.length,
            totalWh,
            totalKwh: Math.round((totalWh / 1000) * 100) / 100,
          },
          message: "Consumption history retrieved",
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Get device abilities ───
    .get("/:deviceId/abilities", async ({ params, set }) => {
      const instance = merossManager.getDevice(params.deviceId);
      if (!instance) {
        set.status = 404;
        return { success: false, error: "Device not found" };
      }

      try {
        const abilities = await instance.device.getAbilities();
        return {
          success: true,
          device: { id: instance.config.ip, name: instance.config.name },
          abilities,
          message: "Abilities retrieved",
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Get debug info ───
    .get("/:deviceId/debug", async ({ params, set }) => {
      const instance = merossManager.getDevice(params.deviceId);
      if (!instance) {
        set.status = 404;
        return { success: false, error: "Device not found" };
      }

      try {
        const debug = await instance.device.getDebug();
        return {
          success: true,
          device: { id: instance.config.ip, name: instance.config.name },
          debug,
          message: "Debug info retrieved",
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Set DND mode (LED on/off) ───
    .post(
      "/:deviceId/dnd",
      async ({ params, body, set }) => {
        const instance = merossManager.getDevice(params.deviceId);
        if (!instance) {
          set.status = 404;
          return { success: false, error: "Device not found" };
        }

        try {
          await instance.device.setDNDMode(body.enabled);
          return {
            success: true,
            device: { id: instance.config.ip, name: instance.config.name },
            dndMode: body.enabled,
            message: `DND mode ${body.enabled ? "enabled" : "disabled"} (LED ${body.enabled ? "off" : "on"})`,
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      },
      { body: MerossDNDModeSchema },
    )

    // ─── Provisioning: Scan WiFi (from device AP) ───
    .get("/provision/wifi-scan", async ({ query, set }) => {
      try {
        const deviceIp = (query as { ip?: string }).ip || "10.10.10.1";
        const networks = await MerossDevice.scanWifi(deviceIp);
        return {
          success: true,
          networks,
          total: networks.length,
          message: "WiFi networks scanned from device",
        };
      } catch (error) {
        set.status = 500;
        return {
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        };
      }
    })

    // ─── Provisioning: Configure key ───
    .post(
      "/provision/key",
      async ({ body, set }) => {
        try {
          await MerossDevice.configureKey(
            body.key,
            body.userId,
            body.mqttHost,
            body.mqttPort,
            body.deviceIp,
          );
          return {
            success: true,
            message: "Key configured on device",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      },
      { body: MerossProvisionKeySchema },
    )

    // ─── Provisioning: Configure WiFi ───
    .post(
      "/provision/wifi",
      async ({ body, set }) => {
        try {
          await MerossDevice.configureWifi(
            body.ssid,
            body.password,
            body.bssid,
            body.channel,
            body.encryption,
            body.cipher,
            body.deviceIp,
          );
          return {
            success: true,
            message:
              "WiFi configured. Device will reboot and connect to the specified network.",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      },
      { body: MerossProvisionWifiSchema },
    );
}
