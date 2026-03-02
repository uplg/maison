import { Elysia } from "elysia";
import { cors } from "@elysiajs/cors";
import { openapi } from "@elysiajs/openapi";
import { jwt } from "@elysiajs/jwt";
import dotenv from "dotenv";
import { DeviceManager } from "./utils/DeviceManager";
import type { IHueLampManager } from "./utils/HueLampManagerInterface";
import { createDeviceRoutes } from "./routes/devices";
import { createFeederRoutes } from "./routes/feeder";
import { createLitterBoxRoutes } from "./routes/litter-box";
import { createFountainRoutes } from "./routes/fountain";
import { createHueLampRoutes } from "./routes/hue-lamps";
import { createAuthRoutes } from "./routes/auth";
import { createTempoRoutes } from "./routes/tempo";
import { createMerossRoutes } from "./routes/meross";
import { MerossManager } from "./utils/MerossManager";

dotenv.config();

const JWT_SECRET = process.env.JWT_SECRET || "super-secret-cat-key-change-me";
const DISABLE_BLUETOOTH = process.env.DISABLE_BLUETOOTH === "true";

// Dynamic import for HueLampManager (real or stub based on environment)
// Use dynamic path to prevent bundler from resolving the real module in Docker
const createHueLampManager = async (): Promise<IHueLampManager> => {
  if (DISABLE_BLUETOOTH) {
    const { HueLampManager } = await import("./utils/HueLampManagerStub");
    return new HueLampManager();
  } else {
    // Use dynamic string to prevent static analysis by bundler
    const modulePath = [".", "utils", "HueLampManager"].join("/");
    const { HueLampManager } = await import(modulePath);
    return new HueLampManager();
  }
};

// Handle uncaught errors to prevent API crashes from socket issues
process.on("uncaughtException", (error) => {
  console.error("⚠️ Uncaught Exception:", error.message);
  // Don't exit - keep the server running
});

process.on("unhandledRejection", (reason, promise) => {
  console.error("⚠️ Unhandled Rejection at:", promise, "reason:", reason);
  // Don't exit - keep the server running
});

// 🔧 Device Manager Initialization
const deviceManager = new DeviceManager();

// 🔌 Meross Manager Initialization
const merossManager = new MerossManager();

// 💡 Hue Lamp Manager - will be initialized async
let hueLampManager: IHueLampManager;

// Initialize and connect all devices on startup
(async () => {
  await deviceManager.initializeDevices();
  console.log("🚀 Device manager initialized");

  // Connect to all devices at startup
  console.log("🔗 Connecting to all devices on startup...");
  await deviceManager.connectAllDevices();

  // Initialize Hue lamp manager (real or stub)
  hueLampManager = await createHueLampManager();
  await hueLampManager.initialize();
  console.log("💡 Hue Lamp manager initialized");

  // Initialize Meross smart plug manager
  await merossManager.initialize();
  console.log("🔌 Meross manager initialized");

  // Start the server after all initialization
  startServer();
})();

// Handle graceful shutdown
const gracefulShutdown = async (signal: string) => {
  console.log(`\n📴 Received ${signal}. Shutting down gracefully...`);
  deviceManager.disconnectAllDevices();
  await hueLampManager.shutdown();
  merossManager.shutdown();
  process.exit(0);
};

process.on("SIGINT", () => gracefulShutdown("SIGINT"));
process.on("SIGTERM", () => gracefulShutdown("SIGTERM"));

const app = new Elysia()
  // 🌐 CORS Configuration (before other middleware)
  .use(
    cors({
      origin: "*",
      methods: ["GET", "POST", "PUT", "DELETE"],
      allowedHeaders: ["Content-Type", "Authorization"],
    }),
  )
  // 📚 OpenAPI
  .use(
    openapi({
      documentation: {
        info: {
          title: "🐱 Cat API",
          version: "1.0.0",
          description: "Multi-device API for cat feeders and litter boxes",
        },
        tags: [
          { name: "auth", description: "Authentication operations" },
          { name: "devices", description: "Device management operations" },
          { name: "feeder", description: "Smart feeder operations" },
          { name: "litter-box", description: "Smart litter box operations" },
          { name: "fountain", description: "Smart fountain operations" },
          {
            name: "hue-lamps",
            description: "Philips Hue Bluetooth lamp operations",
          },
          {
            name: "tempo",
            description: "RTE Tempo electricity pricing colors",
          },
          {
            name: "meross",
            description: "Meross MSS310 smart plug operations",
          },
        ],
        components: {
          securitySchemes: {
            bearerAuth: {
              type: "http",
              scheme: "bearer",
              bearerFormat: "JWT",
            },
          },
        },
        security: [{ bearerAuth: [] }],
      },
    }),
  )
  // 🏠 Root Endpoint (public)
  .get("/", () => {
    return {
      message: "🐱 Cat API",
      version: "1.0.0",
      description:
        "Multi-device API for cat feeders, litter boxes, and fountains",
      endpoints: [
        "GET /",
        "POST /auth/login",
        "POST /auth/verify",
        "GET /devices",
        "GET /devices/stats",
        "POST /devices/connect",
        "POST /devices/disconnect",
        "POST /devices/reconnect",
        "GET /devices/:deviceId/status",
        "POST /devices/:deviceId/feeder/feed",
        "GET /devices/:deviceId/feeder/status",
        "GET /devices/:deviceId/feeder/meal-plan",
        "POST /devices/:deviceId/feeder/meal-plan",
        "GET /devices/:deviceId/litter-box/status",
        "POST /devices/:deviceId/litter-box/clean",
        "POST /devices/:deviceId/litter-box/settings",
        "GET /devices/:deviceId/fountain/status",
        "POST /devices/:deviceId/fountain/power",
        "POST /devices/:deviceId/fountain/reset/water",
        "POST /devices/:deviceId/fountain/reset/filter",
        "POST /devices/:deviceId/fountain/reset/pump",
        "POST /devices/:deviceId/fountain/uv",
        "POST /devices/:deviceId/fountain/eco-mode",
        "GET /hue-lamps",
        "POST /hue-lamps/scan",
        "GET /hue-lamps/stats",
        "POST /hue-lamps/connect",
        "POST /hue-lamps/disconnect",
        "GET /hue-lamps/:lampId",
        "POST /hue-lamps/:lampId/connect",
        "POST /hue-lamps/:lampId/disconnect",
        "POST /hue-lamps/:lampId/power",
        "POST /hue-lamps/:lampId/brightness",
        "POST /hue-lamps/:lampId/state",
        "POST /hue-lamps/:lampId/rename",
        "GET /tempo",
        "POST /tempo/refresh",
        "GET /meross",
        "GET /meross/stats",
        "GET /meross/:deviceId/status",
        "POST /meross/:deviceId/toggle",
        "POST /meross/:deviceId/on",
        "POST /meross/:deviceId/off",
        "GET /meross/:deviceId/electricity",
        "GET /meross/:deviceId/consumption",
        "GET /meross/:deviceId/abilities",
        "GET /meross/:deviceId/debug",
        "POST /meross/:deviceId/dnd",
        "GET /meross/provision/wifi-scan",
        "POST /meross/provision/key",
        "POST /meross/provision/wifi",
      ],
    };
  })
  // 🔑 Auth Routes (public)
  .use(createAuthRoutes())
  // 🔒 JWT for protected routes
  .use(
    jwt({
      name: "jwt",
      secret: JWT_SECRET,
    }),
  )
  // 🛡️ Guard - ALL routes after this require authentication
  .guard(
    {
      async beforeHandle({ headers, jwt, set }) {
        const authHeader = headers.authorization;

        if (!authHeader || !authHeader.startsWith("Bearer ")) {
          set.status = 401;
          return {
            success: false,
            error:
              "Authentication required. Please provide a valid Bearer token.",
          };
        }

        const token = authHeader.substring(7);

        try {
          const payload = await jwt.verify(token);
          if (!payload) {
            set.status = 401;
            return {
              success: false,
              error: "Invalid or expired token",
            };
          }
        } catch {
          set.status = 401;
          return {
            success: false,
            error: "Invalid or expired token",
          };
        }
      },
    },
    (app) =>
      app
        .use(createDeviceRoutes(deviceManager))
        .use(createFeederRoutes(deviceManager))
        .use(createLitterBoxRoutes(deviceManager))
        .use(createFountainRoutes(deviceManager))
        .use(createHueLampRoutes(() => hueLampManager))
        .use(createTempoRoutes())
        .use(createMerossRoutes(merossManager)),
  );

// 🚀 Server Configuration
const port = Number(process.env.PORT || process.env.API_PORT || 3033);
function startServer() {
  app.listen(port, () => {
    console.log(`🚀 Server started on http://localhost:${port}`);
  });
}

export { app };
