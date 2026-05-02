import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import * as api from "./api";
import type { User } from "./api";
import { useNavigate } from "react-router-dom";
import { AuthContext } from "./auth-context";

export function AuthProvider({ children }: { children: ReactNode }) {
  const [accessToken, setAccessToken] = useState<string | null>(null);
  const [user, setUser] = useState<User | null>(null);
  const [needsSetup, setNeedsSetup] = useState(false);
  const [ready, setReady] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    (async () => {
      try {
        // First check if setup is needed
        const setupStatus = await api.checkSetup();
        if (setupStatus.needs_setup) {
          setNeedsSetup(true);
          setReady(true);
          return;
        }

        // Try to restore session from cookie
        const res = await api.refresh();
        setAccessToken(res.access_token);

        // Fetch current user profile
        try {
          const me = await api.getMe(res.access_token);
          setUser(me);
        } catch {
          // not critical
        }
      } catch {
        setAccessToken(null);
        setUser(null);
      } finally {
        setReady(true);
      }
    })();
  }, []);

  async function login(username: string, password: string) {
    const res = await api.login(username, password);
    setAccessToken(res.access_token);
    try {
      const me = await api.getMe(res.access_token);
      setUser(me);
    } catch { /* ignore */ }
    navigate("/agents");
  }

  async function setup(username: string, password: string) {
    const res = await api.setup(username, password);
    setAccessToken(res.access_token);
    setNeedsSetup(false);
    try {
      const me = await api.getMe(res.access_token);
      setUser(me);
    } catch { /* ignore */ }
    navigate("/agents");
  }

  async function refresh() {
    const res = await api.refresh();
    setAccessToken(res.access_token);
    try {
      const me = await api.getMe(res.access_token);
      setUser(me);
    } catch { /* ignore */ }
  }

  async function logout() {
    try {
      await api.logout();
    } finally {
      setAccessToken(null);
      setUser(null);
      navigate("/login");
    }
  }

  return (
    <AuthContext.Provider value={{ accessToken, user, needsSetup, login, setup, logout, refresh, ready }}>
      {children}
    </AuthContext.Provider>
  );
}
