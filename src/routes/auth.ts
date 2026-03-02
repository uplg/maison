import { Elysia, t } from "elysia";
import { jwt } from "@elysiajs/jwt";
import { readFileSync, existsSync } from "fs";
import { join } from "path";

interface User {
  id: string;
  username: string;
  password: string;
  role: string;
}

function loadUsers(): User[] {
  const usersPath = join(process.cwd(), "users.json");

  if (!existsSync(usersPath)) {
    console.warn("⚠️ users.json not found, using default admin user");
    return [{ id: "1", username: "admin", password: "admin", role: "admin" }];
  }

  try {
    const content = readFileSync(usersPath, "utf-8");
    const users = JSON.parse(content) as User[];
    console.log(`✅ Loaded ${users.length} user(s) from users.json`);
    return users;
  } catch (error) {
    console.error("❌ Error loading users.json:", error);
    return [{ id: "1", username: "admin", password: "admin", role: "admin" }];
  }
}

const USERS = loadUsers();
const JWT_SECRET = process.env.JWT_SECRET || "super-secret-cat-key-change-me";

export function createAuthRoutes() {
  return new Elysia({ prefix: "/auth", tags: ["auth"] })
    .use(
      jwt({
        name: "jwt",
        secret: JWT_SECRET,
        exp: "7d",
      }),
    )
    .post(
      "/login",
      async ({ body, jwt, set }) => {
        const { username, password } = body;

        const user = USERS.find((u) => u.username === username && u.password === password);

        if (!user) {
          set.status = 401;
          return {
            success: false,
            error: "Invalid username or password",
          };
        }

        const token = await jwt.sign({
          userId: user.id,
          username: user.username,
          role: user.role,
        });

        return {
          success: true,
          token,
          user: {
            id: user.id,
            username: user.username,
            role: user.role,
          },
        };
      },
      {
        body: t.Object({
          username: t.String({ minLength: 1 }),
          password: t.String({ minLength: 1 }),
        }),
      },
    )
    .post("/verify", async ({ headers, jwt, set }) => {
      const authHeader = headers.authorization;

      if (!authHeader || !authHeader.startsWith("Bearer ")) {
        set.status = 401;
        return {
          success: false,
          error: "No token provided",
        };
      }

      const token = authHeader.substring(7);

      try {
        const payload = await jwt.verify(token);

        if (!payload) {
          set.status = 401;
          return {
            success: false,
            error: "Invalid token",
          };
        }

        return {
          success: true,
          user: {
            id: payload.userId,
            username: payload.username,
            role: payload.role,
          },
        };
      } catch {
        set.status = 401;
        return {
          success: false,
          error: "Invalid token",
        };
      }
    })
    .post("/logout", () => {
      return {
        success: true,
        message: "Logged out successfully",
      };
    });
}
