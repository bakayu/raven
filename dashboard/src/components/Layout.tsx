import { Outlet, NavLink } from "react-router-dom";
import type { NavLinkRenderProps } from "react-router-dom";
import { useAuth } from "../auth-context";

const navItems = [
    { to: "/agents", label: "Agents" },
    { to: "/logs", label: "Logs" },
    { to: "/alerts", label: "Alerts" },
    { to: "/settings", label: "Settings" },
];

export default function Layout() {
    const { accessToken, logout, ready } = useAuth();

    return (
        <div className="min-h-screen bg-zinc-950 text-zinc-100">
            <nav className="border-b border-zinc-800 px-6 py-3 flex items-center gap-8 justify-between">
                <div className="flex items-center gap-8">
                    <span className="text-lg font-bold tracking-tight">raven</span>
                    <div className="flex gap-4">
                        {navItems.map((item) => (
                            <NavLink
                                key={item.to}
                                to={item.to}
                                className={({ isActive }: NavLinkRenderProps) =>
                                    `text-sm px-3 py-1.5 rounded-md transition-colors ${isActive
                                        ? "bg-zinc-800 text-zinc-100"
                                        : "text-zinc-400 hover:text-zinc-200"
                                    }`
                                }
                            >
                                {item.label}
                            </NavLink>
                        ))}
                    </div>
                </div>

                <div>
                    {!ready ? (
                        <span className="text-sm text-zinc-500">checking session…</span>
                    ) : accessToken ? (
                        <button onClick={() => logout()} className="text-sm bg-zinc-800 px-3 py-1 rounded">Sign out</button>
                    ) : (
                        <NavLink to="/login" className="text-sm px-3 py-1.5 rounded-md text-zinc-400 hover:text-zinc-200">Sign in</NavLink>
                    )}
                </div>
            </nav>
            <main className="p-6">
                <Outlet />
            </main>
        </div>
    );
}
