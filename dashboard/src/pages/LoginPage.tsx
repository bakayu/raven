import { useState } from "react";
import type { FormEvent } from "react";
import { useAuth } from "../auth-context";

export default function LoginPage() {
  const { login, ready } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      await login(username, password);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Login failed");
    }
  }

  if (!ready) {
    return <div className="min-h-screen bg-zinc-950 flex items-center justify-center">Loading…</div>;
  }

  return (
    <div className="min-h-screen bg-zinc-950 flex items-center justify-center">
      <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-8 w-full max-w-sm">
        <h1 className="text-xl font-bold text-zinc-100 mb-6">Sign in to Raven</h1>
        <form onSubmit={onSubmit} className="flex flex-col gap-3">
          <label className="text-sm text-zinc-300">Username</label>
          <input
            className="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-zinc-100"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
          />

          <label className="text-sm text-zinc-300">Password</label>
          <input
            type="password"
            className="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-zinc-100"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />

          {error && <div className="text-red-400 text-sm">{error}</div>}

          <button className="mt-3 bg-indigo-600 hover:bg-indigo-500 rounded px-4 py-2">Sign in</button>
        </form>
      </div>
    </div>
  );
}
