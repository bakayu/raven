import { Outlet, NavLink } from "react-router-dom";
import type { NavLinkRenderProps } from "react-router-dom";

const navItems = [
    { to: "/agents", label: "Agents" },
    { to: "/logs", label: "Logs" },
    { to: "/alerts", label: "Alerts" },
    { to: "/settings", label: "Settings" },
];

export default function Layout() {
    return (
        <div className="min-h-screen bg-zinc-950 text-zinc-100">
            <nav className="border-b border-zinc-800 px-6 py-3 flex items-center gap-8">
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
            </nav>
            <main className="p-6">
                <Outlet />
            </main>
        </div>
    );
}
