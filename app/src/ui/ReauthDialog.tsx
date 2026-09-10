import { useState } from "react";

export function ReauthDialog({
  title = "二次验证",
  hint = "查看机密需要重新输入访问密码。",
  onCancel,
  onConfirm,
}: {
  title?: string;
  hint?: string;
  onCancel: () => void;
  onConfirm: (password: string) => Promise<void> | void;
}) {
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  async function submit() {
    if (!pw.trim()) return setErr("请输入访问密码");
    setBusy(true);
    setErr("");
    try {
      await onConfirm(pw);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="wizard-overlay">
      <div className="card" style={{ width: 380, maxWidth: "94vw" }}>
        <div className="card-head">
          <div className="card-title">{title}</div>
        </div>
        <div className="card-body stack">
          <div className="muted">{hint}</div>
          <div className="field">
            <label className="field-label">访问密码</label>
            <input
              className="input"
              type="password"
              autoFocus
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && submit()}
            />
          </div>
          {err && <div className="callout danger sm">{err}</div>}
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" className="btn ghost sm" onClick={onCancel}>
              取消
            </button>
            <button type="button" className="btn primary sm" disabled={busy} onClick={submit}>
              {busy ? "验证中…" : "确认"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
