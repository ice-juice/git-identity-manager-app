import { useState } from "react";
import { FolderPlus } from "lucide-react";
import { errMessage } from "../lib/ipc";

export const GROUP_COLORS = [
  "#6366f1",
  "#0ea5e9",
  "#10b981",
  "#f59e0b",
  "#f43f5e",
  "#8b5cf6",
  "#64748b",
];

export const GROUP_PRESETS = [
  { name: "工作", color: "#6366f1" },
  { name: "个人", color: "#10b981" },
  { name: "云服务", color: "#0ea5e9" },
  { name: "开发", color: "#f59e0b" },
];

export function GroupDialog({
  existing,
  onCancel,
  onConfirm,
}: {
  existing: string[];
  onCancel: () => void;
  onConfirm: (name: string, color: string | null) => Promise<void> | void;
}) {
  const [name, setName] = useState("");
  const [color, setColor] = useState<string | null>(GROUP_COLORS[0]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const taken = new Set(existing.map((n) => n.trim().toLowerCase()));
  const presets = GROUP_PRESETS.filter((p) => !taken.has(p.name.toLowerCase()));

  async function submit() {
    const trimmed = name.trim();
    if (!trimmed) return setErr("请填写分组名称");
    if (taken.has(trimmed.toLowerCase())) return setErr("已有同名分组");
    setBusy(true);
    setErr("");
    try {
      await onConfirm(trimmed, color);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="wizard-overlay">
      <div className="card group-dialog">
        <div className="card-head">
          <div className="card-title row" style={{ gap: 8 }}>
            <FolderPlus size={14} style={{ color: "var(--accent)" }} />
            新建分组
          </div>
        </div>
        <div className="card-body stack">
          <div className="muted">用分组整理条目。建好后可在添加 / 编辑时直接选用。</div>

          <div className="field">
            <label className="field-label">分组名称</label>
            <input
              className="input"
              autoFocus
              maxLength={24}
              placeholder="例如 工作、个人、云服务"
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                if (err) setErr("");
              }}
              onKeyDown={(e) => e.key === "Enter" && submit()}
            />
          </div>

          <div className="field">
            <label className="field-label">标识色</label>
            <div className="color-dots">
              {GROUP_COLORS.map((c) => (
                <button
                  key={c}
                  type="button"
                  className={"color-dot" + (color === c ? " on" : "")}
                  style={{ background: c }}
                  title={c}
                  onClick={() => setColor(c)}
                />
              ))}
              <button
                type="button"
                className={"color-dot none" + (color === null ? " on" : "")}
                title="不使用标识色"
                onClick={() => setColor(null)}
              />
            </div>
          </div>

          {presets.length > 0 && (
            <div className="field">
              <label className="field-label">快捷选用</label>
              <div className="choice-row">
                {presets.map((p) => (
                  <button
                    key={p.name}
                    type="button"
                    className={"choice" + (name === p.name ? " on" : "")}
                    onClick={() => {
                      setName(p.name);
                      setColor(p.color);
                      if (err) setErr("");
                    }}
                  >
                    <span className="group-tab-dot" style={{ background: p.color }} />
                    {p.name}
                  </button>
                ))}
              </div>
            </div>
          )}

          {err && <div className="callout danger sm">{err}</div>}

          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" className="btn ghost sm" onClick={onCancel}>
              取消
            </button>
            <button type="button" className="btn primary sm" disabled={busy} onClick={submit}>
              {busy ? "创建中…" : "创建分组"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
