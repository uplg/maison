import { Elysia } from "elysia";
import { DeviceManager } from "../utils/DeviceManager";
import { parseFountainStatus } from "../utils/Fountain";
import {
  FountainStatusResponseSchema,
  FountainResetResponseSchema,
  FountainUVSettingsSchema,
  FountainUVSettingsResponseSchema,
  FountainEcoModeSchema,
  FountainEcoModeResponseSchema,
  FountainPowerSchema,
  FountainPowerResponseSchema,
} from "../schemas";

/**
 * Fountain routes
 * Handles UV control, resets, eco mode, and fountain-specific status
 */
export function createFountainRoutes(deviceManager: DeviceManager) {
  return (
    new Elysia({ prefix: "/devices", tags: ["fountain"] })

      // 💧 Get Fountain Status
      .get(
        "/:deviceId/fountain/status",
        async ({ params, set }) => {
          const deviceId = params.deviceId;

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "fountain") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a fountain",
              };
            }

            const status = await deviceManager.getDeviceStatus(deviceId);
            const parsedStatus = parseFountainStatus(status);

            return {
              success: true,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              parsed_status: parsedStatus,
              message: "Fountain status retrieved successfully",
              raw_dps: status.dps,
            };
          } catch (error) {
            set.status = 500;
            return {
              success: false,
              error: error instanceof Error ? error.message : "Unknown error",
            };
          }
        },
        {
          response: FountainStatusResponseSchema,
        },
      )

      // 💧 Reset Water Time (DPS 6)
      .post(
        "/:deviceId/fountain/reset/water",
        async ({ params, set }) => {
          const deviceId = params.deviceId;

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "fountain") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a fountain",
              };
            }

            // Try sending 0 to reset (some devices use 0 instead of true)
            await deviceManager.sendCommand(deviceId, 6, 0);

            return {
              success: true,
              message: `Water time counter reset for ${device.config.name}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
            };
          } catch (error) {
            set.status = 500;
            return {
              success: false,
              error: error instanceof Error ? error.message : "Unknown error",
            };
          }
        },
        {
          response: FountainResetResponseSchema,
        },
      )

      // 💧 Reset Filter Life (DPS 7)
      .post(
        "/:deviceId/fountain/reset/filter",
        async ({ params, set }) => {
          const deviceId = params.deviceId;

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "fountain") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a fountain",
              };
            }

            await deviceManager.sendCommand(deviceId, 7, true);

            return {
              success: true,
              message: `Filter life counter reset for ${device.config.name}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
            };
          } catch (error) {
            set.status = 500;
            return {
              success: false,
              error: error instanceof Error ? error.message : "Unknown error",
            };
          }
        },
        {
          response: FountainResetResponseSchema,
        },
      )

      // 💧 Reset Pump Time (DPS 8)
      .post(
        "/:deviceId/fountain/reset/pump",
        async ({ params, set }) => {
          const deviceId = params.deviceId;

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "fountain") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a fountain",
              };
            }

            await deviceManager.sendCommand(deviceId, 8, true);

            return {
              success: true,
              message: `Pump time counter reset for ${device.config.name}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
            };
          } catch (error) {
            set.status = 500;
            return {
              success: false,
              error: error instanceof Error ? error.message : "Unknown error",
            };
          }
        },
        {
          response: FountainResetResponseSchema,
        },
      )

      // 💧 UV Settings (DPS 10: enable/disable, DPS 11: runtime)
      .post(
        "/:deviceId/fountain/uv",
        async ({ params, body, set }) => {
          const deviceId = params.deviceId;

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "fountain") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a fountain",
              };
            }

            const updates: string[] = [];

            // UV Enable/Disable (DPS 10)
            if (body.enabled !== undefined) {
              await deviceManager.sendCommand(deviceId, 10, body.enabled, false);
              updates.push(body.enabled ? "UV light enabled" : "UV light disabled");
            }

            // UV Runtime (DPS 11)
            if (body.runtime !== undefined) {
              if (body.runtime < 0 || body.runtime > 24) {
                set.status = 400;
                return {
                  success: false,
                  error: "UV runtime must be between 0 and 24 hours",
                };
              }
              await deviceManager.sendCommand(deviceId, 11, body.runtime, false);
              updates.push(`UV runtime set to ${body.runtime} hours`);
            }

            // Disconnect after all commands
            await deviceManager.disconnectDevice(deviceId);

            return {
              success: true,
              message: `UV settings updated for ${device.config.name}: ${updates.join(", ")}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              applied_settings: {
                enabled: body.enabled,
                runtime: body.runtime,
              },
            };
          } catch (error) {
            set.status = 500;
            return {
              success: false,
              error: error instanceof Error ? error.message : "Unknown error",
            };
          }
        },
        {
          body: FountainUVSettingsSchema,
          response: FountainUVSettingsResponseSchema,
        },
      )

      // 💧 Eco Mode Settings (DPS 102)
      .post(
        "/:deviceId/fountain/eco-mode",
        async ({ params, body, set }) => {
          const deviceId = params.deviceId;

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "fountain") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a fountain",
              };
            }

            // Eco mode: 1 = mode 1, 2 = mode 2
            // Validate mode value
            if (body.mode < 1 || body.mode > 2) {
              set.status = 400;
              return {
                success: false,
                error: "Eco mode must be 1 or 2",
              };
            }

            await deviceManager.sendCommand(deviceId, 102, body.mode);

            return {
              success: true,
              message: `Eco mode set to ${body.mode} for ${device.config.name}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              eco_mode: body.mode,
            };
          } catch (error) {
            set.status = 500;
            return {
              success: false,
              error: error instanceof Error ? error.message : "Unknown error",
            };
          }
        },
        {
          body: FountainEcoModeSchema,
          response: FountainEcoModeResponseSchema,
        },
      )

      // 💧 Power/Light Control (DPS 1)
      .post(
        "/:deviceId/fountain/power",
        async ({ params, body, set }) => {
          const deviceId = params.deviceId;

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "fountain") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a fountain",
              };
            }

            await deviceManager.sendCommand(deviceId, 1, body.enabled);

            return {
              success: true,
              message: `Light ${
                body.enabled ? "turned on" : "turned off"
              } for ${device.config.name}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              power: body.enabled,
            };
          } catch (error) {
            set.status = 500;
            return {
              success: false,
              error: error instanceof Error ? error.message : "Unknown error",
            };
          }
        },
        {
          body: FountainPowerSchema,
          response: FountainPowerResponseSchema,
        },
      )
  );
}
