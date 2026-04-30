import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { useAuth } from "../auth-context";
import * as api from "../api";
import { useNavigate } from "react-router-dom";

export default function AgentsPage() {
  const { accessToken, ready } = useAuth();
  const [agents, setAgents] = useState<api.Agent[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newTokenName, setNewTokenName] = useState("");
  const [tokens, setTokens] = useState<api.AgentToken[] | null>(null);
  const navigate = useNavigate();

  useEffect(() => {
    if (!ready) return;
    if (!accessToken) {
      navigate("/login");
      return;
    }
    loadAgents();
    loadTokens();
  }, [accessToken, ready]);

  async function loadAgents() {
    if (!accessToken) return;
    setLoading(true);
    setError(null);
    try {
      const data = await api.getAgents(accessToken);
      setAgents(data);
    } catch (e: any) {
      setError(e?.message || "Failed to load agents");
    } finally {
      setLoading(false);
    }
  }

  async function loadTokens() {
    if (!accessToken) return;
    try {
      const t = await api.listAgentTokens(accessToken);
      setTokens(t as any);
    } catch (e) {
      // ignore, maybe not admin
      setTokens(null);
    }
  }

  async function onCreateToken(e: FormEvent) {
    e.preventDefault();
    if (!accessToken) return;
    try {
      const res = await api.createAgentToken(accessToken, newTokenName || "ui-token");
      // show raw token to user
      alert(`New token (store it now): ${res.raw_token}`);
      setNewTokenName("");
      await loadTokens();
    } catch (err) {
      alert(err instanceof Error ? err.message : "Failed to create token");
    }
  }

  async function onRevoke(id: string) {
    if (!accessToken) return;
    if (!confirm("Revoke token?")) return;
    try {
      await api.revokeAgentToken(accessToken, id);
      await loadTokens();
    } catch (err) {
      alert(err instanceof Error ? err.message : "Failed to revoke token");
    }
  }

  return (
    <div>
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold mb-4">Agents</h1>
      </div>

      {loading && <div>Loading agents…</div>}
      {error && <div className="text-red-400">{error}</div>}

      {!loading && agents.length === 0 && <p className="text-zinc-500">No agents connected yet.</p>}

      <div className="grid gap-3">
        {agents.map((a) => (
          <div key={a.id} className="bg-zinc-900 border border-zinc-800 rounded p-3 flex justify-between items-center">
            <div>
              <div className="font-semibold">{a.hostname}</div>
              <div className="text-sm text-zinc-400">{a.os} • {a.agent_version}</div>
              <div className="text-xs text-zinc-500">Last seen: {new Date(a.last_seen_at).toLocaleString()}</div>
            </div>
            <div>
              <span className={`px-2 py-1 rounded text-sm ${a.online ? 'bg-green-600' : 'bg-zinc-700'}`}>{a.online ? 'online' : 'offline'}</span>
            </div>
          </div>
        ))}
      </div>

      <div className="mt-6 bg-zinc-900 border border-zinc-800 rounded p-4">
        <h2 className="font-semibold mb-2">Agent tokens</h2>
        <form onSubmit={onCreateToken} className="flex gap-2">
          <input className="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-zinc-100 flex-1" value={newTokenName} onChange={(e) => setNewTokenName(e.target.value)} placeholder="token name" />
          <button className="bg-indigo-600 px-3 rounded">Create</button>
        </form>

        <div className="mt-3">
          {tokens === null && <div className="text-zinc-500">No token access (not admin)</div>}
          {tokens && tokens.length === 0 && <div className="text-zinc-500">No tokens yet</div>}
          {tokens && tokens.map(t => (
            <div key={t.id} className="flex items-center justify-between mt-2">
              <div>
                <div className="font-medium">{t.name}</div>
                <div className="text-xs text-zinc-500">{t.created_at}</div>
              </div>
              <div>
                <button onClick={() => onRevoke(t.id)} className="text-red-400">Revoke</button>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
