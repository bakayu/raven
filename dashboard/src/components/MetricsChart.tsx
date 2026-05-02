import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { format } from "date-fns";
import type { MetricPoint } from "../api";

interface MetricChartProps {
  data: MetricPoint[];
  label: string;
  unit?: string;
  color?: string;
  domain?: [number, number];
  height?: number;
  formatValue?: (v: number) => string;
}

function CustomTooltip({
  active,
  payload,
  label: ts,
  unit,
  formatValue,
}: {
  active?: boolean;
  payload?: { value: number }[];
  label?: number;
  unit?: string;
  formatValue?: (v: number) => string;
}) {
  if (!active || !payload?.length) return null;
  const val = payload[0].value;
  return (
    <div
      style={{
        background: "var(--bg-card)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-sm)",
        padding: "8px 12px",
        fontSize: 12,
        fontFamily: "var(--font-mono)",
      }}
    >
      <div style={{ color: "var(--text-muted)", marginBottom: 4 }}>
        {ts ? format(new Date(ts * 1000), "HH:mm:ss") : ""}
      </div>
      <div style={{ color: "var(--text-primary)", fontWeight: 500 }}>
        {formatValue ? formatValue(val) : `${val.toFixed(1)}${unit ?? "%"}`}
      </div>
    </div>
  );
}

export default function MetricChart({
  data,
  label,
  unit = "%",
  color = "var(--chart-1)",
  domain = [0, 100],
  height = 180,
  formatValue,
}: MetricChartProps) {
  const chartData = data.map((p) => ({ ts: p.timestamp, value: p.value }));
  const gradientId = `grad-${label.replace(/\s+/g, "-")}`;

  return (
    <div>
      <div
        style={{
          fontSize: 12,
          fontWeight: 500,
          color: "var(--text-muted)",
          marginBottom: 10,
          textTransform: "uppercase",
          letterSpacing: "0.04em",
        }}
      >
        {label}
      </div>
      <ResponsiveContainer width="100%" height={height}>
        <AreaChart data={chartData} margin={{ top: 12, right: 12, left: 0, bottom: 0 }}>
          <defs>
            <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={color} stopOpacity={0.15} />
              <stop offset="100%" stopColor={color} stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid
            strokeDasharray="3 3"
            stroke="var(--border-subtle)"
            vertical={false}
          />
          <XAxis
            dataKey="ts"
            type="number"
            domain={["auto", "auto"]}
            scale="time"
            tickFormatter={(v) => format(new Date(v * 1000), "HH:mm")}
            tick={{ fontSize: 13, fill: "var(--text-muted)", fontFamily: "var(--font-mono)" }}
            tickLine={false}
            axisLine={false}
            minTickGap={30}
          />
          <YAxis
            domain={domain}
            tickFormatter={(v) =>
              formatValue ? formatValue(v) : `${v}${unit}`
            }
            tick={{ fontSize: 13, fill: "var(--text-muted)", fontFamily: "var(--font-mono)" }}
            tickLine={false}
            axisLine={false}
            width={60}
          />
          <Tooltip
            content={
              <CustomTooltip unit={unit} formatValue={formatValue} />
            }
            cursor={{ stroke: "var(--border)", strokeWidth: 1 }}
          />
          <Area
            type="monotone"
            dataKey="value"
            stroke={color}
            strokeWidth={1.5}
            fill={`url(#${gradientId})`}
            dot={false}
            activeDot={{ r: 3, fill: color, strokeWidth: 0 }}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}

// Sparkline — tiny chart for agent cards
interface SparklineProps {
  data: MetricPoint[];
  color?: string;
  height?: number;
}

export function Sparkline({ data, color = "var(--chart-1)", height = 32 }: SparklineProps) {
  const chartData = data.map((p) => ({ ts: p.timestamp, value: p.value }));
  return (
    <ResponsiveContainer width="100%" height={height}>
      <AreaChart data={chartData} margin={{ top: 2, right: 0, left: 0, bottom: 2 }}>
        <defs>
          <linearGradient id={`spark-grad-${color}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} stopOpacity={0.2} />
            <stop offset="100%" stopColor={color} stopOpacity={0} />
          </linearGradient>
        </defs>
        <Area
          type="monotone"
          dataKey="value"
          stroke={color}
          strokeWidth={1.5}
          fill={`url(#spark-grad-${color})`}
          dot={false}
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}
