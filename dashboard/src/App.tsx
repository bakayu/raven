import { Routes, Route, Navigate } from "react-router-dom";
import Layout from "./components/Layout";
import LoginPage from "./pages/LoginPage";
import AgentsPage from "./pages/AgentsPage";
import HostDetailPage from "./pages/HostDetailPage";
import LogsPage from "./pages/LogsPage";
import AlertsPage from "./pages/AlertsPage";
import SettingsPage from "./pages/SettingsPage";
import { useAuth } from "./auth-context";

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { accessToken, ready, needsSetup } = useAuth();
  if (!ready) return null;
  if (needsSetup) return <Navigate to="/login" replace />;
  if (!accessToken) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        path="/"
        element={
          <RequireAuth>
            <Layout />
          </RequireAuth>
        }
      >
        <Route index element={<Navigate to="/agents" replace />} />
        <Route path="agents" element={<AgentsPage />} />
        <Route path="agents/:hostname" element={<HostDetailPage />} />
        <Route path="logs" element={<LogsPage />} />
        <Route path="alerts" element={<AlertsPage />} />
        <Route path="settings" element={<SettingsPage />} />
      </Route>
    </Routes>
  );
}
