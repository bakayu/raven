import { useParams } from "react-router-dom";

export default function HostDetailPage() {
  const { hostname } = useParams();
  return (
    <div>
      <h1 className="text-2xl font-bold mb-4">{hostname}</h1>
      {/* TODO: Phase 7 — time range picker + metric charts */}
      <p className="text-zinc-500">Host detail placeholder</p>
    </div>
  );
}