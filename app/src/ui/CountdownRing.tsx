export function CountdownRing({
  remain,
  period = 30,
  size = 30,
}: {
  remain: number;
  period?: number;
  size?: number;
}) {
  const r = 12;
  const c = 2 * Math.PI * r; // ~75.398
  const safePeriod = period > 0 ? period : 30;
  const progress = Math.max(0, Math.min(1, remain / safePeriod));
  const offset = c * (1 - progress);
  const color =
    remain <= 5 ? "var(--red)" : remain <= 10 ? "var(--amber)" : "var(--accent)";

  return (
    <div
      className="countdown-ring"
      style={{ width: size, height: size, flex: `0 0 ${size}px` }}
      title={`动态验证码剩余 ${remain} 秒`}
    >
      <svg viewBox="0 0 32 32">
        <circle className="bg-circle" cx="16" cy="16" r={r} />
        <circle
          className="val-circle"
          cx="16"
          cy="16"
          r={r}
          stroke={color}
          strokeDasharray={c}
          strokeDashoffset={offset}
        />
      </svg>
      <span className="sec-num" style={{ color: remain <= 5 ? "var(--red)" : undefined }}>
        {remain}
      </span>
    </div>
  );
}
