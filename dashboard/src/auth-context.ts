import { createContext, useContext } from "react";

export type AuthContextValue = {
    accessToken: string | null;
    login: (username: string, password: string) => Promise<void>;
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
