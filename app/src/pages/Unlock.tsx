import { useState } from "react";
import { api, errMessage } from "../lib/ipc";
import { useApp } from "../store";

export function Unlock() {
  const refresh = useApp((s) => s.refresh);
  const [pw, setPw] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [recoveryMode, setRecoveryMode] = useState(false);
  const [recovery, setRecovery] = useState("");

  async function unlock() {
    setErr("");
    setBusy(true);
    try {
      if (recoveryMode) {
        await api.vaultUnlockRecovery(recovery);
      } else {
        await api.vaultUnlock(pw);
      }
      await refresh();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="unlock-stage">
      <div className="unlock-card">
        <div className="unlock-logo">🔐</div>
        <div className="title-lg">解锁工作空间</div>
        <div className="muted" style={{ marginBottom: 20 }}>
          {recoveryMode ? "输入恢复密钥以解锁（忘记密码时使用）" : "输入访问密码继续"}
        </div>

        {recoveryMode ? (
          <textarea
            className="input mono"
            placeholder="恢复密钥 GAM1-..."
            value={recovery}
            onChange={(e) => setRecovery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && e.ctrlKey && unlock()}
          />
        ) : (
          <input
            className="input"
            type="password"
            placeholder="访问密码"
            value={pw}
            autoFocus
            onChange={(e) => setPw(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && unlock()}
          />
        )}

        {err && <div className="err-text">{err}</div>}

        <button className="btn primary lg" style={{ width: "100%", marginTop: 16 }} disabled={busy} onClick={unlock}>
          {busy ? "解锁中…" : "解锁"}
        </button>
        <button
          className="btn ghost sm"
          style={{ marginTop: 12 }}
          onClick={() => {
            setErr("");
            setRecoveryMode((m) => !m);
          }}
        >
          {recoveryMode ? "改用访问密码" : "忘记密码？用恢复密钥"}
        </button>
      </div>
    </div>
  );
}
