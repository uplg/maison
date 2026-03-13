import { createContext, useContext, useState, useCallback, useMemo, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { authApi } from "@/lib/api";
import { useQuery, useQueryClient } from "@tanstack/react-query";

interface User {
  id: string;
  username: string;
  role: string;
}

interface AuthContextType {
  user: User | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  login: (username: string, password: string) => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  // Verify token on mount
  const { isLoading } = useQuery({
    queryKey: ["auth", "verify"],
    queryFn: async () => {
      try {
        const response = await authApi.verify();
        if (response.success && response.user) {
          setUser(response.user);
          return response.user;
        }
      } catch {
        setUser(null);
      }
      return null;
    },
    staleTime: Infinity,
    retry: false,
  });

  const login = useCallback(
    async (username: string, password: string) => {
      const response = await authApi.login(username, password);
      if (response.success) {
        setUser(response.user);
        navigate("/");
      }
    },
    [navigate],
  );

  const logout = useCallback(() => {
    void authApi.logout().catch(() => undefined);
    setUser(null);
    queryClient.clear();
    navigate("/login");
  }, [navigate, queryClient]);

  const value = useMemo(
    () => ({
      user,
      isAuthenticated: !!user,
      isLoading,
      login,
      logout,
    }),
    [user, isLoading, login, logout],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error("useAuth must be used within an AuthProvider");
  }
  return context;
}
