import { Elysia } from "elysia";
import { DeviceManager } from "../utils/DeviceManager";
import { parseFeederStatus } from "../utils/Feeder";
import { parseLitterBoxStatus } from "../utils/Litter";
import {
  ScanDpsQuerySchema,
  DevicesListResponseSchema,
  DeviceConnectionResponseSchema,
  DeviceStatusResponseSchema,
  DpsScanResponseSchema,
  DisconnectDeviceSchema,
  ConnectDeviceSchema,
} from "../schemas";

/**
 * Device management routes
 * Handles device listing, connection, disconnection, and status
 */
export function createDeviceRoutes(deviceManager: DeviceManager) {
  return (
    new Elysia({ prefix: "/devices", tags: ["devices"] })

      // 📱 Device Management Endpoints

      .get(
        "/",
        () => {
          const devices = deviceManager.getAllDevices().map((device) => ({
            id: device.config.id,
            name: device.config.name,
            type: device.type,
            product_name: device.config.product_name,
            model: device.config.model,
            ip: device.config.ip,
            version: device.config.version,
            connected: device.isConnected,
            connecting: device.isConnecting,
            reconnect_attempts: device.reconnectAttempts,
            last_data: device.lastData,
            parsed_data: device.parsedData,
          }));

          return {
            success: true,
            devices,
            total: devices.length,
            message: "Devices list retrieved successfully",
          };
        },
        {
          response: DevicesListResponseSchema,
        },
      )

      // 📊 Connection Statistics
      .get("/stats", () => {
        const stats = deviceManager.getConnectionStats();
        return {
          success: true,
          ...stats,
        };
      })

      // 🔄 Force reconnection of disconnected devices
      .post("/reconnect", async ({ set }) => {
        try {
          await deviceManager.reconnectDisconnected();
          return {
            success: true,
            message: "Reconnection initiated for disconnected devices",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })

      .post(
        "/connect",
        async ({ set }) => {
          try {
            await deviceManager.connectAllDevices();
            return {
              success: true,
              message: "All devices connection initiated",
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
          response: DeviceConnectionResponseSchema,
        },
      )

      .post(
        "/disconnect",
        async ({ set }) => {
          try {
            deviceManager.disconnectAllDevices();
            return {
              success: true,
              message: "All devices disconnected",
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
          response: DeviceConnectionResponseSchema,
        },
      )

      .get(
        "/:deviceId/connect",
        async ({ params, set }) => {
          const deviceId = params.deviceId;
          try {
            await deviceManager.connectDevice(deviceId);
            return {
              success: true,
              message: `Device ${deviceId} connection initiated`,
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
          response: DeviceConnectionResponseSchema,
        },
      )

      .get(
        "/:deviceId/disconnect",
        async ({ params, set }) => {
          const deviceId = params.deviceId;
          try {
            await deviceManager.disconnectDevice(deviceId);
            return {
              success: true,
              message: `Device ${deviceId} disconnected`,
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
          response: DeviceConnectionResponseSchema,
        },
      )

      .get(
        "/:deviceId/status",
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

            const status = await deviceManager.getDeviceStatus(deviceId);

            return {
              success: true,
              device: {
                id: device.config.id,
                name: device.config.name,
                type: device.type,
                connected: device.isConnected,
              },
              parsed_status:
                device.type === "litter-box"
                  ? parseLitterBoxStatus(status)
                  : device.type === "feeder"
                    ? parseFeederStatus(status)
                    : null,
              raw_status: status.dps,
              message: "Device status retrieved successfully",
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
          response: DeviceStatusResponseSchema,
        },
      )

      // Debug endpoint to scan DPS range for a specific device
      .get(
        "/:deviceId/scan-dps",
        async ({ params, query, set }) => {
          const deviceId = params.deviceId;
          const device = deviceManager.getDevice(deviceId);
          if (!device) {
            set.status = 404;
            return {
              success: false,
              error: "Device not found",
            };
          }

          const startDps = parseInt(query.start || "1");
          const endDps = parseInt(query.end || "255");
          const timeout = parseInt(query.timeout || "3000");

          try {
            await deviceManager.connectDevice(deviceId);

            console.log(
              `🔍 Scanning DPS range ${startDps}-${endDps} (timeout: ${timeout}ms per DPS)...`,
            );

            const dpsResults: Record<number, { value: unknown; type: string; length?: number }> =
              {};
            const errors: Record<number, string> = {};
            let scannedCount = 0;
            let foundCount = 0;

            for (let dps = startDps; dps <= endDps; dps++) {
              scannedCount++;
              try {
                console.log(`🔍 Scanning DPS ${dps}... (${scannedCount}/${endDps - startDps + 1})`);

                // Add timeout to prevent hanging on non-existent DPS
                const timeoutPromise = new Promise((_, reject) =>
                  setTimeout(() => reject(new Error("Timeout")), timeout),
                );

                const value = await Promise.race([device.api.get({ dps }), timeoutPromise]);

                if (value !== undefined && value !== null) {
                  dpsResults[dps] = {
                    value: value,
                    type: typeof value,
                    length: typeof value === "string" ? value.length : undefined,
                  };
                  foundCount++;
                  console.log(
                    `✅ DPS ${dps}:`,
                    JSON.stringify(value).substring(0, 100) +
                      (JSON.stringify(value).length > 100 ? "..." : ""),
                  );
                }
              } catch (e) {
                errors[dps] = e instanceof Error ? e.message : "Unknown error";
                if (e instanceof Error && !e.message.includes("Timeout")) {
                  console.warn(`❌ DPS ${dps}:`, e.message);
                }
              }
            }

            await deviceManager.disconnectDevice(deviceId);

            return {
              success: true,
              scan_range: `${startDps}-${endDps}`,
              scanned_count: scannedCount,
              found_count: foundCount,
              available_dps: dpsResults,
              errors_count: Object.keys(errors).length,
              errors: Object.keys(errors).length > 0 ? errors : undefined,
              message: `DPS scan completed: ${foundCount} active DPS found out of ${scannedCount} scanned`,
            };
          } catch (error) {
            console.error("❌ Error scanning DPS:", error);

            await deviceManager.disconnectDevice(deviceId);

            set.status = 500;
            return { success: false, error: "Failed to scan DPS" };
          }
        },
        {
          query: ScanDpsQuerySchema,
          response: DpsScanResponseSchema,
        },
      )
  );
}
