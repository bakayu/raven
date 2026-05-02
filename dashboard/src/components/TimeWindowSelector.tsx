const RANGES = ["5m", "15m", "1h", "6h", "24h", "7d"] as const;

interface TimeRangeSelectorProps {
  value: string;
  onChange: (v: string) => void;
}

export default function TimeRangeSelector({ value, onChange }: TimeRangeSelectorProps) {
  return (
    <div className="time-range">
      {RANGES.map((r) => (
        <button
          key={r}
          className={`time-btn${value === r ? " active" : ""}`}
          onClick={() => onChange(r)}
        >
          {r}
        </button>
      ))}
    </div>
  );
}
