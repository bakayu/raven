import { Outlet, NavLink, useNavigate } from "react-router-dom";
import type { NavLinkRenderProps } from "react-router-dom";
import {
  Server,
  ScrollText,
  // Bell,  // alerts - hidden until alerting engine is ready
  Settings,
  LogOut,
  // BarChart3,  // metrics - phase 6
} from "lucide-react";
import { useAuth } from "../auth-context";

const navItems = [
  { to: "/agents", label: "Agents", icon: Server },
  { to: "/logs", label: "Logs", icon: ScrollText },
  // { to: "/metrics", label: "Metrics", icon: BarChart3 },  // phase 6
  // { to: "/alerts", label: "Alerts", icon: Bell, disabled: true },  // hidden until alerting engine is ready
  { to: "/settings", label: "Settings", icon: Settings },
];

export default function Layout() {
  const { user, logout } = useAuth();
  const navigate = useNavigate();

  return (
    <div className="app-layout">
      {/* Sidebar */}
      <aside className="sidebar">
        {/* Logo */}
        <div
          style={{
            padding: "16px 12px 14px",
            borderBottom: "1px solid var(--border)",
          }}
        >
          <button
            onClick={() => navigate("/agents")}
            style={{
              display: "flex",
              alignItems: "center",
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: 0,
            }}
          >
            <img
              src="/raven-logo-horizontal-full-light.png"
              alt="Raven"
              style={{
                height: 28,
                width: "auto",
                objectFit: "contain",
              }}
            />
          </button>
        </div>

        {/* Navigation */}
        <nav style={{ flex: 1, padding: "12px 8px" }}>
          {navItems.map(({ to, label, icon: Icon, disabled }: any) => {
            if (disabled) {
              return (
                <div
                  key={to}
                  title="coming soon"
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "7px 8px",
                    borderRadius: 6,
                    marginBottom: 2,
                    fontSize: 13,
                    fontWeight: 500,
                    color: "var(--text-disabled)",
                    cursor: "not-allowed",
                    opacity: 0.5,
                  }}
                >
                  <Icon size={15} strokeWidth={1.75} />
                  {label}
                </div>
              );
            }

            return (
              <NavLink
                key={to}
                to={to}
                className={({ isActive }: NavLinkRenderProps) =>
                  isActive ? "sidebar-nav-item active" : "sidebar-nav-item"
                }
                style={({ isActive }: NavLinkRenderProps) => ({
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "7px 8px",
                  borderRadius: 6,
                  marginBottom: 2,
                  textDecoration: "none",
                  fontSize: 13,
                  fontWeight: 500,
                  transition: "all 0.12s ease",
                  color: isActive ? "var(--text-primary)" : "var(--text-muted)",
                  background: isActive ? "var(--accent-muted)" : "transparent",
                })}
                onMouseEnter={(e) => {
                  const el = e.currentTarget as HTMLAnchorElement;
                  if (!el.classList.contains("active")) {
                    el.style.color = "var(--text-secondary)";
                    el.style.background = "var(--accent-muted)";
                  }
                }}
                onMouseLeave={(e) => {
                  const el = e.currentTarget as HTMLAnchorElement;
                  if (!el.classList.contains("active")) {
                    el.style.color = "var(--text-muted)";
                    el.style.background = "transparent";
                  }
                }}
              >
                <Icon size={15} strokeWidth={1.75} />
                {label}
              </NavLink>
            );
          })}
        </nav>

        {/* User / Sign out */}
        <div
          style={{
            padding: "12px 8px",
            borderTop: "1px solid var(--border)",
          }}
        >
          {user && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "8px",
                borderRadius: 6,
                marginBottom: 4,
              }}
            >
              <div
                style={{
                  width: 26,
                  height: 26,
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
                {user.username.charAt(0).toUpperCase()}
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  style={{
                    fontSize: 12,
                    fontWeight: 500,
                    color: "var(--text-primary)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {user.username}
                </div>
                <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
                  {user.role}
                </div>
              </div>
            </div>
          )}
          <button
            onClick={() => logout()}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              width: "100%",
              padding: "7px 8px",
              borderRadius: 6,
              background: "none",
              border: "none",
              cursor: "pointer",
              fontSize: 13,
              color: "var(--text-muted)",
              fontFamily: "var(--font-sans)",
              transition: "all 0.12s ease",
            }}
            onMouseEnter={(e) => {
              (e.currentTarget as HTMLButtonElement).style.color =
                "var(--error)";
              (e.currentTarget as HTMLButtonElement).style.background =
                "var(--error-bg)";
            }}
            onMouseLeave={(e) => {
              (e.currentTarget as HTMLButtonElement).style.color =
                "var(--text-muted)";
              (e.currentTarget as HTMLButtonElement).style.background =
                "transparent";
            }}
          >
            <LogOut size={14} strokeWidth={1.75} />
            Sign out
          </button>
        </div>
      </aside>

      {/* Main */}
      <div className="main-content">
        <div className="page-content">
          <Outlet />
        </div>
      </div>
    </div>
  );
}
