import { Elysia } from "elysia";
import { DeviceManager } from "../utils/DeviceManager";
import { parseLitterBoxStatus } from "../utils/Litter";
import { timeToMinutes } from "../utils/formatters";
import {
  LitterBoxSettingsSchema,
  LitterBoxStatusResponseSchema,
  LitterBoxCleanResponseSchema,
  LitterBoxSettingsResponseSchema,
} from "../schemas";

/**
 * Litter box routes
 * Handles cleaning, settings, and litter box-specific status
 */
export function createLitterBoxRoutes(deviceManager: DeviceManager) {
  return (
    new Elysia({ prefix: "/devices", tags: ["litter-box"] })

      // 🚽 Litter Box Endpoints

      .get(
        "/:deviceId/litter-box/status",
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

            if (device.type !== "litter-box") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a litter box",
              };
            }

            const status = await deviceManager.getDeviceStatus(deviceId);
            const parsedStatus = parseLitterBoxStatus(status);

            return {
              success: true,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              parsed_status: parsedStatus,
              message: "Litter box status retrieved successfully",
              raw_dps: status.dps,
            };
          } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "Unknown error";
            const isTimeout = errorMessage.toLowerCase().includes("timeout");

            set.status = isTimeout ? 504 : 500;
            return {
              success: false,
              error: isTimeout
                ? "Device is not responding. It may be offline or out of range."
                : errorMessage,
            };
          }
        },
        {
          response: LitterBoxStatusResponseSchema,
        },
      )

      .post(
        "/:deviceId/litter-box/clean",
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

            if (device.type !== "litter-box") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a litter box",
              };
            }

            await deviceManager.sendCommand(deviceId, 107, true);

            return {
              success: true,
              message: `Manual cleaning cycle initiated for ${device.config.name}`,
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
          response: LitterBoxCleanResponseSchema,
        },
      )

      .post(
        "/:deviceId/litter-box/settings",
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

            if (device.type !== "litter-box") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a litter box",
              };
            }

            const requestBody = body as {
              clean_delay?: number;
              sleep_mode?: {
                enabled?: boolean;
                start_time?: string;
                end_time?: string;
              };
              preferences?: {
                child_lock?: boolean;
                kitten_mode?: boolean;
                lighting?: boolean;
                prompt_sound?: boolean;
                automatic_homing?: boolean;
              };
              actions?: {
                reset_sand_level?: boolean;
                reset_factory_settings?: boolean;
              };
            };
            const updates: Record<string, string | number | boolean> = {};

            // Process settings updates with validation
            if (requestBody.clean_delay !== undefined) {
              if (
                typeof requestBody.clean_delay !== "number" ||
                requestBody.clean_delay < 0 ||
                requestBody.clean_delay > 1800
              ) {
                set.status = 400;
                return {
                  success: false,
                  error: "clean_delay must be between 0 and 1800 seconds",
                };
              }
              updates["101"] = requestBody.clean_delay;
            }

            if (requestBody.sleep_mode?.enabled !== undefined) {
              updates["102"] = requestBody.sleep_mode.enabled;
            }

            if (requestBody.sleep_mode?.start_time !== undefined) {
              const minutes = timeToMinutes(requestBody.sleep_mode.start_time);
              if (minutes === -1) {
                set.status = 400;
                return {
                  success: false,
                  error: "Invalid start_time format. Use HH:MM",
                };
              }
              updates["103"] = minutes;
            }

            if (requestBody.sleep_mode?.end_time !== undefined) {
              const minutes = timeToMinutes(requestBody.sleep_mode.end_time);
              if (minutes === -1) {
                set.status = 400;
                return {
                  success: false,
                  error: "Invalid end_time format. Use HH:MM",
                };
              }
              updates["104"] = minutes;
            }

            // Process preferences
            if (requestBody.preferences?.child_lock !== undefined) {
              updates["110"] = requestBody.preferences.child_lock;
            }
            if (requestBody.preferences?.kitten_mode !== undefined) {
              updates["111"] = requestBody.preferences.kitten_mode;
            }
            if (requestBody.preferences?.lighting !== undefined) {
              updates["116"] = requestBody.preferences.lighting;
            }
            if (requestBody.preferences?.prompt_sound !== undefined) {
              updates["117"] = requestBody.preferences.prompt_sound;
            }
            if (requestBody.preferences?.automatic_homing !== undefined) {
              updates["119"] = requestBody.preferences.automatic_homing;
            }

            // Process one-time actions
            if (requestBody.actions?.reset_sand_level) {
              updates["113"] = true;
            }
            if (requestBody.actions?.reset_factory_settings) {
              updates["115"] = true;
            }

            if (Object.keys(updates).length === 0) {
              set.status = 400;
              return {
                success: false,
                error: "No valid settings provided",
              };
            }

            // Apply updates
            for (const [dps, value] of Object.entries(updates)) {
              await deviceManager.sendCommand(
                deviceId,
                parseInt(dps),
                value as string | number | boolean,
                false,
              );
              console.log(`✅ Updated DPS ${dps} to:`, value);
            }

            await deviceManager.disconnectDevice(deviceId);

            return {
              success: true,
              message: `Settings updated for ${device.config.name}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              updated_settings: Object.keys(updates).length,
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
          body: LitterBoxSettingsSchema,
          response: LitterBoxSettingsResponseSchema,
        },
      )
  );
}
