import { useState } from "react";
import type { FormEvent } from "react";
import { useAuth } from "../auth-context";
import { Bird, Eye, EyeOff, AlertCircle } from "lucide-react";

export default function LoginPage() {
  const { login, setup, needsSetup, ready, accessToken } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [showPass, setShowPass] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Already logged in — will be redirected by App router
  if (ready && accessToken) return null;

  const isSetup = needsSetup;
  const title = isSetup ? "Create your account" : "Sign in to Raven";
  const subtitle = isSetup
    ? "You're the first user. Set up your admin account to get started."
    : "Enter your credentials to continue.";

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);

    if (isSetup && password !== confirmPassword) {
      setError("Passwords do not match.");
      return;
    }
    if (isSetup && password.length < 8) {
      setError("Password must be at least 8 characters.");
      return;
    }

    setLoading(true);
    try {
      if (isSetup) {
        await setup(username, password);
      } else {
        await login(username, password);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Authentication failed");
    } finally {
      setLoading(false);
    }
  }

  if (!ready) {
    return (
      <div
        style={{
          minHeight: "100vh",
          background: "var(--bg-page)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            width: 32,
            height: 32,
            borderRadius: "50%",
            border: "2px solid var(--border)",
            borderTopColor: "var(--text-secondary)",
            animation: "spin 0.8s linear infinite",
          }}
        />
        <style>{`@keyframes spin { to { transform: rotate(360deg); } }`}</style>
      </div>
    );
  }

  return (
    <div
      style={{
        minHeight: "100vh",
        background: "var(--bg-page)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        padding: "24px",
      }}
    >
      {/* Logo */}
      <div style={{ marginBottom: 32, textAlign: "center" }}>
        <div
          style={{
            width: 44,
            height: 44,
            borderRadius: 10,
            background: "var(--text-primary)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            margin: "0 auto 12px",
          }}
        >
          <Bird size={24} color="#000" strokeWidth={2.5} />
        </div>
        <h1
          style={{
            fontSize: 20,
            fontWeight: 600,
            color: "var(--text-primary)",
            letterSpacing: "-0.02em",
          }}
        >
          {title}
        </h1>
        <p style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 6 }}>
          {subtitle}
        </p>
      </div>

      {/* Card */}
      <div
        style={{
          width: "100%",
          maxWidth: 360,
          background: "var(--bg-card)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius-lg)",
          padding: "28px",
        }}
      >
        <form
          onSubmit={onSubmit}
          style={{ display: "flex", flexDirection: "column", gap: 16 }}
        >
          {/* Username */}
          <div className="form-field">
            <label className="form-label" htmlFor="login-username">
              Username
            </label>
            <input
              id="login-username"
              className="input"
              autoComplete="username"
              autoFocus
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
              disabled={loading}
              placeholder="e.g. admin"
            />
          </div>

          {/* Password */}
          <div className="form-field">
            <label className="form-label" htmlFor="login-password">
              Password
            </label>
            <div style={{ position: "relative" }}>
              <input
                id="login-password"
                className="input"
                type={showPass ? "text" : "password"}
                autoComplete={isSetup ? "new-password" : "current-password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                disabled={loading}
                placeholder={isSetup ? "At least 8 characters" : ""}
                style={{ paddingRight: 40 }}
              />
              <button
                type="button"
                onClick={() => setShowPass((v) => !v)}
                style={{
                  position: "absolute",
                  right: 10,
                  top: "50%",
                  transform: "translateY(-50%)",
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  color: "var(--text-muted)",
                  display: "flex",
                  padding: 2,
                }}
              >
                {showPass ? <EyeOff size={14} /> : <Eye size={14} />}
              </button>
            </div>
          </div>

          {/* Confirm password (setup only) */}
          {isSetup && (
            <div className="form-field">
              <label className="form-label" htmlFor="login-confirm">
                Confirm password
              </label>
              <input
                id="login-confirm"
                className="input"
                type={showPass ? "text" : "password"}
                autoComplete="new-password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                required
                disabled={loading}
                placeholder="Repeat password"
              />
            </div>
          )}

          {/* Error */}
          {error && (
            <div className="alert alert-error" style={{ marginTop: -4 }}>
              <AlertCircle size={14} style={{ flexShrink: 0, marginTop: 1 }} />
              <span>{error}</span>
            </div>
          )}

          {/* Submit */}
          <button
            type="submit"
            className="btn btn-primary"
            disabled={loading}
            style={{ width: "100%", justifyContent: "center", padding: "10px 0", marginTop: 4 }}
          >
            {loading ? (
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span
                  style={{
                    width: 13,
                    height: 13,
                    borderRadius: "50%",
                    border: "2px solid rgba(0,0,0,0.2)",
                    borderTopColor: "#000",
                    animation: "spin 0.7s linear infinite",
                    display: "inline-block",
                  }}
                />
                {isSetup ? "Creating account…" : "Signing in…"}
              </span>
            ) : isSetup ? (
              "Create account"
            ) : (
              "Sign in"
            )}
          </button>
        </form>
      </div>

      <p
        style={{
          marginTop: 20,
          fontSize: 12,
          color: "var(--text-muted)",
          textAlign: "center",
        }}
      >
        Raven — Self-hosted monitoring
      </p>

      <style>{`@keyframes spin { to { transform: rotate(360deg); } }`}</style>
    </div>
  );
}
