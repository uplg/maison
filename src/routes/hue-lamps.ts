import { Elysia } from "elysia";
import type { IHueLampManager } from "../utils/HueLampManagerInterface";
import { parseBrightness } from "../utils/HueLamp";
import {
  HueLampsListResponseSchema,
  HueLampStatusResponseSchema,
  HueLampPowerSchema,
  HueLampBrightnessSchema,
  HueLampTemperatureSchema,
  HueLampStateSchema,
  HueLampRenameSchema,
  HueLampResponseSchema,
} from "../schemas";

/**
 * Hue Lamp routes
 * Handles Philips Hue Bluetooth lamp discovery, connection, and control
 */
export function createHueLampRoutes(getHueLampManager: () => IHueLampManager) {
  return (
    new Elysia({ prefix: "/hue-lamps", tags: ["hue-lamps"] })
      // Inject hueLampManager into context for all routes
      .derive(() => ({
        hueLampManager: getHueLampManager(),
      }))

      // 💡 List all Hue lamps
      .get(
        "/",
        ({ hueLampManager }) => {
          const allLamps = hueLampManager.getAllLamps();

          // Filter out lamps that require pairing (not owned/authorized)
          const accessibleLamps = allLamps.filter((lamp) => !lamp.pairingRequired);

          const lamps = accessibleLamps.map((lamp) => ({
            id: lamp.config.id,
            name: lamp.config.name,
            address: lamp.config.address,
            model: lamp.info.model || lamp.config.model || null,
            manufacturer: lamp.info.manufacturer || "Philips",
            firmware: lamp.info.firmware || null,
            connected: lamp.isConnected,
            connecting: lamp.isConnecting,
            reachable: lamp.state.reachable,
            state: {
              isOn: lamp.state.isOn,
              brightness: parseBrightness(lamp.state.brightness),
              temperature: lamp.state.temperature ?? null,
              temperatureMin: lamp.state.temperatureMin ?? null,
              temperatureMax: lamp.state.temperatureMax ?? null,
            },
            lastSeen: lamp.lastSeen?.toISOString() || null,
          }));

          const stats = hueLampManager.getConnectionStats();

          return {
            success: true,
            lamps,
            total: lamps.length,
            connected: stats.connected,
            reachable: stats.reachable,
            message: "Hue lamps list retrieved successfully",
          };
        },
        {
          response: HueLampsListResponseSchema,
        },
      )

      // 🔍 Trigger a BLE scan for lamps
      .post("/scan", async ({ set, hueLampManager }) => {
        try {
          await hueLampManager.triggerScan();
          return {
            success: true,
            message: "BLE scan triggered",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })

      // 📊 Get connection statistics
      .get("/stats", ({ hueLampManager }) => {
        const stats = hueLampManager.getConnectionStats();
        return {
          success: true,
          ...stats,
        };
      })

      // 🔗 Connect all lamps
      .post("/connect", async ({ set, hueLampManager }) => {
        try {
          await hueLampManager.connectAllLamps();
          return {
            success: true,
            message: "Connection initiated for all Hue lamps",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })

      // 📴 Disconnect all lamps
      .post("/disconnect", async ({ set, hueLampManager }) => {
        try {
          await hueLampManager.disconnectAllLamps();
          return {
            success: true,
            message: "All Hue lamps disconnected",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })

      // 💡 Get specific lamp status
      .get(
        "/:lampId",
        async ({ params, set, hueLampManager }) => {
          const lamp = hueLampManager.getLamp(params.lampId);

          if (!lamp) {
            set.status = 404;
            return {
              success: false,
              error: "Lamp not found",
            };
          }

          // Refresh state if connected
          if (lamp.isConnected) {
            await hueLampManager.refreshLampState(params.lampId);
          }

          return {
            success: true,
            lamp: {
              id: lamp.config.id,
              name: lamp.config.name,
              address: lamp.config.address,
              model: lamp.info.model || lamp.config.model || null,
              manufacturer: lamp.info.manufacturer || "Philips",
              firmware: lamp.info.firmware || null,
              connected: lamp.isConnected,
              connecting: lamp.isConnecting,
              reachable: lamp.state.reachable,
              state: {
                isOn: lamp.state.isOn,
                brightness: parseBrightness(lamp.state.brightness),
                temperature: lamp.state.temperature ?? null,
                temperatureMin: lamp.state.temperatureMin ?? null,
                temperatureMax: lamp.state.temperatureMax ?? null,
              },
              lastSeen: lamp.lastSeen?.toISOString() || null,
            },
            message: "Lamp status retrieved successfully",
          };
        },
        {
          response: HueLampStatusResponseSchema,
        },
      )

      // 🔗 Connect specific lamp
      .post("/:lampId/connect", async ({ params, set, hueLampManager }) => {
        const lamp = hueLampManager.getLamp(params.lampId);

        if (!lamp) {
          set.status = 404;
          return {
            success: false,
            error: "Lamp not found",
          };
        }

        try {
          const connected = await hueLampManager.connectLamp(params.lampId);
          return {
            success: true,
            connected,
            message: connected
              ? "Lamp connected successfully"
              : "Connection initiated (lamp may not be in range)",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })

      // 📴 Disconnect specific lamp
      .post("/:lampId/disconnect", async ({ params, set, hueLampManager }) => {
        const lamp = hueLampManager.getLamp(params.lampId);

        if (!lamp) {
          set.status = 404;
          return {
            success: false,
            error: "Lamp not found",
          };
        }

        try {
          await hueLampManager.disconnectLamp(params.lampId);
          return {
            success: true,
            message: "Lamp disconnected",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })

      // ⚡ Set lamp power (on/off)
      .post(
        "/:lampId/power",
        async ({ params, body, set, hueLampManager }) => {
          const lamp = hueLampManager.getLamp(params.lampId);

          if (!lamp) {
            set.status = 404;
            return {
              success: false,
              error: "Lamp not found",
            };
          }

          if (!lamp.isConnected) {
            set.status = 400;
            return {
              success: false,
              error: "Lamp not connected",
            };
          }

          try {
            const result = await hueLampManager.setPower(params.lampId, body.enabled);

            if (!result) {
              set.status = 500;
              return {
                success: false,
                error: "Failed to set power",
              };
            }

            return {
              success: true,
              state: {
                isOn: body.enabled,
                brightness: parseBrightness(lamp.state.brightness),
              },
              message: `Lamp turned ${body.enabled ? "on" : "off"}`,
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
          body: HueLampPowerSchema,
          response: HueLampResponseSchema,
        },
      )

      // 🔆 Set lamp brightness
      .post(
        "/:lampId/brightness",
        async ({ params, body, set, hueLampManager }) => {
          const lamp = hueLampManager.getLamp(params.lampId);

          if (!lamp) {
            set.status = 404;
            return {
              success: false,
              error: "Lamp not found",
            };
          }

          if (!lamp.isConnected) {
            set.status = 400;
            return {
              success: false,
              error: "Lamp not connected",
            };
          }

          try {
            const result = await hueLampManager.setBrightness(params.lampId, body.brightness);

            if (!result) {
              set.status = 500;
              return {
                success: false,
                error: "Failed to set brightness",
              };
            }

            return {
              success: true,
              state: {
                isOn: lamp.state.isOn,
                brightness: body.brightness,
              },
              message: `Brightness set to ${body.brightness}%`,
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
          body: HueLampBrightnessSchema,
          response: HueLampResponseSchema,
        },
      )

      // �️ Set lamp color temperature
      .post(
        "/:lampId/temperature",
        async ({ params, body, set, hueLampManager }) => {
          const lamp = hueLampManager.getLamp(params.lampId);

          if (!lamp) {
            set.status = 404;
            return {
              success: false,
              error: "Lamp not found",
            };
          }

          if (!lamp.isConnected) {
            set.status = 400;
            return {
              success: false,
              error: "Lamp not connected",
            };
          }

          try {
            const result = await hueLampManager.setTemperature(params.lampId, body.temperature);

            if (!result) {
              set.status = 500;
              return {
                success: false,
                error: "Failed to set temperature (lamp may not support color temperature)",
              };
            }

            return {
              success: true,
              state: {
                isOn: lamp.state.isOn,
                brightness: parseBrightness(lamp.state.brightness),
                temperature: body.temperature,
              },
              message: `Color temperature set to ${body.temperature}%`,
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
          body: HueLampTemperatureSchema,
          response: HueLampResponseSchema,
        },
      )

      // �🎚️ Set lamp state (power + brightness together)
      .post(
        "/:lampId/state",
        async ({ params, body, set, hueLampManager }) => {
          const lamp = hueLampManager.getLamp(params.lampId);

          if (!lamp) {
            set.status = 404;
            return {
              success: false,
              error: "Lamp not found",
            };
          }

          if (!lamp.isConnected) {
            set.status = 400;
            return {
              success: false,
              error: "Lamp not connected",
            };
          }

          try {
            const result = await hueLampManager.setLampState(
              params.lampId,
              body.isOn,
              body.brightness,
            );

            if (!result) {
              set.status = 500;
              return {
                success: false,
                error: "Failed to set lamp state",
              };
            }

            return {
              success: true,
              state: {
                isOn: body.isOn,
                brightness: body.brightness ?? parseBrightness(lamp.state.brightness),
              },
              message: "Lamp state updated",
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
          body: HueLampStateSchema,
          response: HueLampResponseSchema,
        },
      )

      // ✏️ Rename a lamp
      .post(
        "/:lampId/rename",
        async ({ params, body, set, hueLampManager }) => {
          const lamp = hueLampManager.getLamp(params.lampId);

          if (!lamp) {
            set.status = 404;
            return {
              success: false,
              error: "Lamp not found",
            };
          }

          try {
            const result = await hueLampManager.renameLamp(params.lampId, body.name);

            if (!result) {
              set.status = 500;
              return {
                success: false,
                error: "Failed to rename lamp",
              };
            }

            return {
              success: true,
              message: `Lamp renamed to "${body.name}"`,
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
          body: HueLampRenameSchema,
        },
      )

      // 🚫 Blacklist a lamp (remove and prevent re-discovery)
      .post("/:lampId/blacklist", async ({ params, set, hueLampManager }) => {
        try {
          const result = hueLampManager.blacklistLamp(params.lampId);

          if (!result) {
            set.status = 404;
            return {
              success: false,
              error: "Lamp not found",
            };
          }

          return {
            success: true,
            message: "Lamp blacklisted successfully",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })

      // 📋 Get blacklist
      .get("/blacklist/list", ({ hueLampManager }) => {
        const blacklist = hueLampManager.getBlacklist();
        return {
          success: true,
          blacklist,
          total: blacklist.length,
        };
      })

      // ✅ Remove address from blacklist
      .delete("/blacklist/:address", async ({ params, set, hueLampManager }) => {
        try {
          const result = hueLampManager.unblacklistAddress(params.address);

          if (!result) {
            set.status = 404;
            return {
              success: false,
              error: "Address not found in blacklist",
            };
          }

          return {
            success: true,
            message: "Address removed from blacklist",
          };
        } catch (error) {
          set.status = 500;
          return {
            success: false,
            error: error instanceof Error ? error.message : "Unknown error",
          };
        }
      })
  );
}
