import { useEffect, useState, useCallback } from "react";
import type { FormEvent } from "react";
import { useAuth } from "../auth-context";
import * as api from "../api";
import {
  Key,
  Users,
  User,
  Plus,
  Trash2,
  Copy,
  Check,
  AlertCircle,
  Eye,
  EyeOff,
  Shield,
  Lock,
  Terminal,
} from "lucide-react";
import { formatDistanceToNow } from "date-fns";

// ──────────────────────────────────────────────────────────────────────────────
// Token management
// ──────────────────────────────────────────────────────────────────────────────

function TokensSection({ accessToken }: { accessToken: string }) {
  const [tokens, setTokens] = useState<api.AgentToken[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);
  const [newToken, setNewToken] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [revoking, setRevoking] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await api.listAgentTokens(accessToken);
      setTokens(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load tokens");
    } finally {
      setLoading(false);
    }
  }, [accessToken]);

  useEffect(() => { load(); }, [load]);

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    setCreating(true);
    setError(null);
    try {
      const res = await api.createAgentToken(accessToken, name.trim());
      setNewToken(res.raw_token);
      setName("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create token");
    } finally {
      setCreating(false);
    }
  }

  async function onRevoke(id: string) {
    setRevoking(id);
    try {
      await api.revokeAgentToken(accessToken, id);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to revoke token");
    } finally {
      setRevoking(null);
    }
  }

  function copyToken() {
    if (newToken) {
      navigator.clipboard.writeText(newToken);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }

  return (
    <div>
      <div className="section-header">
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Key size={15} color="var(--text-secondary)" strokeWidth={1.75} />
          <span className="section-title">Agent Tokens</span>
        </div>
      </div>

      {error && (
        <div className="alert alert-error" style={{ marginBottom: 12 }}>
          <AlertCircle size={13} />
          {error}
        </div>
      )}

      {/* New token reveal */}
      {newToken && (
        <div
          className="alert alert-success"
          style={{ flexDirection: "column", alignItems: "stretch", marginBottom: 16 }}
        >
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
            <span style={{ fontWeight: 600, fontSize: 13 }}>
              Token created — copy it now
            </span>
            <button
              onClick={() => setNewToken(null)}
              style={{ background: "none", border: "none", cursor: "pointer", color: "inherit", fontSize: 16, lineHeight: 1 }}
            >
              ×
            </button>
          </div>
          <div
            style={{
              background: "#000",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius-sm)",
              padding: "10px 12px",
              display: "flex",
              alignItems: "center",
              gap: 10,
            }}
          >
            <code
              style={{
                flex: 1,
                fontFamily: "var(--font-mono)",
                fontSize: 12,
                color: "var(--success)",
                wordBreak: "break-all",
              }}
            >
              {newToken}
            </code>
            <button
              onClick={copyToken}
              className="btn btn-sm"
              style={{ flexShrink: 0, color: "var(--success)", borderColor: "var(--success)" }}
            >
              {copied ? <Check size={12} /> : <Copy size={12} />}
              {copied ? "Copied" : "Copy"}
            </button>
          </div>
          <p style={{ fontSize: 12, marginTop: 8, color: "var(--success)", opacity: 0.8 }}>
            This token won't be shown again. Store it securely before closing.
          </p>
        </div>
      )}

      {/* Create form */}
      <form
        onSubmit={onCreate}
        style={{ display: "flex", gap: 8, marginBottom: 16 }}
      >
        <input
          className="input"
          placeholder="Token name (e.g. web-server-1)"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
          style={{ flex: 1, fontSize: 13 }}
        />
        <button
          type="submit"
          className="btn btn-primary btn-sm"
          disabled={creating || !name.trim()}
        >
          <Plus size={12} />
          {creating ? "Creating…" : "Create token"}
        </button>
      </form>

      {/* Token list */}
      <div className="card" style={{ overflow: "hidden" }}>
        {loading ? (
          <div style={{ padding: 16 }}>
            {[1, 2].map((i) => (
              <div key={i} className="skeleton" style={{ height: 36, borderRadius: 6, marginBottom: 8 }} />
            ))}
          </div>
        ) : tokens.length === 0 ? (
          <div className="empty-state" style={{ padding: "32px 24px" }}>
            <Terminal size={32} className="empty-icon" />
            <div className="empty-title">No tokens yet</div>
            <p className="empty-desc" style={{ fontSize: 12 }}>
              Create a token to authenticate an agent.
            </p>
          </div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Created</th>
                <th>Last used</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {tokens.map((t) => (
                <tr key={t.id}>
                  <td style={{ color: "var(--text-primary)", fontWeight: 500, fontFamily: "var(--font-mono)", fontSize: 12 }}>
                    {t.name}
                  </td>
                  <td style={{ fontSize: 12 }}>
                    {formatDistanceToNow(new Date(t.created_at), { addSuffix: true })}
                  </td>
                  <td style={{ fontSize: 12 }}>
                    {t.last_used_at
                      ? formatDistanceToNow(new Date(t.last_used_at), { addSuffix: true })
                      : "Never"}
                  </td>
                  <td style={{ textAlign: "right" }}>
                    <button
                      className="btn btn-danger btn-sm"
                      onClick={() => onRevoke(t.id)}
                      disabled={revoking === t.id}
                    >
                      <Trash2 size={11} />
                      {revoking === t.id ? "Revoking…" : "Revoke"}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────────────────
// User management (admin only)
// ──────────────────────────────────────────────────────────────────────────────

function UsersSection({ accessToken, currentUserId }: { accessToken: string; currentUserId?: string }) {
  const [users, setUsers] = useState<api.User[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);

  // Create form
  const [newUsername, setNewUsername] = useState("");
  const [newEmail, setNewEmail] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [newRole, setNewRole] = useState<"admin" | "member">("member");
  const [showPass, setShowPass] = useState(false);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await api.listUsers(accessToken);
      setUsers(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load users");
    } finally {
      setLoading(false);
    }
  }, [accessToken]);

  useEffect(() => { load(); }, [load]);

  async function onCreate(e: FormEvent) {
    e.preventDefault();
    setCreating(true);
    setError(null);
    try {
      await api.createUser(accessToken, {
        username: newUsername,
        email: newEmail || undefined,
        password: newPassword,
        role: newRole,
      });
      setNewUsername(""); setNewEmail(""); setNewPassword(""); setNewRole("member");
      setShowCreate(false);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create user");
    } finally {
      setCreating(false);
    }
  }

  async function onDelete(id: string) {
    if (id === currentUserId) return;
    setDeleting(id);
    try {
      await api.deleteUser(accessToken, id);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete user");
    } finally {
      setDeleting(null);
    }
  }

  return (
    <div>
      <div className="section-header">
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Users size={15} color="var(--text-secondary)" strokeWidth={1.75} />
          <span className="section-title">User Management</span>
        </div>
        <button
          className="btn btn-secondary btn-sm"
          onClick={() => setShowCreate((v) => !v)}
        >
          <Plus size={12} />
          Invite user
        </button>
      </div>

      {error && (
        <div className="alert alert-error" style={{ marginBottom: 12 }}>
          <AlertCircle size={13} />
          {error}
        </div>
      )}

      {/* Create form */}
      {showCreate && (
        <div
          className="card"
          style={{ padding: 20, marginBottom: 16, animation: "fadeIn 0.15s ease" }}
        >
          <p className="section-title" style={{ marginBottom: 14 }}>
            New user
          </p>
          <form
            onSubmit={onCreate}
            style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}
          >
            <div className="form-field">
              <label className="form-label">Username *</label>
              <input
                className="input"
                placeholder="username"
                value={newUsername}
                onChange={(e) => setNewUsername(e.target.value)}
                required
                style={{ fontSize: 13 }}
              />
            </div>
            <div className="form-field">
              <label className="form-label">Email</label>
              <input
                className="input"
                type="email"
                placeholder="user@example.com"
                value={newEmail}
                onChange={(e) => setNewEmail(e.target.value)}
                style={{ fontSize: 13 }}
              />
            </div>
            <div className="form-field" style={{ position: "relative" }}>
              <label className="form-label">Password *</label>
              <div style={{ position: "relative" }}>
                <input
                  className="input"
                  type={showPass ? "text" : "password"}
                  placeholder="Min. 8 characters"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                  required
                  minLength={8}
                  style={{ fontSize: 13, paddingRight: 36 }}
                />
                <button
                  type="button"
                  onClick={() => setShowPass((v) => !v)}
                  style={{
                    position: "absolute",
                    right: 8,
                    top: "50%",
                    transform: "translateY(-50%)",
                    background: "none",
                    border: "none",
                    cursor: "pointer",
                    color: "var(--text-muted)",
                    display: "flex",
                  }}
                >
                  {showPass ? <EyeOff size={13} /> : <Eye size={13} />}
                </button>
              </div>
            </div>
            <div className="form-field">
              <label className="form-label">Role</label>
              <select
                className="select"
                value={newRole}
                onChange={(e) => setNewRole(e.target.value as "admin" | "member")}
                style={{ width: "100%", padding: "8px 28px 8px 10px" }}
              >
                <option value="member">Member</option>
                <option value="admin">Admin</option>
              </select>
            </div>
            <div style={{ gridColumn: "1 / -1", display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => setShowCreate(false)}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="btn btn-primary btn-sm"
                disabled={creating}
              >
                {creating ? "Creating…" : "Create user"}
              </button>
            </div>
          </form>
        </div>
      )}

      {/* User list */}
      <div className="card" style={{ overflow: "hidden" }}>
        {loading ? (
          <div style={{ padding: 16 }}>
            {[1, 2, 3].map((i) => (
              <div key={i} className="skeleton" style={{ height: 44, borderRadius: 6, marginBottom: 8 }} />
            ))}
          </div>
        ) : users.length === 0 ? (
          <div className="empty-state" style={{ padding: "32px 24px" }}>
            <Users size={32} className="empty-icon" />
            <div className="empty-title">No users</div>
          </div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>User</th>
                <th>Role</th>
                <th>Last login</th>
                <th>Created</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.id}>
                  <td>
                    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      <div
                        style={{
                          width: 28,
                          height: 28,
                          borderRadius: "50%",
                          background: "var(--bg-muted)",
                          border: "1px solid var(--border)",
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "center",
                          fontSize: 11,
                          fontWeight: 600,
                          color: "var(--text-secondary)",
                          flexShrink: 0,
                        }}
                      >
                        {u.username.charAt(0).toUpperCase()}
                      </div>
                      <div>
                        <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text-primary)" }}>
                          {u.username}
                          {u.id === currentUserId && (
                            <span style={{ marginLeft: 6, fontSize: 11, color: "var(--text-muted)" }}>
                              (you)
                            </span>
                          )}
                        </div>
                        {u.email && (
                          <div style={{ fontSize: 11, color: "var(--text-muted)" }}>{u.email}</div>
                        )}
                      </div>
                    </div>
                  </td>
                  <td>
                    <span className={u.role === "admin" ? "badge badge-info" : "badge badge-neutral"}>
                      {u.role === "admin" ? <Shield size={10} /> : <User size={10} />}
                      {u.role}
                    </span>
                  </td>
                  <td style={{ fontSize: 12 }}>
                    {u.last_login_at
                      ? formatDistanceToNow(new Date(u.last_login_at), { addSuffix: true })
                      : "Never"}
                  </td>
                  <td style={{ fontSize: 12 }}>
                    {formatDistanceToNow(new Date(u.created_at), { addSuffix: true })}
                  </td>
                  <td style={{ textAlign: "right" }}>
                    {u.id !== currentUserId && (
                      <button
                        className="btn btn-danger btn-sm"
                        onClick={() => onDelete(u.id)}
                        disabled={deleting === u.id}
                      >
                        <Trash2 size={11} />
                        {deleting === u.id ? "Deleting…" : "Delete"}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────────────────
// Profile section
// ──────────────────────────────────────────────────────────────────────────────

function ProfileSection({
  accessToken,
  user,
}: {
  accessToken: string;
  user: api.User | null;
}) {
  const [currentPw, setCurrentPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");
  const [pwLoading, setPwLoading] = useState(false);
  const [pwError, setPwError] = useState<string | null>(null);
  const [pwSuccess, setPwSuccess] = useState(false);
  const [showPw, setShowPw] = useState(false);

  async function onChangePassword(e: FormEvent) {
    e.preventDefault();
    if (newPw !== confirmPw) { setPwError("Passwords do not match"); return; }
    if (newPw.length < 8) { setPwError("Password must be at least 8 characters"); return; }
    setPwLoading(true);
    setPwError(null);
    try {
      await api.changePassword(accessToken, currentPw, newPw);
      setCurrentPw(""); setNewPw(""); setConfirmPw("");
      setPwSuccess(true);
      setTimeout(() => setPwSuccess(false), 3000);
    } catch (err) {
      setPwError(err instanceof Error ? err.message : "Failed to change password");
    } finally {
      setPwLoading(false);
    }
  }

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 20 }}>
        <User size={15} color="var(--text-secondary)" strokeWidth={1.75} />
        <span className="section-title">Profile</span>
      </div>

      {user && (
        <div className="card" style={{ padding: 20, marginBottom: 16 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <div
              style={{
                width: 48,
                height: 48,
                borderRadius: "50%",
                background: "var(--bg-muted)",
                border: "1px solid var(--border)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: 18,
                fontWeight: 600,
                color: "var(--text-secondary)",
              }}
            >
              {user.username.charAt(0).toUpperCase()}
            </div>
            <div>
              <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
                {user.username}
              </div>
              <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 2 }}>
                {user.email || "No email"} · {user.role}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Change password */}
      <div className="card" style={{ padding: 20 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16 }}>
          <Lock size={13} color="var(--text-muted)" strokeWidth={1.75} />
          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text-secondary)" }}>
            Change password
          </span>
        </div>

        {pwError && (
          <div className="alert alert-error" style={{ marginBottom: 12 }}>
            <AlertCircle size={13} />
            {pwError}
          </div>
        )}
        {pwSuccess && (
          <div className="alert alert-success" style={{ marginBottom: 12 }}>
            <Check size={13} />
            Password changed successfully.
          </div>
        )}

        <form
          onSubmit={onChangePassword}
          style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 380 }}
        >
          <div className="form-field">
            <label className="form-label">Current password</label>
            <div style={{ position: "relative" }}>
              <input
                className="input"
                type={showPw ? "text" : "password"}
                value={currentPw}
                onChange={(e) => setCurrentPw(e.target.value)}
                required
                style={{ fontSize: 13, paddingRight: 36 }}
              />
              <button
                type="button"
                onClick={() => setShowPw((v) => !v)}
                style={{
                  position: "absolute",
                  right: 8,
                  top: "50%",
                  transform: "translateY(-50%)",
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  color: "var(--text-muted)",
                  display: "flex",
                }}
              >
                {showPw ? <EyeOff size={13} /> : <Eye size={13} />}
              </button>
            </div>
          </div>
          <div className="form-field">
            <label className="form-label">New password</label>
            <input
              className="input"
              type={showPw ? "text" : "password"}
              placeholder="Min. 8 characters"
              value={newPw}
              onChange={(e) => setNewPw(e.target.value)}
              required
              minLength={8}
              style={{ fontSize: 13 }}
            />
          </div>
          <div className="form-field">
            <label className="form-label">Confirm new password</label>
            <input
              className="input"
              type={showPw ? "text" : "password"}
              value={confirmPw}
              onChange={(e) => setConfirmPw(e.target.value)}
              required
              style={{ fontSize: 13 }}
            />
          </div>
          <div>
            <button
              type="submit"
              className="btn btn-primary btn-sm"
              disabled={pwLoading}
            >
              {pwLoading ? "Saving…" : "Update password"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────────────────
// Main Settings page
// ──────────────────────────────────────────────────────────────────────────────

export default function SettingsPage() {
  const { accessToken, user } = useAuth();
  const [tab, setTab] = useState<"tokens" | "users" | "profile">("tokens");

  if (!accessToken) return null;

  const isAdmin = user?.role === "admin";

  return (
    <div className="fade-in">
      {/* Header */}
      <div style={{ marginBottom: 28 }}>
        <h1 style={{ fontSize: 22, fontWeight: 600, letterSpacing: "-0.02em" }}>
          Settings
        </h1>
        <p style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }}>
          Manage tokens, users, and your account.
        </p>
      </div>

      {/* Tabs */}
      <div className="tabs" style={{ marginBottom: 24 }}>
        <button
          className={`tab${tab === "tokens" ? " active" : ""}`}
          onClick={() => setTab("tokens")}
        >
          Agent Tokens
        </button>
        {isAdmin && (
          <button
            className={`tab${tab === "users" ? " active" : ""}`}
            onClick={() => setTab("users")}
          >
            Users
          </button>
        )}
        <button
          className={`tab${tab === "profile" ? " active" : ""}`}
          onClick={() => setTab("profile")}
        >
          Profile
        </button>
      </div>

      {tab === "tokens" && <TokensSection accessToken={accessToken} />}
      {tab === "users" && isAdmin && (
        <UsersSection accessToken={accessToken} currentUserId={user?.id} />
      )}
      {tab === "profile" && (
        <ProfileSection accessToken={accessToken} user={user} />
      )}
    </div>
  );
}
