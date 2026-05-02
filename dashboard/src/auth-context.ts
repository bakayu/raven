import { createContext, useContext } from "react";
import type { User } from "./api";

export type AuthContextValue = {
  accessToken: string | null;
  user: User | null;
  needsSetup: boolean;
  login: (username: string, password: string) => Promise<void>;
  setup: (username: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
  ready: boolean;
};

export const AuthContext = createContext<AuthContextValue | null>(null);

export function useAuth() {
  const value = useContext(AuthContext);
  if (!value) throw new Error("useAuth must be used within AuthProvider");
  return value;
}
