import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import * as api from "./api";
import { useNavigate } from "react-router-dom";
import { AuthContext } from "./auth-context";

export function AuthProvider({ children }: { children: ReactNode }) {
    const [accessToken, setAccessToken] = useState<string | null>(null);
    const [ready, setReady] = useState(false);
    const navigate = useNavigate();

    useEffect(() => {
        // Try refresh on load to obtain access token from server cookie
        (async () => {
            try {
                const res = await api.refresh();
                setAccessToken(res.access_token);
            } catch {
                setAccessToken(null);
            } finally {
                setReady(true);
            }
        })();
    }, []);

    async function login(username: string, password: string) {
        const res = await api.login(username, password);
        setAccessToken(res.access_token);
        navigate("/agents");
    }

    async function refresh() {
        const res = await api.refresh();
        setAccessToken(res.access_token);
    }

    async function logout() {
        try {
            await api.logout();
        } finally {
            setAccessToken(null);
            navigate("/login");
        }
    }

    return (
        <AuthContext.Provider value={{ accessToken, login, logout, refresh, ready }}>
            {children}
        </AuthContext.Provider>
    );
}
