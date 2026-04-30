export type Agent = {
    id: string;
    token_id: string;
    hostname: string;
    ip?: string | null;
    os: string;
    agent_version: string;
    log_files: unknown;
    first_seen_at: string;
    last_seen_at: string;
    online: boolean;
};

export type AgentToken = {
    id: string;
    name: string;
    token_id: string;
    created_at: string;
};

async function handleJSON(res: Response) {
    const text = await res.text();
    try {
        return text ? JSON.parse(text) : null;
    } catch {
        return text;
    }
}

export async function login(username: string, password: string) {
    const res = await fetch(`/api/auth/login`, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ username, password }),
    });
    const data = await handleJSON(res);
    if (!res.ok) throw new Error(data?.message || res.statusText);
    return data as { access_token: string };
}

export async function refresh() {
    const res = await fetch(`/api/auth/refresh`, {
        method: "POST",
        credentials: "include",
    });
    const data = await handleJSON(res);
    if (!res.ok) throw new Error(data?.message || res.statusText);
    return data as { access_token: string };
}

export async function logout() {
    const res = await fetch(`/api/auth/logout`, {
        method: "POST",
        credentials: "include",
    });
    if (!res.ok) throw new Error("Logout failed");
}

export async function getAgents(accessToken: string) {
    const res = await fetch(`/api/agents`, {
        headers: { Authorization: `Bearer ${accessToken}` },
    });
    const data = await handleJSON(res);
    if (!res.ok) throw new Error(data?.message || res.statusText);
    return data as Agent[];
}

export async function createAgentToken(accessToken: string, name: string) {
    const res = await fetch(`/api/agents/tokens`, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${accessToken}` },
        body: JSON.stringify({ name }),
    });
    const data = await handleJSON(res);
    if (!res.ok) throw new Error(data?.message || res.statusText);
    return data as { token_id: string; raw_token: string };
}

export async function listAgentTokens(accessToken: string) {
    const res = await fetch(`/api/agents/tokens`, {
        headers: { Authorization: `Bearer ${accessToken}` },
    });
    const data = await handleJSON(res);
    if (!res.ok) throw new Error(data?.message || res.statusText);
    return data as AgentToken[];
}

export async function revokeAgentToken(accessToken: string, id: string) {
    const res = await fetch(`/api/agents/tokens/${encodeURIComponent(id)}`, {
        method: "DELETE",
        headers: { Authorization: `Bearer ${accessToken}` },
    });
    if (!res.ok) throw new Error("Failed to revoke token");
}
