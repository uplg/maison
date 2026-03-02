import { Elysia } from "elysia";
import { DeviceManager } from "../utils/DeviceManager";
import { MealPlan, MealPlanEntry } from "../utils/MealPlan";
import { parseFeederStatus } from "../utils/Feeder";
import {
  FeedRequestSchema,
  MealPlanSchema,
  FeederFeedResponseSchema,
  FeederStatusResponseSchema,
  MealPlanResponseSchema,
  MealPlanUpdateResponseSchema,
} from "../schemas";

/**
 * Feeder routes
 * Handles feeding, meal plans, and feeder-specific status
 */
export function createFeederRoutes(deviceManager: DeviceManager) {
  return (
    new Elysia({ prefix: "/devices", tags: ["feeder"] })

      // 🍽️ Feeder Endpoints (Multi-device)

      .post(
        "/:deviceId/feeder/feed",
        async ({ params, body, set }) => {
          const deviceId = params.deviceId;
          const feedBody = {
            portions: body?.portion || 1,
          };

          try {
            const device = deviceManager.getDevice(deviceId);
            if (!device) {
              set.status = 404;
              return {
                success: false,
                error: "Device not found",
              };
            }

            if (device.type !== "feeder") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a feeder",
              };
            }

            if (feedBody.portions > 12) {
              console.warn("portions value seems limited to 12, this may fail");
            }

            await deviceManager.sendCommand(deviceId, 3, feedBody.portions);

            return {
              success: true,
              message: `Manual feed command sent to ${device.config.name} with portions: ${feedBody.portions}`,
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
          body: FeedRequestSchema,
          response: FeederFeedResponseSchema,
        },
      )

      .get(
        "/:deviceId/feeder/status",
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

            if (device.type !== "feeder") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a feeder",
              };
            }

            const status = await deviceManager.getDeviceStatus(deviceId);

            return {
              success: true,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              parsed_status: parseFeederStatus(status),
              raw_dps: status.dps,
              message: "Feeder status retrieved successfully",
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
          response: FeederStatusResponseSchema,
        },
      )

      .get(
        "/:deviceId/feeder/meal-plan",
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

            if (device.type !== "feeder") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a feeder",
              };
            }

            // Get status which includes cached DPS values
            const status = await deviceManager.getDeviceStatus(deviceId);
            const mealPlan = status.dps["1"] as string | undefined;

            return {
              success: true,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              decoded: mealPlan ? MealPlan.decode(mealPlan) : null,
              meal_plan: mealPlan ?? null,
              message: mealPlan ? "Current meal plan retrieved" : "Meal plan not available yet.",
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
          response: MealPlanResponseSchema,
        },
      )

      .post(
        "/:deviceId/feeder/meal-plan",
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

            if (device.type !== "feeder") {
              set.status = 400;
              return {
                success: false,
                error: "Device is not a feeder",
              };
            }

            const requestBody = body as { meal_plan: MealPlanEntry[] };
            if (!requestBody.meal_plan || !Array.isArray(requestBody.meal_plan)) {
              set.status = 400;
              return {
                success: false,
                error: "meal_plan array is required",
              };
            }

            if (requestBody.meal_plan.length > 10) {
              console.warn("This may fail as max supported are 10 meal plans");
            }

            for (let i = 0; i < requestBody.meal_plan.length; i++) {
              const entry = requestBody.meal_plan[i];
              if (!MealPlan.validate(entry)) {
                set.status = 400;
                return {
                  success: false,
                  error: `Invalid meal plan entry at index ${i}`,
                  entry: entry,
                };
              }
            }

            const encodedPlan = MealPlan.encode(requestBody.meal_plan);

            // Send to device - the cache will be updated automatically
            // when the device confirms via the data event
            await deviceManager.sendCommand(deviceId, 1, encodedPlan);

            return {
              success: true,
              message: `Meal plan updated for ${device.config.name}`,
              device: {
                id: device.config.id,
                name: device.config.name,
              },
              encoded_base64: encodedPlan,
              formatted_meal_plan: MealPlan.format(requestBody.meal_plan),
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
          body: MealPlanSchema,
          response: MealPlanUpdateResponseSchema,
        },
      )
  );
}
